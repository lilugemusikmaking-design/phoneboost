"""PhoneBoost Control Center — backend.

This backend is a presentation-layer adapter only. It never fabricates LIVE
runtime state. It exposes:

  - GET /api/system/snapshot   -> RecordedEvidence system snapshot (repo truth)
  - GET /api/live/probe        -> Explicit LIVE unreachability from hosted env
  - GET /api/evidence/index    -> Evidence card index
  - GET /api/evidence/{id}     -> Raw evidence text (redacted by design)
  - GET /api/fixtures/manifest -> Protocol fixture manifest
  - GET /api/roadmap           -> ROADMAP items from repo docs
  - GET /api/release           -> Release identity (repo anchor)

Values come from files checked in at /app/backend/phoneboost_data, sourced
from the PhoneBoost repository release-candidate tag competition-rc-20260824
(HEAD 51accc1a). No number here is invented.
"""
from __future__ import annotations

import logging
import os
from pathlib import Path
from typing import Any

from dotenv import load_dotenv
from fastapi import APIRouter, FastAPI, HTTPException
from starlette.middleware.cors import CORSMiddleware

ROOT_DIR = Path(__file__).parent
load_dotenv(ROOT_DIR / ".env")

DATA_DIR = ROOT_DIR / "phoneboost_data"
EVIDENCE_DIR = DATA_DIR / "evidence"
FIXTURES_DIR = DATA_DIR / "fixtures"

app = FastAPI(title="PhoneBoost Control Center", version="rc-20260824")
api = APIRouter(prefix="/api")

# ---------------------------------------------------------------------------
# Repo-anchored constants. Do not change without repo evidence.
# ---------------------------------------------------------------------------

RELEASE = {
    "product": "PhoneBoost",
    "tag": "competition-rc-20260824",
    "head": "51accc1a8fed188f2254f085004504735368b539",
    "native_baseline": "052471ed3cdbbe66a6c1f7b255f1d70580d91fcc",
    "toolchain": "Rust 1.98.0",
    "validation_date": "2026-08-24",
    "scope": "non-production Linux x86-64 / Android ARM64 proof of concept",
    "repo": "https://github.com/lilugemusikmaking-design/phoneboost",
}

# From docs/competition/IMPLEMENTATION_EVIDENCE.md
TEST_TOTALS = {
    "workspace": {"passed": 278, "failed": 0, "label": "Full Rust workspace"},
    "crates": [
        {"name": "pb-types", "passed": 2, "total": 2},
        {"name": "pb-pbmux", "passed": 58, "total": 58},
        {"name": "pb-worker-core", "passed": 43, "total": 43},
        {"name": "pb-runtime-secure", "passed": 15, "total": 15},
    ],
    "cargo_fmt": "passed",
    "notes": "Two pre-existing non-failing unused_mut warnings remain in PBMUX tests.",
}

C07_CHECKER = {
    "final_verdict": "C07_WIRE_CHECK PASS",
    "command_ack_vectors": 10,
    "heartbeat_vectors": 8,
    "oracle_mutations": 5,
    "expected_verdicts": "all matched",
}

ANDROID_BUILD = {
    "arm64_production_core": "offline release build passed",
    "fixture_isolation": "passed",
    "forbidden_authority_export_scans": "passed",
    "debug_apk": ":app:assembleDebug passed",
    "gradle_tasks": {"total": 36, "executed": 4, "up_to_date": 32},
    "jni_fixture_isolation": "passed",
    "api": 36,
    "abi": "arm64-v8a",
}

# Five-gate ladder from EMERGENT_HANDOFF.md §"State model the UI must preserve"
FIVE_GATES = [
    {
        "id": "paired",
        "label": "Paired",
        "explanation": "Durable static-key trust after Noise XX, SAS comparison, mutual confirmation, and commit.",
    },
    {
        "id": "authenticated",
        "label": "Authenticated",
        "explanation": "Current Noise session proves the pinned peer for this connection.",
    },
    {
        "id": "controller_lease",
        "label": "Controller lease",
        "explanation": "One authenticated controller holds a current lease for the current worker incarnation.",
    },
    {
        "id": "resource_admissible",
        "label": "Resource admissible",
        "explanation": "Fresh Android-local health and ResourceGuard policy permit a specific reservation.",
    },
    {
        "id": "provider_ready",
        "label": "Provider ready",
        "explanation": "A concrete provider has committed resources and can accept the requested operation.",
    },
]

# From README.md and IMPLEMENTATION_EVIDENCE.md
ROADMAP = {
    "working_now": [
        "Linux phoneboostd user-mode daemon + private 0600 Unix control socket",
        "Local peer-credential admission + bounded C12 request framing",
        "system.status endpoint via phoneboostctl",
        "Local-IP Linux↔Android transport (physical: PASS)",
        "Noise XX pairing + QR-01A SAS + PBMUX CONTROL/8 PAIR_CONFIRM + atomic commit",
        "Noise IK reconnect against pinned static peer key",
        "PBMUX authenticated framing, sequencing, quotas, fragmentation, fail-closed dispatch",
        "C07 COMMAND / COMMAND_ACK / METRICS/1 HEARTBEAT codecs + oracle checker",
        "Canonical 256-bit peer IDs (SHA-256 of static public key)",
        "Android foreground worker + Rust/JNI panic boundary + worker incarnation",
        "Android-local health sampling (memory / thermal / battery / power)",
        "ControllerLeaseManager + single-writer ResourceGuard admission logic",
    ],
    "next": [
        "C05→C07 production authorization seam (no-cycle bridge design + review)",
        "Apply authenticated C07 commands to ControllerLeaseManager + ResourceGuard",
        "Publish authenticated-session → controller-lease authority path",
    ],
    "future": [
        "RemoteBuffer storage + operations (bounded, volatile, remote-only)",
        "Native compute providers (explicit remote jobs, worker-authoritative)",
        "End-to-end capacity/compute product paths + benchmark evidence",
        "Native browser bridge for a live Control Center over local socket",
    ],
}

# From README.md + IMPLEMENTATION_EVIDENCE.md
ARCHITECTURE = [
    {"layer": "Linux PC", "role": "Orchestrator", "detail": "phoneboostd + phoneboostctl"},
    {"layer": "Local Control API", "role": "Local authority", "detail": "0600 Unix socket, peer-credential auth, C12 framing"},
    {"layer": "SecureSession", "role": "Transport crypto", "detail": "Noise XX first pair · SAS QR-01A · Noise IK reconnect"},
    {"layer": "PBMUX", "role": "Channel framing", "detail": "Authenticated · sequenced · quota-bounded · fail-closed"},
    {"layer": "Android Trusted Worker", "role": "Remote authority", "detail": "Rust core via JNI · foreground service · worker incarnation"},
    {"layer": "Controller + ResourceGuard", "role": "Admission", "detail": "Single controller lease · Android-local health · ResourceGuard is final admission authority"},
]

# Curated evidence cards. Every source file exists in /app/backend/phoneboost_data.
EVIDENCE_CARDS = [
    {
        "id": "workspace-tests",
        "title": "Rust workspace tests",
        "summary": "278 / 278 PASS — 0 failed — all doc-tests PASS",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "test-totals",
        "source": "docs/competition/IMPLEMENTATION_EVIDENCE.md",
        "detail_key": "workspace",
    },
    {
        "id": "focused-crates",
        "title": "Focused core crates",
        "summary": "pb-types 2/2 · pb-pbmux 58/58 · pb-worker-core 43/43 · pb-runtime-secure 15/15",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "test-totals",
        "source": "docs/competition/IMPLEMENTATION_EVIDENCE.md",
        "detail_key": "crates",
    },
    {
        "id": "c07-checker",
        "title": "C07 wire-addendum checker",
        "summary": "C07_WIRE_CHECK PASS — 10 CMD/ACK vectors · 8 HEARTBEAT vectors · 5 oracle mutations",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "protocol-oracle",
        "source": "scripts/check_c07_wire_addendum_002.py",
        "detail_key": "c07",
    },
    {
        "id": "android-arm64-build",
        "title": "Android ARM64 production core",
        "summary": "Offline release build PASS · fixture isolation PASS · forbidden-authority-export scans PASS",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "build",
        "source": "docs/competition/IMPLEMENTATION_EVIDENCE.md",
        "detail_key": "android_build",
    },
    {
        "id": "android-debug-apk",
        "title": "Android debug APK (offline)",
        "summary": ":app:assembleDebug PASS — 36 Gradle tasks (4 executed, 32 up-to-date) · JNI fixture isolation PASS",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "build",
        "source": "docs/competition/IMPLEMENTATION_EVIDENCE.md",
        "detail_key": "android_apk",
    },
    {
        "id": "a4-local-roundtrip",
        "title": "A4 · Linux local roundtrip",
        "summary": "phoneboostd → phoneboostctl status: PhoneBoost READY · Local API ACTIVE · Android worker NOT_CONFIGURED",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "physical",
        "source": "docs/evidence/a4-local-roundtrip.txt",
        "file": "a4-local-roundtrip.txt",
    },
    {
        "id": "a5-android-worker",
        "title": "A5 · Android worker physical",
        "summary": "Build PASS · Install PASS · connectedDevice FGS PASS · JNI worker core PASS · Incarnation-after-restart PASS",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "physical",
        "source": "docs/evidence/a5-android-worker-physical.txt",
        "file": "a5-android-worker-physical.txt",
    },
    {
        "id": "a6-lease-resourceguard",
        "title": "A6 · Lease + ResourceGuard physical",
        "summary": "C07 L-T01..L-T15 PASS · C08 RG-T01..RG-T18 PASS · Authority AUTH-T01..AUTH-T04 PASS · 32× concurrent 8 MiB bounded to 128 MiB total",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "physical",
        "source": "docs/evidence/a6-lease-resourceguard-physical.txt",
        "file": "a6-lease-resourceguard-physical.txt",
    },
    {
        "id": "c04-local-ip-transport",
        "title": "C04 · Local-IP transport physical",
        "summary": "Direct LAN connect PASS · bidirectional stream PASS · reconnect PASS · Maximum observed state = CONNECTED_UNAUTHENTICATED",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "physical",
        "source": "docs/evidence/c04-local-ip-transport-physical.txt",
        "file": "c04-local-ip-transport-physical.txt",
    },
    {
        "id": "c05-c06-secure-pairing",
        "title": "C05/C06 · Secure pairing physical",
        "summary": "XX PASS · SAS_MATCH PASS · PAIR_CONFIRM PASS · COMMITTED_BOTH PASS · AUTHENTICATED PASS · PING_PONG PASS",
        "provenance": "RECORDED_EVIDENCE",
        "kind": "physical",
        "source": "docs/evidence/c05-c06-secure-pairing-physical.txt",
        "file": "c05-c06-secure-pairing-physical.txt",
    },
]

EVIDENCE_DETAILS: dict[str, Any] = {
    "workspace": TEST_TOTALS,
    "crates": TEST_TOTALS["crates"],
    "c07": C07_CHECKER,
    "android_build": {
        "arm64_production_core": ANDROID_BUILD["arm64_production_core"],
        "fixture_isolation": ANDROID_BUILD["fixture_isolation"],
        "forbidden_authority_export_scans": ANDROID_BUILD["forbidden_authority_export_scans"],
    },
    "android_apk": {
        "debug_apk": ANDROID_BUILD["debug_apk"],
        "gradle_tasks": ANDROID_BUILD["gradle_tasks"],
        "jni_fixture_isolation": ANDROID_BUILD["jni_fixture_isolation"],
        "api": ANDROID_BUILD["api"],
        "abi": ANDROID_BUILD["abi"],
    },
}


def _snapshot_recorded() -> dict[str, Any]:
    """Return the Recorded-Evidence system snapshot.

    Every gate state is UNAVAILABLE because no live Linux/Android bridge is
    attached to this hosted browser. This is the truthful default.
    """
    return {
        "provenance": "RECORDED_EVIDENCE",
        "mode_label": "Recorded Evidence Mode",
        "live_available": False,
        "live_unavailable_reason": (
            "No native PhoneBoost runtime bridge is reachable from this hosted "
            "browser. LIVE state requires a local phoneboostd + Android worker."
        ),
        "release": RELEASE,
        "computer": {
            "label": "Computer (Linux x86-64)",
            "runtime": {
                "value": "phoneboostd — implemented in repository",
                "state": "NOT_RUNNING_IN_HOSTED_BROWSER",
            },
            "local_api": {
                "value": "Private Unix control socket (0600), peer-credential admission, C12 framing",
                "state": "IMPLEMENTED · NOT_REACHABLE_FROM_BROWSER",
            },
            "note": "Local resources are never merged with remote Android capacity.",
        },
        "phone": {
            "label": "Android phone (ARM64)",
            "endpoint": {"value": "No device attached", "state": "NOT_CONNECTED"},
            "worker": {"value": "Foreground service + Rust/JNI worker core", "state": "IMPLEMENTED_NOT_CONNECTED"},
            "incarnation": {"value": "—", "state": "UNKNOWN_NO_SESSION"},
            "health": {
                "value": "Sampled by Android-local runtime only",
                "state": "NOT_MEASURED_IN_BROWSER",
                "note": "Fields such as available memory / thermal / battery / charging are only measured on-device; hosted browsers cannot observe them.",
            },
        },
        "secure_link": {
            "transport": {"value": "Local-IP (LAN) or Unix socket", "state": "NOT_ESTABLISHED"},
            "session": {"value": "Noise XX first-pair · Noise IK reconnect", "state": "NOT_ESTABLISHED"},
            "authentication": {"value": "Pinned 256-bit peer ID (SHA-256 of static public key)", "state": "NOT_AUTHENTICATED"},
            "latency": {"value": "Not measured", "state": "NOT_MEASURED"},
        },
        "gates": [
            {**g, "state": "UNAVAILABLE", "reason": "No live runtime bridge; recorded evidence only."}
            for g in FIVE_GATES
        ],
        "remote_capability": {
            "admitted_capacity": {"value": "None", "state": "NO_LEASE"},
            "reserved": {"value": "None", "state": "NO_RESERVATION"},
            "active_remote_buffer": {"value": "None", "state": "ROADMAP"},
            "active_remote_job": {"value": "None", "state": "ROADMAP"},
            "note": "RemoteBuffer and compute providers are ROADMAP items; identity or authentication does not create capacity.",
        },
        "controller": {
            "lease": {"value": "None", "state": "NO_LEASE"},
            "resource_guard": {"value": "Final admission authority lives on the Android worker", "state": "IMPLEMENTED_NOT_CONNECTED"},
        },
        "security_plain_language": "Your phone only accepts work from a computer you explicitly paired.",
    }


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@api.get("/")
async def root() -> dict[str, Any]:
    return {"product": RELEASE["product"], "tag": RELEASE["tag"], "head": RELEASE["head"]}


@api.get("/release")
async def release() -> dict[str, Any]:
    return {"provenance": "RECORDED_EVIDENCE", "release": RELEASE}


@api.get("/live/probe")
async def live_probe() -> dict[str, Any]:
    """LIVE runtime probe. Always unreachable from a hosted browser."""
    return {
        "provenance": "LIVE",
        "reachable": False,
        "reason": (
            "This deployed Emergent web app cannot open the private local "
            "Unix control socket used by phoneboostd, and no native browser "
            "bridge is installed. LIVE mode remains explicitly unavailable."
        ),
        "requirements": [
            "phoneboostd running on a local Linux x86-64 host",
            "A paired Android ARM64 worker with a current controller lease",
            "A local native bridge exposing the PhoneBoostDataSource contract",
        ],
    }


@api.get("/system/snapshot")
async def system_snapshot() -> dict[str, Any]:
    return _snapshot_recorded()


@api.get("/evidence/index")
async def evidence_index() -> dict[str, Any]:
    return {"provenance": "RECORDED_EVIDENCE", "items": EVIDENCE_CARDS}


@api.get("/evidence/{item_id}")
async def evidence_item(item_id: str) -> dict[str, Any]:
    card = next((c for c in EVIDENCE_CARDS if c["id"] == item_id), None)
    if card is None:
        raise HTTPException(status_code=404, detail="unknown evidence id")
    out: dict[str, Any] = {"provenance": "RECORDED_EVIDENCE", "card": card}
    if "detail_key" in card:
        out["detail"] = EVIDENCE_DETAILS.get(card["detail_key"])
    if "file" in card:
        fp = EVIDENCE_DIR / card["file"]
        if fp.exists():
            out["raw"] = fp.read_text(encoding="utf-8", errors="replace")
    return out


@api.get("/fixtures/manifest")
async def fixtures_manifest() -> dict[str, Any]:
    fp = FIXTURES_DIR / "MANIFEST.json"
    if not fp.exists():
        raise HTTPException(status_code=500, detail="fixture manifest missing")
    import json
    data = json.loads(fp.read_text(encoding="utf-8"))
    file_count = len(data.get("files", []))
    return {"provenance": "RECORDED_EVIDENCE", "file_count": file_count, "manifest": data}


@api.get("/roadmap")
async def roadmap() -> dict[str, Any]:
    return {"provenance": "RECORDED_EVIDENCE", **ROADMAP}


@api.get("/architecture")
async def architecture() -> dict[str, Any]:
    return {"provenance": "RECORDED_EVIDENCE", "layers": ARCHITECTURE, "gates": FIVE_GATES}


app.include_router(api)
app.add_middleware(
    CORSMiddleware,
    allow_credentials=True,
    allow_origins=os.environ.get("CORS_ORIGINS", "*").split(","),
    allow_methods=["*"],
    allow_headers=["*"],
)

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s: %(message)s")
