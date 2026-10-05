use std::collections::HashSet;

use crate::ui::assets::icons::Icon;
use crate::ui::cover_palette;
use crate::ui::widgets::context_menu::ContextMenuItem;

const ADD_TO_PLAYLIST_SUBMENU_ID: usize = 0;
const TRACK_TOOLS_SUBMENU_ID: usize = 1;

/// Link público de YouTube para un id de track.
pub fn youtube_link(track_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={track_id}")
}

/// Operaciones de mantenimiento que corren en el track_manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackTool {
    RefreshMetadata,
    RefreshLyrics,
    Reanalyze,
    Redownload,
}

impl TrackTool {
    /// Texto corto para logs/errores.
    pub fn label(self) -> &'static str {
        match self {
            TrackTool::RefreshMetadata => "actualizar metadatos",
            TrackTool::RefreshLyrics => "buscar la letra",
            TrackTool::Reanalyze => "re-analizar",
            TrackTool::Redownload => "descargar",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackContextAction {
    PlayNow,
    Enqueue,
    FrontEnqueue,
    StartRadio,
    ToggleLike,
    AddToPlaylist(String),
    CopyId,
    CopyYoutubeLink,
    Tool(TrackTool),
    DeleteFromCatalog,
    RemoveFromPlaylist,
}

pub struct TrackContextMenuBuilder<'a> {
    is_liked: bool,
    playlists: Option<(&'a [(String, String)], Option<&'a str>, &'a HashSet<String>)>,
    is_downloaded: Option<bool>,
    with_delete: bool,
    with_remove_from_playlist: bool,
}

impl<'a> TrackContextMenuBuilder<'a> {
    pub fn new(is_liked: bool) -> Self {
        Self {
            is_liked,
            playlists: None,
            is_downloaded: None,
            with_delete: false,
            with_remove_from_playlist: false,
        }
    }

    pub fn with_playlists(
        mut self,
        playlists: &'a [(String, String)],
        current_playlist_id: Option<&'a str>,
        member_of: &'a HashSet<String>,
    ) -> Self {
        self.playlists = Some((playlists, current_playlist_id, member_of));
        self
    }

    /// Agrega el submenú "Track manager" (metadatos / análisis / descarga).
    pub fn with_tools(mut self, is_downloaded: bool) -> Self {
        self.is_downloaded = Some(is_downloaded);
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
            ContextMenuItem::new("Reproducir después", TrackContextAction::FrontEnqueue)
                .icon(Icon::AddQueueFront),
            ContextMenuItem::new("Agregar a la cola", TrackContextAction::Enqueue)
                .icon(Icon::AddQueue),
            ContextMenuItem::new("Iniciar radio", TrackContextAction::StartRadio)
                .icon(Icon::Radio),
            self.like_item(),
            self.add_to_playlist_item(),
            ContextMenuItem::new("Copiar ID", TrackContextAction::CopyId)
                .icon(Icon::Copiar),
            ContextMenuItem::new("Copiar link de YouTube", TrackContextAction::CopyYoutubeLink)
                .icon(Icon::Link),
        ];

        if let Some(is_downloaded) = self.is_downloaded {
            items.push(tools_item(is_downloaded));
        }

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
            Some((playlists, current_id, member_of)) => playlists
                .iter()
                .filter(|(id, _)| Some(id.as_str()) != current_id && !member_of.contains(id.as_str()))
                .map(|(id, name)| {
                    ContextMenuItem::new(name.clone(), TrackContextAction::AddToPlaylist(id.clone()))
                        .icon(Icon::Playlist)
                        .tint(cover_palette::accent(cover_palette::playlist_color(id)))
                })
                .collect(),
            None => Vec::new(),
        };

        ContextMenuItem::submenu("Agregar a playlist", ADD_TO_PLAYLIST_SUBMENU_ID, children)
            .icon(Icon::Playlist)
    }
}

/// Submenú de mantenimiento; letra y análisis solo tienen sentido con audio en disco.
fn tools_item(is_downloaded: bool) -> ContextMenuItem<TrackContextAction> {
    let mut children = vec![
        ContextMenuItem::new("Actualizar metadatos", TrackContextAction::Tool(TrackTool::RefreshMetadata))
            .icon(Icon::EditMetadata),
    ];

    if is_downloaded {
        children.push(
            ContextMenuItem::new("Buscar letra de nuevo", TrackContextAction::Tool(TrackTool::RefreshLyrics))
                .icon(Icon::Lyrics),
        );
        children.push(
            ContextMenuItem::new("Re-analizar BPM y key", TrackContextAction::Tool(TrackTool::Reanalyze))
                .icon(Icon::Analyze),
        );
    }

    let download_label = if is_downloaded { "Descargar de nuevo" } else { "Descargar" };
    children.push(
        ContextMenuItem::new(download_label, TrackContextAction::Tool(TrackTool::Redownload))
            .icon(Icon::Download),
    );

    ContextMenuItem::submenu("Track manager", TRACK_TOOLS_SUBMENU_ID, children)
        .icon(Icon::Tools)
}
