use crate::ui::assets::icons::Icon;
use crate::ui::widgets::context_menu_V2::ContextMenuItem;

const ADD_TO_PLAYLIST_SUBMENU_ID: usize = 0;

#[derive(Debug, Clone, PartialEq)]
pub enum TrackContextAction {
    PlayNow,
    Enqueue,
    FrontEnqueue,
    ToggleLike,
    AddToPlaylist(String),
    CopyId,
    DeleteFromCatalog,
    RemoveFromPlaylist,
}

pub struct TrackContextMenuBuilder<'a> {
    is_liked: bool,
    playlists: Option<(&'a [(String, String)], Option<&'a str>)>,
    with_delete: bool,
    with_remove_from_playlist: bool,
}

impl<'a> TrackContextMenuBuilder<'a> {
    pub fn new(is_liked: bool) -> Self {
        Self {
            is_liked,
            playlists: None,
            with_delete: false,
            with_remove_from_playlist: false,
        }
    }

    pub fn with_playlists(
        mut self,
        playlists: &'a [(String, String)],
        current_playlist_id: Option<&'a str>,
    ) -> Self {
        self.playlists = Some((playlists, current_playlist_id));
        self
    }

    pub fn with_delete(mut self) -> Self {
        self.with_delete = true;
        self
    }

    pub fn with_remove_from_playlist(mut self) -> Self {
        self.with_remove_from_playlist = true;
        self
    }

    pub fn build(self) -> Vec<ContextMenuItem<TrackContextAction>> {
        let mut items = vec![
            ContextMenuItem::new("Reproducir ahora", TrackContextAction::PlayNow)
                .icon(Icon::Play),
            ContextMenuItem::new("Agregar a la cola", TrackContextAction::Enqueue)
                .icon(Icon::AddQueue),
            ContextMenuItem::new("Reproducir después", TrackContextAction::FrontEnqueue)
                .icon(Icon::AddQueueFront),
            self.like_item(),
            self.add_to_playlist_item(),
            ContextMenuItem::new("Copiar ID", TrackContextAction::CopyId)
                .icon(Icon::Copiar),
        ];

        if self.with_delete {
            items.push(
                ContextMenuItem::new("Eliminar del catálogo", TrackContextAction::DeleteFromCatalog)
                    .icon(Icon::Delete),
            );
        }

        if self.with_remove_from_playlist {
            items.push(
                ContextMenuItem::new("Quitar de esta playlist", TrackContextAction::RemoveFromPlaylist)
                    .icon(Icon::DeleteOpen),
            );
        }

        items
    }

    fn like_item(&self) -> ContextMenuItem<TrackContextAction> {
        if self.is_liked {
            ContextMenuItem::new("Quitar de me gusta", TrackContextAction::ToggleLike)
                .icon(Icon::HeartBroken)
        } else {
            ContextMenuItem::new("Me gusta", TrackContextAction::ToggleLike)
                .icon(Icon::Heart)
        }
    }

    fn add_to_playlist_item(&self) -> ContextMenuItem<TrackContextAction> {
        let children = match self.playlists {
            Some((playlists, current_id)) => playlists
                .iter()
                .filter(|(id, _)| Some(id.as_str()) != current_id)
                .map(|(id, name)| {
                    ContextMenuItem::new(name.clone(), TrackContextAction::AddToPlaylist(id.clone()))
                })
                .collect(),
            None => Vec::new(),
        };

        ContextMenuItem::submenu("Agregar a playlist", ADD_TO_PLAYLIST_SUBMENU_ID, children)
            .icon(Icon::Playlist)
    }
}