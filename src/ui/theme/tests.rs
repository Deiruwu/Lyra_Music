//! Fija la paleta vigente: cada token vale exactamente el color que el tema
//! le asigna hoy. Es la red que detecta un remapeo torcido entre los puntos
//! de color repartidos por `src/ui/`.

use super::theme;
use iced::Color;

/// Escalones de elevación, del más hundido al más prominente.
#[test]
fn surface_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.surface.base, Color::from_rgb(0.10, 0.10, 0.13));
    assert_eq!(t.surface.panel, Color::from_rgb(0.14, 0.14, 0.17));
    assert_eq!(t.surface.sunken, Color::from_rgb(0.18, 0.18, 0.21));
    assert_eq!(t.surface.control, Color::from_rgb(0.24, 0.24, 0.27));
    assert_eq!(t.surface.gradient_start, Color::from_rgb(0.22, 0.16, 0.28));
}

/// La escala de elevación debe subir de forma monótona y con saltos visibles.
#[test]
fn surface_ladder_is_monotonic() {
    let t = theme();
    let ladder = [t.surface.base, t.surface.panel, t.surface.sunken, t.surface.control];

    for pair in ladder.windows(2) {
        let step = pair[1].r - pair[0].r;
        assert!(step >= 0.04, "salto de elevación imperceptible: {step}");
    }
}

#[test]
fn content_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.content.primary, Color::from_rgb(0.90, 0.91, 0.94));
    assert_eq!(t.content.secondary, Color::from_rgb(0.74, 0.75, 0.78));
    assert_eq!(t.content.muted, Color::from_rgb(0.56, 0.57, 0.60));
    assert_eq!(t.content.faint, Color::from_rgb(0.40, 0.41, 0.44));
    assert_eq!(t.content.on_accent, Color::BLACK);
    assert_eq!(t.content.on_banner, Color::from_rgba(1.0, 1.0, 1.0, 0.75));
    assert_eq!(t.content.on_control_disabled, Color::from_rgba(1.0, 1.0, 1.0, 0.3));
}

/// La escalera de texto baja de forma monótona: cada escalón se distingue del
/// anterior o deja de ser un escalón.
#[test]
fn content_ladder_is_monotonic() {
    let t = theme();
    let ladder = [t.content.primary, t.content.secondary, t.content.muted, t.content.faint];

    for pair in ladder.windows(2) {
        let step = pair[0].r - pair[1].r;
        assert!(step >= 0.10, "salto de texto imperceptible: {step}");
    }
}

#[test]
fn border_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.border.subtle, Color::from_rgba(1.0, 1.0, 1.0, 0.12));
    assert_eq!(t.border.field, Color::from_rgb(0.30, 0.30, 0.33));
}

#[test]
fn accent_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.accent.primary, Color::from_rgb(0.62, 0.50, 0.84));
    assert_eq!(t.accent.hover, Color::from_rgb(0.72, 0.63, 0.91));
    assert_eq!(t.accent.strong, Color::from_rgb(0.49, 0.37, 0.72));
    assert_eq!(t.accent.strong_hover, Color::from_rgb(0.55, 0.43, 0.78));
}

#[test]
fn overlay_tokens_match_palette() {
    let t = theme();
    assert_eq!(t.overlay.hover, Color::from_rgba(1.0, 1.0, 1.0, 0.06));
    assert_eq!(t.overlay.hover_accent, Color::from_rgba(0.62, 0.50, 0.84, 0.14));
    assert_eq!(t.overlay.selected, Color::from_rgba(0.62, 0.50, 0.84, 0.20));
    assert_eq!(t.overlay.resting, Color::from_rgba(1.0, 1.0, 1.0, 0.03));
    assert_eq!(t.overlay.control_idle, Color::from_rgba(1.0, 1.0, 1.0, 0.12));
    assert_eq!(t.overlay.control_hover, Color::from_rgba(1.0, 1.0, 1.0, 0.16));
    assert_eq!(t.overlay.toggle_on_idle, Color::from_rgba(1.0, 1.0, 1.0, 0.22));
    assert_eq!(t.overlay.toggle_on_hover, Color::from_rgba(1.0, 1.0, 1.0, 0.28));
    assert_eq!(t.overlay.scrim, Color::from_rgba(0.0, 0.0, 0.0, 0.55));
}

/// Selección y hover no deben competir en el mismo canal: el hover es una
/// veladura blanca efímera y la selección un tinte de acento persistente.
#[test]
fn selection_is_separated_from_hover_by_hue() {
    let t = theme();
    let hover = t.overlay.hover;
    let selected = t.overlay.selected;

    assert_eq!(hover.r, hover.b, "el hover debe ser neutro");
    assert!(
        selected.b - selected.r > 0.15,
        "la selección debe llevar tinte de acento, no ser neutra"
    );
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
