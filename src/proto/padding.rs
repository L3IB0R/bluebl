//! Padding PKCS#7 determinista (bloques 256/512/1024/2048).
//!
//! Normaliza el tamaño de los paquetes para resistir análisis de tráfico.
//! El byte de relleno es la propia longitud del relleno: al ser determinista,
//! la firma del paquete puede re-calcularse tras un relay.

/// Bloques estándar de padding.
const BLOCK_SIZES: [usize; 4] = [256, 512, 1024, 2048];

/// Tamaño de bloque óptimo para un frame de `data_size` bytes.
///
/// Se tienen en cuenta ~16 bytes de sobrecarga de cifrado (tag AES-GCM).
pub fn optimal_block_size(data_size: usize) -> usize {
    let total = data_size + 16;
    for block in BLOCK_SIZES {
        if total <= block {
            return block;
        }
    }
    // Mensajes muy grandes: se fragmentarán igualmente, sin padding.
    data_size
}

/// Añade padding PKCS#7 hasta `target_size`. Si no cabe en un byte de
/// longitud (más de 255 bytes de relleno) o ya alcanzó el objetivo, devuelve
/// los datos intactos.
pub fn pad(data: &[u8], target_size: usize) -> Vec<u8> {
    if data.len() >= target_size {
        return data.to_vec();
    }
    let needed = target_size - data.len();
    if needed == 0 || needed > 255 {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(target_size);
    out.extend_from_slice(data);
    out.extend(std::iter::repeat_n(needed as u8, needed));
    out
}

/// Elimina el padding PKCS#7 si es válido. Si los últimos bytes no forman
/// un relleno consistente, devuelve los datos originales sin tocar.
pub fn unpad(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return data.to_vec();
    }
    let last = *data.last().unwrap();
    let len = last as usize;
    if len == 0 || len > data.len() {
        return data.to_vec();
    }
    let start = data.len() - len;
    if data[start..].iter().all(|&b| b == last) {
        data[..start].to_vec()
    } else {
        data.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_y_unpad_round_trip() {
        let data = vec![0xABu8; 100];
        let padded = pad(&data, optimal_block_size(data.len()));
        assert_eq!(padded.len(), 256);
        assert_eq!(padded[100], 156); // 256 - 100 = 156 bytes de relleno
        assert_eq!(unpad(&padded), data);
    }

    #[test]
    fn padding_determinista() {
        // La firma exige que dos codificaciones del mismo paquete coincidan.
        let data = b"hola BlueBl";
        assert_eq!(pad(data, 256), pad(data, 256));
    }

    #[test]
    fn unpad_rechaza_relleno_inconsistente() {
        // Último byte declara 9 bytes de relleno, pero no todos valen 9.
        let mut data = vec![0u8; 100];
        data.extend_from_slice(&[3, 3, 9]);
        assert_eq!(unpad(&data), data);
    }

    #[test]
    fn target_grande_sin_padding() {
        // Más de 2048-16 bytes: no hay bloque, se devuelve intacto.
        let data = vec![7u8; 4000];
        assert_eq!(optimal_block_size(data.len()), 4000);
        assert_eq!(pad(&data, optimal_block_size(data.len())), data);
    }
}
