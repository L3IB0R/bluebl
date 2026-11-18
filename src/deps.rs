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
        lines.push(format!("para instalar: {}", install_command()));
        if !is_termux() {
            lines.push("y arranca el servicio: sudo systemctl start bluetooth".into());
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
}
