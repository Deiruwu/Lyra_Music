use crate::ui::assets::icons::Icon;

/// Identificador único de cada distrito primario del sidebar.
/// Agregar una variante aquí + su `VIEW_DATA` correspondiente en el archivo
/// del distrito es todo lo que se necesita para que aparezca en el menú.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavId {
    Home,
    Explorer,
    Favorites,
    Artists,
    Remix,
    PlaylistsOverview,
}

/// Metadata estática que cada distrito expone sobre sí mismo.
/// El sidebar no sabe nada de la lógica interna de cada vista, solo
/// dibuja un botón usando esto.
#[derive(Debug, Clone, Copy)]
pub struct ViewData {
    pub id: NavId,
    pub icon: Icon,
    pub label: &'static str,
}

impl ViewData {
    pub const fn new(id: NavId, icon: Icon, label: &'static str) -> Self {
        Self { id, icon, label }
    }
}