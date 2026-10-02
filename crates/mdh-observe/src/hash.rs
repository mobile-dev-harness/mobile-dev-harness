/// FNV-1a: stable across Rust versions and platforms, unlike `DefaultHasher`, so keys survive in
/// persisted CLI sessions.
pub(crate) struct Fnv(pub(crate) u64);

impl Fnv {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    pub(crate) fn field(&mut self, s: &str) {
        self.bytes(s.as_bytes());
        self.bytes(&[0xff]); // never occurs in UTF-8, so fields can't run into each other
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
}
