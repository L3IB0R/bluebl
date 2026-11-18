//! Sistema de temas: paletas nombradas por atmósfera, organizadas en
//! categorías y fáciles de extender.
//!
//! Cada tema define siete colores base; el resto (bordes, atenuados, el
//! resaltado…) se derivan por mezcla para que sumar un tema nuevo sea una
//! sola línea.

use ratatui::style::Color;

/// Categoría de un tema.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Base,
    Terminales,
    Juegos,
    Cine,
}

impl Category {
    pub fn name(&self) -> &'static str {
        match self {
            Category::Base => "Base y accesibilidad",
            Category::Terminales => "Terminales clásicos",
            Category::Juegos => "Videojuegos",
            Category::Cine => "Cine",
        }
    }

    pub const ALL: [Category; 4] = [
        Category::Base,
        Category::Terminales,
        Category::Juegos,
        Category::Cine,
    ];
}

/// Colores derivados por mezcla.
fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color::Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

const fn rgb(c: (u8, u8, u8)) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

/// Paleta completa de un tema.
#[derive(Clone, Debug)]
pub struct Palette {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub accent2: Color,
    pub dim: Color,
    pub border: Color,
    pub border_active: Color,
    pub input_bg: Color,
    pub system: Color,
    pub self_msg: Color,
    pub peer_msg: Color,
    pub error: Color,
    pub highlight: Color,
    pub status_bg: Color,
}

/// Tema completo.
#[derive(Clone, Debug)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub palette: Palette,
}

impl Theme {
    /// Construye un tema a partir de sus siete colores base; el resto
    /// (bordes, atenuados, resaltados) se deriva por mezcla.
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: &'static str,
        name: &'static str,
        category: Category,
        bg: (u8, u8, u8),
        fg: (u8, u8, u8),
        accent: (u8, u8, u8),
        accent2: (u8, u8, u8),
        error: (u8, u8, u8),
    ) -> Theme {
        Theme {
            id,
            name,
            category,
            palette: Palette {
                bg: rgb(bg),
                fg: rgb(fg),
                accent: rgb(accent),
                accent2: rgb(accent2),
                dim: mix(fg, bg, 0.55),
                border: mix(fg, bg, 0.65),
                border_active: rgb(accent),
                input_bg: mix(bg, fg, 0.08),
                system: rgb(accent2),
                self_msg: rgb(accent),
                peer_msg: rgb(fg),
                error: rgb(error),
                highlight: mix(accent, bg, 0.25),
                status_bg: mix(bg, fg, 0.06),
            },
        }
    }
}

/// Todas las paletas, en orden estable.
pub fn all_themes() -> Vec<Theme> {
    let mut list: Vec<Theme> = Vec::with_capacity(48);

    // ── Base y accesibilidad ────────────────────────────────────────────
    list.push(Theme::new(
        "oscuro",
        "Oscuro",
        Category::Base,
        (18, 20, 28),
        (220, 224, 232),
        (110, 168, 254),
        (120, 200, 160),
        (230, 90, 90),
    ));
    list.push(Theme::new(
        "claro",
        "Claro",
        Category::Base,
        (246, 246, 242),
        (36, 38, 44),
        (30, 90, 200),
        (20, 130, 90),
        (190, 40, 40),
    ));
    list.push(Theme::new(
        "contraste-alto",
        "Alto contraste",
        Category::Base,
        (0, 0, 0),
        (255, 255, 255),
        (255, 255, 0),
        (0, 255, 255),
        (255, 70, 70),
    ));
    list.push(Theme::new(
        "contraste-alto-2",
        "Alto contraste (ámbar)",
        Category::Base,
        (0, 0, 0),
        (255, 220, 150),
        (255, 200, 60),
        (120, 230, 255),
        (255, 90, 90),
    ));
    list.push(Theme::new(
        "neutro-suave",
        "Neutro suave",
        Category::Base,
        (32, 33, 36),
        (210, 210, 208),
        (150, 160, 175),
        (140, 175, 155),
        (220, 110, 110),
    ));
    list.push(Theme::new(
        "monocromo",
        "Monocromo",
        Category::Base,
        (12, 12, 12),
        (205, 205, 205),
        (250, 250, 250),
        (150, 150, 150),
        (230, 230, 230),
    ));

    // ── Terminales clásicos ─────────────────────────────────────────────
    list.push(Theme::new(
        "fosforo-verde",
        "Fósforo verde",
        Category::Terminales,
        (4, 20, 8),
        (80, 255, 120),
        (140, 255, 160),
        (60, 220, 140),
        (255, 90, 90),
    ));
    list.push(Theme::new(
        "fosforo-ambar",
        "Fósforo ámbar",
        Category::Terminales,
        (24, 14, 4),
        (255, 190, 80),
        (255, 215, 120),
        (255, 160, 60),
        (255, 90, 90),
    ));
    list.push(Theme::new(
        "solarized-dark",
        "Solarized Dark",
        Category::Terminales,
        (0, 43, 54),
        (147, 161, 161),
        (38, 139, 210),
        (133, 153, 0),
        (220, 50, 47),
    ));
    list.push(Theme::new(
        "solarized-light",
        "Solarized Light",
        Category::Terminales,
        (253, 246, 227),
        (101, 123, 131),
        (38, 139, 210),
        (133, 153, 0),
        (220, 50, 47),
    ));
    list.push(Theme::new(
        "dracula",
        "Dracula",
        Category::Terminales,
        (40, 42, 54),
        (248, 248, 242),
        (255, 121, 198),
        (80, 250, 123),
        (255, 85, 85),
    ));
    list.push(Theme::new(
        "gruvbox",
        "Gruvbox",
        Category::Terminales,
        (40, 40, 40),
        (235, 219, 178),
        (250, 189, 47),
        (142, 192, 124),
        (251, 73, 52),
    ));
    list.push(Theme::new(
        "nord",
        "Nord",
        Category::Terminales,
        (46, 52, 64),
        (216, 222, 233),
        (136, 192, 208),
        (163, 190, 140),
        (191, 97, 106),
    ));
    list.push(Theme::new(
        "monokai",
        "Monokai",
        Category::Terminales,
        (39, 40, 34),
        (248, 248, 242),
        (166, 226, 46),
        (174, 129, 255),
        (249, 38, 114),
    ));
    list.push(Theme::new(
        "tokyo-night",
        "Tokyo Night",
        Category::Terminales,
        (26, 27, 38),
        (169, 177, 214),
        (130, 170, 255),
        (158, 206, 106),
        (247, 118, 142),
    ));
    list.push(Theme::new(
        "catppuccin",
        "Catppuccin",
        Category::Terminales,
        (30, 30, 46),
        (205, 214, 244),
        (203, 166, 247),
        (166, 227, 161),
        (243, 139, 168),
    ));
    list.push(Theme::new(
        "one-dark",
        "One Dark",
        Category::Terminales,
        (40, 44, 52),
        (171, 178, 191),
        (97, 175, 239),
        (152, 195, 121),
        (224, 108, 117),
    ));
    list.push(Theme::new(
        "rose-pine",
        "Rosé Pine",
        Category::Terminales,
        (25, 23, 36),
        (224, 220, 233),
        (235, 188, 215),
        (156, 231, 191),
        (235, 111, 133),
    ));

    // ── Videojuegos ─────────────────────────────────────────────────────
    list.push(Theme::new(
        "night-city",
        "Night City",
        Category::Juegos,
        (8, 6, 20),
        (230, 225, 255),
        (255, 40, 180),
        (60, 230, 255),
        (255, 70, 70),
    ));
    list.push(Theme::new(
        "pip-boy-verde",
        "Pip-Boy verde",
        Category::Juegos,
        (6, 16, 6),
        (120, 255, 130),
        (170, 255, 175),
        (90, 230, 120),
        (255, 100, 100),
    ));
    list.push(Theme::new(
        "pip-boy-ambar",
        "Pip-Boy ámbar",
        Category::Juegos,
        (20, 13, 4),
        (255, 195, 90),
        (255, 220, 140),
        (255, 170, 70),
        (255, 95, 95),
    ));
    list.push(Theme::new(
        "mojave",
        "Mojave NCR",
        Category::Juegos,
        (26, 20, 12),
        (235, 215, 175),
        (90, 160, 220),
        (200, 70, 60),
        (240, 90, 80),
    ));
    list.push(Theme::new(
        "black-mesa",
        "Black Mesa",
        Category::Juegos,
        (14, 16, 18),
        (210, 215, 220),
        (240, 130, 40),
        (90, 190, 220),
        (235, 80, 70),
    ));
    list.push(Theme::new(
        "aperture",
        "Aperture",
        Category::Juegos,
        (242, 242, 242),
        (40, 40, 40),
        (0, 120, 210),
        (240, 150, 30),
        (200, 40, 40),
    ));
    list.push(Theme::new(
        "normandia",
        "Normandía",
        Category::Juegos,
        (24, 26, 22),
        (205, 210, 190),
        (180, 175, 140),
        (120, 150, 100),
        (210, 90, 70),
    ));
    list.push(Theme::new(
        "rapture",
        "Rapture",
        Category::Juegos,
        (6, 14, 22),
        (200, 225, 235),
        (60, 190, 210),
        (240, 180, 90),
        (235, 90, 80),
    ));
    list.push(Theme::new(
        "yharnam",
        "Yharnam",
        Category::Juegos,
        (16, 12, 14),
        (215, 205, 200),
        (170, 60, 70),
        (140, 130, 160),
        (220, 70, 70),
    ));
    list.push(Theme::new(
        "hyrule",
        "Hyrule",
        Category::Juegos,
        (10, 18, 14),
        (225, 235, 215),
        (70, 190, 120),
        (230, 190, 80),
        (225, 85, 75),
    ));
    list.push(Theme::new(
        "midgar",
        "Taller de Midgar",
        Category::Juegos,
        (18, 16, 20),
        (220, 215, 210),
        (240, 170, 60),
        (90, 170, 230),
        (235, 90, 80),
    ));
    list.push(Theme::new(
        "deus-ex",
        "DEUS EX",
        Category::Juegos,
        (10, 12, 14),
        (200, 215, 205),
        (60, 200, 150),
        (240, 190, 70),
        (235, 85, 75),
    ));
    list.push(Theme::new(
        "neon-drift",
        "Neon Drift",
        Category::Juegos,
        (12, 8, 24),
        (235, 230, 255),
        (255, 90, 220),
        (80, 255, 220),
        (255, 80, 110),
    ));
    list.push(Theme::new(
        "island",
        "Island",
        Category::Juegos,
        (10, 22, 30),
        (230, 240, 240),
        (80, 210, 200),
        (250, 200, 110),
        (240, 100, 90),
    ));

    // ── Cine ────────────────────────────────────────────────────────────
    list.push(Theme::new(
        "lluvia-neon",
        "Lluvia de neón",
        Category::Cine,
        (10, 8, 18),
        (225, 222, 240),
        (255, 60, 150),
        (70, 200, 255),
        (255, 80, 90),
    ));
    list.push(Theme::new(
        "lluvia-fria",
        "Lluvia fría",
        Category::Cine,
        (14, 18, 24),
        (205, 215, 225),
        (110, 160, 210),
        (150, 190, 180),
        (220, 95, 95),
    ));
    list.push(Theme::new(
        "spinner",
        "Spinner",
        Category::Cine,
        (6, 8, 12),
        (200, 215, 230),
        (90, 220, 255),
        (255, 170, 60),
        (255, 90, 90),
    ));
    list.push(Theme::new(
        "matrix",
        "Matrix",
        Category::Cine,
        (2, 8, 4),
        (120, 255, 140),
        (60, 255, 110),
        (170, 255, 180),
        (255, 100, 100),
    ));
    list.push(Theme::new(
        "la-red",
        "La Red",
        Category::Cine,
        (16, 6, 6),
        (235, 215, 210),
        (220, 70, 60),
        (250, 150, 90),
        (255, 90, 80),
    ));
    list.push(Theme::new(
        "nostromo",
        "Nostromo",
        Category::Cine,
        (10, 12, 10),
        (205, 215, 195),
        (235, 190, 80),
        (120, 200, 140),
        (235, 100, 80),
    ));
    list.push(Theme::new(
        "neo-tokyo",
        "Neo-Tokyo",
        Category::Cine,
        (14, 6, 16),
        (235, 225, 240),
        (255, 95, 180),
        (95, 210, 255),
        (255, 85, 100),
    ));
    list.push(Theme::new(
        "shell",
        "Shell",
        Category::Cine,
        (8, 12, 16),
        (215, 225, 235),
        (70, 190, 220),
        (160, 210, 160),
        (230, 100, 90),
    ));
    list.push(Theme::new(
        "arrakis",
        "Arrakis",
        Category::Cine,
        (24, 18, 10),
        (240, 225, 195),
        (235, 170, 80),
        (120, 190, 160),
        (230, 90, 70),
    ));
    list.push(Theme::new(
        "nebulosa",
        "Nebulosa",
        Category::Cine,
        (12, 10, 24),
        (225, 222, 245),
        (150, 120, 255),
        (90, 210, 235),
        (240, 95, 130),
    ));

    list
}

/// Busca un tema por id exacto.
pub fn find(id: &str) -> Option<Theme> {
    all_themes().into_iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hay_al_menos_cuarenta_temas() {
        assert!(
            all_themes().len() >= 40,
            "sólo hay {} temas",
            all_themes().len()
        );
    }

    #[test]
    fn ids_unicos() {
        let themes = all_themes();
        for (i, a) in themes.iter().enumerate() {
            for b in themes.iter().skip(i + 1) {
                assert_ne!(a.id, b.id, "id duplicado: {}", a.id);
                assert_ne!(a.name, b.name, "nombre duplicado: {}", a.name);
            }
        }
    }

    #[test]
    fn categorias_no_vacias() {
        for cat in Category::ALL {
            assert!(
                all_themes().iter().any(|t| t.category == cat),
                "categoría vacía: {}",
                cat.name()
            );
        }
    }

    #[test]
    fn find_por_id() {
        assert!(find("dracula").is_some());
        assert!(find("no-existe").is_none());
        let t = find("oscuro").unwrap();
        assert_eq!(t.name, "Oscuro");
    }

    #[test]
    fn derivados_usan_el_bg() {
        let t = find("claro").unwrap();
        // En un tema claro el borde debe ser más suave que el texto.
        assert_ne!(t.palette.border, t.palette.fg);
        assert_ne!(t.palette.dim, t.palette.fg);
    }
}
