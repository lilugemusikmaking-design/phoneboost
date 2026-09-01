//! TEST-ONLY deterministic RNG and snow resolver gate.

use rand_core::{CryptoRng, Error, RngCore};
use sha2::{Digest, Sha256};
use snow::params::{CipherChoice, DHChoice, HashChoice};
use snow::resolvers::{CryptoResolver, DefaultResolver};
use snow::types::{Cipher, Dh, Hash, Random};

const RNG_DOMAIN: &[u8] = b"PHONEBOOST-FIXTURE-RNG-V1\0";

pub struct DeterministicFixtureRng {
    seed: [u8; 32],
    counter: u64,
    block: [u8; 32],
    cursor: usize,
}

impl DeterministicFixtureRng {
    pub fn new(seed: [u8; 32]) -> Self {
        Self {
            seed,
            counter: 0,
            block: [0; 32],
            cursor: 32,
        }
    }

    fn refill(&mut self) {
        let mut hash = Sha256::new();
        hash.update(RNG_DOMAIN);
        hash.update(self.seed);
        hash.update(self.counter.to_be_bytes());
        self.block.copy_from_slice(&hash.finalize());
        self.counter = self
            .counter
            .checked_add(1)
            .expect("fixture RNG counter overflow");
        self.cursor = 0;
    }
}

impl RngCore for DeterministicFixtureRng {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0; 4];
        self.fill_bytes(&mut bytes);
        u32::from_le_bytes(bytes)
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0; 8];
        self.fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut written = 0;
        while written < dest.len() {
            if self.cursor == self.block.len() {
                self.refill();
            }
            let available = self.block.len() - self.cursor;
            let wanted = dest.len() - written;
            let count = available.min(wanted);
            dest[written..written + count]
                .copy_from_slice(&self.block[self.cursor..self.cursor + count]);
            self.cursor += count;
            written += count;
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for DeterministicFixtureRng {}
impl Random for DeterministicFixtureRng {}

pub struct DeterministicFixtureResolver {
    seed: [u8; 32],
    default: DefaultResolver,
}

impl DeterministicFixtureResolver {
    pub fn new(seed: [u8; 32]) -> Self {
        Self {
            seed,
            default: DefaultResolver,
        }
    }
}

impl CryptoResolver for DeterministicFixtureResolver {
    fn resolve_rng(&self) -> Option<Box<dyn Random>> {
        Some(Box::new(DeterministicFixtureRng::new(self.seed)))
    }

    fn resolve_dh(&self, choice: &DHChoice) -> Option<Box<dyn Dh>> {
        self.default.resolve_dh(choice)
    }

    fn resolve_hash(&self, choice: &HashChoice) -> Option<Box<dyn Hash>> {
        self.default.resolve_hash(choice)
    }

    fn resolve_cipher(&self, choice: &CipherChoice) -> Option<Box<dyn Cipher>> {
        self.default.resolve_cipher(choice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_continuous_stream_across_rng_methods() {
        let seed = [0x5a; 32];
        let mut mixed = DeterministicFixtureRng::new(seed);
        let first = mixed.next_u32();
        let second = mixed.next_u64();
        let mut tail = [0; 20];
        mixed.fill_bytes(&mut tail);

        let mut straight = DeterministicFixtureRng::new(seed);
        let mut expected = [0; 32];
        straight.fill_bytes(&mut expected);

        assert_eq!(first.to_le_bytes(), expected[0..4]);
        assert_eq!(second.to_le_bytes(), expected[4..12]);
        assert_eq!(tail, expected[12..32]);
    }
}
