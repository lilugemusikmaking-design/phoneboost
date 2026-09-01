#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use pb_fixture_gen::test_only_resolver::DeterministicFixtureResolver;
use pb_pbmux::{
    DispatchMode, Frame, Header, ReassemblyAccounting, SequenceTracker, authorize_dispatch, decode,
    encode, pair_confirm_frame,
};
use pb_secure::{
    NOISE_XX_NAME, PROLOGUE, PairingActor, PairingGuard, PairingTransition, PersistOutcome,
    SAS_DOMAIN, SasDerivation, XxTranscript, complete_xx, derive_sas_diagnostics, noise_xx_params,
    prior_committed_key_matches,
};
use pb_types::{
    Channel, ControlType, FLAG_END, FLAG_START, MAX_LOGICAL_MESSAGE, MAX_NOISE_CIPHERTEXT,
    MAX_PBMUX_PAYLOAD, MAX_PBMUX_PLAINTEXT, MAX_REASSEMBLY_PER_CHANNEL, MAX_REASSEMBLY_PER_SESSION,
    Mutation, PAIRING_COOLDOWN_MS, PBMUX_HEADER_LEN, PairingState,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use snow::Builder;
use snow::params::DHChoice;
use snow::resolvers::{CryptoResolver, DefaultResolver};

const BASE_CLOCK_MS: u64 = 1_700_000_000_000;
const CANONICAL_SOURCE_FILES: [&str; 5] = [
    "PHONEBOOST_SPEC_V0_7_QR01B_LOCKED_IMPLEMENTATION_BASELINE.docx",
    "PHONEBOOST_CONTRACT_SET_V1_3_QR01A_QR01B_LOCKED.docx",
    "PHONEBOOST_TECH_SHEET_V1_3_QR01A_QR01B_LOCKED_IMPLEMENTATION_HANDOFF.docx",
    "PHONEBOOST_PSEUDOCODE_CRITICAL_PATHS_V1_1_FINAL.docx",
    "PHONEBOOST_FIXTURE_GENERATION_SPEC_V1_0.docx",
];

struct Arguments {
    inputs: PathBuf,
    output: PathBuf,
    git_commit: String,
}

#[derive(Clone)]
struct QrVector {
    candidate_index: u64,
    static_linux: [u8; 32],
    static_android: [u8; 32],
    seed_linux: [u8; 32],
    seed_android: [u8; 32],
    public_linux: [u8; 32],
    public_android: [u8; 32],
    ephemeral_linux: [u8; 32],
    ephemeral_android: [u8; 32],
    transcript: XxTranscript,
    sas: SasDerivation,
    candidate_0_n: u32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("pb-fixture-gen: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    create_layout(&args.output)?;

    let canonical = generate_qr_vector(0)?;
    let leading = search_qr_vector(|vector| vector.sas.candidate_n % 1_000_000 < 100_000)?;
    let rejection =
        search_qr_vector(|vector| vector.candidate_0_n >= 16_000_000 && vector.sas.counter >= 1)?;

    write_json(
        &args.output.join("qr01a/vector_canonical.json"),
        &qr_json("canonical", &canonical),
    )?;
    write_json(
        &args.output.join("qr01a/vector_leading_zero.json"),
        &qr_json("leading_zero", &leading),
    )?;
    write_json(
        &args.output.join("qr01a/vector_rejection_sampling.json"),
        &qr_json("rejection_sampling", &rejection),
    )?;
    generate_prologue_mismatch(&args.output, &canonical)?;
    generate_control8(&args.output)?;
    generate_pbmux(&args.output)?;
    generate_pairing(&args.output)?;
    generate_pairing_guard(&args.output)?;
    generate_readmes(&args.output)?;
    generate_manifest(&args)?;

    println!(
        "generated fixtures: canonical k={}, leading-zero k={}, rejection k={}",
        canonical.candidate_index, leading.candidate_index, rejection.candidate_index
    );
    println!("output: {}", args.output.display());
    Ok(())
}

fn parse_args() -> Result<Arguments, Box<dyn Error>> {
    let mut inputs = None;
    let mut output = None;
    let mut git_commit = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--inputs" => inputs = args.next().map(PathBuf::from),
            "--output" => output = args.next().map(PathBuf::from),
            "--git-commit" => git_commit = args.next(),
            _ => return Err(format!("unknown or incomplete argument: {arg}").into()),
        }
    }
    Ok(Arguments {
        inputs: inputs.ok_or("--inputs is required")?,
        output: output.ok_or("--output is required")?,
        git_commit: git_commit.ok_or("--git-commit is required")?,
    })
}

fn create_layout(output: &Path) -> Result<(), Box<dyn Error>> {
    for directory in ["qr01a", "control8", "pbmux", "pairing", "pairing_guard"] {
        fs::create_dir_all(output.join(directory))?;
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(hex::encode(sha256_bytes(&fs::read(path)?)))
}

fn static_private(role: &str) -> [u8; 32] {
    match role {
        "linux" => sha256_bytes(b"PHONEBOOST-FIXTURE-STATIC-LINUX-V1\0"),
        "android" => sha256_bytes(b"PHONEBOOST-FIXTURE-STATIC-ANDROID-V1\0"),
        _ => unreachable!("fixed generator role"),
    }
}

fn role_seed(role: &str, candidate_index: u64) -> [u8; 32] {
    let domain = match role {
        "linux" => b"PHONEBOOST-FIXTURE-SEED-LINUX-V1\0".as_slice(),
        "android" => b"PHONEBOOST-FIXTURE-SEED-ANDROID-V1\0".as_slice(),
        _ => unreachable!("fixed generator role"),
    };
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(candidate_index.to_be_bytes());
    hasher.finalize().into()
}

fn public_key(private: &[u8; 32]) -> Result<[u8; 32], Box<dyn Error>> {
    let resolver = DefaultResolver;
    let mut dh = resolver
        .resolve_dh(&DHChoice::Curve25519)
        .ok_or("snow DefaultResolver lacks Curve25519")?;
    dh.set(private);
    let mut public = [0_u8; 32];
    public.copy_from_slice(dh.pubkey());
    Ok(public)
}

fn candidate_for(material: &[u8; 32], counter: u32) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(material);
    hasher.update(counter.to_be_bytes());
    let block: [u8; 32] = hasher.finalize().into();
    u32::from_be_bytes([0, block[0], block[1], block[2]])
}

fn build_fixture_states(
    static_linux: &[u8; 32],
    static_android: &[u8; 32],
    seed_linux: [u8; 32],
    seed_android: [u8; 32],
    prologue_linux: &[u8],
    prologue_android: &[u8],
) -> Result<(snow::HandshakeState, snow::HandshakeState), Box<dyn Error>> {
    let params = noise_xx_params()?;
    let initiator = Builder::with_resolver(
        params.clone(),
        Box::new(DeterministicFixtureResolver::new(seed_linux)),
    )
    .local_private_key(static_linux)
    .prologue(prologue_linux)
    .build_initiator()?;
    let responder = Builder::with_resolver(
        params,
        Box::new(DeterministicFixtureResolver::new(seed_android)),
    )
    .local_private_key(static_android)
    .prologue(prologue_android)
    .build_responder()?;
    Ok((initiator, responder))
}

fn generate_qr_vector(candidate_index: u64) -> Result<QrVector, Box<dyn Error>> {
    let static_linux = static_private("linux");
    let static_android = static_private("android");
    let seed_linux = role_seed("linux", candidate_index);
    let seed_android = role_seed("android", candidate_index);
    let public_linux = public_key(&static_linux)?;
    let public_android = public_key(&static_android)?;
    let (initiator, responder) = build_fixture_states(
        &static_linux,
        &static_android,
        seed_linux,
        seed_android,
        PROLOGUE,
        PROLOGUE,
    )?;
    let transcript = complete_xx(initiator, responder)?;
    if transcript.message_1.len() < 32 || transcript.message_2.len() < 32 {
        return Err("Noise XX messages are too short for ephemeral diagnostics".into());
    }
    let mut ephemeral_linux = [0_u8; 32];
    ephemeral_linux.copy_from_slice(&transcript.message_1[..32]);
    let mut ephemeral_android = [0_u8; 32];
    ephemeral_android.copy_from_slice(&transcript.message_2[..32]);
    let sas = derive_sas_diagnostics(&transcript.handshake_hash)?;
    let candidate_0_n = candidate_for(&sas.material, 0);
    Ok(QrVector {
        candidate_index,
        static_linux,
        static_android,
        seed_linux,
        seed_android,
        public_linux,
        public_android,
        ephemeral_linux,
        ephemeral_android,
        transcript,
        sas,
        candidate_0_n,
    })
}

fn search_qr_vector(predicate: impl Fn(&QrVector) -> bool) -> Result<QrVector, Box<dyn Error>> {
    for candidate_index in 0..=1_000_000 {
        let vector = generate_qr_vector(candidate_index)?;
        if predicate(&vector) {
            return Ok(vector);
        }
    }
    Err("deterministic QR seed search exhausted at 1,000,000 candidates".into())
}

fn qr_json(name: &str, vector: &QrVector) -> Value {
    json!({
        "fixture_schema": "phoneboost.qr01a.v1",
        "case": name,
        "noise_pattern": NOISE_XX_NAME,
        "prologue_ascii": String::from_utf8_lossy(PROLOGUE),
        "prologue_hex": hex::encode(PROLOGUE),
        "rng_candidate_index": vector.candidate_index,
        "static_private_linux_hex": hex::encode(vector.static_linux),
        "static_private_android_hex": hex::encode(vector.static_android),
        "rng_seed_linux_hex": hex::encode(vector.seed_linux),
        "rng_seed_android_hex": hex::encode(vector.seed_android),
        "static_public_linux_hex": hex::encode(vector.public_linux),
        "static_public_android_hex": hex::encode(vector.public_android),
        "ephemeral_public_linux_hex": hex::encode(vector.ephemeral_linux),
        "ephemeral_public_android_hex": hex::encode(vector.ephemeral_android),
        "handshake_msg_1_hex": hex::encode(&vector.transcript.message_1),
        "handshake_msg_2_hex": hex::encode(&vector.transcript.message_2),
        "handshake_msg_3_hex": hex::encode(&vector.transcript.message_3),
        "handshake_hash_hex": hex::encode(vector.transcript.handshake_hash),
        "sas_domain_hex": hex::encode(SAS_DOMAIN),
        "material_hex": hex::encode(vector.sas.material),
        "candidate_0_n": vector.candidate_0_n,
        "counter": vector.sas.counter,
        "block_hex": hex::encode(vector.sas.block),
        "candidate_n": vector.sas.candidate_n,
        "sas": vector.sas.sas,
    })
}

fn generate_prologue_mismatch(output: &Path, canonical: &QrVector) -> Result<(), Box<dyn Error>> {
    let mut altered = *PROLOGUE;
    altered[0] ^= 0x01;
    let (initiator, responder) = build_fixture_states(
        &canonical.static_linux,
        &canonical.static_android,
        canonical.seed_linux,
        canonical.seed_android,
        PROLOGUE,
        &altered,
    )?;
    let result = complete_xx(initiator, responder);
    if result.is_ok() {
        return Err("prologue mismatch unexpectedly completed Noise XX".into());
    }
    write_json(
        &output.join("qr01a/vector_prologue_mismatch.json"),
        &json!({
            "fixture_schema": "phoneboost.qr01a.prologue_mismatch.v1",
            "noise_pattern": NOISE_XX_NAME,
            "initiator_prologue_hex": hex::encode(PROLOGUE),
            "responder_prologue_hex": hex::encode(altered),
            "changed_byte_index": 0,
            "xor_mask": 1,
            "successful_xx": false,
            "sas_emitted": false,
            "expected": "no successful XX / no SAS",
        }),
    )
}

fn generate_control8(output: &Path) -> Result<(), Box<dyn Error>> {
    let canonical_frame = pair_confirm_frame(0x0102_0304_0506_0708, 0)?;
    let canonical_bytes = encode(&canonical_frame)?;
    let expected =
        "50424d31010000030008002801020304050607080000000000000000000000000000000000000000";
    if hex::encode(&canonical_bytes) != expected {
        return Err("CONTROL/8 generated bytes differ from locked canonical hex".into());
    }
    fs::write(
        output.join("control8/pair_confirm_empty.bin"),
        &canonical_bytes,
    )?;
    write_json(
        &output.join("control8/pair_confirm_empty.json"),
        &json!({
            "fixture_schema": "phoneboost.control8.v1",
            "channel": 0,
            "flags": 3,
            "message_type": 8,
            "header_len": 40,
            "request_id": "0102030405060708",
            "sequence": 0,
            "fragment_index": 0,
            "payload_len": 0,
            "logical_message_len": 0,
            "frame_hex": hex::encode(&canonical_bytes),
            "binary_file": "pair_confirm_empty.bin",
            "binary_size": canonical_bytes.len(),
        }),
    )?;

    let first = pair_confirm_frame(0x1111_1111_1111_1111, 0)?;
    let second = pair_confirm_frame(0x2222_2222_2222_2222, 1)?;
    authorize_dispatch(&first, DispatchMode::PairingControlOnly)?;
    authorize_dispatch(&second, DispatchMode::PairingControlOnly)?;
    let mut actor = PairingActor::new();
    let first_transition = actor.peer_confirm();
    let second_transition = actor.peer_confirm();
    write_json(
        &output.join("control8/pair_confirm_duplicate.json"),
        &json!({
            "fixture_schema": "phoneboost.control8.duplicate.v1",
            "frame_1_hex": hex::encode(encode(&first)?),
            "frame_2_hex": hex::encode(encode(&second)?),
            "request_ids_differ": true,
            "first_state_changed": first_transition.state_changed,
            "second_state_changed": second_transition.state_changed,
            "persist_intents": 0,
            "final_peer_confirmed": actor.peer_confirmed(),
        }),
    )
}

fn single_frame(
    channel: Channel,
    message_type: u16,
    request_id: u64,
    sequence: u64,
    payload: Vec<u8>,
) -> Frame {
    Frame {
        header: Header {
            channel,
            flags: FLAG_START | FLAG_END,
            message_type,
            request_id,
            sequence,
            fragment_index: 0,
            payload_len: payload.len() as u32,
            logical_message_len: payload.len() as u32,
        },
        payload,
    }
}

fn fragmentation_payload() -> Vec<u8> {
    let mut payload = Vec::with_capacity(MAX_LOGICAL_MESSAGE);
    for block_index in 0..(MAX_LOGICAL_MESSAGE / 32) {
        payload.extend_from_slice(&sha256_bytes(&(block_index as u64).to_be_bytes()));
    }
    payload
}

fn generate_pbmux(output: &Path) -> Result<(), Box<dyn Error>> {
    let header_frame = single_frame(
        Channel::Control,
        ControlType::Ping as u16,
        0x1122_3344_5566_7788,
        0x0102_0304_0506_0708,
        Vec::new(),
    );
    let header_bytes = encode(&header_frame)?;
    fs::write(output.join("pbmux/header_golden.bin"), &header_bytes)?;
    write_json(
        &output.join("pbmux/header_golden.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.header.v1",
            "magic_ascii": "PBM1",
            "version": 1,
            "channel": 0,
            "flags": 3,
            "message_type": 1,
            "header_len": 40,
            "request_id": "1122334455667788",
            "sequence": "0102030405060708",
            "fragment_index": 0,
            "payload_len": 0,
            "logical_message_len": 0,
            "frame_hex": hex::encode(&header_bytes),
            "binary_file": "header_golden.bin",
        }),
    )?;

    let payload: Vec<u8> = (0..MAX_PBMUX_PAYLOAD).map(|index| index as u8).collect();
    let max_frame = single_frame(Channel::RemoteBuffer, 5, 0x6000, 0, payload);
    let max_bytes = encode(&max_frame)?;
    if max_bytes.len() != MAX_PBMUX_PLAINTEXT {
        return Err("maximum PBMUX plaintext length mismatch".into());
    }
    fs::write(output.join("pbmux/max_payload_60kib.bin"), &max_bytes)?;
    write_json(
        &output.join("pbmux/max_payload_60kib.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.max_payload.v1",
            "payload_rule": "payload[i] = i mod 256",
            "payload_len": MAX_PBMUX_PAYLOAD,
            "pbmux_plaintext_len": max_bytes.len(),
            "noise_ciphertext_len_with_16_byte_tag": MAX_NOISE_CIPHERTEXT,
            "frame_sha256": hex::encode(sha256_bytes(&max_bytes)),
            "binary_file": "max_payload_60kib.bin",
        }),
    )?;

    let logical = fragmentation_payload();
    let chunks: Vec<&[u8]> = logical.chunks(MAX_PBMUX_PAYLOAD).collect();
    if chunks.len() != 69 || chunks.last().map(|chunk| chunk.len()) != Some(16_384) {
        return Err("D-01 fragmentation did not produce locked 69/16384 layout".into());
    }
    let mut frames_hex = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        let is_first = index == 0;
        let is_last = index + 1 == chunks.len();
        let frame = Frame {
            header: Header {
                channel: Channel::RemoteBuffer,
                flags: (if is_first { FLAG_START } else { 0 })
                    | (if is_last { FLAG_END } else { 0 }),
                message_type: 5,
                request_id: 0x0040_0000,
                sequence: index as u64,
                fragment_index: index as u32,
                payload_len: chunk.len() as u32,
                logical_message_len: if is_first {
                    MAX_LOGICAL_MESSAGE as u32
                } else {
                    0
                },
            },
            payload: chunk.to_vec(),
        };
        frames_hex.push(hex::encode(encode(&frame)?));
    }
    write_json(
        &output.join("pbmux/fragmentation_4mib.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.fragmentation.v1",
            "payload_rule": "block=SHA256(u64_be(i/32)); byte[i]=block[i mod 32]",
            "logical_message_len": MAX_LOGICAL_MESSAGE,
            "logical_payload_sha256": hex::encode(sha256_bytes(&logical)),
            "fragment_count": frames_hex.len(),
            "full_fragment_count": 68,
            "final_payload_len": 16_384,
            "frames_hex": frames_hex,
        }),
    )?;

    generate_reassembly_limits(output)?;

    let mut gap_tracker = SequenceTracker::default();
    gap_tracker.accept(0)?;
    let gap_error = gap_tracker.accept(2).expect_err("sequence gap must fail");
    write_json(
        &output.join("pbmux/sequence_gap.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.sequence.v1",
            "expected_sequence": 1,
            "received_sequence": 2,
            "error_kind": format!("{:?}", gap_error.kind),
            "error_scope": "SESSION",
            "action": "close_session",
        }),
    )?;
    let mut duplicate_tracker = SequenceTracker::default();
    duplicate_tracker.accept(0)?;
    let duplicate_error = duplicate_tracker
        .accept(0)
        .expect_err("sequence duplicate must fail");
    write_json(
        &output.join("pbmux/sequence_duplicate.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.sequence.v1",
            "expected_sequence": 1,
            "received_sequence": 0,
            "error_kind": format!("{:?}", duplicate_error.kind),
            "error_scope": "SESSION",
            "action": "close_session",
        }),
    )?;

    let malformed_valid = encode(&single_frame(Channel::Control, 1, 7, 0, vec![0xaa]))?;
    let malformed = &malformed_valid[..PBMUX_HEADER_LEN];
    let malformed_error = decode(malformed).expect_err("payload mismatch must fail");
    write_json(
        &output.join("pbmux/payload_len_mismatch.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.payload_mismatch.v1",
            "frame_hex": hex::encode(malformed),
            "declared_payload_len": 1,
            "actual_payload_len": 0,
            "error_kind": format!("{:?}", malformed_error.kind),
            "allocation_before": 0,
            "allocation_after": 0,
        }),
    )
}

fn generate_reassembly_limits(output: &Path) -> Result<(), Box<dyn Error>> {
    let mut channel = ReassemblyAccounting::default();
    let first = channel.try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)?;
    let second = channel.try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)?;
    let before = channel.channel_bytes(Channel::RemoteBuffer);
    let rejected = channel
        .try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)
        .is_err();
    let after = channel.channel_bytes(Channel::RemoteBuffer);
    channel.finish(first);
    channel.finish(second);

    let mut count = ReassemblyAccounting::default();
    let mut tickets = Vec::new();
    for _ in 0..16 {
        tickets.push(count.try_start(Channel::Compute, 1)?);
    }
    let count_before = count.channel_count(Channel::Compute);
    let count_rejected = count.try_start(Channel::Compute, 1).is_err();
    let count_after = count.channel_count(Channel::Compute);
    for ticket in tickets {
        count.finish(ticket);
    }
    write_json(
        &output.join("pbmux/reassembly_channel_limit.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.reassembly_limit.v1",
            "channel_limit_bytes": MAX_REASSEMBLY_PER_CHANNEL,
            "logical_message_max": MAX_LOGICAL_MESSAGE,
            "accepted_full_messages": 2,
            "third_full_start_rejected": rejected,
            "bytes_before_rejected_start": before,
            "bytes_after_rejected_start": after,
            "data_inflight_limit": 16,
            "count_before_rejected_start": count_before,
            "seventeenth_start_rejected": count_rejected,
            "count_after_rejected_start": count_after,
        }),
    )?;

    let mut session = ReassemblyAccounting::default();
    let mut tickets = Vec::new();
    for channel_id in [
        Channel::Resource,
        Channel::RemoteBuffer,
        Channel::Compute,
        Channel::AiRpc,
    ] {
        tickets.push(session.try_start(channel_id, MAX_LOGICAL_MESSAGE)?);
    }
    let session_before = session.session_bytes();
    let fifth_rejected = session
        .try_start(Channel::Metrics, MAX_LOGICAL_MESSAGE)
        .is_err();
    let session_after = session.session_bytes();
    for ticket in tickets {
        session.finish(ticket);
    }
    write_json(
        &output.join("pbmux/reassembly_session_limit.json"),
        &json!({
            "fixture_schema": "phoneboost.pbmux.reassembly_limit.v1",
            "session_limit_bytes": MAX_REASSEMBLY_PER_SESSION,
            "accepted_full_messages": 4,
            "fifth_full_start_rejected": fifth_rejected,
            "bytes_before_rejected_start": session_before,
            "bytes_after_rejected_start": session_after,
        }),
    )
}

fn state_name(state: PairingState) -> &'static str {
    match state {
        PairingState::Unpaired => "UNPAIRED",
        PairingState::PairingXx => "PAIRING_XX",
        PairingState::SasPending => "SAS_PENDING",
        PairingState::LocalConfirmed => "LOCAL_CONFIRMED",
        PairingState::PeerConfirmed => "PEER_CONFIRMED",
        PairingState::MutualConfirmed => "MUTUAL_CONFIRMED",
        PairingState::TrustCommitting => "TRUST_COMMITTING",
        PairingState::Paired => "PAIRED",
        PairingState::PairRejected => "PAIR_REJECTED",
        PairingState::PairingFailed => "PAIRING_FAILED",
    }
}

fn pairing_step(event: &str, transition: Mutation<PairingTransition>) -> Value {
    json!({
        "event": event,
        "state": state_name(transition.value.state),
        "state_changed": transition.state_changed,
        "send_pair_confirm": transition.value.send_pair_confirm,
        "persist_commit": transition.value.persist_commit,
    })
}

fn prior_committed_exchange(
    committed_key: &[u8; 32],
    presented_key: &[u8; 32],
    different_key: &[u8; 32],
) -> Result<Value, Box<dyn Error>> {
    let mut mismatch_actor = PairingActor::new();
    let mismatch = mismatch_actor.prior_committed_local_basis(committed_key, different_key);

    let mut responder = PairingActor::new();
    let mut initiator = PairingActor::new();
    let responder_fresh_sas = responder.fresh_sas_display_required();
    let initiator_fresh_sas = initiator.fresh_sas_display_required();

    let responder_basis = responder.prior_committed_local_basis(committed_key, presented_key);
    let responder_duplicate = responder.prior_committed_local_basis(committed_key, presented_key);
    if !responder_basis.value.send_pair_confirm {
        return Err("exact PRIOR_COMMITTED key did not emit CONTROL/8".into());
    }
    let responder_frame = pair_confirm_frame(0x4242_0000_0000_0001, 0)?;
    authorize_dispatch(&responder_frame, DispatchMode::PairingControlOnly)?;
    let initiator_peer = initiator.peer_confirm();

    let initiator_human = initiator.local_confirm();
    if !initiator_human.value.send_pair_confirm {
        return Err("initiator human confirmation did not emit CONTROL/8".into());
    }
    let initiator_frame = pair_confirm_frame(0x4141_0000_0000_0001, 0)?;
    authorize_dispatch(&initiator_frame, DispatchMode::PairingControlOnly)?;
    let responder_peer = responder.peer_confirm();

    let both_mutual_before_commit = responder.state() == PairingState::MutualConfirmed
        && initiator.state() == PairingState::MutualConfirmed;
    let responder_commit = responder.begin_trust_commit();
    let initiator_commit = initiator.begin_trust_commit();
    let responder_duplicate_commit = responder.begin_trust_commit();
    let initiator_duplicate_commit = initiator.begin_trust_commit();
    let responder_paired = responder.persist_result(PersistOutcome::Succeeded)?;
    let initiator_paired = initiator.persist_result(PersistOutcome::Succeeded)?;

    let responder_send_count = usize::from(responder_basis.value.send_pair_confirm)
        + usize::from(responder_duplicate.value.send_pair_confirm);
    let initiator_send_count = usize::from(initiator_human.value.send_pair_confirm);
    let persist_intent_count = usize::from(responder_commit.value.persist_commit)
        + usize::from(initiator_commit.value.persist_commit)
        + usize::from(responder_duplicate_commit.value.persist_commit)
        + usize::from(initiator_duplicate_commit.value.persist_commit);

    Ok(json!({
        "exact_key_match": prior_committed_key_matches(committed_key, presented_key),
        "fresh_sas_displayed_by_responder": responder_fresh_sas,
        "fresh_sas_displayed_by_initiator": initiator_fresh_sas,
        "initiator_human_confirmation_required": true,
        "responder_pair_confirm_hex": hex::encode(encode(&responder_frame)?),
        "initiator_pair_confirm_hex": hex::encode(encode(&initiator_frame)?),
        "responder_pair_confirm_send_count": responder_send_count,
        "initiator_pair_confirm_send_count": initiator_send_count,
        "both_mutual_before_commit": both_mutual_before_commit,
        "total_persist_intents": persist_intent_count,
        "responder_steps": [
            pairing_step("PRIOR_COMMITTED_LOCAL_BASIS", responder_basis),
            pairing_step("DUPLICATE_LOCAL_BASIS", responder_duplicate),
            pairing_step("PEER_CONTROL_8", responder_peer),
            pairing_step("BEGIN_TRUST_COMMIT", responder_commit),
            pairing_step("DUPLICATE_BEGIN_TRUST_COMMIT", responder_duplicate_commit),
            pairing_step("PERSIST_ATOMIC_SUCCEEDED", responder_paired),
        ],
        "initiator_steps": [
            pairing_step("PEER_CONTROL_8", initiator_peer),
            pairing_step("HUMAN_CONFIRMS_FRESH_SAS", initiator_human),
            pairing_step("BEGIN_TRUST_COMMIT", initiator_commit),
            pairing_step("DUPLICATE_BEGIN_TRUST_COMMIT", initiator_duplicate_commit),
            pairing_step("PERSIST_ATOMIC_SUCCEEDED", initiator_paired),
        ],
        "different_key_attempt": pairing_step("DIFFERENT_KEY", mismatch),
        "different_key_final_state": state_name(mismatch_actor.state()),
    }))
}

fn generate_pairing(output: &Path) -> Result<(), Box<dyn Error>> {
    let mut before_local = PairingActor::new();
    let peer = before_local.peer_confirm();
    write_json(
        &output.join("pairing/peer_confirm_before_local.json"),
        &json!({
            "fixture_schema": "phoneboost.pairing.v1",
            "timeline": ["SAS_PENDING", "PAIR_CONFIRM_RECEIVED"],
            "peer_confirmed": before_local.peer_confirmed(),
            "local_confirmed": before_local.local_confirmed(),
            "state": state_name(before_local.state()),
            "state_changed": peer.state_changed,
            "persist_commit": peer.value.persist_commit,
        }),
    )?;

    let mut duplicate = PairingActor::new();
    let first = duplicate.peer_confirm();
    let second = duplicate.peer_confirm();
    let local = duplicate.local_confirm();
    let commit = duplicate.begin_trust_commit();
    write_json(
        &output.join("pairing/duplicate_pair_confirm.json"),
        &json!({
            "fixture_schema": "phoneboost.pairing.v1",
            "first_state_changed": first.state_changed,
            "second_state_changed": second.state_changed,
            "second_persist_commit": second.value.persist_commit,
            "local_confirm_persist_commit": local.value.persist_commit,
            "begin_commit_persist_commit": commit.value.persist_commit,
            "total_persist_intents": usize::from(commit.value.persist_commit),
            "state": state_name(duplicate.state()),
        }),
    )?;

    let static_key = static_private("linux");
    let different_key = static_private("android");
    let one_sided_exchange = prior_committed_exchange(&static_key, &static_key, &different_key)?;
    write_json(
        &output.join("pairing/one_sided_commit_recovery.json"),
        &json!({
            "fixture_schema": "phoneboost.pairing.recovery.v1",
            "timeline": [
                "B_COMMITTED",
                "A_CRASH_BEFORE_COMMIT",
                "A_RESTARTS_ABSENT",
                "FRESH_XX",
                "B_PRIOR_COMMITTED_RECOVERY",
                "A_HUMAN_CONFIRMS_FRESH_SAS"
            ],
            "responder_exact_key_match": prior_committed_key_matches(&static_key, &static_key),
            "fresh_xx_required": true,
            "fresh_sas_displayed_by_responder": true,
            "initiator_human_confirmation_required": true,
            "pending_trust_persisted": false,
            "exchange": one_sided_exchange,
        }),
    )?;

    let mut committed = PairingActor::new();
    committed.local_confirm();
    committed.peer_confirm();
    committed.begin_trust_commit();
    committed.persist_result(PersistOutcome::Succeeded)?;
    write_json(
        &output.join("pairing/crash_after_commit_before_ui.json"),
        &json!({
            "fixture_schema": "phoneboost.pairing.recovery.v1",
            "atomic_rename_and_directory_fsync_complete": true,
            "ui_success_displayed": false,
            "process_crashed": true,
            "restart_record": "COMMITTED",
            "reconstructed_state": state_name(committed.state()),
            "false_loss_of_trust": false,
        }),
    )?;

    let recovery_exchange = prior_committed_exchange(&static_key, &static_key, &different_key)?;
    write_json(
        &output.join("pairing/prior_committed_recovery.json"),
        &json!({
            "fixture_schema": "phoneboost.pairing.recovery.v1",
            "exact_key_match_enables_recovery": prior_committed_key_matches(&static_key, &static_key),
            "different_key_disables_recovery": !prior_committed_key_matches(&static_key, &different_key),
            "responder_local_confirmation_basis": true,
            "fresh_sas_display_required": true,
            "initiator_human_confirmation_required": true,
            "automatic_pin_replacement": false,
            "exchange": recovery_exchange,
        }),
    )
}

fn guard_json(name: &str, guard: PairingGuard, extra: Value) -> Value {
    json!({
        "fixture_schema": "phoneboost.pairing_guard.v1",
        "case": name,
        "base_now_ms": BASE_CLOCK_MS,
        "mismatch_count": guard.mismatch_count,
        "cooldown_until_wall_ms": guard.cooldown_until_wall_ms,
        "updated_wall_ms": guard.updated_wall_ms,
        "extra": extra,
    })
}

fn generate_pairing_guard(output: &Path) -> Result<(), Box<dyn Error>> {
    let mut one = PairingGuard::new(BASE_CLOCK_MS);
    one.record_mismatch(BASE_CLOCK_MS);
    write_json(
        &output.join("pairing_guard/mismatch_1.json"),
        &guard_json("mismatch_1", one, json!({"cooldown_active": false})),
    )?;

    let mut three = PairingGuard::new(BASE_CLOCK_MS);
    three.record_mismatch(BASE_CLOCK_MS);
    three.record_mismatch(BASE_CLOCK_MS + 1);
    three.record_mismatch(BASE_CLOCK_MS + 2);
    write_json(
        &output.join("pairing_guard/mismatch_3_cooldown.json"),
        &guard_json(
            "mismatch_3_cooldown",
            three,
            json!({
                "third_mismatch_now_ms": BASE_CLOCK_MS + 2,
                "cooldown_duration_ms": PAIRING_COOLDOWN_MS,
                "pairing_admitted": three.admit(BASE_CLOCK_MS + 3).value,
            }),
        ),
    )?;

    let persisted = three;
    let mut restarted = persisted;
    let admitted_after_restart = restarted.admit(BASE_CLOCK_MS + 4);
    write_json(
        &output.join("pairing_guard/restart_during_cooldown.json"),
        &guard_json(
            "restart_during_cooldown",
            restarted,
            json!({
                "reloaded_same_global_state": true,
                "pairing_admitted": admitted_after_restart.value,
            }),
        ),
    )?;

    let mut changed_key = persisted;
    let admitted_changed_key = changed_key.admit(BASE_CLOCK_MS + 5);
    write_json(
        &output.join("pairing_guard/changed_untrusted_peer_key.json"),
        &guard_json(
            "changed_untrusted_peer_key",
            changed_key,
            json!({
                "untrusted_peer_key_changed": true,
                "guard_partitioned_by_peer": false,
                "pairing_admitted": admitted_changed_key.value,
            }),
        ),
    )?;

    let cancel_guard = one;
    let cancel = cancel_guard.user_cancelled();
    write_json(
        &output.join("pairing_guard/user_cancel_no_increment.json"),
        &guard_json(
            "user_cancel_no_increment",
            cancel_guard,
            json!({
                "state_changed": cancel.state_changed,
                "mismatch_count_before": 1,
                "mismatch_count_after": cancel_guard.mismatch_count,
            }),
        ),
    )
}

fn generate_readmes(output: &Path) -> Result<(), Box<dyn Error>> {
    let readmes = [
        (
            "README.md",
            "# PhoneBoost protocol fixtures\n\nGenerated by `pb-fixture-gen`; verify with `pb-fixture-check`. All private keys and RNG seeds are TEST-ONLY.\n",
        ),
        (
            "qr01a/README.md",
            "# QR-01A\n\nDeterministic snow 0.9.6 Noise XX transcripts and locked SAS derivation vectors. Inputs are TEST-ONLY.\n",
        ),
        (
            "control8/README.md",
            "# CONTROL/8\n\nGolden empty `PAIR_CONFIRM` frames and duplicate-idempotence evidence. Binary files exclude the Noise wrapper.\n",
        ),
        (
            "pbmux/README.md",
            "# PBMUX/1\n\nGenerated wire bytes, fragmentation, sequence and pre-allocation quota evidence for the locked 40-byte big-endian header.\n",
        ),
    ];
    for (relative, text) in readmes {
        fs::write(output.join(relative), text.as_bytes())?;
    }
    Ok(())
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

fn generate_manifest(args: &Arguments) -> Result<(), Box<dyn Error>> {
    let mut sources = Vec::new();
    for file in CANONICAL_SOURCE_FILES {
        let path = args.inputs.join(file);
        sources.push(json!({
            "file": file,
            "sha256": sha256_file(&path)?,
        }));
    }

    let mut paths = Vec::new();
    collect_files(&args.output, &args.output, &mut paths)?;
    paths.sort();
    let mut files = Vec::new();
    for relative in paths {
        let path = args.output.join(&relative);
        files.push(json!({
            "path": relative.to_string_lossy().replace('\\', "/"),
            "size": fs::metadata(&path)?.len(),
            "sha256": sha256_file(&path)?,
        }));
    }

    let mut generator = BTreeMap::new();
    generator.insert("crate", json!("pb-fixture-gen"));
    generator.insert("git_commit", json!(args.git_commit));
    generator.insert("rustc", json!("1.98.0"));
    generator.insert("snow", json!("0.9.6"));
    write_json(
        &args.output.join("MANIFEST.json"),
        &json!({
            "fixture_manifest_schema": "phoneboost.fixtures.v1",
            "canonical_sources": sources,
            "generator": generator,
            "files": files,
        }),
    )
}

fn write_json(path: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d01_payload_is_exact_and_stable() {
        let payload = fragmentation_payload();
        assert_eq!(payload.len(), MAX_LOGICAL_MESSAGE);
        assert_eq!(&payload[..32], &sha256_bytes(&0_u64.to_be_bytes()));
        assert_eq!(&payload[32..64], &sha256_bytes(&1_u64.to_be_bytes()));
    }

    #[test]
    fn d04_static_keys_are_role_separated() {
        assert_ne!(static_private("linux"), static_private("android"));
    }
}
