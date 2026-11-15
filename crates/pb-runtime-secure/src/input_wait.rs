//! Readiness wait for both authenticated endpoints, including Android.
//! Never consumes bytes, changes socket mode, or extends protocol deadlines.
use crate::RuntimeError;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    Readable,
    Timeout,
    PartialPrefix,
}

pub(crate) fn wait(stream: &TcpStream, timeout: Duration) -> Result<Wake, RuntimeError> {
    #[cfg(feature = "diagnostic-timing")]
    let mut observation = crate::wait_diagnostics::WaitTrace::new();
    // A single queued prefix byte is readable to poll, but cannot be consumed
    // by the existing record reader. Retain its old bounded sleep to avoid spin.
    let mut prefix = [0; 2];
    match stream.peek(&mut prefix) {
        Ok(0) => {
            #[cfg(feature = "diagnostic-timing")]
            observation.observed(pb_types::put_timing::WAIT_CLOSED_OR_ERROR);
            return Err(RuntimeError::SessionLost);
        }
        Ok(1) => {
            // A FIN behind an incomplete prefix cannot be consumed by the
            // record reader. Detect it without spinning or waiting for liveness.
            let mut fds = [PollFd::new(stream, PollFlags::IN | PollFlags::RDHUP)];
            let zero = Timespec::default();
            match poll(&mut fds, Some(&zero)) {
                Ok(_)
                    if fds[0].revents().intersects(
                        PollFlags::RDHUP | PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL,
                    ) =>
                {
                    return Err(RuntimeError::SessionLost);
                }
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(_) => return Err(RuntimeError::SessionLost),
            }
            #[cfg(feature = "diagnostic-timing")]
            observation.observed(pb_types::put_timing::WAIT_PARTIAL);
            std::thread::sleep(timeout);
            pb_types::timed!(SessionPartialWait, 0, ());
            return Ok(Wake::PartialPrefix);
        }
        Ok(_) => {
            #[cfg(feature = "diagnostic-timing")]
            observation.observed(pb_types::put_timing::WAIT_ALREADY_READABLE);
            pb_types::timed!(SessionReadableWake, 0, ());
            return Ok(Wake::Readable);
        }
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
        Err(_) => return Err(RuntimeError::SessionLost),
    }
    let started = Instant::now();
    let timeout_spec = Timespec::try_from(timeout).map_err(|_| RuntimeError::SessionLost)?;
    let mut fds = [PollFd::new(stream, PollFlags::IN)];
    let wake = match poll(&mut fds, Some(&timeout_spec)) {
        Ok(0) => Wake::Timeout,
        Ok(_) if fds[0].revents().contains(PollFlags::NVAL) => {
            return Err(RuntimeError::SessionLost);
        }
        // HUP/ERR wake the existing receive path, which handles EOF and errors.
        Ok(_) => Wake::Readable,
        Err(rustix::io::Errno::INTR) => {
            // No unbounded retry or signal-induced spin. Recheck cancellation
            // and liveness after the remainder of this existing wait quantum.
            std::thread::sleep(timeout.saturating_sub(started.elapsed()));
            Wake::Timeout
        }
        Err(_) => return Err(RuntimeError::SessionLost),
    };
    #[cfg(feature = "diagnostic-timing")]
    observation.after_poll(stream, wake == Wake::Timeout);
    match wake {
        Wake::Readable => pb_types::timed!(SessionReadableWake, 0, ()),
        Wake::Timeout => pb_types::timed!(SessionTimeoutWake, 0, ()),
        Wake::PartialPrefix => unreachable!(),
    }
    Ok(wake)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};

    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (receiver, _) = listener.accept().unwrap();
        receiver.set_nonblocking(true).unwrap();
        (sender, receiver)
    }

    #[test]
    fn queued_and_delayed_data_wake_without_consuming_or_changing_mode() {
        for delayed in [false, true] {
            let (mut sender, mut receiver) = pair();
            if !delayed {
                sender.write_all(b"ab").unwrap();
            }
            let writer = std::thread::spawn(move || {
                if delayed {
                    std::thread::sleep(Duration::from_millis(30));
                    sender.write_all(b"ab").unwrap();
                }
                sender
            });
            let start = Instant::now();
            assert_eq!(wait(&receiver, Duration::from_secs(2)), Ok(Wake::Readable));
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "must wake before the timeout"
            );
            let _sender = writer.join().unwrap();
            let mut bytes = [0; 2];
            receiver.read_exact(&mut bytes).unwrap();
            assert_eq!(bytes, *b"ab");
            assert_eq!(receiver.read_timeout().unwrap(), None);
            assert_eq!(
                receiver.read(&mut bytes).unwrap_err().kind(),
                ErrorKind::WouldBlock
            );
        }
    }

    #[test]
    fn empty_and_partial_prefix_waits_are_bounded_and_do_not_spin() {
        for partial in [false, true] {
            let (mut sender, receiver) = pair();
            if partial {
                sender.write_all(&[7]).unwrap();
            }
            let start = Instant::now();
            let mut waits = 0;
            while start.elapsed() < Duration::from_millis(100) {
                let wake = wait(&receiver, Duration::from_millis(20)).unwrap();
                assert_eq!(
                    wake,
                    if partial {
                        Wake::PartialPrefix
                    } else {
                        Wake::Timeout
                    }
                );
                waits += 1;
            }
            assert!(waits <= 6, "idle must block, not busy-spin: {waits}");
            if partial {
                sender.write_all(&[8]).unwrap();
                assert_eq!(wait(&receiver, Duration::from_secs(1)), Ok(Wake::Readable));
            }
        }
    }

    #[test]
    fn truncated_prefix_then_fin_is_session_lost() {
        let (mut sender, receiver) = pair();
        sender.write_all(&[7]).unwrap();
        sender.shutdown(Shutdown::Write).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match wait(&receiver, Duration::from_millis(20)) {
                Err(RuntimeError::SessionLost) => break,
                Ok(Wake::PartialPrefix) => assert!(Instant::now() < deadline),
                other => panic!("unexpected truncated record result: {other:?}"),
            }
        }
    }

    #[test]
    fn remote_close_and_local_shutdown_wake_and_fail_closed() {
        for local in [false, true] {
            let (sender, receiver) = pair();
            let closer = receiver.try_clone().unwrap();
            let closing = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                if local {
                    closer.shutdown(Shutdown::Both).unwrap();
                }
                drop(sender);
            });
            let start = Instant::now();
            assert!(matches!(
                wait(&receiver, Duration::from_secs(2)),
                Ok(Wake::Readable) | Err(RuntimeError::SessionLost)
            ));
            closing.join().unwrap();
            assert!(start.elapsed() < Duration::from_secs(1));
            assert_eq!(
                wait(&receiver, Duration::from_millis(20)),
                Err(RuntimeError::SessionLost)
            );
        }
    }
}
