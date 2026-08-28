# C08/C09 Wire V0.1 Locked 001 — Golden Vectors

Status: TEST-ONLY DOCUMENTATION ORACLES. NO NOISE CIPHERTEXT OR PRODUCTION SECRETS.

Each ordinary `.bin` contains one complete PBMUX plaintext frame. The 4 MiB fixture
contains 69 complete PBMUX plaintext frames concatenated in sequence; each frame remains
self-delimiting through its 40-byte header and payload length.

| Vector | Direction | Expected | Bytes | SHA-256 | Meaning |
|---|---|---|---:|---|---|
| `GV-C08-01-reserve-request.bin` | `host->worker` | `PASS` | 88 | `be01d3506c27a4860bb63d8e0cb9b0c84a5677a670897fb37f64f827b2729914` | RESERVE |
| `GV-C08-02-reserve-success.bin` | `worker->host` | `PASS` | 112 | `d4edd9deacd8d2bf280bc3d4de2acfde5e396223ee2ae076189675ed5ee2bd37` | RESERVE_ACK RESERVED |
| `GV-C08-03-reserve-refused-stale.bin` | `worker->host` | `PASS` | 112 | `1b58fc2e9e5de6e1352019c9ec31a4d7775414ae1b988e859322124138a5814e` | RESERVE_ACK REFUSED_STALE_STATE |
| `GV-C08-04-commit-request.bin` | `host->worker` | `PASS` | 88 | `8b18d52b98328600d177c8127459006b2b8fe3ded4824df30dbe411d71c736f6` | COMMIT |
| `GV-C08-05-commit-result.bin` | `worker->host` | `PASS` | 112 | `b343cc6cb9045579ad80e1f6d4190bc4af78e4252b98e177ce27e57be533983c` | COMMIT COMPLETED |
| `GV-C08-06-release-request.bin` | `host->worker` | `PASS` | 88 | `bef071ae1377236598d9a624b25a24b1e566d3ec8191f5a234b233b69d039bc6` | RELEASE |
| `GV-C08-07-release-result.bin` | `worker->host` | `PASS` | 112 | `acf9c93e4d00915a47abd9f267d8ea97b1a3e90ce640cd83c7e26673c9dc4b01` | RELEASE COMPLETED |
| `GV-C08-08-expire-notify.bin` | `worker->host` | `PASS` | 104 | `cb3b973caac4248bc153b726aa43b25a5090bc83a479b8e2f32a3c336b64c7e4` | EXPIRE_NOTIFY |
| `GV-C08-09-malformed-presence.bin` | `worker->host` | `REJECT` | 112 | `b04f8d87c90fe6b14aed64a928f5807b8938b9e20dfebeb1e57e747fde47579c` | reservation_present=2 |
| `GV-C08-10-request-id-conflict.bin` | `host->worker` | `REQUEST_ID_CONFLICT` | 88 | `01ae2e2f8f2367241012916836d41640a728b1002cfab72030820e2bdd2384ac` | same lease/request_id, different amount after GV-C08-01 |
| `GV-C09-01-alloc-request.bin` | `host->worker` | `PASS` | 104 | `2f3123b9bfc052984a722060ea2036de67ee3da2f528315dfd1331e29db0f4b4` | ALLOC |
| `GV-C09-02-alloc-ack.bin` | `worker->host` | `PASS` | 140 | `bc49f6479c04055e7ee870381649bb344142a6b8085d6bf6ed22faf629783f6c` | ALLOC_ACK ALLOCATED |
| `GV-C09-03-put-small-request.bin` | `host->worker` | `PASS` | 118 | `9c81a324bd32a51960cda41010e5bfdb492701cca4823822716acdfa99562419` | PUT small staged body |
| `GV-C09-04-put-result.bin` | `worker->host` | `PASS` | 140 | `3e1761b7fb11962810b1fdbb2d3a9ec56167f93b84a24b59782622893a8b8357` | PUT COMPLETED |
| `GV-C09-05-get-request.bin` | `host->worker` | `PASS` | 104 | `bd9406e67e9ee5418bec48ecbc75f361c25ffb927d94fd3cbe95bef17968312d` | GET |
| `GV-C09-06-data-response.bin` | `worker->host` | `PASS` | 144 | `275b1f5e0b22bf1f35cde9e6bb81f0edaf05b74f94e470e49355e22996ee3009` | DATA exact body |
| `GV-C09-07-stat-request.bin` | `host->worker` | `PASS` | 88 | `67f28922af6471d4976e4c880d4774a5d7fec624e07154f93fbfb371e983544d` | STAT |
| `GV-C09-08-stat-result.bin` | `worker->host` | `PASS` | 140 | `45a132a2df5370958e901a34fab6ca0d801b321c14785b34962064a324745cb1` | STAT metadata only |
| `GV-C09-09-touch-request.bin` | `host->worker` | `PASS` | 88 | `dc46b82bf359c729bcb1d0cae7dd50819ead45a1ab9d348fd837bd996f4996c1` | TOUCH |
| `GV-C09-10-touch-result.bin` | `worker->host` | `PASS` | 140 | `ba5f07df03094e7f3aca6bcc4e1ed5f53a0768e355f05d1a829901491d381c1b` | TOUCH bounded TTL |
| `GV-C09-11-free-request.bin` | `host->worker` | `PASS` | 88 | `233b1a51eb5ad10314e61f9f5263a7ff86f042c38331fa36fd05384ca7300579` | FREE |
| `GV-C09-12-free-result.bin` | `worker->host` | `PASS` | 140 | `acee7657f983fc46a5e8230b5d2c92fa2adfeea246751d070d9afd0ee46161ed` | FREE terminal |
| `GV-C09-13-lost-result.bin` | `worker->host` | `PASS` | 140 | `daa29e0b90ba0b063985e83ce4d220dcfd838215ea08ca6a95593c7cc4fb6411` | BUFFER_LOST tombstone |
| `GV-C09-14-stale-lease-result.bin` | `worker->host` | `PASS` | 140 | `4968d94e9d49932f61a718129466bffda6f12efdc05b183547173adca013c757` | STALE_CONTROLLER_LEASE no handle leak |
| `GV-C09-15-malformed-put-length.bin` | `host->worker` | `REJECT` | 118 | `89b9aed877ede352b9927cf7153280b29e9faa9233d49914f7eca2097c2733d4` | PUT data_len/body mismatch |
| `GV-C09-16-put-4mib-fragmented.bin` | `host->worker` | `PASS` | 4197064 | `85ad72de953aba8a55c1c6a40c6730b2b3f1d20e40d7d4e354543bf88a1e9e48` | 69 concatenated PBMUX frames, exact 4 MiB logical PUT |

Deterministic constants and all payload layouts are fixed by
`../PHONEBOOST_C08_C09_WIRE_ADDENDUM_V0_1_LOCKED_001.md`.
The independent checker regenerates every byte without importing PhoneBoost crates.

Expected meanings:

- `PASS`: exact V0.1 frame or fragmented logical message accepted.
- `REJECT`: malformed/noncanonical data rejected before domain mutation.
- `REQUEST_ID_CONFLICT`: structurally valid RESERVE that conflicts with the prior
  same-lease/same-request-ID fixture in the stateful C08 oracle.
