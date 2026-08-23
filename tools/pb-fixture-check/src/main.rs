#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

const HEADER_LEN: usize = 40;
const MAX_PAYLOAD: usize = 61_440;
const MAX_PLAINTEXT: usize = 61_480;
const MAX_CIPHERTEXT: usize = 61_496;
const MAX_LOGICAL: usize = 4_194_304;
const PROLOGUE: &[u8; 64] = b"PhoneBoost|core=1|pbmux=1|role=linux-initiator/android-responder";
const SAS_DOMAIN: &[u8; 18] = b"PHONEBOOST-SAS-V1\0";
const CANONICAL_SOURCE_FILES: [&str; 5] = [
    "PHONEBOOST_SPEC_V0_7_QR01B_LOCKED_IMPLEMENTATION_BASELINE.docx",
    "PHONEBOOST_CONTRACT_SET_V1_3_QR01A_QR01B_LOCKED.docx",
    "PHONEBOOST_TECH_SHEET_V1_3_QR01A_QR01B_LOCKED_IMPLEMENTATION_HANDOFF.docx",
    "PHONEBOOST_PSEUDOCODE_CRITICAL_PATHS_V1_1_FINAL.docx",
    "PHONEBOOST_FIXTURE_GENERATION_SPEC_V1_0.docx",
];

struct Arguments {
    fixtures: PathBuf,
    inputs: PathBuf,
    scan_core: Option<PathBuf>,
}

#[derive(Debug)]
struct ManualHeader {
    magic: [u8; 4],
    version: u8,
    channel: u8,
    flags: u16,
    message_type: u16,
    header_len: u16,
    request_id: u64,
    sequence: u64,
    fragment_index: u32,
    payload_len: u32,
    logical_message_len: u32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("pb-fixture-check: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    verify_manifest(&args.fixtures, &args.inputs)?;
    verify_qr(&args.fixtures)?;
    verify_control8(&args.fixtures)?;
    verify_pbmux(&args.fixtures)?;
    verify_pairing(&args.fixtures)?;
    verify_pairing_guard(&args.fixtures)?;
    println!(
        "fixtures verified independently: {}",
        args.fixtures.display()
    );
    if let Some(scan_core) = args.scan_core {
        let scanned = scan_core_artifacts(&scan_core, &args.fixtures)?;
        println!("production-exclusion scan passed: {scanned} core artifacts");
    }
    Ok(())
}

fn parse_args() -> Result<Arguments, Box<dyn Error>> {
    let mut fixtures = None;
    let mut inputs = None;
    let mut scan_core = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--fixtures" => fixtures = args.next().map(PathBuf::from),
            "--inputs" => inputs = args.next().map(PathBuf::from),
            "--scan-core" => scan_core = args.next().map(PathBuf::from),
            _ => return Err(format!("unknown or incomplete argument: {argument}").into()),
        }
    }
    Ok(Arguments {
        fixtures: fixtures.ok_or("--fixtures is required")?,
        inputs: inputs.ok_or("--inputs is required")?,
        scan_core,
    })
}

fn fail(path: &Path, reason: impl AsRef<str>) -> Box<dyn Error> {
    format!("{}: {}", path.display(), reason.as_ref()).into()
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(hex::encode(sha256(&fs::read(path)?)))
}

fn json_file(path: &Path) -> Result<Value, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if !bytes.ends_with(b"\n") {
        return Err(fail(path, "JSON is missing its final LF"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn string<'a>(path: &Path, value: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| fail(path, format!("missing string field {key}")))
}

fn u64_field(path: &Path, value: &Value, key: &str) -> Result<u64, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| fail(path, format!("missing integer field {key}")))
}

fn bool_field(path: &Path, value: &Value, key: &str) -> Result<bool, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| fail(path, format!("missing boolean field {key}")))
}

fn decode_hex(path: &Path, value: &Value, key: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    hex::decode(string(path, value, key)?)
        .map_err(|error| fail(path, format!("invalid {key}: {error}")))
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.file_name().and_then(|name| name.to_str()) != Some("MANIFEST.json") {
            files.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

fn verify_manifest(fixtures: &Path, inputs: &Path) -> Result<(), Box<dyn Error>> {
    let path = fixtures.join("MANIFEST.json");
    let manifest = json_file(&path)?;
    if string(&path, &manifest, "fixture_manifest_schema")? != "phoneboost.fixtures.v1" {
        return Err(fail(&path, "wrong fixture manifest schema"));
    }

    let source_array = manifest
        .get("canonical_sources")
        .and_then(Value::as_array)
        .ok_or_else(|| fail(&path, "canonical_sources is not an array"))?;
    if source_array.len() != CANONICAL_SOURCE_FILES.len() {
        return Err(fail(&path, "canonical source count is not five"));
    }
    for (source, expected_name) in source_array.iter().zip(CANONICAL_SOURCE_FILES) {
        let actual_name = source
            .get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| fail(&path, "canonical source file is missing"))?;
        let actual_hash = source
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| fail(&path, "canonical source hash is missing"))?;
        if actual_name != expected_name {
            return Err(fail(
                &path,
                format!("unexpected canonical source {actual_name}"),
            ));
        }
        if actual_hash != sha256_file(&inputs.join(expected_name))? {
            return Err(fail(
                &path,
                format!("canonical source hash mismatch for {expected_name}"),
            ));
        }
    }

    let entries = manifest
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| fail(&path, "files is not an array"))?;
    let mut manifest_paths = Vec::new();
    for entry in entries {
        let relative = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| fail(&path, "manifest path missing"))?;
        if relative == "MANIFEST.json" || relative.starts_with('/') || relative.contains("..") {
            return Err(fail(&path, format!("unsafe/self manifest path {relative}")));
        }
        let fixture_path = fixtures.join(relative);
        let size = entry
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(|| fail(&path, format!("size missing for {relative}")))?;
        let expected_hash = entry
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| fail(&path, format!("hash missing for {relative}")))?;
        if fs::metadata(&fixture_path)?.len() != size {
            return Err(fail(&fixture_path, "size differs from MANIFEST"));
        }
        if sha256_file(&fixture_path)? != expected_hash {
            return Err(fail(&fixture_path, "SHA-256 differs from MANIFEST"));
        }
        manifest_paths.push(relative.to_owned());
    }
    let mut sorted = manifest_paths.clone();
    sorted.sort();
    if manifest_paths != sorted {
        return Err(fail(&path, "file entries are not lexicographically sorted"));
    }

    let mut actual = Vec::new();
    collect_files(fixtures, fixtures, &mut actual)?;
    let actual: BTreeSet<String> = actual
        .into_iter()
        .map(|item| item.to_string_lossy().replace('\\', "/"))
        .collect();
    let declared: BTreeSet<String> = manifest_paths.into_iter().collect();
    if actual != declared {
        return Err(fail(
            &path,
            "MANIFEST file set differs from fixture directory",
        ));
    }
    Ok(())
}

fn verify_qr(fixtures: &Path) -> Result<(), Box<dyn Error>> {
    for (name, case) in [
        ("vector_canonical.json", "canonical"),
        ("vector_leading_zero.json", "leading_zero"),
        ("vector_rejection_sampling.json", "rejection_sampling"),
    ] {
        let path = fixtures.join("qr01a").join(name);
        let vector = json_file(&path)?;
        if string(&path, &vector, "fixture_schema")? != "phoneboost.qr01a.v1"
            || string(&path, &vector, "case")? != case
        {
            return Err(fail(&path, "QR schema/case mismatch"));
        }
        if decode_hex(&path, &vector, "prologue_hex")? != PROLOGUE
            || string(&path, &vector, "prologue_ascii")?.as_bytes() != PROLOGUE
        {
            return Err(fail(&path, "locked prologue mismatch"));
        }
        if decode_hex(&path, &vector, "sas_domain_hex")? != SAS_DOMAIN {
            return Err(fail(&path, "locked SAS domain mismatch"));
        }
        for key in [
            "static_private_linux_hex",
            "static_private_android_hex",
            "rng_seed_linux_hex",
            "rng_seed_android_hex",
            "static_public_linux_hex",
            "static_public_android_hex",
            "ephemeral_public_linux_hex",
            "ephemeral_public_android_hex",
            "handshake_hash_hex",
            "material_hex",
            "block_hex",
        ] {
            if decode_hex(&path, &vector, key)?.len() != 32 {
                return Err(fail(&path, format!("{key} is not 32 bytes")));
            }
        }
        let message_1 = decode_hex(&path, &vector, "handshake_msg_1_hex")?;
        let message_2 = decode_hex(&path, &vector, "handshake_msg_2_hex")?;
        let message_3 = decode_hex(&path, &vector, "handshake_msg_3_hex")?;
        if message_1.len() < 32 || message_2.len() < 32 || message_3.is_empty() {
            return Err(fail(&path, "handshake messages are absent/truncated"));
        }
        if message_1[..32] != decode_hex(&path, &vector, "ephemeral_public_linux_hex")?
            || message_2[..32] != decode_hex(&path, &vector, "ephemeral_public_android_hex")?
        {
            return Err(fail(
                &path,
                "ephemeral diagnostic does not match handshake messages",
            ));
        }

        let handshake_hash = decode_hex(&path, &vector, "handshake_hash_hex")?;
        let mut material_input = Vec::from(SAS_DOMAIN.as_slice());
        material_input.extend_from_slice(&handshake_hash);
        let material = sha256(&material_input);
        if material.as_slice() != decode_hex(&path, &vector, "material_hex")? {
            return Err(fail(&path, "SAS material arithmetic mismatch"));
        }
        let counter = u64_field(&path, &vector, "counter")?;
        if counter > u32::MAX as u64 {
            return Err(fail(&path, "SAS counter exceeds u32"));
        }
        for rejected_counter in 0..counter as u32 {
            if sas_candidate(&material, rejected_counter) < 16_000_000 {
                return Err(fail(
                    &path,
                    "an earlier SAS candidate should have been accepted",
                ));
            }
        }
        let (block, candidate) = sas_block_candidate(&material, counter as u32);
        if candidate >= 16_000_000 {
            return Err(fail(&path, "stored final SAS candidate is not accepted"));
        }
        if block.as_slice() != decode_hex(&path, &vector, "block_hex")?
            || candidate as u64 != u64_field(&path, &vector, "candidate_n")?
        {
            return Err(fail(&path, "SAS block/candidate mismatch"));
        }
        let sas = string(&path, &vector, "sas")?;
        if sas.len() != 6 || sas != format!("{:06}", candidate % 1_000_000) {
            return Err(fail(&path, "SAS is not the exact six-digit result"));
        }
        if case == "leading_zero" && !sas.starts_with('0') {
            return Err(fail(&path, "leading-zero case has no leading zero"));
        }
        if case == "rejection_sampling"
            && (counter == 0 || u64_field(&path, &vector, "candidate_0_n")? < 16_000_000)
        {
            return Err(fail(&path, "rejection-sampling predicate is not satisfied"));
        }
    }

    let path = fixtures.join("qr01a/vector_prologue_mismatch.json");
    let mismatch = json_file(&path)?;
    if bool_field(&path, &mismatch, "successful_xx")?
        || bool_field(&path, &mismatch, "sas_emitted")?
        || u64_field(&path, &mismatch, "changed_byte_index")? != 0
        || u64_field(&path, &mismatch, "xor_mask")? != 1
    {
        return Err(fail(&path, "prologue mismatch outcome is not fail-closed"));
    }
    let initiator = decode_hex(&path, &mismatch, "initiator_prologue_hex")?;
    let responder = decode_hex(&path, &mismatch, "responder_prologue_hex")?;
    let differences = initiator
        .iter()
        .zip(&responder)
        .filter(|(left, right)| left != right)
        .count();
    if differences != 1 || initiator[0] ^ responder[0] != 1 {
        return Err(fail(
            &path,
            "prologue mismatch changes more than byte 0 XOR 1",
        ));
    }
    Ok(())
}

fn sas_block_candidate(material: &[u8; 32], counter: u32) -> ([u8; 32], u32) {
    let mut input = Vec::with_capacity(36);
    input.extend_from_slice(material);
    input.extend_from_slice(&counter.to_be_bytes());
    let block = sha256(&input);
    let candidate = u32::from_be_bytes([0, block[0], block[1], block[2]]);
    (block, candidate)
}

fn sas_candidate(material: &[u8; 32], counter: u32) -> u32 {
    sas_block_candidate(material, counter).1
}

fn be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("fixed offset"))
}

fn be_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().expect("fixed offset"))
}

fn parse_header(path: &Path, bytes: &[u8]) -> Result<ManualHeader, Box<dyn Error>> {
    if bytes.len() < HEADER_LEN {
        return Err(fail(path, "frame shorter than 40-byte header"));
    }
    let header = ManualHeader {
        magic: bytes[0..4].try_into().expect("fixed offset"),
        version: bytes[4],
        channel: bytes[5],
        flags: be_u16(bytes, 6),
        message_type: be_u16(bytes, 8),
        header_len: be_u16(bytes, 10),
        request_id: be_u64(bytes, 12),
        sequence: be_u64(bytes, 20),
        fragment_index: be_u32(bytes, 28),
        payload_len: be_u32(bytes, 32),
        logical_message_len: be_u32(bytes, 36),
    };
    if header.magic != *b"PBM1"
        || header.version != 1
        || header.header_len as usize != HEADER_LEN
        || header.channel > 5
        || header.flags & !0x000f != 0
        || HEADER_LEN + header.payload_len as usize != bytes.len()
        || header.payload_len as usize > MAX_PAYLOAD
    {
        return Err(fail(path, "manual PBMUX header validation failed"));
    }
    Ok(header)
}

fn manual_pair_confirm_bytes(request_id: u64, sequence: u64) -> [u8; HEADER_LEN] {
    let mut bytes = [0_u8; HEADER_LEN];
    bytes[0..4].copy_from_slice(b"PBM1");
    bytes[4] = 1;
    bytes[5] = 0;
    bytes[6..8].copy_from_slice(&3_u16.to_be_bytes());
    bytes[8..10].copy_from_slice(&8_u16.to_be_bytes());
    bytes[10..12].copy_from_slice(&(HEADER_LEN as u16).to_be_bytes());
    bytes[12..20].copy_from_slice(&request_id.to_be_bytes());
    bytes[20..28].copy_from_slice(&sequence.to_be_bytes());
    bytes
}

fn verify_control8(fixtures: &Path) -> Result<(), Box<dyn Error>> {
    let bin_path = fixtures.join("control8/pair_confirm_empty.bin");
    let bytes = fs::read(&bin_path)?;
    let expected = manual_pair_confirm_bytes(0x0102_0304_0506_0708, 0);
    if bytes != expected {
        return Err(fail(
            &bin_path,
            "independent CONTROL/8 construction differs",
        ));
    }
    let header = parse_header(&bin_path, &bytes)?;
    if header.message_type != 8
        || header.request_id == 0
        || header.fragment_index != 0
        || header.logical_message_len != 0
        || header.payload_len != 0
    {
        return Err(fail(&bin_path, "PAIR_CONFIRM fields are not exact"));
    }
    let json_path = fixtures.join("control8/pair_confirm_empty.json");
    let metadata = json_file(&json_path)?;
    if decode_hex(&json_path, &metadata, "frame_hex")? != bytes {
        return Err(fail(&json_path, "JSON frame differs from binary"));
    }
    let duplicate_path = fixtures.join("control8/pair_confirm_duplicate.json");
    let duplicate = json_file(&duplicate_path)?;
    if !bool_field(&duplicate_path, &duplicate, "first_state_changed")?
        || bool_field(&duplicate_path, &duplicate, "second_state_changed")?
        || !bool_field(&duplicate_path, &duplicate, "final_peer_confirmed")?
    {
        return Err(fail(
            &duplicate_path,
            "duplicate PAIR_CONFIRM is not idempotent",
        ));
    }
    for key in ["frame_1_hex", "frame_2_hex"] {
        let frame = decode_hex(&duplicate_path, &duplicate, key)?;
        let parsed = parse_header(&duplicate_path, &frame)?;
        if parsed.message_type != 8 || parsed.request_id == 0 {
            return Err(fail(
                &duplicate_path,
                format!("{key} is not valid CONTROL/8"),
            ));
        }
    }
    Ok(())
}

fn verify_pbmux(fixtures: &Path) -> Result<(), Box<dyn Error>> {
    let header_path = fixtures.join("pbmux/header_golden.bin");
    let header_bytes = fs::read(&header_path)?;
    let header = parse_header(&header_path, &header_bytes)?;
    if header.channel != 0
        || header.flags != 3
        || header.message_type != 1
        || header.request_id != 0x1122_3344_5566_7788
        || header.sequence != 0x0102_0304_0506_0708
    {
        return Err(fail(&header_path, "header golden offsets/values differ"));
    }

    let max_path = fixtures.join("pbmux/max_payload_60kib.bin");
    let max_bytes = fs::read(&max_path)?;
    let max_header = parse_header(&max_path, &max_bytes)?;
    if max_header.payload_len as usize != MAX_PAYLOAD
        || max_bytes.len() != MAX_PLAINTEXT
        || MAX_PLAINTEXT + 16 != MAX_CIPHERTEXT
    {
        return Err(fail(
            &max_path,
            "maximum payload/plaintext/ciphertext arithmetic differs",
        ));
    }
    for (index, byte) in max_bytes[HEADER_LEN..].iter().enumerate() {
        if *byte != index as u8 {
            return Err(fail(
                &max_path,
                format!("payload rule mismatch at byte {index}"),
            ));
        }
    }

    verify_fragmentation(&fixtures.join("pbmux/fragmentation_4mib.json"))?;
    let channel_path = fixtures.join("pbmux/reassembly_channel_limit.json");
    let channel = json_file(&channel_path)?;
    if u64_field(&channel_path, &channel, "channel_limit_bytes")? != 8 * 1024 * 1024
        || !bool_field(&channel_path, &channel, "third_full_start_rejected")?
        || !bool_field(&channel_path, &channel, "seventeenth_start_rejected")?
        || u64_field(&channel_path, &channel, "bytes_before_rejected_start")?
            != u64_field(&channel_path, &channel, "bytes_after_rejected_start")?
        || u64_field(&channel_path, &channel, "count_before_rejected_start")?
            != u64_field(&channel_path, &channel, "count_after_rejected_start")?
    {
        return Err(fail(&channel_path, "channel/count quota evidence mismatch"));
    }
    let session_path = fixtures.join("pbmux/reassembly_session_limit.json");
    let session = json_file(&session_path)?;
    if u64_field(&session_path, &session, "session_limit_bytes")? != 16 * 1024 * 1024
        || !bool_field(&session_path, &session, "fifth_full_start_rejected")?
        || u64_field(&session_path, &session, "bytes_before_rejected_start")?
            != u64_field(&session_path, &session, "bytes_after_rejected_start")?
    {
        return Err(fail(&session_path, "session quota evidence mismatch"));
    }
    for name in ["sequence_gap.json", "sequence_duplicate.json"] {
        let path = fixtures.join("pbmux").join(name);
        let value = json_file(&path)?;
        if string(&path, &value, "error_scope")? != "SESSION"
            || string(&path, &value, "action")? != "close_session"
        {
            return Err(fail(&path, "sequence error does not close session"));
        }
    }
    let mismatch_path = fixtures.join("pbmux/payload_len_mismatch.json");
    let mismatch = json_file(&mismatch_path)?;
    let mismatch_bytes = decode_hex(&mismatch_path, &mismatch, "frame_hex")?;
    if mismatch_bytes.len() != HEADER_LEN
        || be_u32(&mismatch_bytes, 32) != 1
        || u64_field(&mismatch_path, &mismatch, "allocation_before")?
            != u64_field(&mismatch_path, &mismatch, "allocation_after")?
    {
        return Err(fail(
            &mismatch_path,
            "payload mismatch/pre-allocation evidence differs",
        ));
    }
    Ok(())
}

fn verify_fragmentation(path: &Path) -> Result<(), Box<dyn Error>> {
    let value = json_file(path)?;
    let frames = value
        .get("frames_hex")
        .and_then(Value::as_array)
        .ok_or_else(|| fail(path, "frames_hex is missing"))?;
    if frames.len() != 69
        || u64_field(path, &value, "logical_message_len")? != MAX_LOGICAL as u64
        || u64_field(path, &value, "final_payload_len")? != 16_384
    {
        return Err(fail(path, "fragment count/logical length/tail differs"));
    }
    let mut logical = Vec::with_capacity(MAX_LOGICAL);
    for (index, encoded) in frames.iter().enumerate() {
        let encoded = encoded
            .as_str()
            .ok_or_else(|| fail(path, format!("frame {index} is not hex text")))?;
        let bytes = hex::decode(encoded)?;
        let header = parse_header(path, &bytes)?;
        let first = index == 0;
        let last = index + 1 == frames.len();
        let expected_flags = (if first { 1 } else { 0 }) | (if last { 2 } else { 0 });
        if header.channel != 2
            || header.message_type != 5
            || header.flags != expected_flags
            || header.sequence != index as u64
            || header.fragment_index != index as u32
            || header.logical_message_len != if first { MAX_LOGICAL as u32 } else { 0 }
        {
            return Err(fail(path, format!("fragment {index} header mismatch")));
        }
        logical.extend_from_slice(&bytes[HEADER_LEN..]);
    }
    if logical.len() != MAX_LOGICAL
        || hex::encode(sha256(&logical)) != string(path, &value, "logical_payload_sha256")?
    {
        return Err(fail(path, "reassembled logical payload/hash mismatch"));
    }
    let (blocks, remainder) = logical.as_chunks::<32>();
    if !remainder.is_empty() {
        return Err(fail(path, "D-01 payload is not a whole number of blocks"));
    }
    for (block_index, block) in blocks.iter().enumerate() {
        if block != &sha256(&(block_index as u64).to_be_bytes()) {
            return Err(fail(
                path,
                format!("D-01 payload mismatch at block {block_index}"),
            ));
        }
    }
    Ok(())
}

fn verify_pairing(fixtures: &Path) -> Result<(), Box<dyn Error>> {
    let before_path = fixtures.join("pairing/peer_confirm_before_local.json");
    let before = json_file(&before_path)?;
    if !bool_field(&before_path, &before, "peer_confirmed")?
        || bool_field(&before_path, &before, "local_confirmed")?
        || bool_field(&before_path, &before, "persist_commit")?
    {
        return Err(fail(
            &before_path,
            "peer-confirm-before-local semantics differ",
        ));
    }
    let duplicate_path = fixtures.join("pairing/duplicate_pair_confirm.json");
    let duplicate = json_file(&duplicate_path)?;
    if bool_field(&duplicate_path, &duplicate, "second_state_changed")?
        || bool_field(&duplicate_path, &duplicate, "local_confirm_persist_commit")?
        || !bool_field(&duplicate_path, &duplicate, "begin_commit_persist_commit")?
        || u64_field(&duplicate_path, &duplicate, "total_persist_intents")? != 1
        || string(&duplicate_path, &duplicate, "state")? != "TRUST_COMMITTING"
    {
        return Err(fail(
            &duplicate_path,
            "pairing duplicate caused a second change/commit",
        ));
    }
    let one_sided_path = fixtures.join("pairing/one_sided_commit_recovery.json");
    let one_sided = json_file(&one_sided_path)?;
    for key in [
        "responder_exact_key_match",
        "fresh_xx_required",
        "fresh_sas_displayed_by_responder",
        "initiator_human_confirmation_required",
    ] {
        if !bool_field(&one_sided_path, &one_sided, key)? {
            return Err(fail(&one_sided_path, format!("{key} is not true")));
        }
    }
    let crash_path = fixtures.join("pairing/crash_after_commit_before_ui.json");
    let crash = json_file(&crash_path)?;
    if string(&crash_path, &crash, "restart_record")? != "COMMITTED"
        || bool_field(&crash_path, &crash, "false_loss_of_trust")?
    {
        return Err(fail(&crash_path, "crash-after-commit recovery differs"));
    }
    let prior_path = fixtures.join("pairing/prior_committed_recovery.json");
    let prior = json_file(&prior_path)?;
    if !bool_field(&prior_path, &prior, "exact_key_match_enables_recovery")?
        || !bool_field(&prior_path, &prior, "different_key_disables_recovery")?
        || !bool_field(&prior_path, &prior, "fresh_sas_display_required")?
        || bool_field(&prior_path, &prior, "automatic_pin_replacement")?
    {
        return Err(fail(
            &prior_path,
            "PRIOR_COMMITTED recovery semantics differ",
        ));
    }
    verify_prior_committed_exchange(&one_sided_path, &one_sided)?;
    verify_prior_committed_exchange(&prior_path, &prior)?;
    Ok(())
}

fn verify_prior_committed_exchange(path: &Path, fixture: &Value) -> Result<(), Box<dyn Error>> {
    let exchange = fixture
        .get("exchange")
        .ok_or_else(|| fail(path, "missing exchange evidence"))?;
    for key in [
        "exact_key_match",
        "fresh_sas_displayed_by_responder",
        "fresh_sas_displayed_by_initiator",
        "initiator_human_confirmation_required",
        "both_mutual_before_commit",
    ] {
        if !bool_field(path, exchange, key)? {
            return Err(fail(path, format!("exchange {key} is not true")));
        }
    }
    if u64_field(path, exchange, "responder_pair_confirm_send_count")? != 1
        || u64_field(path, exchange, "initiator_pair_confirm_send_count")? != 1
        || u64_field(path, exchange, "total_persist_intents")? != 2
    {
        return Err(fail(path, "CONTROL/8 or PERSIST_ATOMIC count differs"));
    }

    for key in ["responder_pair_confirm_hex", "initiator_pair_confirm_hex"] {
        let bytes = decode_hex(path, exchange, key)?;
        let header = parse_header(path, &bytes)?;
        if bytes.len() != HEADER_LEN
            || header.channel != 0
            || header.flags != 3
            || header.message_type != 8
            || header.request_id == 0
            || header.fragment_index != 0
            || header.payload_len != 0
            || header.logical_message_len != 0
        {
            return Err(fail(path, format!("{key} is not exact empty CONTROL/8")));
        }
    }

    verify_pairing_steps(
        path,
        exchange,
        "responder_steps",
        &[
            ("LOCAL_CONFIRMED", true, true, false),
            ("LOCAL_CONFIRMED", false, false, false),
            ("MUTUAL_CONFIRMED", true, false, false),
            ("TRUST_COMMITTING", true, false, true),
            ("TRUST_COMMITTING", false, false, false),
            ("PAIRED", true, false, false),
        ],
    )?;
    verify_pairing_steps(
        path,
        exchange,
        "initiator_steps",
        &[
            ("PEER_CONFIRMED", true, false, false),
            ("MUTUAL_CONFIRMED", true, true, false),
            ("TRUST_COMMITTING", true, false, true),
            ("TRUST_COMMITTING", false, false, false),
            ("PAIRED", true, false, false),
        ],
    )?;

    let different = exchange
        .get("different_key_attempt")
        .ok_or_else(|| fail(path, "different-key evidence missing"))?;
    if string(path, different, "state")? != "SAS_PENDING"
        || bool_field(path, different, "state_changed")?
        || bool_field(path, different, "send_pair_confirm")?
        || bool_field(path, different, "persist_commit")?
        || string(path, exchange, "different_key_final_state")? != "SAS_PENDING"
    {
        return Err(fail(path, "different key did not disable recovery"));
    }
    Ok(())
}

fn verify_pairing_steps(
    path: &Path,
    exchange: &Value,
    key: &str,
    expected: &[(&str, bool, bool, bool)],
) -> Result<(), Box<dyn Error>> {
    let steps = exchange
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| fail(path, format!("{key} is not an array")))?;
    if steps.len() != expected.len() {
        return Err(fail(path, format!("{key} length differs")));
    }
    for (step, (state, changed, send, persist)) in steps.iter().zip(expected) {
        if string(path, step, "state")? != *state
            || bool_field(path, step, "state_changed")? != *changed
            || bool_field(path, step, "send_pair_confirm")? != *send
            || bool_field(path, step, "persist_commit")? != *persist
        {
            return Err(fail(path, format!("{key} transition differs at {state}")));
        }
    }
    Ok(())
}

fn verify_pairing_guard(fixtures: &Path) -> Result<(), Box<dyn Error>> {
    let one_path = fixtures.join("pairing_guard/mismatch_1.json");
    let one = json_file(&one_path)?;
    if u64_field(&one_path, &one, "mismatch_count")? != 1
        || !one
            .get("cooldown_until_wall_ms")
            .is_some_and(Value::is_null)
    {
        return Err(fail(&one_path, "first mismatch guard state differs"));
    }
    let three_path = fixtures.join("pairing_guard/mismatch_3_cooldown.json");
    let three = json_file(&three_path)?;
    if u64_field(&three_path, &three, "mismatch_count")? != 3 {
        return Err(fail(&three_path, "third mismatch count differs"));
    }
    let until = u64_field(&three_path, &three, "cooldown_until_wall_ms")?;
    let updated = u64_field(&three_path, &three, "updated_wall_ms")?;
    if until != updated + 600_000 {
        return Err(fail(&three_path, "cooldown is not exactly +600000 ms"));
    }
    for name in [
        "restart_during_cooldown.json",
        "changed_untrusted_peer_key.json",
    ] {
        let path = fixtures.join("pairing_guard").join(name);
        let value = json_file(&path)?;
        let extra = value
            .get("extra")
            .ok_or_else(|| fail(&path, "extra evidence missing"))?;
        if extra.get("pairing_admitted").and_then(Value::as_bool) != Some(false) {
            return Err(fail(&path, "cooldown was bypassed"));
        }
    }
    let cancel_path = fixtures.join("pairing_guard/user_cancel_no_increment.json");
    let cancel = json_file(&cancel_path)?;
    let extra = cancel
        .get("extra")
        .ok_or_else(|| fail(&cancel_path, "cancel evidence missing"))?;
    if extra.get("mismatch_count_before") != extra.get("mismatch_count_after")
        || extra.get("state_changed").and_then(Value::as_bool) != Some(false)
    {
        return Err(fail(&cancel_path, "USER_CANCELLED changed the guard"));
    }
    Ok(())
}

fn recursively_collect_artifacts(
    current: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            recursively_collect_artifacts(&path, output)?;
        } else {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let core_name = name.starts_with("libpb_types")
                || name.starts_with("libpb_pbmux")
                || name.starts_with("libpb_secure");
            let supported = matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("rlib" | "rmeta" | "so" | "a")
            );
            if core_name && supported {
                output.push(path);
            }
        }
    }
    Ok(())
}

fn scan_core_artifacts(directory: &Path, fixtures: &Path) -> Result<usize, Box<dyn Error>> {
    let mut patterns: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    patterns.insert(
        "static-linux".to_owned(),
        sha256(b"PHONEBOOST-FIXTURE-STATIC-LINUX-V1\0").to_vec(),
    );
    patterns.insert(
        "static-android".to_owned(),
        sha256(b"PHONEBOOST-FIXTURE-STATIC-ANDROID-V1\0").to_vec(),
    );
    patterns.insert(
        "fixture-rng-domain".to_owned(),
        b"PHONEBOOST-FIXTURE-RNG-V1\0".to_vec(),
    );
    for name in [
        "vector_canonical.json",
        "vector_leading_zero.json",
        "vector_rejection_sampling.json",
    ] {
        let path = fixtures.join("qr01a").join(name);
        let value = json_file(&path)?;
        for role in ["linux", "android"] {
            let key = format!("rng_seed_{role}_hex");
            patterns.insert(format!("{name}:{role}"), decode_hex(&path, &value, &key)?);
        }
    }
    let mut artifacts = Vec::new();
    recursively_collect_artifacts(directory, &mut artifacts)?;
    artifacts.sort();
    if artifacts.is_empty() {
        return Err(fail(directory, "no core release artifacts found"));
    }
    for artifact in &artifacts {
        let bytes = fs::read(artifact)?;
        for (label, pattern) in &patterns {
            if pattern.len() <= bytes.len()
                && bytes.windows(pattern.len()).any(|window| window == pattern)
            {
                return Err(fail(
                    artifact,
                    format!("contains TEST-ONLY pattern {label}"),
                ));
            }
        }
    }
    Ok(artifacts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_pair_confirm_is_locked_40_bytes() {
        let bytes = manual_pair_confirm_bytes(0x0102_0304_0506_0708, 0);
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(
            hex::encode(bytes),
            "50424d31010000030008002801020304050607080000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn sas_candidate_uses_be24() {
        let material = sha256(b"checker-test");
        let (block, candidate) = sas_block_candidate(&material, 0);
        assert_eq!(
            candidate,
            u32::from_be_bytes([0, block[0], block[1], block[2]])
        );
    }
}
