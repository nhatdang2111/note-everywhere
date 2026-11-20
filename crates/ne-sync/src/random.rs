//! Operating-system randomness for client ids and jitter.

use ne_sync_protocol::ClientId;

fn fill(buf: &mut [u8]) {
    // Same stance as `ne-crypto`: continuing without randomness is worse than stopping.
    getrandom::fill(buf).expect("operating system random number generator");
}

pub fn client_id() -> ClientId {
    let mut id = [0u8; ne_sync_protocol::CLIENT_ID_LEN];
    fill(&mut id);
    ClientId(id)
}

/// Uniform in `[0, 1)`.
pub fn unit_f64() -> f64 {
    let mut bytes = [0u8; 8];
    fill(&mut bytes);
    // 53 random bits fill an f64 mantissa exactly.
    (u64::from_le_bytes(bytes) >> 11) as f64 / (1u64 << 53) as f64
}
