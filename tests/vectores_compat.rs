//! Pruebas cruzadas contra vectores generados con una implementación
//! independiente de la especificación (Python + cryptography).
//!
//! Estos bytes son la verdad del wire format: si nuestro encoder deja de
//! coincidir con ellos, la app de referencia no podrá leernos.

use bluebl::identity::{derive_peer_id, verify_signature, Identity};
use bluebl::proto::packet::msg_type;
use bluebl::proto::Packet;
use ed25519_dalek::{Signer, SigningKey};

fn hex_file(path: &str) -> String {
    std::fs::read_to_string(path).expect("fixture")
}

fn decode_hex(s: &str) -> Vec<u8> {
    let s = s.trim();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
    decode_hex(&hex_file(&path))
}

#[test]
fn vector1_broadcast_decode_y_encode() {
    let wire = fixture("vector1.hex");
    let p = Packet::decode(&wire).unwrap();
    assert_eq!(p.version, 1);
    assert_eq!(p.msg_type, msg_type::MESSAGE);
    assert_eq!(p.ttl, 7);
    assert_eq!(p.timestamp, 1_700_000_000_000);
    assert_eq!(p.sender, [0x11; 8]);
    assert!(p.is_broadcast());
    assert_eq!(p.payload, b"hola malla");
    assert_eq!(p.signature, None);
    // Y nuestro encoder produce exactamente los mismos bytes.
    assert_eq!(p.encode(true).unwrap(), wire);
}

#[test]
fn vector2_comprimido_con_destinatario() {
    let wire = fixture("vector2.hex");
    let p = Packet::decode(&wire).unwrap();
    assert_eq!(p.recipient, Some([0x22; 8]));
    assert_eq!(p.payload, vec![b'a'; 400]);
    assert!(p.wire.as_ref().unwrap().compressed);
    // La re-codificación reutiliza los bytes originales: idéntico frame.
    assert_eq!(p.encode(true).unwrap(), wire);
}

#[test]
fn vector_firma_verifica_aunque_cambie_el_ttl() {
    let wire = fixture("vector_firma.hex");
    let pub_bytes = fixture("firma_pub.hex");
    let mut public = [0u8; 32];
    public.copy_from_slice(&pub_bytes);

    let p = Packet::decode(&wire).unwrap();
    assert_eq!(p.ttl, 7);
    assert_eq!(p.payload, b"hola desde python");
    assert!(p.signature.is_some());
    // La firma se calculó con ttl=0: verificar con ttl=7 en el frame.
    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&public).unwrap();
    assert!(p.verify(&verifying).unwrap());

    // Un relay decrementa el TTL: la firma sigue viva.
    let mut relayed = p.clone();
    relayed.ttl = 3;
    assert!(relayed.verify(&verifying).unwrap());
}

#[test]
fn firma_nuestra_es_identica_a_la_de_python() {
    // Ed25519 es determinista: misma clave + mismo preimage = misma firma.
    // Si nuestro preimage difiriera en un solo byte, no coincidiría.
    let seed = [42u8; 32];
    let sig_expected = fixture("firma_sig.hex");
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&sig_expected);

    let key = SigningKey::from_bytes(&seed);
    let mut p = Packet::new(msg_type::MESSAGE, [0x11; 8], b"hola desde python".to_vec());
    p.timestamp = 1_700_000_000_000;
    let sig_ours = key.sign(&p.for_signing().unwrap()).to_bytes();
    assert_eq!(sig_ours.to_vec(), sig, "el preimage no coincide con Python");

    // Y el paquete completo con firma también byte a byte.
    p.sign(&key).unwrap();
    let wire = fixture("vector_firma.hex");
    assert_eq!(p.encode(true).unwrap(), wire);
}

#[test]
fn peer_id_vector_conocido() {
    // Vector: sha256(32 bytes de 0x07)[..8] calculado con hashlib.
    let noise_public = [7u8; 32];
    let id = derive_peer_id(&noise_public);
    assert_eq!(hex::encode(id), "4bb06f8e4e3a7715");
    assert_ne!(id, derive_peer_id(&[8u8; 32]));
}

#[test]
fn identidad_local_firma_y_verifica() {
    let id = Identity::generate();
    let sig = id.sign(b"anuncio de prueba");
    assert!(verify_signature(
        &sig,
        b"anuncio de prueba",
        &id.signing_public()
    ));
}
