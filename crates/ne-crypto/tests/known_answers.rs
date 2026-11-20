//! Published test vectors for the primitives this crate builds on.
//!
//! XChaCha20-Poly1305 has its own known-answer test next to the code that
//! wraps it (draft-irtf-cfrg-xchacha-03, appendix A.3.1).

#![cfg(feature = "client")]

use hkdf::Hkdf;
use sha2::Sha256;

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    digits
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("ascii");
            u8::from_str_radix(pair, 16).expect("hex")
        })
        .collect()
}

/// RFC 5869 appendix A.1: basic test case with SHA-256.
#[test]
fn hkdf_sha256_rfc5869_case_1() {
    let ikm = hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
    let salt = hex("000102030405060708090a0b0c");
    let info = hex("f0f1f2f3f4f5f6f7f8f9");
    let expected = hex(
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf\
         34007208d5b887185865",
    );
    let mut okm = [0u8; 42];
    Hkdf::<Sha256>::new(Some(&salt), &ikm)
        .expand(&info, &mut okm)
        .expect("expand");
    assert_eq!(okm.as_slice(), expected.as_slice());
}

/// RFC 5869 appendix A.2: longer inputs and outputs.
#[test]
fn hkdf_sha256_rfc5869_case_2() {
    let ikm: Vec<u8> = (0x00u8..=0x4f).collect();
    let salt: Vec<u8> = (0x60u8..=0xaf).collect();
    let info: Vec<u8> = (0xb0u8..=0xff).collect();
    let expected = hex(
        "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c\
         59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71\
         cc30c58179ec3e87c14c01d5c1f3434f1d87",
    );
    let mut okm = [0u8; 82];
    Hkdf::<Sha256>::new(Some(&salt), &ikm)
        .expand(&info, &mut okm)
        .expect("expand");
    assert_eq!(okm.as_slice(), expected.as_slice());
}

/// RFC 5869 appendix A.3: zero-length salt and info.
#[test]
fn hkdf_sha256_rfc5869_case_3() {
    let ikm = hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
    let expected = hex(
        "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d\
         9d201395faa4b61a96c8",
    );
    let mut okm = [0u8; 42];
    Hkdf::<Sha256>::new(None, &ikm)
        .expand(&[], &mut okm)
        .expect("expand");
    assert_eq!(okm.as_slice(), expected.as_slice());
}
