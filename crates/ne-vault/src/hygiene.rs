//! Second-password hygiene.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Whether the proposed vault password is the same as the account password.
///
/// The two defeat the point of a second password when equal, so the UI should
/// warn (or refuse). Both inputs are only borrowed; this keeps no copy. They
/// are compared through SHA-256 digests with a constant-time equality, so the
/// running time depends neither on where the passwords differ nor on their
/// lengths beyond hashing. The digests are wiped before returning.
pub fn reuses_account_password(vault_password: &[u8], account_password: &[u8]) -> bool {
    let vault_digest: Zeroizing<[u8; 32]> = Zeroizing::new(Sha256::digest(vault_password).into());
    let account_digest: Zeroizing<[u8; 32]> =
        Zeroizing::new(Sha256::digest(account_password).into());
    vault_digest[..].ct_eq(&account_digest[..]).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_equal_passwords_only() {
        assert!(reuses_account_password(b"correct horse", b"correct horse"));
        assert!(!reuses_account_password(b"correct horse", b"correct horsf"));
        assert!(!reuses_account_password(
            b"correct horse",
            b"correct horse "
        ));
        assert!(!reuses_account_password(b"a", b"A"));
        assert!(!reuses_account_password("mật khẩu".as_bytes(), b"mat khau"));
        assert!(reuses_account_password(
            "mật khẩu".as_bytes(),
            "mật khẩu".as_bytes()
        ));
    }
}
