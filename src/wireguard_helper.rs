use base64::Engine;
use boringtun::{noise::{handshake::{parse_handshake_anon, HalfHandshake}, Tunn, Packet}, x25519::{StaticSecret, self}};

pub fn extract_handshake(private_key: &StaticSecret, buf: &[u8]) -> Option<HalfHandshake> {
    let parsed = Tunn::parse_incoming_packet(buf);
    let Ok(packet) = parsed else {return None; };
    let Packet::HandshakeInit(p) = packet else {return None; };

    let static_public = x25519::PublicKey::from(private_key);
    let Ok(handshake) = parse_handshake_anon(private_key, &static_public, &p) else {return None;};

    return Some(handshake);
}

pub fn print_key(key: [u8; 32]) {
    let public_key = base64::engine::general_purpose::STANDARD.encode(key);
    println!("handshake from {}", public_key);
}

pub fn parse_key(x: &str) -> anyhow::Result<[u8; 32]> {
    let b = base64::engine::general_purpose::STANDARD.decode(x)?;
    Ok(b[..].try_into()?)
}