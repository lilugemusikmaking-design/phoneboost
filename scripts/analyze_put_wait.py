#!/usr/bin/env python3
"""Local intervals only. Numeric pseudonyms join PUTs, never clock epochs.

Host and Android tables are overlays, not additive. A separate causally nested
Android interval gives a non-overlapping duration residual (not a USB RTT).
"""
import argparse
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parents[1]
NAMES = re.findall(r"\b([A-Z]\w+)\b", re.search(r"phases!\((.*?)\);", (ROOT / "crates/pb-types/src/diagnostic_timing.rs").read_text(), re.S).group(1))
P = {name: i for i, name in enumerate(NAMES)}
READY, ENQUEUE, START, SENT, NOTIFY, AWAKE = range(1000, 1006)
TX, RX, REASSEMBLED, HQ, HS, HE, HO, BUILT, RESP = range(1010, 1019)


def check_trace(trace):
    assert trace["schema"] == 1 and trace["dropped"] == 0, "trace incomplete"
    rows = trace["rows"]
    assert 0 < len(rows) <= 32768
    for row in rows:
        assert len(row) == 6 and all(type(v) is int and v >= 0 for v in row)
        assert row[3] <= row[4] and row[2] > 0
    return sorted(rows, key=lambda r: (r[3], r[4]))


def select(rows, kind, key=None):
    return [r for r in rows if r[0] == kind and (key is None or r[1] == key)]


def one(rows, kind, key):
    found = select(rows, kind, key)
    assert len(found) == 1, f"missing/duplicate event {kind}"
    return found[0]


def duration(a, b):
    # Both events MUST be from the same input trace/clock.
    assert b >= a, "local event order invalid"
    return (b - a) / 1e6


def clipped(rows, kind, thread, begin, end):
    assert end >= begin
    return sum(max(0, min(r[4], end) - max(r[3], begin)) for r in rows
               if r[0] == kind and r[2] == thread) / 1e6


def stats(values):
    return dict(median=statistics.median(values), min=min(values), max=max(values),
                values=values)


def analyze(host, android):
    h, a = check_trace(host), check_trace(android)
    # Host trace begins at daemon startup; Android is reset at the job boundary.
    # Join the complete Android PUT set, and account for any earlier 1-byte
    # readiness probe explicitly. Never discard an unmatched in-job request.
    android_keys = {r[1] for r in select(a, REASSEMBLED)}
    all_starts = select(h, START)
    starts = [r for r in all_starts if r[1] in android_keys]
    assert len(starts) == 17, "requires exactly 17 observed logical PUTs"
    keys = [r[1] for r in starts]
    assert len(set(keys)) == 17 and 0 not in keys, "ambiguous request pseudonym"
    assert set(r[1] for r in select(a, REASSEMBLED)) == set(keys)
    first_ready = min(one(h, READY, r[5])[3] for r in starts)
    setup = []
    for r in all_starts:
        if r[1] in android_keys: continue
        ready, awake = one(h, READY, r[5]), one(h, AWAKE, r[5])
        assert ready[5] == 1 and awake[3] < first_ready, "unmatched in-job PUT"
        setup.append(dict(bytes=ready[5], caller_ms=duration(ready[3], awake[3])))
    puts = []
    previous_android_sent = None
    for ordinal, start in enumerate(starts, 1):
        key, caller, ht = start[1], start[5], start[2]
        ready, enqueue, awake = [one(h, k, caller) for k in (READY, ENQUEUE, AWAKE)]
        sent, notify = [one(h, k, key) for k in (SENT, NOTIFY)]
        assert awake[5] == 1, "caller did not receive success"
        tx, rx = select(h, TX, key), select(h, RX, key)
        arx, atx = select(a, RX, key), select(a, TX, key)
        assert len(tx) == len(arx) == (69 if ordinal < 17 else 1)
        assert len(rx) == len(atx) == 1
        assert tx[0][5] & 1 and tx[-1][5] & 2
        assert arx[0][5] & 1 and arx[-1][5] & 2
        assert ready[5] == (4194240 if ordinal < 17 else 1024)
        assert sent[5] == len(tx)
        reassembled, queue, handler, ended, observed, built, response = [
            one(a, k, key) for k in (REASSEMBLED, HQ, HS, HE, HO, BUILT, RESP)]
        at = arx[0][2]
        horder = [ready[3], enqueue[3], start[3], tx[0][3], tx[-1][4],
                  sent[3], rx[0][3], rx[-1][4], notify[3], awake[3]]
        aorder = [arx[0][3], arx[-1][4], reassembled[3], queue[3], handler[3],
                  ended[3], observed[3], built[3], atx[0][3], atx[-1][4], response[3]]
        assert horder == sorted(horder) and aorder == sorted(aorder), "local causality/order"
        hs = lambda phase, lo=start[3], hi=sent[3]: clipped(h, P[phase], ht, lo, hi)
        az = lambda phase, lo=arx[0][3], hi=reassembled[3]: clipped(a, P[phase], at, lo, hi)
        host_ms = dict(
            caller_total=duration(ready[3], awake[3]),
            caller_before_enqueue=duration(ready[3], enqueue[3]),
            host_queue_before_send=duration(enqueue[3], start[3]),
            host_send_window=duration(start[3], sent[3]),
            host_crypto=hs("Encrypt"), host_socket_write=hs("WireWrite"),
            host_frame_encode=hs("FrameEncode"),
            host_wait_after_send=duration(sent[3], rx[0][3]),
            host_response_processing=duration(rx[0][3], notify[3]),
            host_response_read=hs("WireReadPrefix", rx[0][3], notify[3]) + hs("WireReadBody", rx[0][3], notify[3]),
            host_response_decrypt=hs("Decrypt", rx[0][3], notify[3]),
            host_queue_after_response=duration(notify[3], awake[3]),
            host_session_poll=clipped(h, P["SessionPoll"], ht, ready[3], awake[3]),
            host_poll_while_queued=clipped(h, P["SessionPoll"], ht, enqueue[3], start[3]),
            host_poll_waiting_response=clipped(h, P["SessionPoll"], ht, sent[3], rx[0][3]),
        )
        host_ms["host_send_other"] = host_ms["host_send_window"] - sum(host_ms[k] for k in ("host_crypto", "host_socket_write", "host_frame_encode"))
        assert host_ms["host_send_other"] >= -0.01
        exclusive = ("caller_before_enqueue", "host_queue_before_send", "host_send_window", "host_wait_after_send", "host_response_processing", "host_queue_after_response")
        assert abs(sum(host_ms[k] for k in exclusive) - host_ms["caller_total"]) < 1e-6
        android_ms = dict(
            android_local_window=duration(arx[0][3], response[3]),
            android_receive_read=az("WireReadPrefix") + az("WireReadBody"),
            android_decrypt=az("Decrypt"), android_frame_decode=az("FrameDecode"),
            android_reassembly=az("Reassembly"),
            android_session_poll_in_receive=az("SessionPoll"),
            android_handler_queue=duration(queue[3], handler[3]),
            android_handler_execution=duration(handler[3], ended[3]),
            android_copy=clipped(a, P["WorkerPutCopy"], handler[2], handler[3], ended[3]),
            android_handler_completion_poll_delay=duration(ended[3], observed[3]),
            android_handler_poll_overlay=az("BufferHandlerPoll", queue[3], observed[3]),
            android_request_parse=az("BufferRequestParse", reassembled[3], queue[3]),
            android_response_send=duration(observed[3], response[3]),
            android_response_crypto=az("Encrypt", atx[0][3], response[3]),
            android_response_write=az("WireWrite", atx[0][3], response[3]),
        )
        # Causal envelope: first request record observed AFTER host starts sending;
        # response construction complete BEFORE host observes a response prefix.
        # Subtract DURATIONS only. This residual includes leading host send work,
        # response send and transport/scheduling. Crypto overlaps Android and must
        # NOT also be subtracted from it.
        android_core = duration(arx[0][3], atx[0][3])
        host_envelope = duration(start[3], rx[0][3])
        residual = host_envelope - android_core
        assert residual >= -0.01, "invalid causal duration envelope"
        exclusive_a = ["android_receive_read", "android_decrypt", "android_frame_decode",
            "android_reassembly", "android_session_poll_in_receive", "android_handler_queue",
            "android_handler_execution", "android_handler_completion_poll_delay",
            "android_request_parse", "android_response_send"]
        android_ms["android_local_other"] = android_ms["android_local_window"] - sum(android_ms[k] for k in exclusive_a)
        assert android_ms["android_local_other"] >= -0.05, "double counted Android intervals"
        android_ms["android_pre_first_record_poll_overlay"] = (
            az("SessionPoll", previous_android_sent, arx[0][3]) if previous_android_sent is not None else None)
        previous_android_sent = response[3]
        puts.append(dict(ordinal=ordinal, bytes=ready[5], frames=len(tx), host_ms=host_ms,
            android_ms=android_ms, android_core_ms=android_core,
            host_send_to_response_ms=host_envelope, outside_android_core_ms=residual))
    assert sum(p["bytes"] for p in puts) == 67108864
    total = {side: {name: sum(p[side][name] for p in puts if p[side][name] is not None)
                    for name in puts[0][side]} for side in ("host_ms", "android_ms")}
    for key in ("android_core_ms", "host_send_to_response_ms", "outside_android_core_ms"):
        total[key] = sum(p[key] for p in puts)
    return dict(puts=puts, total=total, excluded_host_setup_puts=setup, distribution_ms={
        name: stats([p[side][name] for p in puts]) for side, name in [
            ("host_ms", "caller_total"), ("host_ms", "host_queue_before_send"),
            ("host_ms", "host_wait_after_send"), ("android_ms", "android_decrypt"),
            ("android_ms", "android_session_poll_in_receive"),
            ("android_ms", "android_handler_completion_poll_delay") ]})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("host", type=Path)
    parser.add_argument("android", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = analyze(json.loads(args.host.read_text()), json.loads(args.android.read_text()))
    if args.output: args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["total"], indent=2))


if __name__ == "__main__": main()
