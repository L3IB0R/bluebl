//! Comprueba la barra inferior: atajos junto al estado y escalera por ancho.

use bluebl::app::App;
use bluebl::config::Config;
use bluebl::identity::Identity;
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
    (App::new(cfg, rx, Identity::generate()), tx)
}

fn ultima_linea(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    let y = buf.area.height - 1;
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
        .collect()
}

#[test]
fn atajos_junto_al_estado_en_pantalla_ancha() {
    let (mut app, _tx) = app();
    let line = ultima_linea(&mut app, 140, 30);
    assert!(line.contains("pares"), "falta el estado:\n[{line}]");
    assert!(line.contains("F1 ayuda"), "faltan los atajos:\n[{line}]");
    assert!(
        line.contains("^Q salir"),
        "falta el último atajo:\n[{line}]"
    );
    // La pista completa es la que cabe en 140 columnas.
    assert!(
        line.contains("F1 ayuda · ^W quién · ^P QR · ^T temas · ^Q salir"),
        "debería caber la escalera entera:\n[{line}]"
    );
}

#[test]
fn atajos_se_acortan_al_estrechar() {
    let (mut app, _tx) = app();
    // 70 columnas: no cabe la escalera entera, pero sí una versión corta.
    let line = ultima_linea(&mut app, 70, 24);
    assert!(line.contains("pares"), "falta el estado:\n[{line}]");
    assert!(
        line.contains("F1 ayuda"),
        "debería caber al menos «F1 ayuda»:\n[{line}]"
    );
    assert!(
        !line.contains("^P QR"),
        "la versión larga no debería caber:\n[{line}]"
    );
}

#[test]
fn pantalla_minima_solo_estado() {
    let (mut app, _tx) = app();
    // 36 columnas (teléfono): el estado manda; los atajos ceden.
    let line = ultima_linea(&mut app, 36, 20);
    assert!(line.contains("pares"), "falta el estado:\n[{line}]");
    assert!(
        !line.contains("F1 ayuda"),
        "aquí no caben atajos:\n[{line}]"
    );
}

#[test]
fn estado_largo_no_desborda() {
    let (mut app, _tx) = app();
    app.status = "vecino conectado: akaomi-phone-5g".into();
    let line = ultima_linea(&mut app, 60, 24);
    assert!(line.contains("pares"), "falta el estado:\n[{line}]");
    // Nunca debe haber textos encimados sin separación ni cortes a medias
    // de la palabra «pares».
    assert!(
        !line.contains("paresF1"),
        "estado y atajos pegados:\n[{line}]"
    );
}
