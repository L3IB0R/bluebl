//! BlueBl: chat mesh por Bluetooth LE, en la terminal.

use bluebl::app::App;
use bluebl::config::Config;
use bluebl::{mesh, themes, ui};
use ratatui::DefaultTerminal;
use std::time::Duration;

const HELP: &str = "BlueBl — chat mesh por Bluetooth LE, en la terminal

Uso: bluebl [OPCIÓN]

Opciones:
  -t, --theme NOMBRE   arrancar con un tema (queda guardado en la config)
      --themes         listar los temas disponibles
      --deps           comprobar la radio BLE y cómo instalarla
  -c, --config         mostrar la ruta del archivo de configuración
  -h, --help           esta ayuda
  -V, --version        versión de BlueBl

Dentro del programa:
  intro enviar · tab cambiar panel · ↑↓ / avpag rePag desplazar
  ctrl+T selector de temas · f1 ayuda · ctrl+Q salir

Comandos: /help · /nick · /join · /msg · /who · /theme · /density

Configuración: ~/.config/bluebl/config";

/// Qué hacer tras interpretar la línea de comandos.
enum Cli {
    /// Seguir y arrancar la interfaz.
    Run,
    /// Terminar con este código de salida.
    Exit(i32),
}

fn parse_args(cfg: &mut Config) -> Cli {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Cli::Exit(0);
            }
            "-V" | "--version" => {
                println!("bluebl {}", env!("CARGO_PKG_VERSION"));
                return Cli::Exit(0);
            }
            "-c" | "--config" => {
                println!("{}", Config::path().display());
                return Cli::Exit(0);
            }
            "--themes" => {
                for cat in themes::Category::ALL {
                    println!("\n{}", cat.name());
                    for t in themes::all_themes().iter().filter(|t| t.category == cat) {
                        println!("  {:20} {}", t.name, t.id);
                    }
                }
                println!();
                return Cli::Exit(0);
            }
            "--deps" => {
                for line in bluebl::deps::report() {
                    println!("{line}");
                }
                return Cli::Exit(if bluebl::deps::radio_ready() { 0 } else { 1 });
            }
            "-t" | "--theme" => {
                i += 1;
                let Some(id) = args.get(i) else {
                    eprintln!("falta el nombre del tema: --theme NOMBRE");
                    return Cli::Exit(2);
                };
                match themes::find(id) {
                    Some(t) => {
                        cfg.theme = t.id.to_string();
                        cfg.save();
                        println!("tema: {}", t.name);
                    }
                    None => {
                        eprintln!("tema desconocido: {id}");
                        eprintln!("mira la lista con: bluebl --themes");
                        return Cli::Exit(2);
                    }
                }
            }
            other => {
                eprintln!("opción desconocida: {other}");
                eprintln!("prueba con --help");
                return Cli::Exit(2);
            }
        }
        i += 1;
    }
    Cli::Run
}

fn main() -> std::io::Result<()> {
    let mut cfg = Config::load();
    if let Cli::Exit(code) = parse_args(&mut cfg) {
        std::process::exit(code);
    }

    // Canal de eventos: el backend BLE (u otro) le cuenta a la TUI lo que
    // pasa en la malla.
    let (events_tx, rx) = mesh::offline();
    let mut app = App::new(cfg, rx);

    // Fase 1: radio BLE real en Linux. En el resto de plataformas el
    // backend abre un canal sin radio (Fase 2).
    let net_tx = bluebl::net::spawn(app.identity.clone(), app.nickname.clone(), events_tx);
    app.attach_net(net_tx);

    // Aviso breve si la radio no está lista (estado completo: /deps).
    if !bluebl::deps::radio_ready() {
        eprintln!(
            "bluebl: la radio BLE no está lista — {}",
            bluebl::deps::install_command()
        );
        eprintln!("        (estado completo: bluebl --deps o /deps dentro del programa)");
        eprintln!();
    }

    let mut terminal = ratatui::init();
    let res = run(&mut terminal, &mut app);
    ratatui::restore();

    // Apagar la radio antes de salir (el backend desregistra el anuncio).
    if let Some(net) = app.net.take() {
        let _ = net.send(bluebl::net::NetCommand::Shutdown);
    }
    res
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    loop {
        app.drain_mesh();
        if !app.running {
            break;
        }

        // Poll corto: la interfaz respira sin quemar CPU.
        if crossterm::event::poll(Duration::from_millis(80))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                // En modo crudo pueden llegar también las liberaciones.
                if key.kind == crossterm::event::KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }

        terminal.draw(|f| ui::draw(f, app))?;
    }
    Ok(())
}
