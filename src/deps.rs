//! Detección de dependencias del sistema: adaptador Bluetooth, herramientas
//! y servicio activo. Si algo falta, BlueBl indica el comando exacto para
//! instalarlo según la distribución.

use std::path::PathBuf;
use std::process::Command;

/// Resultado de una comprobación.
#[derive(Clone, Debug)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// ¿Existe un ejecutable en el PATH?
pub fn has_cmd(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate: PathBuf = dir.join(name);
        candidate.is_file()
    })
}

/// Identificador de la distribución, si se puede leer.
pub fn distro_id() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("ID=") {
            return Some(v.trim_matches('"').to_lowercase());
        }
    }
    None
}

/// ¿Estamos en Termux?
pub fn is_termux() -> bool {
    std::env::var("PREFIX")
        .map(|p| p.contains("com.termux"))
        .unwrap_or(false)
}

/// Bloqueo rfkill del adaptador Bluetooth, si lo hay.
///
/// Devuelve `Some("…")` con la descripción del bloqueo: por software
/// (se desbloquea con rfkill) o físico (interruptor del equipo).
pub fn rfkill_blocked() -> Option<String> {
    let entries = std::fs::read_dir("/sys/class/rfkill").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let kind = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        if !kind.trim().eq_ignore_ascii_case("bluetooth") {
            continue;
        }
        let hard = std::fs::read_to_string(path.join("hard")).unwrap_or_default();
        if hard.trim() == "1" {
            return Some("bloqueo físico: revisa el interruptor inalámbrico del equipo".into());
        }
        let soft = std::fs::read_to_string(path.join("soft")).unwrap_or_default();
        if soft.trim() == "1" {
            return Some("bloqueo por software: la radio está apagada por rfkill".into());
        }
    }
    None
}

/// Comando para desbloquear la radio.
pub fn unblock_command() -> String {
    if has_cmd("rfkill") {
        "sudo rfkill unblock bluetooth".to_string()
    } else {
        "sudo apt install rfkill && sudo rfkill unblock bluetooth".to_string()
    }
}

/// Versión mínima de BlueZ: a partir de la 5.78 se corrigió el bug de
/// publicidad extendida que rechazaba el anuncio LE con
/// "Invalid Parameters (0x0d)" — el fallo que dejaba a BlueBl en "sin radio".
pub const MIN_BLUEZ: (u32, u32) = (5, 78);

/// Binarios candidatos de `bluetoothd` (no siempre está en el PATH).
fn bluetoothd_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if has_cmd("bluetoothd") {
        list.push(PathBuf::from("bluetoothd"));
    }
    for p in [
        "/usr/libexec/bluetooth/bluetoothd",
        "/usr/sbin/bluetoothd",
        "/libexec/bluetooth/bluetoothd",
        "/usr/lib/bluetooth/bluetoothd",
    ] {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            list.push(pb);
        }
    }
    if let Ok(prefix) = std::env::var("PREFIX") {
        let pb = PathBuf::from(prefix).join("libexec/bluetooth/bluetoothd");
        if pb.is_file() {
            list.push(pb);
        }
    }
    list
}

/// Extrae la primera versión `mayor.menor` de un texto
/// ("5.85", "5.72.1", "1:5.72-0ubuntu7", "bluez 5.84"…).
fn parse_version(text: &str) -> Option<(u32, u32)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'.' {
            continue;
        }
        let dot = i;
        i += 1;
        let minor_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == minor_start {
            continue;
        }
        if let (Ok(major), Ok(minor)) = (text[start..dot].parse(), text[minor_start..i].parse()) {
            return Some((major, minor));
        }
    }
    None
}

/// Versión del paquete `bluez` según el gestor del sistema.
fn package_version() -> Option<(u32, u32)> {
    let probes: [(&str, &[&str]); 3] = [
        ("dpkg-query", &["-W", "-f=${Version}", "bluez"]),
        ("rpm", &["-q", "--queryformat", "%{VERSION}", "bluez"]),
        ("pacman", &["-Qi", "bluez"]),
    ];
    for (cmd, args) in probes {
        if !has_cmd(cmd) {
            continue;
        }
        let Ok(out) = Command::new(cmd).args(args).output() else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(v) = parse_version(&text) {
            return Some(v);
        }
    }
    None
}

/// Versión de BlueZ (`bluetoothd -v`), si se puede leer.
pub fn bluez_version() -> Option<(u32, u32)> {
    for bin in bluetoothd_candidates() {
        let Ok(out) = Command::new(&bin).arg("-v").output() else {
            continue;
        };
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        if let Some(v) = parse_version(&text) {
            return Some(v);
        }
    }
    // Sin daemon legible: el paquete instalado también sabe la versión.
    package_version()
}

/// ¿La base del sistema es Ubuntu 24.04 "noble" (Mint 22.x, Pop 24.04…)?
fn base_noble() -> bool {
    os_release("VERSION_CODENAME=").as_deref() == Some("noble")
        || os_release("UBUNTU_CODENAME=").as_deref() == Some("noble")
}

/// Campo de `/etc/os-release` (p. ej. `os_release("VERSION_CODENAME=")`).
fn os_release(key: &str) -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix(key)
            .map(|v| v.trim_matches('"').to_string())
    })
}

/// Comando para actualizar BlueZ a una versión que sirva (>= 5.78).
pub fn update_command() -> String {
    if is_termux() {
        return "pkg upgrade bluez".to_string();
    }
    match distro_id().as_deref() {
        Some("debian") | Some("ubuntu") | Some("linuxmint") | Some("pop") | Some("elementary") => {
            if base_noble() {
                // En noble el repo oficial se queda en 5.72: hace falta el PPA.
                "sudo add-apt-repository ppa:giner/bluez && sudo apt update && sudo apt install bluez"
                    .to_string()
            } else {
                "sudo apt update && sudo apt install --only-upgrade bluez".to_string()
            }
        }
        Some("fedora") | Some("rhel") | Some("centos") => "sudo dnf upgrade bluez".to_string(),
        Some("arch") | Some("manjaro") | Some("endeavouros") => {
            "sudo pacman -Syu bluez".to_string()
        }
        Some("opensuse") | Some("suse") => "sudo zypper update bluez".to_string(),
        Some("alpine") => "sudo apk update && sudo apk upgrade bluez".to_string(),
        Some("gentoo") => "sudo emerge --update --newuse net-wireless/bluez".to_string(),
        _ => "actualiza bluez con el gestor de tu distro — https://pkgs.org/search/?q=bluez"
            .to_string(),
    }
}

/// Qué comando sugerir si la radio no está lista: actualizar BlueZ si está
/// viejo; instalar bluez si falta.
pub fn remedio() -> String {
    match bluez_version() {
        Some(v) if v < MIN_BLUEZ => update_command(),
        _ => install_command(),
    }
}

/// Comando de instalación de bluez para esta plataforma.
pub fn install_command() -> String {
    if is_termux() {
        return "pkg install bluez".to_string();
    }
    match distro_id().as_deref() {
        Some("debian") | Some("ubuntu") | Some("linuxmint") | Some("pop") => {
            "sudo apt install bluez".to_string()
        }
        Some("fedora") | Some("rhel") | Some("centos") => "sudo dnf install bluez".to_string(),
        Some("arch") | Some("manjaro") | Some("endeavouros") => {
            "sudo pacman -S bluez bluez-utils".to_string()
        }
        Some("opensuse") | Some("suse") => "sudo zypper install bluez".to_string(),
        Some("alpine") => "sudo apk add bluez".to_string(),
        Some("gentoo") => "sudo emerge net-wireless/bluez".to_string(),
        _ => "instala bluez con el gestor de paquetes de tu distro".to_string(),
    }
}

/// Comprueba el estado de la radio BLE.
pub fn check() -> Vec<Check> {
    let mut checks = Vec::new();

    let btctl = has_cmd("bluetoothctl");
    checks.push(Check {
        name: "bluetoothctl",
        ok: btctl,
        detail: if btctl {
            "encontrado".into()
        } else {
            "no está en el PATH".into()
        },
    });

    // Versión de BlueZ: con < 5.78 el anuncio LE puede ser rechazado.
    let bz = bluez_version();
    checks.push(Check {
        name: "BlueZ (bluetoothd)",
        ok: match bz {
            Some(v) => v >= MIN_BLUEZ,
            // Sin versión legible no bloqueamos: el resto de casillas
            // (bluetoothctl, servicio, adaptador) ya cubren la ausencia.
            None => true,
        },
        detail: match bz {
            Some(v) if v >= MIN_BLUEZ => format!("{}.{}", v.0, v.1),
            Some(v) => format!(
                "{}.{} — antiguo: el anuncio LE puede ser rechazado (mínimo {}.{}).",
                v.0, v.1, MIN_BLUEZ.0, MIN_BLUEZ.1
            ),
            None => "versión no disponible".into(),
        },
    });

    // Servicio Bluetooth (systemd cuando exista).
    if has_cmd("systemctl") {
        let active = Command::new("systemctl")
            .args(["is-active", "bluetooth"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
            .unwrap_or(false);
        checks.push(Check {
            name: "servicio bluetooth",
            ok: active,
            detail: if active {
                "activo".into()
            } else {
                "detenido (systemctl start bluetooth)".into()
            },
        });
    }

    // Bloqueo rfkill: sin esto, BlueZ no acepta ni encender la radio.
    let blocked = rfkill_blocked();
    checks.push(Check {
        name: "radio sin bloquear",
        ok: blocked.is_none(),
        detail: blocked.clone().unwrap_or_else(|| "sin bloqueos".into()),
    });

    // Adaptador: `bluetoothctl list` responde "Controller ..." si hay radio.
    if btctl {
        let out = Command::new("bluetoothctl")
            .args(["list"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let has_adapter = out.contains("Controller");
        checks.push(Check {
            name: "adaptador",
            ok: has_adapter,
            detail: if has_adapter {
                out.lines()
                    .next()
                    .map(|l| l.trim().to_string())
                    .unwrap_or_else(|| "detectado".into())
            } else if is_termux() {
                "Termux usa la radio del teléfono (requiere permisos)".into()
            } else {
                "ninguno visible — ¿está apagado o sin permisos?".into()
            },
        });
    }

    checks
}

/// ¿Está todo listo para la radio?
pub fn radio_ready() -> bool {
    check().iter().all(|c| c.ok)
}

/// Informe para mostrar en la interfaz.
pub fn report() -> Vec<String> {
    let mut lines = vec!["Dependencias:".to_string()];
    for c in check() {
        let mark = if c.ok { "✓" } else { "✗" };
        lines.push(format!("  {mark} {}: {}", c.name, c.detail));
    }
    if !radio_ready() {
        lines.push(String::new());
        if let Some(why) = rfkill_blocked() {
            lines.push(format!("  ✗ radio bloqueada: {why}"));
            lines.push(format!("para desbloquear: {}", unblock_command()));
            lines.push(String::new());
        }
        // BlueZ viejo: instalar no basta, hay que actualizar.
        let bz_viejo = matches!(bluez_version(), Some(v) if v < MIN_BLUEZ);
        if bz_viejo {
            let v = bluez_version().unwrap_or((0, 0));
            lines.push(format!(
                "  ✗ BlueZ {}.{} es anterior a la {}.{}: en Ubuntu 24.04 / Mint 22.3 \
                 el anuncio LE sale rechazado (Invalid Parameters 0x0d)",
                v.0, v.1, MIN_BLUEZ.0, MIN_BLUEZ.1
            ));
            lines.push(format!("para actualizar: {}", update_command()));
            lines.push("o busca el paquete de tu distro: https://pkgs.org/search/?q=bluez".into());
        } else {
            lines.push(format!("para instalar: {}", install_command()));
            if !is_termux() {
                lines.push("y arranca el servicio: sudo systemctl start bluetooth".into());
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comandos_del_sistema() {
        // `/bin/sh` siempre existe en Linux; un nombre absurdo, no.
        assert!(has_cmd("sh"));
        assert!(!has_cmd("esto-no-existe-9f3k"));
    }

    #[test]
    fn reporte_con_formato() {
        let lines = report();
        assert!(lines[0].contains("Dependencias"));
        assert!(lines.iter().any(|l| l.contains("bluetoothctl")));
        assert!(lines.iter().any(|l| l.contains("adaptador")));
    }

    #[test]
    fn comando_de_instalacion_no_vacio() {
        assert!(!install_command().is_empty());
    }

    #[test]
    fn comprobacion_coherente() {
        // Si la radio está lista, todas las casillas están en verde.
        if radio_ready() {
            assert!(check().iter().all(|c| c.ok));
        }
    }

    #[test]
    fn rfkill_se_lee_sin_explotar() {
        // En este equipo puede haber bloqueo o no; sólo pedimos que lea
        // /sys sin paniquear y que el comando de desbloqueo no esté vacío.
        let _ = rfkill_blocked();
        assert!(!unblock_command().is_empty());
    }

    #[test]
    fn parsea_versiones_de_bluez() {
        assert_eq!(parse_version("5.85"), Some((5, 85)));
        assert_eq!(parse_version("5.72.1"), Some((5, 72)));
        assert_eq!(parse_version("1:5.72-0ubuntu7"), Some((5, 72)));
        assert_eq!(parse_version("bluetoothd 5.84"), Some((5, 84)));
        assert_eq!(parse_version("v5.78"), Some((5, 78)));
        assert_eq!(parse_version("no hay versión aquí"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn umbral_de_bluez() {
        // El bug de publicidad extendida se corrigió en la 5.78.
        assert!((5, 77) < MIN_BLUEZ);
        assert!((5, 78) >= MIN_BLUEZ);
        assert!((5, 85) >= MIN_BLUEZ);
        assert!((6, 0) >= MIN_BLUEZ);
    }

    #[test]
    fn chequeo_de_version_en_el_reporte() {
        let lines = report();
        assert!(lines.iter().any(|l| l.contains("BlueZ (bluetoothd)")));
        // Si la versión es legible y vieja, el informe manda a actualizar.
        if let Some(v) = bluez_version() {
            if v < MIN_BLUEZ {
                assert!(lines.iter().any(|l| l.contains("actualizar")));
            }
        }
    }

    #[test]
    fn comando_de_actualizacion_no_vacio() {
        assert!(!update_command().is_empty());
        assert!(!remedio().is_empty());
    }

    #[test]
    fn la_version_no_disponible_no_tumba_el_chequeo() {
        // Sin bluetoothd legible no bloqueamos: mandan las otras casillas.
        let checks = check();
        if let Some(c) = checks.iter().find(|c| c.name == "BlueZ (bluetoothd)") {
            if bluez_version().is_none() {
                assert!(c.ok);
            }
        }
    }
}
