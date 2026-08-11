use std::collections::{HashMap, HashSet};
use iced::widget::image::Handle;
use iced::Task;

/// Caché de thumbnails con ciclo de vida atado a la ventana visible.
///
/// El mapa interno ES la fuente de verdad: si una key no está presente,
/// no existe para el sistema (no se está descargando, no se muestra).
/// No requiere epochs ni tokens de generación — llamar a `sync` con la
/// lista de keys deseadas invalida y limpia todo lo demás.
pub struct AsyncThumbnail {
    slots: HashMap<String, Slot>,
}

enum Slot {
    Loading,
    Ready(Handle),
}

impl AsyncThumbnail {
    pub fn new() -> Self {
        Self { slots: HashMap::new() }
    }

    /// Sincroniza el caché contra el conjunto de `(key, url)` deseado.
    ///
    /// - Elimina del caché cualquier key que no esté en `wanted`.
    /// - Encola descarga para cada key de `wanted` que no esté presente.
    /// - Es idempotente: llamar repetidamente con el mismo `wanted` no
    ///   dispara descargas duplicadas.
    ///
    /// Llamar una vez, al final de `update()`, con la ventana visible
    /// actual (scroll, filtro, sort ya aplicados).
    pub fn sync<Msg: 'static + Send>(
        &mut self,
        wanted: &[(String, String)],
        to_message: impl Fn(String, Vec<u8>) -> Msg + Send + Sync + 'static + Clone,
    ) -> Task<Msg> {
        let wanted_keys: HashSet<&str> = wanted.iter().map(|(k, _)| k.as_str()).collect();
        self.slots.retain(|k, _| wanted_keys.contains(k.as_str()));

        let mut tasks = Vec::new();
        for (key, url) in wanted {
            if self.slots.contains_key(key) {
                continue;
            }
            self.slots.insert(key.clone(), Slot::Loading);

            let key = key.clone();
            let url = url.clone();
            let to_message = to_message.clone();
            tasks.push(Task::perform(download(url), move |bytes| {
                to_message(key.clone(), bytes)
            }));
        }

        Task::batch(tasks)
    }

    /// Escribe el resultado de una descarga. Si la key ya no está
    /// presente (salió de la ventana visible antes de terminar), el
    /// resultado se descarta sin efecto.
    pub fn on_loaded(&mut self, key: String, bytes: Vec<u8>) {
        if bytes.is_empty() || !self.slots.contains_key(&key) {
            return;
        }
        self.slots.insert(key, Slot::Ready(Handle::from_bytes(bytes)));
    }

    /// Devuelve el handle listo para renderizar, o `None` si aún se
    /// está descargando o no fue solicitado.
    pub fn get(&self, key: &str) -> Option<&Handle> {
        match self.slots.get(key)? {
            Slot::Ready(h) => Some(h),
            Slot::Loading => None,
        }
    }
}

impl Default for AsyncThumbnail {
    fn default() -> Self {
        Self::new()
    }
}

async fn download(url: String) -> Vec<u8> {
    use image::ImageReader;
    use std::io::Cursor;

    let Ok(resp) = reqwest::get(&url).await else { return Vec::new() };
    let Ok(bytes) = resp.bytes().await else { return Vec::new() };

    let Ok(reader) = ImageReader::new(Cursor::new(&bytes)).with_guessed_format() else {
        return Vec::new();
    };
    let Ok(img) = reader.decode() else {
        return Vec::new();
    };

    let (w, h) = (img.width(), img.height());
    let size = w.min(h);
    let x = (w - size) / 2;
    let y = (h - size) / 2;
    let cropped = img.crop_imm(x, y, size, size);

    let mut out = Vec::new();
    if cropped
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .is_err()
    {
        return Vec::new();
    }

    out
}