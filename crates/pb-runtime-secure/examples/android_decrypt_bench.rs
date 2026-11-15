//! Offline diagnostic only. Fixed public TEST keys/nonces never enter a live session.
//! A host-generated fixture is replayed by both Android binaries, so an encoder and
//! decoder with the same defect cannot pass by agreeing only with each other.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::time::Instant;

use pb_secure::{PROLOGUE, noise_xx_params};
use pb_types::{MAX_NOISE_CIPHERTEXT, MAX_PBMUX_PLAINTEXT};
use rustix::time::{ClockId, clock_gettime};
use serde_json::json;
use snow::{Builder, TransportState};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// Exact ciphertext-length distribution in #35 run2-android.json, phase Decrypt.
const DISTRIBUTION: &[(usize, usize)] = &[
    (56, 2),
    (104, 6),
    (120, 1),
    (140, 1),
    (1144, 1),
    (16440, 16),
    (61496, 1088),
];

fn transports() -> Result<(TransportState, TransportState)> {
    // Only this standalone fixture tool uses fixed ephemerals. Production builders
    // and protocol constants are unchanged. These bytes are public test material.
    let mut a = Builder::new(noise_xx_params()?)
        .local_private_key(&[1; 32])
        .fixed_ephemeral_key_for_testing_only(&[3; 32])
        .prologue(PROLOGUE)
        .build_initiator()?;
    let mut b = Builder::new(noise_xx_params()?)
        .local_private_key(&[2; 32])
        .fixed_ephemeral_key_for_testing_only(&[4; 32])
        .prologue(PROLOGUE)
        .build_responder()?;
    let mut wire = [0; 256];
    let mut plain = [0; 256];
    let n = a.write_message(&[], &mut wire)?;
    b.read_message(&wire[..n], &mut plain)?;
    let n = b.write_message(&[], &mut wire)?;
    a.read_message(&wire[..n], &mut plain)?;
    let n = a.write_message(&[], &mut wire)?;
    b.read_message(&wire[..n], &mut plain)?;
    Ok((a.into_transport_mode()?, b.into_transport_mode()?))
}

fn cpu_ns() -> u128 {
    let t = clock_gettime(ClockId::ThreadCPUTime);
    t.tv_sec as u128 * 1_000_000_000 + t.tv_nsec as u128
}

fn plaintext() -> Vec<u8> {
    (0..MAX_PBMUX_PLAINTEXT)
        .map(|i| (31 * i + 7) as u8)
        .collect()
}

fn write_fixture(path: &str) -> Result<()> {
    let (mut tx, _) = transports()?;
    let mut file = File::create(path)?;
    file.write_all(b"PBDECRYPT1")?;
    let plain = plaintext();
    let mut out = vec![0; MAX_NOISE_CIPHERTEXT];
    for &(size, count) in DISTRIBUTION {
        for _ in 0..count {
            let n = tx.write_message(&plain[..size - 16], &mut out)?;
            assert_eq!(n, size);
            file.write_all(&(n as u16).to_be_bytes())?;
            file.write_all(&out[..n])?;
        }
    }
    file.sync_all()?;
    Ok(())
}

fn read_fixture(path: &str) -> Result<Vec<Vec<u8>>> {
    // At most ~64 MiB once; keep the full fixture out of the timed region. This
    // is an isolated diagnostic, not a PhoneBoost allocation/admission claim.
    let mut file = File::open(path)?;
    let mut magic = [0; 10];
    file.read_exact(&mut magic)?;
    if &magic != b"PBDECRYPT1" {
        return Err("invalid fixture magic".into());
    }
    let mut records = Vec::new();
    for &(size, count) in DISTRIBUTION {
        for _ in 0..count {
            let mut len = [0; 2];
            file.read_exact(&mut len)?;
            if usize::from(u16::from_be_bytes(len)) != size {
                return Err("fixture record size mismatch".into());
            }
            let mut bytes = vec![0; size];
            file.read_exact(&mut bytes)?;
            records.push(bytes);
        }
    }
    if file.read(&mut [0])? != 0 {
        return Err("trailing fixture data".into());
    }
    Ok(records)
}

fn verify_rejection(records: &[Vec<u8>]) -> Result<()> {
    let (_, mut rx) = transports()?;
    let mut out = vec![0; MAX_PBMUX_PLAINTEXT];
    let nonce = rx.receiving_nonce();
    let mut bad = records[0].clone();
    *bad.last_mut().unwrap() ^= 1;
    if rx.read_message(&bad, &mut out).is_ok() || rx.receiving_nonce() != nonce {
        return Err("tamper was accepted or advanced nonce".into());
    }
    rx.read_message(&records[0], &mut out)?;
    if rx.read_message(&records[0], &mut out).is_ok() {
        return Err("replay was accepted".into());
    }
    let nonce = rx.receiving_nonce();
    let mut bad = records[1].clone();
    bad[0] ^= 1;
    if rx.read_message(&bad, &mut out).is_ok() || rx.receiving_nonce() != nonce {
        return Err("nonzero-nonce tamper accepted or advanced nonce".into());
    }
    rx.read_message(&records[1], &mut out)?;
    Ok(())
}

fn measure(records: &[Vec<u8>], run: usize) -> Result<serde_json::Value> {
    let clock_start = Instant::now();
    for _ in 0..1000 {
        std::hint::black_box(cpu_ns());
        let t = Instant::now();
        std::hint::black_box(t.elapsed());
        std::hint::black_box(cpu_ns());
    }
    let clock_pair_ns_per_record = clock_start.elapsed().as_nanos() / 1000;
    let (_, mut rx) = transports()?;
    let expected = plaintext();
    let mut groups: BTreeMap<usize, (usize, u128, u128)> = BTreeMap::new();
    let mut samples = Vec::new();
    for record in records {
        // Match wire.rs allocation, but exclude it just like the Decrypt probe.
        let mut out = vec![0; MAX_PBMUX_PLAINTEXT];
        let cpu = cpu_ns();
        let wall = Instant::now();
        let n = std::hint::black_box(rx.read_message(record, &mut out))?;
        let wall_ns = wall.elapsed().as_nanos();
        let thread_ns = cpu_ns() - cpu;
        if n != record.len() - 16 || out[..n] != expected[..n] {
            return Err("plaintext mismatch".into());
        }
        let g = groups.entry(record.len()).or_default();
        g.0 += 1;
        g.1 += wall_ns;
        g.2 += thread_ns;
        samples.push(json!([record.len(), wall_ns, thread_ns]));
    }
    let wall_ns: u128 = groups.values().map(|g| g.1).sum();
    let cpu_ns: u128 = groups.values().map(|g| g.2).sum();
    let ciphertext_bytes: usize = records.iter().map(Vec::len).sum();
    Ok(json!({"run":run,"warmup":run==0,"records":records.len(),
        "ciphertext_bytes":ciphertext_bytes,"plaintext_bytes":ciphertext_bytes-16*records.len(),
        "wall_ns":wall_ns,"thread_cpu_ns":cpu_ns,"groups":groups,"samples":samples,
        "clock_pair_ns_per_record":clock_pair_ns_per_record,
        "plaintext_exact":true,"tamper_and_replay_rejected":true}))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.as_slice() {
        [_, mode, path] if mode == "--write-fixture" => write_fixture(path),
        [_, mode, path] if mode == "--measure" => {
            let records = read_fixture(path)?;
            verify_rejection(&records)?;
            for run in 0..4 {
                println!("{}", measure(&records, run)?);
            }
            Ok(())
        }
        _ => Err("usage: android_decrypt_bench --write-fixture|--measure FILE".into()),
    }
}
