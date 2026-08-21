use iced::widget::text::Wrapping;
use iced::widget::{container, text};
use iced::{alignment::Horizontal, Color, Element, Font, Length};

/// Ancho promedio de un glifo como fracción del tamaño de fuente, calibrado
/// para las fuentes proporcionales usadas en la app (SF Pro / PRO_DISPLAY).
const AVG_GLYPH_WIDTH_RATIO: f32 = 0.55;

fn truncate_to_px(content: &str, max_px: f32, size: f32) -> String {
    let avg_glyph_width = (size * AVG_GLYPH_WIDTH_RATIO).max(1.0);
    let max_chars = ((max_px / avg_glyph_width).floor() as usize).max(1);

    if content.chars().count() <= max_chars {
        content.to_string()
    } else {
        let keep = max_chars.saturating_sub(1).max(1);
        format!("{}…", content.chars().take(keep).collect::<String>())
    }
}

/// Texto de una sola línea que nunca se desborda hacia widgets vecinos ni se
/// apila en una segunda línea.
///
/// Si `width` es un ancho fijo (`Length::Fixed`), el texto se trunca con
/// "…" estimando cuántos caracteres entran en ese ancho. Para anchos
/// dinámicos (`Fill`/`FillPortion`, cuyo tamaño real no se conoce en este
/// punto) no se puede estimar el corte, así que solo se aplica el `clip`
/// como garantía final contra el desborde — sigue siendo una sola línea,
/// simplemente sin elipsis.
pub fn single_line_text<'a, Message: 'a>(
    content: impl AsRef<str>,
    font: Font,
    size: f32,
    color: Color,
    width: Length,
) -> Element<'a, Message> {
    single_line_text_aligned(content, font, size, color, width, Horizontal::Left)
}

/// Igual que [`single_line_text`], pero alineando el texto dentro de su caja.
pub fn single_line_text_aligned<'a, Message: 'a>(
    content: impl AsRef<str>,
    font: Font,
    size: f32,
    color: Color,
    width: Length,
    align_x: Horizontal,
) -> Element<'a, Message> {
    let display = match width {
        Length::Fixed(max_px) => truncate_to_px(content.as_ref(), max_px, size),
        _ => content.as_ref().to_string(),
    };

    // El texto interno hereda el mismo `width` que la caja que lo envuelve
    // (en vez de forzar `Fill`): con `Length::Shrink` esto deja que ambos se
    // midan por el contenido real, para que lo que venga después en un
    // `row!` (p.ej. el botón de like del track actual) quede pegado al
    // texto en vez del borde de una caja de ancho fijo/flexible. Para
    // Fill/Fixed/FillPortion el resultado es idéntico al de antes.
    container(
        text(display)
            .font(font)
            .size(size)
            .color(color)
            .width(width)
            .align_x(align_x)
            .wrapping(Wrapping::None),
    )
    .width(width)
    .clip(true)
    .into()
}
