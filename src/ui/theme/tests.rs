//! Fija la paleta vigente: cada token vale exactamente el color que el tema
//! le asigna hoy. Es la red que detecta un remapeo torcido entre los ~170
//! puntos de color repartidos por `src/ui/`.

use super::theme;
use iced::Color;

#[test]
fn background_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.background.app, Color::from_rgb(0.10, 0.10, 0.10));
    assert_eq!(t.background.surface, Color::from_rgb(0.15, 0.15, 0.15));
}

#[test]
fn surface_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.surface.elevated, Color::from_rgb(0.11, 0.11, 0.11));
    assert_eq!(t.surface.panel, Color::from_rgb(0.12, 0.12, 0.12));
    assert_eq!(t.surface.raised, Color::from_rgb(0.14, 0.14, 0.14));
    assert_eq!(t.surface.field, Color::from_rgb(0.15, 0.15, 0.15));
    assert_eq!(t.surface.control, Color::from_rgb(0.20, 0.20, 0.20));
    assert_eq!(t.surface.placeholder, Color::from_rgb(0.18, 0.18, 0.18));
    assert_eq!(t.surface.gradient_start, Color::from_rgb(0.22, 0.16, 0.28));
    assert_eq!(t.surface.gradient_end, Color::from_rgb(0.09, 0.09, 0.09));
}

#[test]
fn content_tokens_match_palette() {
    let t = theme();
    let bone = Color::from_rgb(0.957, 0.925, 0.863);
    let soft = Color::from_rgb(0.77, 0.73, 0.69);
    let warm = Color::from_rgb(0.573, 0.514, 0.455);
    let faint = Color::from_rgb(0.43, 0.40, 0.36);

    assert_eq!(t.content.primary, bone);
    assert_eq!(t.content.primary_alt, bone);
    assert_eq!(t.content.active, bone);
    assert_eq!(t.content.secondary, soft);
    assert_eq!(t.content.secondary_alt, soft);
    assert_eq!(t.content.secondary_alt2, soft);
    assert_eq!(t.content.tertiary, warm);
    assert_eq!(t.content.tertiary_alt, warm);
    assert_eq!(t.content.tertiary_alt2, warm);
    assert_eq!(t.content.muted, warm);
    assert_eq!(t.content.muted_alt, warm);
    assert_eq!(t.content.muted_alt2, warm);
    assert_eq!(t.content.faint, faint);
    assert_eq!(t.content.faint_alt, faint);
    assert_eq!(t.content.disabled, warm);
    assert_eq!(t.content.disabled_alt, warm);
    assert_eq!(t.content.disabled_alt2, warm);
    assert_eq!(t.content.on_accent, Color::BLACK);
    assert_eq!(t.content.on_banner, Color::from_rgba(1.0, 1.0, 1.0, 0.75));
    assert_eq!(t.content.on_control_disabled, Color::from_rgba(1.0, 1.0, 1.0, 0.3));
}

#[test]
fn border_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.border.subtle, Color::from_rgba(1.0, 1.0, 1.0, 0.12));
    assert_eq!(t.border.field, Color::from_rgb(0.3, 0.3, 0.3));
    assert_eq!(t.border.drag, Color::from_rgba(1.0, 1.0, 1.0, 0.15));
}

#[test]
fn accent_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.accent.primary, Color::from_rgb(0.62, 0.50, 0.84));
    assert_eq!(t.accent.hover, Color::from_rgb(0.72, 0.63, 0.91));
    assert_eq!(t.accent.strong, Color::from_rgb(0.49, 0.37, 0.72));
    assert_eq!(t.accent.strong_hover, Color::from_rgb(0.55, 0.43, 0.78));
    assert_eq!(t.accent.control_active, Color::from_rgb(0.62, 0.50, 0.84));
}

#[test]
fn overlay_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.overlay.hover_subtle, Color::from_rgba(1.0, 1.0, 1.0, 0.03));
    assert_eq!(t.overlay.hover_row, Color::from_rgba(1.0, 1.0, 1.0, 0.06));
    assert_eq!(t.overlay.hover_item, Color::from_rgba(1.0, 1.0, 1.0, 0.10));
    assert_eq!(t.overlay.selected, Color::from_rgba(1.0, 1.0, 1.0, 0.08));
    assert_eq!(t.overlay.card_idle, Color::from_rgba(1.0, 1.0, 1.0, 0.025));
    assert_eq!(t.overlay.card_hover, Color::from_rgba(1.0, 1.0, 1.0, 0.05));
    assert_eq!(t.overlay.card_border_idle, Color::from_rgba(1.0, 1.0, 1.0, 0.10));
    assert_eq!(t.overlay.card_border_hover, Color::from_rgba(1.0, 1.0, 1.0, 0.16));
    assert_eq!(t.overlay.control_idle, Color::from_rgba(1.0, 1.0, 1.0, 0.1));
    assert_eq!(t.overlay.control_hover, Color::from_rgba(1.0, 1.0, 1.0, 0.18));
    assert_eq!(t.overlay.control_disabled, Color::from_rgba(1.0, 1.0, 1.0, 0.04));
    assert_eq!(t.overlay.toggle_idle, Color::from_rgba(1.0, 1.0, 1.0, 0.08));
    assert_eq!(t.overlay.toggle_hover, Color::from_rgba(1.0, 1.0, 1.0, 0.14));
    assert_eq!(t.overlay.toggle_on_idle, Color::from_rgba(1.0, 1.0, 1.0, 0.22));
    assert_eq!(t.overlay.toggle_on_hover, Color::from_rgba(1.0, 1.0, 1.0, 0.28));
    assert_eq!(t.overlay.scrim_cover, Color::from_rgba(0.0, 0.0, 0.0, 0.4));
    assert_eq!(t.overlay.scrim_strong, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.55 });
    assert_eq!(t.overlay.scrim_play, Color::from_rgba(0.0, 0.0, 0.0, 0.6));
}

#[test]
fn elevation_matches_palette() {
    let t = theme();
    assert_eq!(t.elevation.shadow.color, Color::from_rgba(0.0, 0.0, 0.0, 0.45));
    assert_eq!(t.elevation.shadow.offset, iced::Vector::new(0.0, 8.0));
    assert_eq!(t.elevation.shadow.blur_radius, 32.0);
}

#[test]
fn status_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.status.liked, Color::from_rgb(0.91, 0.39, 0.62));
    assert_eq!(t.status.cached, Color::from_rgb(0.50, 0.60, 0.60));
    assert_eq!(t.status.error, Color::from_rgb(0.73, 0.49, 0.49));
}
