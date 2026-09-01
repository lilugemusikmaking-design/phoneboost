#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use pb_fixture_gen::test_only_resolver::DeterministicFixtureResolver;
    use pb_pbmux::{
        DispatchMode, Reassembler, ReassemblyAccounting, SequenceTracker, authorize_dispatch,
        decode, validate_pair_confirm,
    };
    use pb_secure::{
        PROLOGUE, PairingActor, PairingGuard, PersistOutcome, complete_xx, derive_sas,
        noise_xx_params,
    };
    use pb_types::{
        Channel, MAX_LOGICAL_MESSAGE, MAX_PBMUX_PAYLOAD, MAX_PBMUX_PLAINTEXT, PAIRING_COOLDOWN_MS,
        PairingState,
    };
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use snow::Builder;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocol-fixtures")
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).expect("fixture JSON readable"))
            .expect("fixture JSON valid")
    }

    fn field<'a>(value: &'a Value, name: &str) -> &'a str {
        value[name].as_str().expect("fixture string field")
    }

    fn bytes(value: &Value, name: &str) -> Vec<u8> {
        hex::decode(field(value, name)).expect("fixture lowercase hex")
    }

    fn array32(value: &Value, name: &str) -> [u8; 32] {
        bytes(value, name)
            .try_into()
            .expect("fixture field is 32 bytes")
    }

    #[test]
    fn control8_and_max_payload_replay() {
        let root = fixtures();
        let pair_bytes = fs::read(root.join("control8/pair_confirm_empty.bin")).unwrap();
        let pair = decode(&pair_bytes).unwrap();
        validate_pair_confirm(&pair).unwrap();
        authorize_dispatch(&pair, DispatchMode::PairingControlOnly).unwrap();
        assert_eq!(pair_bytes.len(), 40);

        let max_bytes = fs::read(root.join("pbmux/max_payload_60kib.bin")).unwrap();
        let max_frame = decode(&max_bytes).unwrap();
        assert_eq!(max_frame.payload.len(), MAX_PBMUX_PAYLOAD);
        assert_eq!(max_bytes.len(), MAX_PBMUX_PLAINTEXT);
        assert_eq!(max_frame.payload[0], 0);
        assert_eq!(max_frame.payload[255], 255);
        assert_eq!(max_frame.payload[256], 0);
    }

    #[test]
    fn fragmentation_reassembles_committed_frames() {
        let path = fixtures().join("pbmux/fragmentation_4mib.json");
        let fixture = read_json(&path);
        let frames = fixture["frames_hex"].as_array().unwrap();
        assert_eq!(frames.len(), 69);
        let mut sequence = SequenceTracker::default();
        let mut reassembler = Reassembler::default();
        let mut result = None;
        for encoded in frames {
            let frame = decode(&hex::decode(encoded.as_str().unwrap()).unwrap()).unwrap();
            sequence.accept(frame.header.sequence).unwrap();
            if let Some(completed) = reassembler.accept(frame).unwrap() {
                assert!(result.is_none());
                result = Some(completed);
            }
        }
        let result = result.expect("final fragment completes the logical payload");
        assert_eq!(result.len(), MAX_LOGICAL_MESSAGE);
        assert_eq!(
            hex::encode(Sha256::digest(&result)),
            field(&fixture, "logical_payload_sha256")
        );
        assert_eq!(reassembler.accounting().session_bytes(), 0);
    }

    #[test]
    fn sequence_and_quota_replay() {
        let root = fixtures();
        for name in ["sequence_gap.json", "sequence_duplicate.json"] {
            let fixture = read_json(&root.join("pbmux").join(name));
            let mut tracker = SequenceTracker::default();
            tracker.accept(0).unwrap();
            let received = fixture["received_sequence"].as_u64().unwrap();
            assert!(tracker.accept(received).is_err());
        }

        let channel_fixture = read_json(&root.join("pbmux/reassembly_channel_limit.json"));
        let mut channel = ReassemblyAccounting::default();
        let first = channel
            .try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)
            .unwrap();
        let second = channel
            .try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)
            .unwrap();
        let before = channel.channel_bytes(Channel::RemoteBuffer);
        assert!(
            channel
                .try_start(Channel::RemoteBuffer, MAX_LOGICAL_MESSAGE)
                .is_err()
        );
        assert_eq!(before, channel.channel_bytes(Channel::RemoteBuffer));
        assert_eq!(
            before as u64,
            channel_fixture["bytes_before_rejected_start"]
                .as_u64()
                .unwrap()
        );
        channel.finish(first);
        channel.finish(second);

        let session_fixture = read_json(&root.join("pbmux/reassembly_session_limit.json"));
        let mut session = ReassemblyAccounting::default();
        for channel in [
            Channel::Resource,
            Channel::RemoteBuffer,
            Channel::Compute,
            Channel::AiRpc,
        ] {
            session.try_start(channel, MAX_LOGICAL_MESSAGE).unwrap();
        }
        let before = session.session_bytes();
        assert!(
            session
                .try_start(Channel::Metrics, MAX_LOGICAL_MESSAGE)
                .is_err()
        );
        assert_eq!(before, session.session_bytes());
        assert_eq!(
            before as u64,
            session_fixture["bytes_before_rejected_start"]
                .as_u64()
                .unwrap()
        );
    }

    #[test]
    fn pairing_and_guard_replay() {
        let root = fixtures();
        let duplicate = read_json(&root.join("control8/pair_confirm_duplicate.json"));
        let first = decode(&bytes(&duplicate, "frame_1_hex")).unwrap();
        let second = decode(&bytes(&duplicate, "frame_2_hex")).unwrap();
        authorize_dispatch(&first, DispatchMode::PairingControlOnly).unwrap();
        authorize_dispatch(&second, DispatchMode::PairingControlOnly).unwrap();
        let mut actor = PairingActor::new();
        assert!(actor.peer_confirm().state_changed);
        assert!(!actor.peer_confirm().state_changed);
        let local = actor.local_confirm();
        assert_eq!(local.value.state, PairingState::MutualConfirmed);
        assert!(!local.value.persist_commit);
        let commit = actor.begin_trust_commit();
        assert!(commit.value.persist_commit);
        actor.persist_result(PersistOutcome::Succeeded).unwrap();
        assert_eq!(actor.state(), PairingState::Paired);

        let guard_fixture = read_json(&root.join("pairing_guard/mismatch_3_cooldown.json"));
        let base = guard_fixture["base_now_ms"].as_u64().unwrap();
        let mut guard = PairingGuard::new(base);
        guard.record_mismatch(base);
        guard.record_mismatch(base + 1);
        guard.record_mismatch(base + 2);
        assert_eq!(
            guard.cooldown_until_wall_ms,
            Some(base + 2 + PAIRING_COOLDOWN_MS)
        );
        let mut reloaded = guard;
        assert!(!reloaded.admit(base + 4).value);
        let mismatch_one = read_json(&root.join("pairing_guard/mismatch_1.json"));
        assert_eq!(mismatch_one["mismatch_count"].as_u64(), Some(1));
        let cancellation = reloaded.user_cancelled();
        assert!(!cancellation.state_changed);
    }

    #[test]
    fn prior_committed_recovery_replays_two_actors_through_commit() {
        let root = fixtures();
        let committed_key = sha256_array(b"PHONEBOOST-FIXTURE-STATIC-LINUX-V1\0");
        let different_key = sha256_array(b"PHONEBOOST-FIXTURE-STATIC-ANDROID-V1\0");

        for name in [
            "pairing/prior_committed_recovery.json",
            "pairing/one_sided_commit_recovery.json",
        ] {
            let fixture = read_json(&root.join(name));
            let exchange = &fixture["exchange"];
            let mut responder = PairingActor::new();
            let mut initiator = PairingActor::new();
            assert!(responder.fresh_sas_display_required());
            assert!(initiator.fresh_sas_display_required());

            let mismatch = responder.prior_committed_local_basis(&committed_key, &different_key);
            assert!(!mismatch.state_changed);
            assert!(!mismatch.value.send_pair_confirm);
            assert_eq!(responder.state(), PairingState::SasPending);

            let basis = responder.prior_committed_local_basis(&committed_key, &committed_key);
            assert!(basis.value.send_pair_confirm);
            assert_eq!(basis.value.state, PairingState::LocalConfirmed);
            let duplicate = responder.prior_committed_local_basis(&committed_key, &committed_key);
            assert!(!duplicate.state_changed);
            assert!(!duplicate.value.send_pair_confirm);

            let responder_frame = decode(&bytes(exchange, "responder_pair_confirm_hex")).unwrap();
            authorize_dispatch(&responder_frame, DispatchMode::PairingControlOnly).unwrap();
            assert_eq!(
                initiator.peer_confirm().value.state,
                PairingState::PeerConfirmed
            );

            let human = initiator.local_confirm();
            assert!(human.value.send_pair_confirm);
            assert_eq!(human.value.state, PairingState::MutualConfirmed);
            let initiator_frame = decode(&bytes(exchange, "initiator_pair_confirm_hex")).unwrap();
            authorize_dispatch(&initiator_frame, DispatchMode::PairingControlOnly).unwrap();
            assert_eq!(
                responder.peer_confirm().value.state,
                PairingState::MutualConfirmed
            );
            assert_eq!(responder.state(), PairingState::MutualConfirmed);
            assert_eq!(initiator.state(), PairingState::MutualConfirmed);

            let responder_commit = responder.begin_trust_commit();
            let initiator_commit = initiator.begin_trust_commit();
            assert!(responder_commit.value.persist_commit);
            assert!(initiator_commit.value.persist_commit);
            assert!(!responder.begin_trust_commit().value.persist_commit);
            assert!(!initiator.begin_trust_commit().value.persist_commit);
            responder.persist_result(PersistOutcome::Succeeded).unwrap();
            initiator.persist_result(PersistOutcome::Succeeded).unwrap();
            assert_eq!(responder.state(), PairingState::Paired);
            assert_eq!(initiator.state(), PairingState::Paired);

            assert_eq!(
                exchange["responder_pair_confirm_send_count"].as_u64(),
                Some(1)
            );
            assert_eq!(
                exchange["initiator_pair_confirm_send_count"].as_u64(),
                Some(1)
            );
            assert_eq!(exchange["total_persist_intents"].as_u64(), Some(2));
            assert_eq!(exchange["both_mutual_before_commit"].as_bool(), Some(true));
        }
    }

    #[test]
    fn qr01a_replays_stored_deterministic_inputs() {
        let root = fixtures().join("qr01a");
        for name in [
            "vector_canonical.json",
            "vector_leading_zero.json",
            "vector_rejection_sampling.json",
        ] {
            let fixture = read_json(&root.join(name));
            let static_linux = array32(&fixture, "static_private_linux_hex");
            let static_android = array32(&fixture, "static_private_android_hex");
            let seed_linux = array32(&fixture, "rng_seed_linux_hex");
            let seed_android = array32(&fixture, "rng_seed_android_hex");
            let params = noise_xx_params().unwrap();
            let initiator = Builder::with_resolver(
                params.clone(),
                Box::new(DeterministicFixtureResolver::new(seed_linux)),
            )
            .local_private_key(&static_linux)
            .prologue(PROLOGUE)
            .build_initiator()
            .unwrap();
            let responder = Builder::with_resolver(
                params,
                Box::new(DeterministicFixtureResolver::new(seed_android)),
            )
            .local_private_key(&static_android)
            .prologue(PROLOGUE)
            .build_responder()
            .unwrap();
            let transcript = complete_xx(initiator, responder).unwrap();
            assert_eq!(transcript.message_1, bytes(&fixture, "handshake_msg_1_hex"));
            assert_eq!(transcript.message_2, bytes(&fixture, "handshake_msg_2_hex"));
            assert_eq!(transcript.message_3, bytes(&fixture, "handshake_msg_3_hex"));
            assert_eq!(
                transcript.handshake_hash,
                array32(&fixture, "handshake_hash_hex")
            );
            let sas = derive_sas(&transcript.handshake_hash).unwrap();
            assert_eq!(sas, field(&fixture, "sas"));
        }

        let mismatch = read_json(&root.join("vector_prologue_mismatch.json"));
        let static_linux = sha256_array(b"PHONEBOOST-FIXTURE-STATIC-LINUX-V1\0");
        let static_android = sha256_array(b"PHONEBOOST-FIXTURE-STATIC-ANDROID-V1\0");
        let canonical = read_json(&root.join("vector_canonical.json"));
        let seed_linux = array32(&canonical, "rng_seed_linux_hex");
        let seed_android = array32(&canonical, "rng_seed_android_hex");
        let altered: [u8; 64] = bytes(&mismatch, "responder_prologue_hex")
            .try_into()
            .unwrap();
        let params = noise_xx_params().unwrap();
        let initiator = Builder::with_resolver(
            params.clone(),
            Box::new(DeterministicFixtureResolver::new(seed_linux)),
        )
        .local_private_key(&static_linux)
        .prologue(PROLOGUE)
        .build_initiator()
        .unwrap();
        let responder = Builder::with_resolver(
            params,
            Box::new(DeterministicFixtureResolver::new(seed_android)),
        )
        .local_private_key(&static_android)
        .prologue(&altered)
        .build_responder()
        .unwrap();
        assert!(complete_xx(initiator, responder).is_err());
    }

    fn sha256_array(input: &[u8]) -> [u8; 32] {
        Sha256::digest(input).into()
    }
}
