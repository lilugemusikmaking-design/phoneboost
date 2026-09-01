# C10 COMPUTE Wire V0.1 Locked 001 — Golden Vectors

Status: TEST-ONLY DOCUMENTATION ORACLES. NO NOISE CIPHERTEXT OR PRODUCTION SECRETS.

Each `.bin` contains one complete fixed-size PBMUX plaintext frame. Direction and
stateful expected outcomes are metadata in this README and the independent checker.

| Vector | Direction | Expected | Bytes | SHA-256 | Meaning |
|---|---|---|---:|---|---|
| `GV-C10-01-submit-blake3-remote-buffer.bin` | `host->worker` | `PASS` | 124 | `e6054bdc9a4e7e08ead19c4e835a4225163f3cab17de0b8bcd9da41d3e83e694` | SUBMIT provider 1/1, class-2 reservation, RemoteBuffer `abc` |
| `GV-C10-02-status-request.bin` | `host->worker` | `PASS` | 88 | `ca66e2e5d2e357b5d0d7f5cabf249cf0d401b0f1cabd13e352b90e8bfa8d4c00` | STATUS request |
| `GV-C10-03-status-running-response.bin` | `worker->host` | `PASS` | 96 | `c07fc885609601748879c13fd3b17e8458bced0bb06db9e7a478ed37e8bf2e83` | STATUS RUNNING |
| `GV-C10-04-result-completed.bin` | `worker->host` | `PASS` | 128 | `8c0cc2df78c76e7b795aa02740bb3b7eeca37ca04f65175e5c039ab1dbaa3329` | RESULT COMPLETED with exact BLAKE3(`abc`) |
| `GV-C10-05-result-failed.bin` | `worker->host` | `PASS` | 128 | `ea34e2c6c0c4ea203f4f4193a18974d14002fb88168861d83a725c4c77f0fb24` | RESULT FAILED PROVIDER_TIMEOUT |
| `GV-C10-06-cancel-request.bin` | `host->worker` | `PASS` | 88 | `32c67ca39821d9cfb20a2db8de573fce6b31f7b1527d0d50a67ef4d63712102f` | CANCEL request |
| `GV-C10-07-cancel-response.bin` | `worker->host` | `PASS` | 96 | `fa36c030dfcbe35b201ea9902893faaf7927f5379295842bb0f06ba819996e53` | CANCEL terminal response |
| `GV-C10-08-unsupported-provider.bin` | `host->worker` | `UNSUPPORTED_PROVIDER` | 124 | `7b77c7c319576f82c76ebb2d2d627ac15f5a2f2134fb53280a82f05c7908615f` | unassigned provider pair 2/1 |
| `GV-C10-09-stale-lease.bin` | `worker->host` | `PASS` | 128 | `8ffc2356ba87cef44d41d641a3742df19457d1671eef22f4d3e7295630a5f029` | absent-job STALE_CONTROLLER_LEASE |
| `GV-C10-10-buffer-lost.bin` | `worker->host` | `PASS` | 128 | `e5dec8a8b823a32aab5f36cedcdd131a3332739a171825ab3257164864b3b553` | absent-job BUFFER_LOST |
| `GV-C10-11-invalid-digest-presence.bin` | `worker->host` | `REJECT` | 128 | `0c55c42f4354d3b42e4be78cdde06171997590ab14ab9da125042b3751940528` | FAILED illegally carries digest |
| `GV-C10-12-malformed-reserved-zero.bin` | `host->worker` | `REJECT` | 124 | `2630b9ed06e9e11d8b4709b6b539c071fc7ab193ae60bc55b15216cde40ae169` | SUBMIT reserved byte nonzero |
| `GV-C10-13-wrong-direction.bin` | `worker->host` | `REJECT` | 124 | `784a82a3bde200b7ff67fdfdc4051e9f6d0ef5a3e1a5666ef59568862f29ff8b` | SUBMIT in worker-to-host direction |
| `GV-C10-14-request-id-conflict.bin` | `host->worker` | `REQUEST_ID_CONFLICT` | 124 | `df9d96d5624cf33051d34ad75567326d58d81631a1974f5408b6cea15b5afe27` | same lease/request_id as vector 01, different SUBMIT payload |
| `GV-C10-15-duplicate-submit-replay.bin` | `host->worker` | `REPLAY` | 124 | `e6054bdc9a4e7e08ead19c4e835a4225163f3cab17de0b8bcd9da41d3e83e694` | byte-identical replay of vector 01 |
| `GV-C10-16-consumed-reservation-reuse.bin` | `host->worker` | `RESERVATION_INVALID` | 124 | `87f9a097bd591b01260025f66726b0e374fe11129799c776892b436a62a1bbed` | new request_id attempts reuse of consumed class-2 reservation |
| `GV-C10-17-session-loss-non-resurrection.bin` | `host->worker` | `NON_RESURRECTED` | 88 | `99f71a5d3fc7ac28924b22ac298f3e3d38e41ae41580112673a959552fed37f3` | fresh-session query cannot resurrect old nonterminal job |

Deterministic constants and layouts are fixed by
`../PHONEBOOST_C10_WIRE_ADDENDUM_V0_1_LOCKED_001.md`.
The checker manually regenerates and parses every byte using Python stdlib only.

Expected meanings:

- `PASS`: canonical frame accepted.
- `REJECT`: malformed or direction-invalid logical message.
- `UNSUPPORTED_PROVIDER`: structurally valid typed provider refusal.
- `REQUEST_ID_CONFLICT`: same lease/request ID with different SUBMIT bytes.
- `REPLAY`: identical SUBMIT returns the existing job/outcome without new work.
- `RESERVATION_INVALID`: a consumed class-2 reservation cannot back another job.
- `NON_RESURRECTED`: session loss terminalizes old work and fresh session state
  cannot resume or report it successful.
