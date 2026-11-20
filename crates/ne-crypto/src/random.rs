//! The single source of randomness for the crate.

/// Returns `N` bytes from the operating system's CSPRNG.
///
/// Panics if the OS cannot supply randomness, the same choice `rand`'s
/// `OsRng` makes: carrying on with predictable keys or nonces would be worse
/// than stopping. The release profile aborts on panic.
pub(crate) fn array<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("operating system random number generator unavailable");
    bytes
}
