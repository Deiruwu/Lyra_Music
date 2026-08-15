use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use iced::widget::image::Handle;
use iced::Task;

use crate::ui::utils::data_dir::ensure_covers_dir;
use crate::ui::utils::image::{crop_and_encode_cover, load_local_cover};

/// Máximo lado en px al que se redimensiona la portada GRANDE al guardarla
/// (la del header/vista detalle), para que el archivo en disco sea liviano
/// y la carga sea instantánea.
const COVER_SAVE_SIZE_LARGE: u32 = 512;
/// Máximo lado en px al que se redimensiona la portada PEQUEÑA (la del
/// sidebar, que se pinta ~40px). Al guardar una versión ya a este tamaño y
/// consumirla así, evitamos que la GPU haga un downscale brutal de 512→40
/// (aliasing/resampling) en el render.
const COVER_SAVE_SIZE_SMALL: u32 = 64;

/// Variante de una portada de playlist. Cada variante es un archivo distinto
/// en disco y una clave distinta en el caché RAM; aísla los dos consumidores
/// (header 512px, sidebar ~40px) para que ninguno rescale al otro tamaño y
/// no colisionen en el `HashMap` de slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverVariant {
    /// Portada grande (header/detalle) — 512px, clave `"<id>"`.
    Large,
    /// Portada pequeña (sidebar) — 64px, clave `"<id>_small"`.
    Small,
}

impl CoverVariant {
    pub fn suffix(self) -> &'static str {
        match self {
            CoverVariant::Large => "",
            CoverVariant::Small => "_small",
        }
    }

    /// Clave única del caché RAM para esta variante de una playlist.
    pub fn key(self, playlist_id: &str) -> String {
        format!("{playlist_id}{}", self.suffix())
    }

    /// Tamaño máximo en px al que se redimensiona esta variante.
    fn save_size(self) -> u32 {
        match self {
            CoverVariant::Large => COVER_SAVE_SIZE_LARGE,
            CoverVariant::Small => COVER_SAVE_SIZE_SMALL,
        }
    }
}


/// Gestor de portadas de playlists, atado al ciclo de vida de la app.
///
/// A diferencia de `AsyncThumbnail` (que descarga de URLs remotas vía HTTP),
/// las portadas de playlists son archivos LOCALES en
/// `<data_dir>/lyra/covers/<playlist_id>.jpg` — ver `utils::image`. Este
/// gestor sigue el mismo patrón de "sync / retain / load-only-missing" que
/// `AsyncThumbnail`, pero leyendo del disco en vez de la red.
///
/// `import_cover` recibe el camino de una imagen elegida por el usuario y la
/// recorta/redimensiona/re-encodea a JPEG guardando DOS variantes:
/// `<covers_dir>/<playlist_id>.jpg` (grande, 512px) y
/// `<covers_dir>/<playlist_id>_small.jpg` (pequeña, 64px) — cada una con su
/// clave de caché `<id>` / `<id>_small`.
pub struct CoverManager {
    slots: HashMap<String, Slot>,
}

enum Slot {
    Loading,
    Ready(Handle),
}

impl Default for CoverManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverManager {
    pub fn new() -> Self {
        Self { slots: HashMap::new() }
    }

    /// Recibe las claves (`"<id>"` para la variante `Large`, `"<id>_small"`
    /// para la `Small`) que la UI quiere tener vivas ahora mismo y el camino
    /// del archivo local correspondiente a cada una, y una función
    /// constructora de mensajes. Retiene en RAM solo las claves pedidas
    /// (descartando las demás) y despacha una carga de disco por cada clave
    /// que todavía no esté en memoria.
    pub fn sync<Msg: 'static + Send>(
        &mut self,
        wanted: &[(String, String)],
        to_message: impl Fn(String, Vec<u8>) -> Msg + Send + Sync + 'static + Clone,
    ) -> Task<Msg> {
        let wanted_keys: HashSet<&str> = wanted.iter().map(|(k, _)| k.as_str()).collect();

        self.slots.retain(|k, _| wanted_keys.contains(k.as_str()));

        let mut tasks = Vec::new();
        for (key, path) in wanted {
            if self.slots.contains_key(key) {
                continue;
            }

            self.slots.insert(key.clone(), Slot::Loading);

            let key = key.clone();
            let path = path.clone();
            let to_message = to_message.clone();
            // Decodificamos al tamaño propio de la variante (clave con sufijo
            // `_small` → 64px; el resto → 512px) para que cada caché quede a
            // su resolución y no se rescale brutalmente en el render.
            let max_size = if key.ends_with("_small") { CoverVariant::Small.save_size() } else { CoverVariant::Large.save_size() };

            tasks.push(Task::perform(
                async move { load_local_cover(&path, max_size).unwrap_or_default() },
                move |bytes| to_message(key.clone(), bytes),
            ));
        }

        Task::batch(tasks)
    }

    /// Registra los bytes decodeados y los convierte en un `Handle`. Si la
    /// clave ya fue descartada (cambió la vista), descarta los bytes.
    pub fn on_loaded(&mut self, key: String, bytes: Vec<u8>) {
        if bytes.is_empty() || !self.slots.contains_key(&key) {
            return;
        }
        self.slots.insert(key, Slot::Ready(Handle::from_bytes(bytes)));
    }

    /// Obtiene el handle de la portada si ya terminó de cargar/decodificar.
    pub fn get(&self, key: &str) -> Option<&Handle> {
        match self.slots.get(key)? {
            Slot::Ready(h) => Some(h),
            Slot::Loading => None,
        }
    }

    /// Importa una imagen elegida por el usuario como portada de una playlist:
    /// la recorta a cuadrado, la redimensiona y re-encodea a JPEG guardando
    /// AMBAS variantes (grande 512px y pequeña 64px) en
    /// `<covers_dir>/<id>.jpg` y `<covers_dir>/<id>_small.jpg`. Devuelve el
    /// camino del archivo GRANDE resultante (el que se persiste en la DB como
    /// `cover_url`) si todo salió bien.
    pub fn import_cover(&mut self, playlist_id: &str, src_path: &std::path::Path) -> Result<PathBuf, String> {
        let bytes = std::fs::read(src_path).map_err(|e| e.to_string())?;

        let encoded_large = crop_and_encode_cover(&bytes, CoverVariant::Large.save_size())
            .ok_or_else(|| "No se pudo codificar la portada grande".to_string())?;
        let encoded_small = crop_and_encode_cover(&bytes, CoverVariant::Small.save_size())
            .ok_or_else(|| "No se pudo codificar la portada miniatura".to_string())?;

        ensure_covers_dir().map_err(|e| e.to_string())?;

        let dest_large = covers_path(playlist_id, CoverVariant::Large);
        let dest_small = covers_path(playlist_id, CoverVariant::Small);

        std::fs::write(&dest_large, encoded_large).map_err(|e| e.to_string())?;
        std::fs::write(&dest_small, encoded_small).map_err(|e| e.to_string())?;

        // Descartamos ambos slots en RAM para que el próximo `sync` recargue
        // del disco los archivos recién escritos y la UI repinte al instante
        // en vez de seguir mostrando el Handle anterior hasta reiniciar.
        self.invalidate(playlist_id);

        Ok(dest_large)
    }

    /// Descartar en memoria ambas variantes de una playlist. Tras invalidar,
    /// el próximo [`sync`](Self::sync) volverá a cargarlas del disco.
    pub fn invalidate(&mut self, playlist_id: &str) {
        self.slots.remove(&CoverVariant::Large.key(playlist_id));
        self.slots.remove(&CoverVariant::Small.key(playlist_id));
    }
}

/// Camino de la portada de una playlist: `<covers_dir>/<id>[_small].jpg`.
pub fn covers_path(playlist_id: &str, variant: CoverVariant) -> PathBuf {
    let sanitized: String = playlist_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    crate::ui::utils::data_dir::covers_dir().join(format!("{sanitized}{}.jpg", variant.suffix()))
}
