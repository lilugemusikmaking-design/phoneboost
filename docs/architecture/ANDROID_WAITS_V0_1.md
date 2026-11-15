# Android waits v0.1

Base master93f6446dd795ca32ebf0a40273b68e3ec7dbd09c, PR36 merged preserving history.

## Causal audit before behavior changes

Receiver socket: `configure_session_stream` uses nonblocking/no timeout. `receive_frame_if_available` peeks2 bytes. WouldBlock or one byte returnsNone. Once2 bytes exist it temporarily uses blocking reads with unchanged5s HANDSHAKE_TIMEOUT, then restores nonblocking. EOF/errors remain fail-closed. Linux initiator already uses `input_wait::wait`; Android/responder sleeps20ms unconditionally when no frame is ready. The existing rustix1.1.6 poll API and RDHUP support Android/linux_kernel; reuse it rather than introduce a primitive.

Partial prefix: keep the bounded20ms fallback. Level-trigger poll with one queued byte would spin; replacing the record reader is outside scope. Partial payload timeout remains unchanged. Heartbeat2s, offline10s and active-request bounds stay unchanged; cancellation is still checked at existing loop boundaries.

Handler completion: both buffer and compute dispatch already use scoped threads plus `sync_channel(1)`. Session loops `try_recv` then sleeps20ms. Existing trace HANDLER_END(1015) → HANDLER_OBSERVED(1016) measures the local delay directly. Use `recv_timeout(20ms)` with liveness checks before AND after a completion/disconnect wake, preserving SessionLost priority. Scoped join still relies on a cooperative handler after peer revocation; no new hard handler-cancellation guarantee.

## Instrumentation / baseline

Opt-in local events1020 WAIT_READY and1021 WAIT_FINISHED: values1=already readable,2=data observed,3=timer/no readiness observed,4=partial prefix,5=closed/error. They use no payload/session/socket identifier and no wire field. Both endpoints retain their own clock domains. Readiness is locally observed, not an exact kernel arrival timestamp.

The diagnostic legacy responder observes readiness once with bounded poll, then sleeps the remainder of the same20ms quantum. Errors are observation-only; baseline never wakes early or changes receive outcomes. Feature/trace OFF retains original sleep. Candidate uses identical observation and returns on readiness. Compare ready→finished tail only for complete-input observations; do not claim all634ms is recoverable.

## Physical controls

Keep Battery SaverON throughout, profile1 for BOTH variants: existing guard allows one thread in Throttle;128MiB budget observed. No forced OFF, no synthetic Health, no threshold changes. Same NEON config/dependencies/Noise/PBMUX/input64MiB/host release/USB, identical probes. Two warmups + at least3 alternating pairs after software tests. Historical profile2/OFF measurements are context only, not the new baseline.

NEXT_ACTION = build the instrumented legacy baseline, implement/test only the two waits, then compare physically with unchanged phone power state.
