import asyncio

import server


def test_recorded_release_is_current_p0_closure_truth():
    snapshot = server._snapshot_recorded()
    assert snapshot["provenance"] == "RECORDED_EVIDENCE"
    assert snapshot["release"]["head"] == "b53ea3b84a4085ab45de58385f115f1cbd9176ed"
    assert snapshot["release"]["native_baseline"] == "162539c2ec3721f1aa45557900988e2a4291202f"
    assert snapshot["live_available"] is False
    assert all(gate["state"] == "UNAVAILABLE" for gate in snapshot["gates"])


def test_hosted_probe_never_claims_live_native_state():
    probe = asyncio.run(server.live_probe())
    assert probe["provenance"] == "UNAVAILABLE"
    assert probe["reachable"] is False
    assert "local" in probe["reason"].lower()


def test_p0_physical_closure_is_on_recorded_evidence_surface():
    item = next(
        card
        for card in server.EVIDENCE_CARDS
        if card["id"] == "c07-c12-p0-remote-compute"
    )
    assert item["provenance"] == "RECORDED_EVIDENCE"
    assert item["file"] == "c07-c12-p0-remote-compute-physical.txt"
    assert "REMOTE_SUCCESS" in item["summary"]
    assert "NO_FALSE_REMOTE_SUCCESS PASS" in item["summary"]


def test_p1_local_browser_proof_is_on_recorded_evidence_surface():
    item = next(
        card
        for card in server.EVIDENCE_CARDS
        if card["id"] == "p1-live-bridge-local-browser"
    )
    assert item["provenance"] == "RECORDED_EVIDENCE"
    assert item["file"] == "p1-live-bridge-local-browser-physical.txt"
    assert "REMOTE_SUCCESS" in item["summary"]
    assert "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE" in item["summary"]


def test_obsolete_p0_roadmap_claims_are_removed():
    joined = "\n".join(server.ROADMAP["next"] + server.ROADMAP["future"])
    assert "C05→C07 production authorization seam" not in joined
    assert "RemoteBuffer storage + operations" not in joined
    assert "Native compute providers" not in joined


def test_cors_configuration_is_closed_by_default_and_rejects_wildcard():
    assert server._configured_origins("") == []
    assert server._configured_origins(" * ") == []
    assert server._configured_origins(" https://control.example , * ,") == [
        "https://control.example"
    ]
