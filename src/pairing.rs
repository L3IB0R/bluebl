//! Emparejamiento compatible con la app de referencia: QR `bitchat://verify`
//! firmado con Ed25519 y huella digital (SHA-256 de la clave pública Noise)
//! para comparar a ojo entre dos pantallas.
//!
//! El QR lo genera BlueBl y lo escanea el teléfono (desde Linux no se
//! escanea: para eso está la huella, que se compara con la que muestra la
//! app bajo el icono de candado del contacto).

use crate::identity::{derive_peer_id, verify_signature, Identity};
use sha2::{Digest, Sha256};

/// Contexto de firma del QR (idéntico al de la app de referencia).
const CONTEXT: &str = "bitchat-verify-v1";
/// Versión del esquema del QR.
const SCHEMA: i64 = 1;
/// Vigencia del QR: la app rechaza los mayores de 5 minutos.
pub const QR_MAX_AGE_SECS: i64 = 5 * 60;

/// Contacto leído de un QR `bitchat://verify`.
pub struct Scanned {
    pub id: String,
    pub nickname: String,
    pub noise_public: String,
    pub signing_public: String,
}

/// Huella digital: SHA-256 de la clave pública Noise en hex minúsculas.
/// Es la misma que muestra la app de referencia («fingerprint»).
pub fn fingerprint(noise_public: &[u8; 32]) -> String {
    hex::encode(Sha256::digest(noise_public))
}

/// Huella a partir de la clave pública en hex (como viaja en el announce).
pub fn fingerprint_from_hex(noise_hex: &str) -> Option<String> {
    let bytes = hex::decode(noise_hex).ok()?;
    let arr: [u8; 32] = bytes.try_into().ok()?;
    Some(fingerprint(&arr))
}

/// Formatea la huella como la app: mayúsculas, grupos de 4 caracteres y
/// cuatro grupos por línea (64 hex → 4 líneas).
pub fn format_fingerprint(fp: &str) -> String {
    let upper = fp.to_uppercase();
    let mut out = String::new();
    for (i, ch) in upper.chars().enumerate() {
        if i > 0 {
            if i % 16 == 0 {
                out.push('\n');
            } else if i % 4 == 0 {
                out.push(' ');
            }
        }
        out.push(ch);
    }
    out
}

/// Bytes canónicos que firma el QR: cada campo con su longitud delante
/// (≤255 bytes) y en el orden fijo del esquema.
fn canonical_bytes(
    v: i64,
    noise: &str,
    sign: &str,
    npub: &str,
    nick: &str,
    ts: i64,
    nonce: &str,
) -> Vec<u8> {
    let v_s = v.to_string();
    let ts_s = ts.to_string();
    let noise_l = noise.to_lowercase();
    let sign_l = sign.to_lowercase();
    let mut out = Vec::new();
    for field in [CONTEXT, &v_s, &noise_l, &sign_l, npub, nick, &ts_s, nonce] {
        let bytes = field.as_bytes();
        let take = bytes.len().min(255);
        out.push(take as u8);
        out.extend_from_slice(&bytes[..take]);
    }
    out
}

/// Construye la URL `bitchat://verify?…` firmada con nuestra clave
/// Ed25519. `ts` en segundos Unix y `nonce` de 16 bytes aleatorios.
pub fn build_verify_url(identity: &Identity, nickname: &str, ts: i64, nonce: &[u8; 16]) -> String {
    let noise = hex::encode(identity.noise_public());
    let sign = hex::encode(identity.signing_public());
    let nonce_b64 = base64url(nonce);
    let nick = sanitize_nick(nickname);
    let msg = canonical_bytes(SCHEMA, &noise, &sign, "", &nick, ts, &nonce_b64);
    let sig = hex::encode(identity.sign(&msg));
    format!(
        "bitchat://verify?v={SCHEMA}&noise={noise}&sign={sign}&nick={}&ts={ts}&nonce={nonce_b64}&sig={sig}",
        percent_encode(&nick)
    )
}

/// Lee una URL `bitchat://verify?…`: valida firma, frescura y deriva el
/// peer ID de la clave pública (misma regla canónica que el announce).
pub fn parse_verify_url(url: &str, now: i64) -> Result<Scanned, String> {
    let rest = url
        .trim()
        .strip_prefix("bitchat://verify?")
        .ok_or("no es un QR de bitchat (esperaba bitchat://verify?…)")?;

    let mut v: Option<i64> = None;
    let mut noise: Option<String> = None;
    let mut sign: Option<String> = None;
    let mut nick: Option<String> = None;
    let mut ts: Option<i64> = None;
    let mut nonce: Option<String> = None;
    let mut sig: Option<String> = None;
    for pair in rest.split('&') {
        let (k, val) = pair.split_once('=').unwrap_or((pair, ""));
        let val = percent_decode(val)?;
        match k {
            "v" => v = val.parse().ok(),
            "noise" => noise = Some(val.to_lowercase()),
            "sign" => sign = Some(val.to_lowercase()),
            "nick" => nick = Some(val),
            "ts" => ts = val.parse().ok(),
            "nonce" => nonce = Some(val),
            "sig" => sig = Some(val.to_lowercase()),
            _ => {} // npub y compañía: ignorados
        }
    }
    let (v, noise, sign, nick, ts, nonce, sig) = match (v, noise, sign, nick, ts, nonce, sig) {
        (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f), Some(g)) => (a, b, c, d, e, f, g),
        _ => return Err("al campo del QR".into()),
    };
    if v != SCHEMA {
        return Err(format!("versión de QR no soportada: {v}"));
    }
    // Frescura en ambos sentidos: un QR fechado en el futuro no gana
    // ventana de validez (misma regla que la app).
    if (now - ts).abs() > QR_MAX_AGE_SECS {
        return Err("el QR caducó (dura 5 min) — genera uno nuevo con /pair".into());
    }
    let noise_b = hex32(&noise).map_err(|e| e.to_string())?;
    let sign_b = hex32(&sign).map_err(|e| e.to_string())?;
    let sig_b = hex64(&sig).map_err(|e| e.to_string())?;
    let msg = canonical_bytes(v, &noise, &sign, "", &nick, ts, &nonce);
    if !verify_signature(&sig_b, &msg, &sign_b) {
        return Err("la firma del QR no coincide con su clave pública".into());
    }
    Ok(Scanned {
        id: hex::encode(derive_peer_id(&noise_b)),
        nickname: nick,
        noise_public: noise,
        signing_public: sign,
    })
}

/// Apodo apto para el QR: sin separadores ni raros que rompan la URL.
fn sanitize_nick(nick: &str) -> String {
    let clean: String = nick
        .chars()
        .map(|c| if c == '|' { '_' } else { c })
        .collect();
    if clean.trim().is_empty() {
        "bluebl".to_string()
    } else {
        clean
    }
}

/// Percent-encoding RFC 3986 (sin reservados): lo que hace URLComponents.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Percent-decoding (sólo %XX; «+» se queda como «+», como en Foundation).
fn percent_decode(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 3 > bytes.len() {
                return Err("URL malformada (% suelto)".into());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| "URL malformada (% con bytes raros)".to_string())?;
            let v = u8::from_str_radix(hex, 16)
                .map_err(|_| format!("URL malformada: %{hex} no es hex"))?;
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "URL con texto no UTF-8".into())
}

/// Base64 URL-safe sin relleno (el nonce de 16 bytes → 22 caracteres).
fn base64url(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(T[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(T[n as usize & 63] as char);
        }
    }
    out
}

fn hex32(s: &str) -> Result<[u8; 32], &'static str> {
    if s.len() != 64 {
        return Err("clave pública con tamaño distinto de 32 bytes");
    }
    hex::decode(s)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or("clave pública ilegible")
}

fn hex64(s: &str) -> Result<[u8; 64], &'static str> {
    if s.len() != 128 {
        return Err("firma con tamaño distinto de 64 bytes");
    }
    hex::decode(s)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or("firma ilegible")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url_de(id: &Identity, nick: &str, ts: i64) -> String {
        let nonce = [7u8; 16];
        build_verify_url(id, nick, ts, &nonce)
    }

    #[test]
    fn url_se_lee_con_la_propia_firma() {
        let id = Identity::generate();
        let url = url_de(&id, "kaomi", 1_700_000_000);
        let sc = parse_verify_url(&url, 1_700_000_000).expect("debe leerse");
        assert_eq!(sc.nickname, "kaomi");
        assert_eq!(sc.noise_public, hex::encode(id.noise_public()));
        assert_eq!(sc.signing_public, hex::encode(id.signing_public()));
        assert_eq!(sc.id, hex::encode(id.peer_id()));
    }

    #[test]
    fn firma_tampeada_se_rechaza() {
        let id = Identity::generate();
        let url = url_de(&id, "kaomi", 1_700_000_000);
        let mut chars: Vec<char> = url.chars().collect();
        // Alterar el último dígito de la firma.
        let last = chars.len() - 1;
        chars[last] = if chars[last] == '0' { '1' } else { '0' };
        let bad: String = chars.into_iter().collect();
        assert!(parse_verify_url(&bad, 1_700_000_000).is_err());
    }

    #[test]
    fn qr_caducado_se_rechaza() {
        let id = Identity::generate();
        let url = url_de(&id, "kaomi", 1_700_000_000);
        // Fuera de la ventana ±5 min (en cualquier sentido).
        assert!(parse_verify_url(&url, 1_700_000_000 + 301).is_err());
        assert!(parse_verify_url(&url, 1_700_000_000 - 301).is_err());
        assert!(parse_verify_url(&url, 1_700_000_000 + 299).is_ok());
    }

    #[test]
    fn nick_con_caracteres_raros_sobrevive() {
        let id = Identity::generate();
        let url = url_de(&id, "luna nube&ñ=1", 1_700_000_000);
        let sc = parse_verify_url(&url, 1_700_000_000).expect("debe leerse");
        assert_eq!(sc.nickname, "luna nube&ñ=1");
    }

    #[test]
    fn huella_como_la_app() {
        let id = Identity::generate();
        let fp = fingerprint(&id.noise_public());
        assert_eq!(fp.len(), 64);
        // Misma huella que la app: sha256 de la clave pública Noise.
        let expected = hex::encode(Sha256::digest(id.noise_public()));
        assert_eq!(fp, expected);
        let pretty = format_fingerprint(&fp);
        let lines: Vec<&str> = pretty.lines().collect();
        assert_eq!(lines.len(), 4, "4 líneas de 16 caracteres");
        assert_eq!(lines[0].len(), 19, "4 grupos de 4 con espacios");
        assert!(pretty
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' || c == '\n'));
        // Redondeo: desde hex crudo se recupera igual.
        assert_eq!(
            fingerprint_from_hex(&hex::encode(id.noise_public())),
            Some(fp)
        );
    }

    #[test]
    fn base64url_sin_relleno() {
        assert_eq!(base64url(&[0u8; 16]), "AAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(base64url(&[0xffu8; 3]), "____");
        assert_eq!(base64url(&[0xfb, 0xef]), "--8");
    }

    #[test]
    fn url_de_una_ventana_de_tiempo_util() {
        // El QR que ve la terminal debe caber en una pantalla de terminal:
        // ni demasiados módulos para el QR ni un texto eterno.
        let id = Identity::generate();
        let url = url_de(&id, "Kaomi", 1_700_000_000);
        assert!(url.len() < 400, "URL larga: {} bytes", url.len());
    }
}
