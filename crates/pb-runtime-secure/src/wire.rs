use std::io::{Read, Write};

use pb_types::{MAX_NOISE_CIPHERTEXT, MAX_PBMUX_PLAINTEXT};
use snow::TransportState;

pub const RECORD_PREFIX_BYTES: usize = 2;
const MAX_RECORD_BYTES: usize = u16::MAX as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecureWireError {
    Io,
    RecordLength,
    Crypto,
}

impl std::fmt::Display for SecureWireError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Io => "secure wire I/O failed",
            Self::RecordLength => "secure wire record length invalid",
            Self::Crypto => "SESSION_CRYPTO_ERROR",
        })
    }
}

impl std::error::Error for SecureWireError {}

pub(crate) fn write_record(
    stream: &mut (impl Write + ?Sized),
    bytes: &[u8],
) -> Result<(), SecureWireError> {
    if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES {
        return Err(SecureWireError::RecordLength);
    }
    let length = u16::try_from(bytes.len()).map_err(|_| SecureWireError::RecordLength)?;
    stream
        .write_all(&length.to_be_bytes())
        .and_then(|()| stream.write_all(bytes))
        .and_then(|()| stream.flush())
        .map_err(|_| SecureWireError::Io)
}

pub(crate) fn read_record(stream: &mut (impl Read + ?Sized)) -> Result<Vec<u8>, SecureWireError> {
    let mut prefix = [0_u8; RECORD_PREFIX_BYTES];
    stream
        .read_exact(&mut prefix)
        .map_err(|_| SecureWireError::Io)?;
    let length = usize::from(u16::from_be_bytes(prefix));
    if length == 0 || length > MAX_RECORD_BYTES {
        return Err(SecureWireError::RecordLength);
    }
    let mut record = vec![0_u8; length];
    stream
        .read_exact(&mut record)
        .map_err(|_| SecureWireError::Io)?;
    Ok(record)
}

pub(crate) fn write_encrypted(
    stream: &mut (impl Write + ?Sized),
    transport: &mut TransportState,
    plaintext: &[u8],
) -> Result<(), SecureWireError> {
    if plaintext.is_empty() || plaintext.len() > MAX_PBMUX_PLAINTEXT {
        return Err(SecureWireError::RecordLength);
    }
    let mut ciphertext = vec![0_u8; MAX_NOISE_CIPHERTEXT];
    let length = transport
        .write_message(plaintext, &mut ciphertext)
        .map_err(|_| SecureWireError::Crypto)?;
    if length > MAX_NOISE_CIPHERTEXT {
        return Err(SecureWireError::RecordLength);
    }
    write_record(stream, &ciphertext[..length])
}

pub(crate) fn read_encrypted(
    stream: &mut (impl Read + ?Sized),
    transport: &mut TransportState,
) -> Result<Vec<u8>, SecureWireError> {
    let ciphertext = read_record(stream)?;
    if ciphertext.len() > MAX_NOISE_CIPHERTEXT {
        return Err(SecureWireError::RecordLength);
    }
    let mut plaintext = vec![0_u8; MAX_PBMUX_PLAINTEXT];
    let length = transport
        .read_message(&ciphertext, &mut plaintext)
        .map_err(|_| SecureWireError::Crypto)?;
    plaintext.truncate(length);
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pb_secure::{production_xx_initiator, production_xx_responder};

    fn transports() -> (TransportState, TransportState) {
        let mut initiator = production_xx_initiator(&[1; 32]).unwrap();
        let mut responder = production_xx_responder(&[2; 32]).unwrap();
        let mut wire = [0_u8; 256];
        let mut plain = [0_u8; 256];
        let length = initiator.write_message(&[], &mut wire).unwrap();
        responder.read_message(&wire[..length], &mut plain).unwrap();
        let length = responder.write_message(&[], &mut wire).unwrap();
        initiator.read_message(&wire[..length], &mut plain).unwrap();
        let length = initiator.write_message(&[], &mut wire).unwrap();
        responder.read_message(&wire[..length], &mut plain).unwrap();
        (
            initiator.into_transport_mode().unwrap(),
            responder.into_transport_mode().unwrap(),
        )
    }

    #[test]
    fn encrypted_record_contains_no_plaintext_pbmux() {
        let (mut initiator, mut responder) = transports();
        let plaintext = b"PBM1 canonical encrypted body";
        let mut network = Vec::new();
        write_encrypted(&mut network, &mut initiator, plaintext).unwrap();
        assert!(
            !network
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
        let recovered = read_encrypted(&mut network.as_slice(), &mut responder).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn oversized_and_zero_records_fail_closed() {
        assert_eq!(
            write_record(&mut Vec::new(), &[]),
            Err(SecureWireError::RecordLength)
        );
        let empty = [0_u8, 0];
        let mut input = empty.as_slice();
        assert_eq!(read_record(&mut input), Err(SecureWireError::RecordLength));
    }
}
