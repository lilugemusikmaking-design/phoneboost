use std::ffi::OsString;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::time::Duration;

use pb_host::{
    ReadyRuntime, StartupOutcome, TransportCandidate, TransportManager, TransportState,
    host_startup, os_jitter_sample, retry_delay_ms, serve_local_client,
};

struct DaemonArguments {
    manual_endpoint: Option<SocketAddr>,
}

fn main() {
    let Some(arguments) = parse_arguments(std::env::args_os().skip(1)) else {
        eprintln!("usage: phoneboostd --foreground [--manual-endpoint <literal-ip:port>]");
        std::process::exit(2);
    };

    match host_startup() {
        Ok(StartupOutcome::Ready(ready)) => {
            println!("READY");
            let _flush_result = io::stdout().flush();
            if let Some(endpoint) = arguments.manual_endpoint
                && start_manual_transport(endpoint).is_err()
            {
                eprintln!("C04_TRANSPORT state=UNAVAILABLE reason=THREAD_START_FAILED");
                std::process::exit(1);
            }
            run_local_loop(ready);
        }
        Ok(outcome @ StartupOutcome::AlreadyRunning(_)) => {
            println!("{}", outcome.as_str());
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn parse_arguments(args: impl IntoIterator<Item = OsString>) -> Option<DaemonArguments> {
    let mut args = args.into_iter();
    if !matches!(args.next().as_deref(), Some(argument) if argument == "--foreground") {
        return None;
    }
    let Some(next) = args.next() else {
        return Some(DaemonArguments {
            manual_endpoint: None,
        });
    };
    if next != "--manual-endpoint" {
        return None;
    }
    let endpoint = args.next()?.into_string().ok()?.parse().ok()?;
    if args.next().is_some() {
        return None;
    }
    Some(DaemonArguments {
        manual_endpoint: Some(endpoint),
    })
}

fn start_manual_transport(endpoint: SocketAddr) -> io::Result<()> {
    std::thread::Builder::new()
        .name("phoneboost-c04-transport".to_owned())
        .spawn(move || run_manual_transport(endpoint))?;
    Ok(())
}

fn run_manual_transport(endpoint: SocketAddr) -> ! {
    let mut transport = TransportManager::new(TransportCandidate::manual(endpoint));
    let mut retry_attempt = 0_usize;
    loop {
        match transport.connect() {
            Ok(()) => {
                let metrics = transport.metrics();
                println!(
                    "C04_TRANSPORT state=CONNECTED_UNAUTHENTICATED type=LOCAL_IP rtt_ms={} reconnect_count={} stability_score={} trust=NONE",
                    metrics.rtt_ms.unwrap_or(0),
                    metrics.reconnect_count,
                    metrics.stability_score,
                );
                let _flush_result = io::stdout().flush();
                while transport.state() == TransportState::ConnectedUnauthenticated {
                    std::thread::sleep(Duration::from_millis(200));
                    match transport.poll_loss() {
                        Ok(false) => {}
                        Ok(true) | Err(_) => break,
                    }
                }
                let metrics = transport.metrics();
                println!(
                    "C04_TRANSPORT state=LOST reconnect_count={} stability_score={} trust=NONE",
                    metrics.reconnect_count, metrics.stability_score,
                );
                let _flush_result = io::stdout().flush();
            }
            Err(_) => {
                println!("C04_TRANSPORT state=LOST reason=CONNECT_FAILED trust=NONE");
                let _flush_result = io::stdout().flush();
            }
        }
        let Ok(sample) = os_jitter_sample() else {
            eprintln!("C04_TRANSPORT state=UNAVAILABLE reason=JITTER_ENTROPY_FAILED");
            loop {
                std::thread::park();
            }
        };
        let delay_ms = retry_delay_ms(retry_attempt, sample);
        retry_attempt = retry_attempt.saturating_add(1);
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
}

fn run_local_loop(ready: ReadyRuntime) -> ! {
    loop {
        match ready.accept_local_client() {
            Ok(client) => {
                let _worker = std::thread::Builder::new()
                    .name("phoneboost-local-client".to_owned())
                    .spawn(move || serve_local_client(client));
            }
            Err(_) => std::thread::yield_now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_and_manual_endpoint_arguments_are_exact_and_closed() {
        assert!(parse_arguments([OsString::from("--foreground")]).is_some());
        assert!(parse_arguments([]).is_none());
        assert!(parse_arguments([OsString::from("foreground")]).is_none());
        assert!(
            parse_arguments([
                OsString::from("--foreground"),
                OsString::from("--manual-endpoint"),
                OsString::from("192.0.2.1:4105"),
            ])
            .is_some()
        );
        assert!(
            parse_arguments([OsString::from("--foreground"), OsString::from("--extra"),]).is_none()
        );
        assert!(
            parse_arguments([
                OsString::from("--foreground"),
                OsString::from("--manual-endpoint"),
                OsString::from("device.local:4105"),
            ])
            .is_none()
        );
    }
}
