use std::net::SocketAddr;

use pb_host::{TransportCandidate, TransportManager, TransportState};

const D0_PROBE: [u8; 17] = *b"PHONEBOOST-C04-D0";

fn main() {
    let endpoint = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| {
            eprintln!("usage: c04-lan-probe <literal-ip:port>");
            std::process::exit(2);
        });
    let mut transport = TransportManager::new(TransportCandidate::manual(endpoint));
    let mut loss_pass = false;
    for connection in 0..2 {
        transport
            .connect()
            .unwrap_or_else(|error| fail(&error.to_string()));
        if transport.state() != TransportState::ConnectedUnauthenticated {
            fail("state exceeded or missed CONNECTED_UNAUTHENTICATED");
        }
        transport
            .send(&D0_PROBE)
            .unwrap_or_else(|error| fail(&error.to_string()));
        let mut echoed = [0_u8; D0_PROBE.len()];
        transport
            .recv_exact(&mut echoed)
            .unwrap_or_else(|error| fail(&error.to_string()));
        if echoed != D0_PROBE {
            fail("raw byte echo differs");
        }
        let mut eof = [0_u8; 1];
        if transport
            .recv(&mut eof)
            .unwrap_or_else(|error| fail(&error.to_string()))
            != 0
            || transport.state() != TransportState::Lost
        {
            fail("peer close did not become TRANSPORT_LOST");
        }
        loss_pass = true;
        if connection == 0 && transport.metrics().reconnect_count != 0 {
            fail("first connection counted as reconnect");
        }
    }
    let metrics = transport.metrics();
    if metrics.reconnect_count != 1 || !loss_pass {
        fail("reconnect accounting mismatch");
    }
    println!(
        "C04_PROBE connect=PASS bidirectional=PASS loss=PASS reconnect=PASS max_state=CONNECTED_UNAUTHENTICATED rtt_ms={} tx_Bps={} rx_Bps={} reconnect_count={} stability_score={} permission_state=NOT_APPLICABLE_HOST",
        metrics.rtt_ms.unwrap_or(0),
        metrics.sustained_tx_bps.unwrap_or(0),
        metrics.sustained_rx_bps.unwrap_or(0),
        metrics.reconnect_count,
        metrics.stability_score,
    );
}

fn fail(message: &str) -> ! {
    eprintln!("C04_PROBE FAIL: {message}");
    std::process::exit(1);
}
