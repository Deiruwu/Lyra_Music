use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use image::GenericImageView;
use ksni::{MenuItem, Tray, TrayMethods};
use crate::audio::manager::TrackManager;

static ICON: LazyLock<ksni::Icon> = LazyLock::new(|| {
    let img = image::load_from_memory_with_format(
        include_bytes!("../assets/icons/atelier_icon_32.png"),
        image::ImageFormat::Png,
    )
        .expect("icono válido");
    let (width, height) = img.dimensions();
    let mut data = img.into_rgba8().into_vec();
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }
    ksni::Icon { width: width as i32, height: height as i32, data }
});

pub struct TrayFlags {
    pub show_window: AtomicBool,
    pub quit:        AtomicBool,
}

impl TrayFlags {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            show_window: AtomicBool::new(false),
            quit:        AtomicBool::new(false),
        })
    }
}

pub struct AppTray {
    pub manager: Arc<TrackManager>,
    pub flags:   Arc<TrayFlags>,
}

impl Tray for AppTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        "Atelier".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ICON.clone()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Atelier".into(),
            description: "Reproductor de música".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.flags.show_window.store(true, Ordering::Relaxed);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        use ksni::menu::*;
        let is_playing = self.manager.state.is_playing();

        vec![
            StandardItem {
                label: "Mostrar".into(),
                activate: Box::new(|this: &mut Self| {
                    this.flags.show_window.store(true, Ordering::Relaxed);
                }),
                ..Default::default()
            }.into(),
            MenuItem::Separator,
            StandardItem {
                label: if is_playing { "Pausar".into() } else { "Reproducir".into() },
                activate: Box::new(|this: &mut Self| {
                    if this.manager.state.is_playing() {
                        this.manager.pause();
                    } else {
                        this.manager.resume();
                    }
                }),
                ..Default::default()
            }.into(),
            StandardItem {
                label: "Siguiente".into(),
                activate: Box::new(|this: &mut Self| {
                    this.manager.skip_next();
                }),
                ..Default::default()
            }.into(),
            MenuItem::Separator,
            StandardItem {
                label: "Salir".into(),
                activate: Box::new(|this: &mut Self| {
                    // Señalizar a iced para que cierre limpiamente
                    this.flags.quit.store(true, Ordering::Relaxed);
                }),
                ..Default::default()
            }.into(),
        ]
    }
}

pub fn spawn_tray(manager: Arc<TrackManager>) -> Arc<TrayFlags> {
    let flags      = TrayFlags::new();
    let flags_tray = Arc::clone(&flags);

    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("No se pudo crear runtime para el tray")
            .block_on(async move {
                let handle = AppTray { manager, flags: flags_tray }
                    .spawn()
                    .await
                    .expect("No se pudo iniciar el tray");

                std::mem::forget(handle);
                std::future::pending::<()>().await;
            });
    });

    flags
}