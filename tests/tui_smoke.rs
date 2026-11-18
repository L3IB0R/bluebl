//! Humo de interfaz: la TUI se dibuja en tres tamaños de pantalla y lo
//! esencial aparece (cabecera, canales, chat, entrada y barra de estado).

use bluebl::app::App;
use bluebl::config::Config;
use bluebl::mesh::offline;
use bluebl::ui;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::sync::mpsc::Sender;

fn app() -> (App, Sender<bluebl::mesh::MeshEvent>) {
    let (tx, rx) = offline();
    let cfg = Config {
        nick: "akaomi".into(),
        ..Config::default()
    };
    (App::new(cfg, rx), tx)
}

/// Renderiza y devuelve el buffer como texto plano.
fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        out.push('\n');
    }
    out
}

#[test]
fn pantalla_ancha_muestra_los_dos_paneles() {
    let (mut app, _tx) = app();
    let screen = render(&mut app, 120, 30);
    assert!(screen.contains("BlueBl"), "falta la cabecera:\n{screen}");
    assert!(screen.contains("#general"), "falta el canal");
    assert!(screen.contains("Canales"), "falta el panel de canales");
    assert!(screen.contains("Pares"), "falta el panel de pares");
    assert!(screen.contains("bienvenida"), "falta el mensaje del chat");
    assert!(screen.contains("mensaje"), "falta el cuadro de entrada");
    assert!(screen.contains("pares"), "falta la barra de estado");
}

#[test]
fn pantalla_estrecha_colapsa_a_vista_unica() {
    let (mut app, _tx) = app();
    app.panel = bluebl::app::Panel::Chat;
    let screen = render(&mut app, 70, 24);
    assert!(screen.contains("BlueBl"));
    assert!(screen.contains("#general"));
    // Sin los paneles laterales dobles: el chat ocupa todo.
    assert!(
        !screen.contains("Pares"),
        "no debe haber panel lateral:\n{screen}"
    );
}

#[test]
fn movil_minusculas_sigue_funcionando() {
    let (mut app, _tx) = app();
    // Teléfono de 36 columnas: nada debe romperse ni desbordar.
    let screen = render(&mut app, 36, 20);
    assert!(screen.contains("#general"));
    assert!(screen.contains("bienvenida"));
}

#[test]
fn el_selector_de_temas_se_pinta() {
    let (mut app, _tx) = app();
    app.overlay = Some(bluebl::app::Overlay::Themes { selected: 0 });
    let screen = render(&mut app, 100, 34);
    assert!(
        screen.contains("temas"),
        "falta el overlay de temas:\n{screen}"
    );
    assert!(screen.contains("Dracula"), "faltan los temas en la lista");
    assert!(
        screen.contains("Videojuegos"),
        "falta la categoría de juegos"
    );
}

#[test]
fn la_ayuda_se_pinta() {
    let (mut app, _tx) = app();
    app.overlay = Some(bluebl::app::Overlay::Help);
    let screen = render(&mut app, 100, 30);
    assert!(screen.contains("/join"), "faltan los comandos:\n{screen}");
    assert!(screen.contains("/density"));
}

#[test]
fn cambio_de_tema_cambia_la_paleta() {
    let (mut app, _tx) = app();
    let antes = format!("{:?}", app.theme().palette.bg);
    app.theme_idx = app.themes.iter().position(|t| t.id == "claro").unwrap();
    let despues = format!("{:?}", app.theme().palette.bg);
    assert_ne!(antes, despues, "el fondo debe cambiar con el tema");
    let screen = render(&mut app, 100, 30);
    assert!(screen.contains("Claro"));
}
