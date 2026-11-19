//! Modo nodo (repetidor) en segundo plano: `bluebl --nodo` deja la radio
//! reenviando paquetes sin interfaz; `bluebl --nodo --off` lo detiene.
//!
//! El reenvío (flood con TTL y dedup) vive en la capa de red y está
//! siempre activo: este módulo sólo aporta el arranque en segundo plano
//! y su parada. El proceso hijo es el propio `bluebl` con un flag
//! interno (`--nodo-fg`), desacoplado de la terminal con `setsid`.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Archivo con el PID del nodo en marcha (junto al config).
fn pid_path() -> PathBuf {
    crate::config::Config::path().with_file_name("nodo.pid")
}

/// Log del nodo (junto al config): lo escribe el hijo por su stdout.
fn log_path() -> PathBuf {
    crate::config::Config::path().with_file_name("nodo.log")
}

/// PID escrito en el archivo, si es un número válido (> 1).
fn parse_pid(text: &str) -> Option<i32> {
    let pid: i32 = text.trim().parse().ok()?;
    (pid > 1).then_some(pid)
}

/// PID del nodo en marcha en `path`, si el proceso sigue vivo y sigue
/// siendo bluebl (el `/proc/…/comm` evita confundirlo con un PID
/// reutilizado).
fn pid_vivo_en(path: &std::path::Path) -> Option<i32> {
    let pid = parse_pid(&std::fs::read_to_string(path).ok()?)?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    (comm.trim() == "bluebl").then_some(pid)
}

/// PID del nodo en marcha, si lo hay.
fn pid_vivo() -> Option<i32> {
    pid_vivo_en(&pid_path())
}

/// `bluebl --nodo` — arranca el repetidor en segundo plano.
pub fn arrancar() -> i32 {
    if let Some(pid) = pid_vivo() {
        println!("el modo nodo ya está en marcha (pid {pid})");
        println!("para pararlo: bluebl --nodo --off");
        return 0;
    }
    let _ = std::fs::remove_file(pid_path()); // PID caducado
    if !crate::deps::radio_ready() {
        eprintln!(
            "bluebl: la radio BLE no está lista — {}",
            crate::deps::remedio()
        );
        return 1;
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("no encuentro el propio binario: {e}");
            return 1;
        }
    };
    if let Some(dir) = pid_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let log = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("no puedo abrir {}: {e}", log_path().display());
            return 1;
        }
    };
    let log_err = match log.try_clone() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("no puedo duplicar el log: {e}");
            return 1;
        }
    };

    let mut cmd = Command::new(exe);
    cmd.arg("--nodo-fg")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    // Nueva sesión: el hijo no recibe SIGHUP ni Ctrl+C de la terminal.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            let _ = libc::setsid();
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("no he podido arrancar el modo nodo: {e}");
            return 1;
        }
    };

    // Confirmar que arranca de verdad antes de darlo por bueno.
    std::thread::sleep(Duration::from_millis(400));
    if let Ok(Some(status)) = child.try_wait() {
        eprintln!(
            "el modo nodo no ha arrancado ({status}) — últimas líneas de {}:",
            log_path().display()
        );
        if let Ok(text) = std::fs::read_to_string(log_path()) {
            let lineas: Vec<&str> = text.lines().collect();
            for line in lineas.iter().skip(lineas.len().saturating_sub(5)) {
                eprintln!("  {line}");
            }
        }
        return 1;
    }

    let pid = child.id() as i32;
    if let Err(e) = std::fs::write(pid_path(), format!("{pid}\n")) {
        eprintln!("aviso: no puedo escribir {}: {e}", pid_path().display());
    }
    println!("modo nodo en marcha (pid {pid}) — reenviando paquetes entre vecinos");
    println!("  log: {}", log_path().display());
    println!("  para pararlo: bluebl --nodo --off");
    0
}

/// `bluebl --nodo --off` — detiene el repetidor.
pub fn detener() -> i32 {
    let Some(pid) = pid_vivo() else {
        let _ = std::fs::remove_file(pid_path());
        println!("no hay ningún modo nodo en marcha");
        return 0;
    };
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    for _ in 0..40 {
        if pid_vivo().is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if pid_vivo().is_some() {
        eprintln!("el pid {pid} no responde a SIGTERM — pruébalo a mano: kill -9 {pid}");
        return 1;
    }
    let _ = std::fs::remove_file(pid_path());
    println!("modo nodo detenido (pid {pid})");
    0
}

/// `--nodo-fg`: el proceso real del repetidor (lo arranca [`arrancar`]).
///
/// Sólo drena y registra: el reenvío ocurre en el hilo de la radio. Al
/// morir el backend (o la radio), el canal se cierra y el proceso sale
/// limpio, retirando su PID.
pub fn correr() -> i32 {
    let cfg = crate::config::Config::load();
    let identity = crate::identity::Identity::load_or_generate(&cfg.identity_path());
    let (tx, rx) = crate::mesh::offline();
    // El sender se queda vivo aquí: si se suelta, el backend creería que
    // le han pedido parar.
    let _net = crate::net::spawn(identity, cfg.nick.clone(), tx);

    println!(
        "[{}] modo nodo activo (pid {}) — reenviando en la malla",
        crate::app::now_hm(),
        std::process::id()
    );
    if !crate::deps::radio_ready() {
        println!(
            "[{}] aviso: la radio no está lista — {}",
            crate::app::now_hm(),
            crate::deps::remedio()
        );
    }

    loop {
        use crate::mesh::MeshEvent;
        match rx.recv() {
            Ok(MeshEvent::Status(s)) => println!("[{}] {s}", crate::app::now_hm()),
            Ok(MeshEvent::PeerJoined { nickname, .. }) => {
                println!("[{}] entra {nickname}", crate::app::now_hm())
            }
            Ok(MeshEvent::PeerLeft { id }) => {
                println!("[{}] sale {}", crate::app::now_hm(), &id[..8.min(id.len())])
            }
            // Los mensajes no se registran: el log es de actividad, no de
            // contenido.
            Ok(_) => {}
            Err(_) => {
                println!(
                    "[{}] la radio se ha caído: modo nodo terminado",
                    crate::app::now_hm()
                );
                break;
            }
        }
    }
    let _ = std::fs::remove_file(pid_path());
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pid_sólo_números_grandes() {
        assert_eq!(parse_pid("1234\n"), Some(1234));
        assert_eq!(parse_pid("  42  "), Some(42));
        assert_eq!(parse_pid(""), None);
        assert_eq!(parse_pid("abc"), None);
        assert_eq!(parse_pid("0"), None);
        assert_eq!(parse_pid("1"), None); // el PID 1 no es nuestro
        assert_eq!(parse_pid("-5"), None);
        assert_eq!(parse_pid("12a4"), None);
    }

    #[test]
    fn archivos_del_nodo_junto_al_config() {
        assert_eq!(pid_path().file_name().unwrap(), "nodo.pid");
        assert_eq!(log_path().file_name().unwrap(), "nodo.log");
        // Mismo directorio que el config: nada suelto por ahí.
        assert_eq!(pid_path().parent(), crate::config::Config::path().parent());
    }

    #[test]
    fn sin_pid_file_no_hay_nodo() {
        // Un archivo inexistente (o con basura) no da ningún nodo vivo.
        let dir = std::env::temp_dir().join(format!("bluebl-nodo-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ausente = dir.join("nodo.pid");
        assert_eq!(pid_vivo_en(&ausente), None);

        let basura = dir.join("basura.pid");
        std::fs::write(&basura, "esto no es un pid\n").unwrap();
        assert_eq!(pid_vivo_en(&basura), None);

        // Un PID real pero de otro proceso: no lo damos por nuestro.
        let ajeno = dir.join("ajeno.pid");
        std::fs::write(&ajeno, "1\n").unwrap();
        assert_eq!(pid_vivo_en(&ajeno), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
