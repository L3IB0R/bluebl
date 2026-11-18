//! Compresión raw deflate (sin cabeceras zlib/gzip), compatible con el
//! `Deflater(raw = true)` de la app de referencia y con iOS.

use crate::proto::{ProtoError, ProtoResult};
use std::io::{Read, Write};

/// Tamaño mínimo para considerar compresión.
pub const COMPRESSION_THRESHOLD: usize = 100;

/// Techo de expansión de DEFLATE: cada match de 258 bytes cuesta al menos
/// 2 bits de código; un original mayor que `compressed * 1032` es imposible.
pub const MAX_DEFLATE_RATIO: u64 = 1032;

/// ¿Merece la pena comprimir? Umbral de tamaño + chequeo de entropía:
/// si ≥90% de los bytes son distintos, el dato ya parece comprimido.
pub fn should_compress(data: &[u8]) -> bool {
    if data.len() < COMPRESSION_THRESHOLD {
        return false;
    }
    let mut seen = [false; 256];
    for &b in data {
        seen[b as usize] = true;
    }
    let unique = seen.iter().filter(|&&s| s).count();
    let denom = data.len().min(256) as f64;
    (unique as f64) / denom < 0.9
}

/// Comprime en raw deflate. Devuelve `None` si no ganó nada.
pub fn compress(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < COMPRESSION_THRESHOLD {
        return None;
    }
    let mut out = Vec::new();
    {
        let mut enc = flate2::write::DeflateEncoder::new(&mut out, flate2::Compression::default());
        enc.write_all(data).ok()?;
        enc.finish().ok()?;
    }
    if !out.is_empty() && out.len() < data.len() {
        Some(out)
    } else {
        None
    }
}

/// Descomprime verificando el ratio y que el resultado tenga el tamaño
/// declarado (protección contra bombas deflate).
pub fn decompress(compressed: &[u8], original_size: usize) -> ProtoResult<Vec<u8>> {
    if compressed.is_empty() {
        return Err(ProtoError::BadCompression);
    }
    if original_size as u64 > compressed.len() as u64 * MAX_DEFLATE_RATIO {
        return Err(ProtoError::CompressionBomb);
    }
    let mut out = Vec::with_capacity(original_size);
    let mut dec = flate2::read::DeflateDecoder::new(compressed);
    dec.read_to_end(&mut out)
        .map_err(|_| ProtoError::BadCompression)?;
    if out.len() != original_size {
        return Err(ProtoError::BadCompression);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_repetitivo_se_comprime() {
        let data = vec![b'h'; 4096];
        assert!(should_compress(&data));
        let comp = compress(&data).expect("debería comprimir");
        assert!(comp.len() < data.len());
        assert_eq!(decompress(&comp, data.len()).unwrap(), data);
    }

    #[test]
    fn datos_pequeños_no_se_comprimen() {
        assert!(!should_compress(b"hola"));
        assert!(compress(b"hola").is_none());
    }

    #[test]
    fn alta_entropía_no_se_comprime() {
        // 256 valores distintos en 256 bytes: ratio 1.0 → ya es "ruido".
        let data: Vec<u8> = (0..=255).collect();
        assert!(!should_compress(&data));
    }

    #[test]
    fn bomba_deflate_rechazada() {
        // Original declarado enorme respecto al comprimido.
        let comp = compress(&vec![b'a'; 4096]).unwrap();
        assert!(matches!(
            decompress(&comp, 4096 * 2000),
            Err(ProtoError::CompressionBomb)
        ));
    }

    #[test]
    fn tamano_original_debe_coincidir() {
        let comp = compress(&vec![b'a'; 4096]).unwrap();
        assert!(matches!(
            decompress(&comp, 4095),
            Err(ProtoError::BadCompression)
        ));
    }
}
