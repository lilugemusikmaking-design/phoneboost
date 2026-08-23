use std::ffi::OsString;
use std::io::{self, Write};

use pb_host::{ReadyRuntime, StartupOutcome, host_startup, serve_local_client};

fn main() {
    if !foreground_requested(std::env::args_os().skip(1)) {
        eprintln!("usage: phoneboostd --foreground");
        std::process::exit(2);
    }

    match host_startup() {
        Ok(StartupOutcome::Ready(ready)) => {
            println!("READY");
            let _flush_result = io::stdout().flush();
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

fn foreground_requested(args: impl IntoIterator<Item = OsString>) -> bool {
    let mut args = args.into_iter();
    matches!(args.next().as_deref(), Some(argument) if argument == "--foreground")
        && args.next().is_none()
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
    fn foreground_flag_is_exact_and_closed() {
        assert!(foreground_requested([OsString::from("--foreground")]));
        assert!(!foreground_requested([]));
        assert!(!foreground_requested([OsString::from("foreground")]));
        assert!(!foreground_requested([
            OsString::from("--foreground"),
            OsString::from("--extra"),
        ]));
    }
}
