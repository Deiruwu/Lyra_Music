pub mod atelier;
pub mod palette;
pub mod semantic;

pub use semantic::Semantic;

/// Tema activo de la aplicación.
pub fn theme() -> &'static Semantic {
    &atelier::ATELIER
}

#[cfg(test)]
mod tests;
