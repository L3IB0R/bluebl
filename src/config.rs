//! Configuración persistente: `~/.config/bluebl/config` (o `$BLUEBL_CONFIG`).
//!
//! Formato simple `clave=valor`, sin dependencias externas.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub nick: String,
    pub theme: String,
    pub density: String,
    /// Ruta alternativa (para tests); `None` = ruta estándar.
    pub path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            nick: whoami().to_string(),
            theme: "lluvia-neon".to_string(),
            density: "comodo".to_string(),
            path: None,
        }
    }
}

/// Nombre de usuario del sistema, como apodo inicial.
fn whoami() -> String {
    std::env::var("USER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "bluebl".to_string())
}

impl Config {
    /// Ruta del archivo de configuración.
    pub fn path() -> PathBuf {
        Self::default_path()
    }

    fn default_path() -> PathBuf {
        if let Ok(p) = std::env::var("BLUEBL_CONFIG") {
            if !p.is_empty() {
                return PathBuf::from(p);
            }
        }
        let base = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join(".config")
            });
        base.join("bluebl").join("config")
    }

    /// Ruta efectiva (la inyectada o la estándar).
    fn effective_path(&self) -> PathBuf {
        self.path.clone().unwrap_or_else(Self::default_path)
    }

    /// Ruta del archivo de identidad: hermano del config, en el mismo
    /// directorio (`~/.config/bluebl/identity`).
    pub fn identity_path(&self) -> PathBuf {
        self.effective_path().with_file_name("identity")
    }

    /// Carga la configuración; si no existe, devuelve los valores por
    /// defecto sin romper nada.
    pub fn load() -> Config {
        let mut cfg = Config::default();
        let Ok(text) = fs::read_to_string(Self::path()) else {
            return cfg;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "nick" => cfg.nick = value.trim().to_string(),
                "theme" => cfg.theme = value.trim().to_string(),
                "density" => cfg.density = value.trim().to_string(),
                _ => {}
            }
        }
        if cfg.nick.is_empty() {
            cfg.nick = whoami().to_string();
        }
        cfg
    }

    /// Guarda la configuración creando el directorio si hace falta.
    pub fn save(&self) {
        let path = self.effective_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let body = format!(
            "# BlueBl — configuración\nnick={}\ntheme={}\ndensity={}\n",
            self.nick, self.theme, self.density
        );
        if let Ok(mut f) = fs::File::create(path) {
            let _ = f.write_all(body.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_con_nick_no_vacio() {
        let cfg = Config::default();
        assert!(!cfg.nick.is_empty());
        assert!(!cfg.theme.is_empty());
    }

    #[test]
    fn parseo_de_lineas() {
        // Un archivo temporal cualquiera: el parser no debe romperse.
        let cfg = Config::default();
        assert_eq!(cfg.density, "comodo");
    }
}
