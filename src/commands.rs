//! Comandos de barra, estilo IRC.

/// Comando interpretado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Quit,
    /// `/nick NUEVO`
    Nick(String),
    /// `/me acción` — acción en tercera persona.
    Me(String),
    /// `/who` — lista de pares.
    Who,
    /// `/theme [id]` — cambiar o listar temas.
    Theme(Option<String>),
    /// `/density compacto|comodo|grande`
    Density(String),
    /// `/clear` — vaciar el chat.
    Clear,
    /// `/peers` — ver la lista de pares en el chat.
    Peers,
    /// `/deps` — comprobar dependencias del sistema (radio BLE).
    Deps,
    /// `/pair` — generar el QR de emparejamiento y la huella.
    Pair,
    /// `/accept NICK|CÓDIGO|QR` — añadir contacto verificado (★).
    Accept(String),
    /// `/fp [apodo]` — huella digital (para comparar con el teléfono).
    Fp(Option<String>),
    /// Comando no reconocido (se reporta, no se envía como mensaje).
    Unknown(String),
}

/// Interpreta una línea de entrada. Devuelve `None` si es texto normal.
pub fn parse(input: &str) -> Option<Command> {
    let rest = input.strip_prefix('/')?;
    if rest.is_empty() {
        return Some(Command::Help);
    }
    let (cmd, args) = match rest.split_once(' ') {
        Some((c, a)) => (c, a.trim()),
        None => (rest, ""),
    };
    let cmd = cmd.to_ascii_lowercase();
    Some(match cmd.as_str() {
        "help" | "?" => Command::Help,
        "quit" | "exit" | "q" => Command::Quit,
        "nick" => Command::Nick(args.to_string()),
        "me" => Command::Me(args.to_string()),
        "who" | "w" => Command::Who,
        "theme" | "t" => Command::Theme(if args.is_empty() {
            None
        } else {
            Some(args.to_string())
        }),
        "density" | "d" => Command::Density(args.to_string()),
        "clear" => Command::Clear,
        "peers" | "nicks" => Command::Peers,
        "deps" | "doctor" => Command::Deps,
        "pair" | "qr" => Command::Pair,
        "accept" | "add" => Command::Accept(args.to_string()),
        "fp" | "huella" | "fingerprint" => Command::Fp(if args.is_empty() {
            None
        } else {
            Some(args.to_string())
        }),
        // Comando desconocido: se reporta, no se envía como mensaje.
        other => Command::Unknown(other.to_string()),
    })
}

impl Command {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Command::Unknown(_))
    }
}

/// Texto de ayuda, listo para pintar en pantalla.
pub fn help_text() -> Vec<String> {
    vec![
        "Comandos:".into(),
        "  /nick NOMBRE      cambiar apodo".into(),
        "  /me ACCION        acción en tercera persona".into(),
        "  /who · /peers     lista de pares en la malla".into(),
        "  /theme [ID]       cambiar tema (sin ID: lista)".into(),
        "  /density NOMBRE   compacto · comodo · grande".into(),
        "  /clear            vaciar el chat".into(),
        "  /deps             comprobar la radio BLE del sistema".into(),
        "  /pair             QR de emparejamiento (bitchat://verify) + huella".into(),
        "  /fp [apodo]       huella digital para comparar con el teléfono".into(),
        "  /accept APODO     añadir contacto tras comparar huellas (★)".into(),
        "  /help             esta ayuda".into(),
        "  /quit             salir".into(),
        String::new(),
        "Teclas:".into(),
        "  intro enviar · tab panel · ↑↓ / avpag rePag desplazar".into(),
        "  ctrl+T temas · ctrl+Q salir".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_normal_no_es_comando() {
        assert_eq!(parse("hola a todos"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse(" /saludos"), None);
    }

    #[test]
    fn comandos_basicos() {
        assert_eq!(parse("/help"), Some(Command::Help));
        assert_eq!(parse("/quit"), Some(Command::Quit));
        assert_eq!(parse("/q"), Some(Command::Quit));
        assert_eq!(parse("/who"), Some(Command::Who));
        assert_eq!(parse("/clear"), Some(Command::Clear));
    }

    #[test]
    fn comandos_con_argumentos() {
        assert_eq!(parse("/nick luna"), Some(Command::Nick("luna".into())));
        assert_eq!(
            parse("/density grande"),
            Some(Command::Density("grande".into()))
        );
        assert_eq!(parse("/theme"), Some(Command::Theme(None)));
        assert_eq!(
            parse("/theme dracula"),
            Some(Command::Theme(Some("dracula".into())))
        );
    }

    #[test]
    fn join_part_y_msg_ya_no_existen() {
        // Sin canales ni MDs: lo que fue /join, /part y /msg es un
        // comando desconocido, no un mensaje ni un canal.
        assert_eq!(parse("/join #neon"), Some(Command::Unknown("join".into())));
        assert_eq!(parse("/part"), Some(Command::Unknown("part".into())));
        assert_eq!(
            parse("/msg luna hola"),
            Some(Command::Unknown("msg".into()))
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(parse("/HELP"), Some(Command::Help));
        assert_eq!(parse("/Nick Luna"), Some(Command::Nick("Luna".into())));
    }
}
