use base64::{Engine, prelude::BASE64_STANDARD};

fn random_bytes(n: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).unwrap();
    bytes
}

pub fn random_hex(n: usize) -> String {
    hex::encode(random_bytes(n))
}

pub fn random_base64(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).unwrap();
    BASE64_STANDARD.encode(bytes)
}
