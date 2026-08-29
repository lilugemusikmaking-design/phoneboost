use std::ffi::OsString;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pb_host::{
    AutoUseController, AutoUseReason, AutoUseState, AvahiDiscovery, FixedDeviceDiscovery,
    ReadyRuntime, StartupOutcome, TransportCandidate, TransportManager, TransportState,
    host_startup, initialize_remote_secure, os_jitter_sample, retry_delay_ms, serve_local_client,
};
use pb_runtime_secure::{SecureRuntime, run_initiator_session};

const AUTO_USE_STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const AUTO_USE_STARTUP_POLL: Duration = Duration::from_millis(10);

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
            let runtime = match initialize_remote_secure() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!(
                        "C05_SECURE state=UNAVAILABLE reason={}",
                        error.reason_code()
                    );
                    std::process::exit(1);
                }
            };
            let mut auto_use = None;
            if let Some(endpoint) = arguments.manual_endpoint
                && runtime.snapshot().committed_peer_count == 0
            {
                if start_manual_transport(endpoint, runtime).is_err() {
                    eprintln!("C04_TRANSPORT state=UNAVAILABLE reason=THREAD_START_FAILED");
                    std::process::exit(1);
                }
            } else {
                let discovery: Arc<dyn pb_host::DeviceDiscovery> =
                    if let Some(endpoint) = arguments.manual_endpoint {
                        Arc::new(FixedDeviceDiscovery::new(TransportCandidate::manual(
                            endpoint,
                        )))
                    } else {
                        Arc::new(AvahiDiscovery::new())
                    };
                let controller = match AutoUseController::new(runtime, discovery) {
                    Ok(controller) => controller,
                    Err(_) => {
                        eprintln!("AUTO_USE state=UNAVAILABLE reason=THREAD_START_FAILED");
                        std::process::exit(1);
                    }
                };
                controller.enable();
                let startup = wait_for_auto_use_startup(
                    || {
                        let status = controller.current_node_status();
                        (status.state(), status.reason())
                    },
                    AUTO_USE_STARTUP_TIMEOUT,
                );
                let (report, ready) = auto_use_startup_report(startup);
                if ready {
                    println!("{report}");
                    let _flush_result = io::stdout().flush();
                } else {
                    eprintln!("{report}");
                }
                auto_use = Some(controller);
            }
            if auto_use.is_none() {
                println!("READY");
                let _flush_result = io::stdout().flush();
            }
            let _auto_use = auto_use;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AutoUseStartup {
    Ready,
    DiscoveryBackendUnavailable,
    StatusTimeout,
}

fn wait_for_auto_use_startup(
    mut status: impl FnMut() -> (AutoUseState, AutoUseReason),
    timeout: Duration,
) -> AutoUseStartup {
    let deadline = Instant::now() + timeout;
    loop {
        let (state, reason) = status();
        if reason == AutoUseReason::DiscoveryBackendUnavailable {
            return AutoUseStartup::DiscoveryBackendUnavailable;
        }
        if state != AutoUseState::Discovering {
            return AutoUseStartup::Ready;
        }
        if Instant::now() >= deadline {
            return AutoUseStartup::StatusTimeout;
        }
        std::thread::sleep(AUTO_USE_STARTUP_POLL);
    }
}

const fn auto_use_startup_report(startup: AutoUseStartup) -> (&'static str, bool) {
    match startup {
        AutoUseStartup::Ready => ("READY", true),
        AutoUseStartup::DiscoveryBackendUnavailable => (
            "AUTO_USE state=UNAVAILABLE reason=DISCOVERY_BACKEND_UNAVAILABLE",
            false,
        ),
        AutoUseStartup::StatusTimeout => (
            "AUTO_USE state=UNAVAILABLE reason=STARTUP_STATUS_TIMEOUT",
            false,
        ),
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

fn start_manual_transport(endpoint: SocketAddr, runtime: Arc<SecureRuntime>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("phoneboost-c04-transport".to_owned())
        .spawn(move || run_manual_transport(endpoint, runtime))?;
    Ok(())
}

fn run_manual_transport(endpoint: SocketAddr, runtime: Arc<SecureRuntime>) -> ! {
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
                while transport.state() == TransportState::ConnectedUnauthenticated
                    && !runtime.session_requested()
                {
                    std::thread::sleep(Duration::from_millis(200));
                    match transport.poll_loss() {
                        Ok(false) => {}
                        Ok(true) | Err(_) => break,
                    }
                }
                if transport.state() == TransportState::ConnectedUnauthenticated {
                    match transport.take_connected_stream() {
                        Ok(mut stream) => match run_initiator_session(&mut stream, &runtime) {
                            Ok(_) => println!("C05_SECURE state=LOST reason=SESSION_CLOSED"),
                            Err(error) => {
                                println!("C05_SECURE state=LOST reason={}", error.reason_code())
                            }
                        },
                        Err(_) => println!("C05_SECURE state=LOST reason=DEVICE_LOST"),
                    }
                    let _flush_result = io::stdout().flush();
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

    #[test]
    fn unavailable_discovery_is_visible_and_never_reports_ready() {
        let outcome = wait_for_auto_use_startup(
            || {
                (
                    AutoUseState::Unavailable,
                    AutoUseReason::DiscoveryBackendUnavailable,
                )
            },
            Duration::ZERO,
        );
        assert_eq!(outcome, AutoUseStartup::DiscoveryBackendUnavailable);
        assert_ne!(outcome, AutoUseStartup::Ready);
        let (visible, ready) = auto_use_startup_report(outcome);
        assert_eq!(
            visible,
            "AUTO_USE state=UNAVAILABLE reason=DISCOVERY_BACKEND_UNAVAILABLE"
        );
        assert!(!ready);
        assert_ne!(visible, "READY");
    }
}
