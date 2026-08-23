#![forbid(unsafe_code)]

mod pairing;

use std::fmt;

pub use pairing::{
    PairingActor, PairingError, PairingGuard, PairingTransition, PersistOutcome,
    prior_committed_key_matches,
};
use sha2::{Digest, Sha256};
use snow::{Builder, HandshakeState, params::NoiseParams};

pub const NOISE_XX_NAME: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
pub const NOISE_IK_NAME: &str = "Noise_IK_25519_ChaChaPoly_SHA256";
pub const PROLOGUE: &[u8; 64] = b"PhoneBoost|core=1|pbmux=1|role=linux-initiator/android-responder";
pub const SAS_DOMAIN: &[u8; 18] = b"PHONEBOOST-SAS-V1\0";

#[cfg(any(test, feature = "fixture-diagnostics"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SasDerivation {
    pub material: [u8; 32],
    pub counter: u32,
    pub block: [u8; 32],
    pub candidate_n: u32,
    pub sas: String,
}

#[derive(Debug)]
pub enum SecureError {
    InvalidNoiseParameters,
    Noise(String),
    HandshakeIncomplete,
    HandshakeHashMismatch,
    SasCounterExhausted,
}

impl fmt::Display for SecureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNoiseParameters => write!(f, "invalid locked Noise parameters"),
            Self::Noise(message) => write!(f, "Noise handshake failed: {message}"),
            Self::HandshakeIncomplete => write!(f, "Noise XX handshake did not finish"),
            Self::HandshakeHashMismatch => write!(f, "Noise XX endpoint handshake hashes differ"),
            Self::SasCounterExhausted => write!(f, "SAS rejection-sampling counter exhausted"),
        }
    }
}

impl std::error::Error for SecureError {}

impl From<snow::Error> for SecureError {
    fn from(value: snow::Error) -> Self {
        Self::Noise(value.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XxTranscript {
    pub message_1: Vec<u8>,
    pub message_2: Vec<u8>,
    pub message_3: Vec<u8>,
    pub handshake_hash: [u8; 32],
}

pub fn noise_xx_params() -> Result<NoiseParams, SecureError> {
    NOISE_XX_NAME
        .parse()
        .map_err(|_| SecureError::InvalidNoiseParameters)
}

pub fn production_xx_initiator(static_private: &[u8; 32]) -> Result<HandshakeState, SecureError> {
    Ok(Builder::new(noise_xx_params()?)
        .local_private_key(static_private)
        .prologue(PROLOGUE)
        .build_initiator()?)
}

pub fn production_xx_responder(static_private: &[u8; 32]) -> Result<HandshakeState, SecureError> {
    Ok(Builder::new(noise_xx_params()?)
        .local_private_key(static_private)
        .prologue(PROLOGUE)
        .build_responder()?)
}

pub fn complete_xx(
    mut initiator: HandshakeState,
    mut responder: HandshakeState,
) -> Result<XxTranscript, SecureError> {
    let mut wire = vec![0_u8; 65_535];
    let mut plaintext = vec![0_u8; 65_535];

    let len_1 = initiator.write_message(&[], &mut wire)?;
    let message_1 = wire[..len_1].to_vec();
    responder.read_message(&message_1, &mut plaintext)?;

    let len_2 = responder.write_message(&[], &mut wire)?;
    let message_2 = wire[..len_2].to_vec();
    initiator.read_message(&message_2, &mut plaintext)?;

    let len_3 = initiator.write_message(&[], &mut wire)?;
    let message_3 = wire[..len_3].to_vec();
    responder.read_message(&message_3, &mut plaintext)?;

    if !initiator.is_handshake_finished() || !responder.is_handshake_finished() {
        return Err(SecureError::HandshakeIncomplete);
    }
    let mut initiator_hash = [0_u8; 32];
    initiator_hash.copy_from_slice(initiator.get_handshake_hash());
    let mut responder_hash = [0_u8; 32];
    responder_hash.copy_from_slice(responder.get_handshake_hash());
    if initiator_hash != responder_hash {
        return Err(SecureError::HandshakeHashMismatch);
    }

    initiator.into_transport_mode()?;
    responder.into_transport_mode()?;

    Ok(XxTranscript {
        message_1,
        message_2,
        message_3,
        handshake_hash: initiator_hash,
    })
}

struct SasParts {
    #[cfg(any(test, feature = "fixture-diagnostics"))]
    material: [u8; 32],
    #[cfg(any(test, feature = "fixture-diagnostics"))]
    counter: u32,
    #[cfg(any(test, feature = "fixture-diagnostics"))]
    block: [u8; 32],
    #[cfg(any(test, feature = "fixture-diagnostics"))]
    candidate_n: u32,
    sas: String,
}

fn derive_sas_parts(handshake_hash: &[u8; 32]) -> Result<SasParts, SecureError> {
    let mut material_hasher = Sha256::new();
    material_hasher.update(SAS_DOMAIN);
    material_hasher.update(handshake_hash);
    let material: [u8; 32] = material_hasher.finalize().into();

    let mut counter = 0_u32;
    loop {
        let mut block_hasher = Sha256::new();
        block_hasher.update(material);
        block_hasher.update(counter.to_be_bytes());
        let block: [u8; 32] = block_hasher.finalize().into();
        let candidate_n = u32::from_be_bytes([0, block[0], block[1], block[2]]);
        if candidate_n < 16_000_000 {
            return Ok(SasParts {
                #[cfg(any(test, feature = "fixture-diagnostics"))]
                material,
                #[cfg(any(test, feature = "fixture-diagnostics"))]
                counter,
                #[cfg(any(test, feature = "fixture-diagnostics"))]
                block,
                #[cfg(any(test, feature = "fixture-diagnostics"))]
                candidate_n,
                sas: format!("{:06}", candidate_n % 1_000_000),
            });
        }
        counter = counter
            .checked_add(1)
            .ok_or(SecureError::SasCounterExhausted)?;
    }
}

pub fn derive_sas(handshake_hash: &[u8; 32]) -> Result<String, SecureError> {
    Ok(derive_sas_parts(handshake_hash)?.sas)
}

#[cfg(any(test, feature = "fixture-diagnostics"))]
pub fn derive_sas_diagnostics(handshake_hash: &[u8; 32]) -> Result<SasDerivation, SecureError> {
    let parts = derive_sas_parts(handshake_hash)?;
    Ok(SasDerivation {
        material: parts.material,
        counter: parts.counter,
        block: parts.block,
        candidate_n: parts.candidate_n,
        sas: parts.sas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_byte_constants_are_exact() {
        assert_eq!(PROLOGUE.len(), 64);
        assert_eq!(SAS_DOMAIN.len(), 18);
        assert_eq!(SAS_DOMAIN, b"PHONEBOOST-SAS-V1\0");
    }

    #[test]
    fn sas_is_six_digits_and_reproducible() {
        let hash = [0x42; 32];
        let first = derive_sas(&hash).unwrap();
        let second = derive_sas(&hash).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 6);
        assert!(first.bytes().all(|byte| byte.is_ascii_digit()));
        let diagnostics = derive_sas_diagnostics(&hash).unwrap();
        assert_eq!(diagnostics.sas, first);
        assert!(diagnostics.candidate_n < 16_000_000);
    }
}
