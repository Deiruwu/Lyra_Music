//! Editor modal para elegir el recorte cuadrado de una portada de playlist:
//! la imagen con un recuadro que se arrastra con el mouse y se agranda o
//! achica con la rueda o el slider.

use std::path::PathBuf;

use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::image::Handle;
use iced::widget::{button, canvas as canvas_widget, column, container, image, opaque, row, slider, stack, text};
use iced::{Alignment, Color, ContentFit, Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::styles::container as container_style;
use crate::ui::styles::slider as slider_style;
use crate::ui::theme::theme;
use crate::ui::utils::image::CropRegion;

/// Lado del área donde se pinta la imagen (cabe entera, centrada).
const CANVAS_SIZE: f32 = 420.0;
/// El recuadro no baja de esta fracción del lado menor de la imagen.
const MIN_SIDE_FRACTION: f32 = 0.2;
/// Cuánto cambia el recuadro por cada paso de la rueda.
const WHEEL_ZOOM_STEP: f32 = 0.08;
const SELECTION_STROKE: f32 = 2.0;
/// Opacidad del velo sobre lo que queda fuera del recuadro.
const OUTSIDE_SHADE_ALPHA: f32 = 0.75;

#[derive(Debug, Clone)]
pub enum CropEditorMessage {
    /// Nueva esquina superior izquierda del recuadro, en píxeles de la imagen original.
    Moved(Point),
    /// Factor de escala del recuadro (rueda del mouse), manteniendo el centro.
    Zoomed(f32),
    /// Tamaño del recuadro como fracción del lado menor (slider).
    SizeChanged(f32),
    Save,
    Cancel,
}

/// Resultado de procesar un mensaje del editor.
pub enum CropEditorOutcome {
    Editing,
    /// `target`: a qué pertenece el recorte (id de playlist, perfil…).
    Save { target: String, source_path: PathBuf, region: CropRegion },
    Cancel,
}

pub struct CoverCropEditor {
    target: String,
    title: &'static str,
    source_path: PathBuf,
    preview: Handle,
    /// Tamaño de la imagen original (la previsualización puede estar reducida).
    image_size: Size,
    /// Recuadro en píxeles de la imagen original.
    origin: Point,
    side: f32,
}

impl CoverCropEditor {
    /// Abre el editor con el recuadro más grande posible, centrado (el recorte de siempre).
    pub fn new(target: String, source_path: PathBuf, preview_bytes: Vec<u8>, width: u32, height: u32) -> Self {
        let image_size = Size::new(width.max(1) as f32, height.max(1) as f32);
        let side = image_size.width.min(image_size.height);
        let origin = Point::new((image_size.width - side) / 2.0, (image_size.height - side) / 2.0);

        Self {
            target,
            title: "Recortar portada",
            source_path,
            preview: Handle::from_bytes(preview_bytes),
            image_size,
            origin,
            side,
        }
    }

    pub fn with_title(mut self, title: &'static str) -> Self {
        self.title = title;
        self
    }

    pub fn update(&mut self, message: CropEditorMessage) -> CropEditorOutcome {
        match message {
            CropEditorMessage::Moved(origin) => self.move_to(origin),
            CropEditorMessage::Zoomed(factor) => self.resize(self.side * factor),
            CropEditorMessage::SizeChanged(fraction) => self.resize(fraction * self.max_side()),
            CropEditorMessage::Save => {
                return CropEditorOutcome::Save {
                    target: self.target.clone(),
                    source_path: self.source_path.clone(),
                    region: CropRegion {
                        x: self.origin.x.round() as u32,
                        y: self.origin.y.round() as u32,
                        side: self.side.round() as u32,
                    },
                };
            }
            CropEditorMessage::Cancel => return CropEditorOutcome::Cancel,
        }
        CropEditorOutcome::Editing
    }

    pub fn view(&self) -> Element<'_, CropEditorMessage> {
        let title = text(self.title).font(SF_PRO).size(typography::TEXT_16).color(theme().content.primary);
        let hint = text("Arrastra el recuadro; la rueda o el slider cambian su tamaño.")
            .font(SF_PRO)
            .size(typography::TEXT_12)
            .color(theme().content.muted);

        // La imagen va como widget debajo: dentro de un canvas, iced pinta las
        // imágenes después de las figuras y taparía el velo y el recuadro.
        let picture = image(self.preview.clone())
            .width(Length::Fixed(CANVAS_SIZE))
            .height(Length::Fixed(CANVAS_SIZE))
            .content_fit(ContentFit::Contain);
        let overlay = canvas_widget(CropCanvas { editor: self })
            .width(Length::Fixed(CANVAS_SIZE))
            .height(Length::Fixed(CANVAS_SIZE));
        let crop_area = stack![picture, overlay];

        let size_row = row![
            text("Tamaño").font(SF_PRO).size(typography::TEXT_12).color(theme().content.secondary),
            slider(MIN_SIDE_FRACTION..=1.0, self.side / self.max_side(), CropEditorMessage::SizeChanged)
                .step(0.01)
                .style(slider_style::track),
        ]
            .spacing(spacing::SP_12)
            .align_y(Alignment::Center)
            .width(Length::Fixed(CANVAS_SIZE));

        let action = |label: &'static str, color, message| {
            button(text(label).font(SF_PRO).size(typography::TEXT_13).color(color))
                .style(button_style::context_menu_item)
                .padding(Padding { top: spacing::SP_8, bottom: spacing::SP_8, left: spacing::SP_18, right: spacing::SP_18 })
                .on_press(message)
        };
        let buttons = row![
            action("Cancelar", theme().content.secondary, CropEditorMessage::Cancel),
            action("Guardar", theme().content.primary, CropEditorMessage::Save),
        ]
            .spacing(spacing::SP_10);

        let card = container(
            column![
                column![title, hint].spacing(spacing::SP_4),
                crop_area,
                size_row,
                container(buttons).width(Length::Fixed(CANVAS_SIZE)).align_x(Alignment::End),
            ]
                .spacing(spacing::SP_16),
        )
            .padding(spacing::SP_24)
            .style(container_style::context_menu);

        // Modal: el fondo bloquea la app pero no cierra (se pierde el recorte por un clic de más).
        opaque(
            container(opaque(card))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_| container::Style {
                    background: Some(theme().overlay.scrim.into()),
                    ..Default::default()
                }),
        )
    }

    fn max_side(&self) -> f32 {
        self.image_size.width.min(self.image_size.height)
    }

    fn move_to(&mut self, origin: Point) {
        self.origin = Point::new(
            origin.x.clamp(0.0, self.image_size.width - self.side),
            origin.y.clamp(0.0, self.image_size.height - self.side),
        );
    }

    /// Cambia el lado manteniendo el centro del recuadro.
    fn resize(&mut self, side: f32) {
        let max = self.max_side();
        let side = side.clamp((max * MIN_SIDE_FRACTION).max(1.0), max);
        let center = self.origin + Vector::new(self.side / 2.0, self.side / 2.0);
        self.side = side;
        self.move_to(center - Vector::new(side / 2.0, side / 2.0));
    }
}

/// Dónde cae la imagen dentro del canvas y a qué escala.
struct Placement {
    offset: Vector,
    scale: f32,
}

impl Placement {
    fn of(image_size: Size, bounds: Size) -> Self {
        let scale = (bounds.width / image_size.width).min(bounds.height / image_size.height);
        let offset = Vector::new(
            (bounds.width - image_size.width * scale) / 2.0,
            (bounds.height - image_size.height * scale) / 2.0,
        );
        Self { offset, scale }
    }

    /// Punto del canvas → píxel de la imagen original.
    fn to_image(&self, point: Point) -> Point {
        Point::new((point.x - self.offset.x) / self.scale, (point.y - self.offset.y) / self.scale)
    }

    fn to_canvas(&self, point: Point) -> Point {
        Point::new(point.x * self.scale + self.offset.x, point.y * self.scale + self.offset.y)
    }
}

struct CropCanvas<'a> {
    editor: &'a CoverCropEditor,
}

impl CropCanvas<'_> {
    fn selection_contains(&self, image_point: Point) -> bool {
        let e = self.editor;
        image_point.x >= e.origin.x
            && image_point.x <= e.origin.x + e.side
            && image_point.y >= e.origin.y
            && image_point.y <= e.origin.y + e.side
    }
}

impl canvas::Program<CropEditorMessage> for CropCanvas<'_> {
    /// Desplazamiento entre el cursor y la esquina del recuadro mientras se arrastra.
    type State = Option<Vector>;

    fn update(
        &self,
        grab: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<CropEditorMessage>> {
        let placement = Placement::of(self.editor.image_size, bounds.size());

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let image_point = placement.to_image(cursor.position_in(bounds)?);
                let offset = if self.selection_contains(image_point) {
                    image_point - self.editor.origin
                } else {
                    // Clic fuera del recuadro: lo centra ahí y empieza a arrastrarlo.
                    Vector::new(self.editor.side / 2.0, self.editor.side / 2.0)
                };
                *grab = Some(offset);
                Some(canvas::Action::publish(CropEditorMessage::Moved(image_point - offset)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let offset = (*grab)?;
                let image_point = placement.to_image(cursor.position_from(bounds.position())?);
                Some(canvas::Action::publish(CropEditorMessage::Moved(image_point - offset)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                grab.take().map(|_| canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if y == 0.0 {
                    return None;
                }
                let factor = if y > 0.0 { 1.0 - WHEEL_ZOOM_STEP } else { 1.0 + WHEEL_ZOOM_STEP };
                Some(canvas::Action::publish(CropEditorMessage::Zoomed(factor)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, _grab: &Self::State, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let editor = self.editor;
        let placement = Placement::of(editor.image_size, bounds.size());
        let mut frame = Frame::new(renderer, bounds.size());

        let image_top_left = placement.to_canvas(Point::ORIGIN);
        let image_bottom_right = placement.to_canvas(Point::new(editor.image_size.width, editor.image_size.height));
        let image_rect = Rectangle::new(image_top_left, Size::new(image_bottom_right.x - image_top_left.x, image_bottom_right.y - image_top_left.y));

        let sel_top_left = placement.to_canvas(editor.origin);
        let sel_side = editor.side * placement.scale;
        let sel_bottom = sel_top_left.y + sel_side;
        let sel_right = sel_top_left.x + sel_side;

        // Oscurece lo que queda fuera del recuadro.
        let veil = Color { a: OUTSIDE_SHADE_ALPHA, ..theme().overlay.scrim };
        let shade = |frame: &mut Frame, x: f32, y: f32, w: f32, h: f32| {
            if w > 0.0 && h > 0.0 {
                frame.fill_rectangle(Point::new(x, y), Size::new(w, h), veil);
            }
        };
        shade(&mut frame, image_rect.x, image_rect.y, image_rect.width, sel_top_left.y - image_rect.y);
        shade(&mut frame, image_rect.x, sel_bottom, image_rect.width, image_bottom_right.y - sel_bottom);
        shade(&mut frame, image_rect.x, sel_top_left.y, sel_top_left.x - image_rect.x, sel_side);
        shade(&mut frame, sel_right, sel_top_left.y, image_bottom_right.x - sel_right, sel_side);

        // Tercios como guía de composición.
        let guide = Stroke::default().with_width(1.0).with_color(theme().border.subtle);
        for i in 1..3 {
            let t = sel_side * i as f32 / 3.0;
            frame.stroke(&Path::line(Point::new(sel_top_left.x + t, sel_top_left.y), Point::new(sel_top_left.x + t, sel_bottom)), guide);
            frame.stroke(&Path::line(Point::new(sel_top_left.x, sel_top_left.y + t), Point::new(sel_right, sel_top_left.y + t)), guide);
        }

        frame.stroke(
            &Path::rectangle(sel_top_left, Size::new(sel_side, sel_side)),
            Stroke::default().with_width(SELECTION_STROKE).with_color(theme().accent.primary),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, grab: &Self::State, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if grab.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let Some(position) = cursor.position_in(bounds) else { return mouse::Interaction::default() };
        let image_point = Placement::of(self.editor.image_size, bounds.size()).to_image(position);
        if self.selection_contains(image_point) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::Crosshair
        }
    }
}
