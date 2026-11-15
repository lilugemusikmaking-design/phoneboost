//! Observation only: local readiness observation is NOT a kernel arrival timestamp.
use pb_types::put_timing as trace;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant};

pub(crate) struct WaitTrace {
    start: Option<Instant>,
    outcome: u64,
    reported: bool,
}

impl WaitTrace {
    pub(crate) fn new() -> Self {
        Self {
            start: trace::now(),
            outcome: trace::WAIT_CLOSED_OR_ERROR,
            reported: false,
        }
    }
    pub(crate) fn observed(&mut self, outcome: u64) {
        self.outcome = outcome;
        self.reported = true;
        trace::interval(trace::WAIT_READY, 0, self.start, outcome);
    }
    pub(crate) fn after_poll(&mut self, stream: &TcpStream, timed_out: bool) {
        if self.start.is_some() {
            self.observed(if timed_out {
                trace::WAIT_TIMER
            } else {
                classify(stream, false)
            });
        }
    }
}

impl Drop for WaitTrace {
    fn drop(&mut self) {
        if !self.reported {
            self.observed(self.outcome);
        }
        trace::interval(trace::WAIT_FINISHED, 0, self.start, self.outcome);
    }
}

fn classify(stream: &TcpStream, immediate: bool) -> u64 {
    match stream.peek(&mut [0; 2]) {
        Ok(0) => trace::WAIT_CLOSED_OR_ERROR,
        Ok(1) => trace::WAIT_PARTIAL,
        Ok(_) => {
            if immediate {
                trace::WAIT_ALREADY_READABLE
            } else {
                trace::WAIT_DATA
            }
        }
        Err(_) => trace::WAIT_CLOSED_OR_ERROR,
    }
}

/// Instrumented legacy responder: observe once, then retain the entire original
/// quantum. Probe failures never alter the baseline's outcome or socket state.
pub(crate) fn legacy_sleep(stream: &TcpStream, duration: Duration) {
    let mut observation = WaitTrace::new();
    let Some(start) = observation.start else {
        std::thread::sleep(duration);
        return;
    };
    let kind = match stream.peek(&mut [0; 2]) {
        Err(e) if e.kind() == ErrorKind::WouldBlock => {
            let mut fds = [PollFd::new(stream, PollFlags::IN | PollFlags::RDHUP)];
            match Timespec::try_from(duration) {
                Ok(timeout) => match poll(&mut fds, Some(&timeout)) {
                    Ok(0) => trace::WAIT_TIMER,
                    Ok(_) => classify(stream, false),
                    Err(_) => trace::WAIT_CLOSED_OR_ERROR,
                },
                Err(_) => trace::WAIT_CLOSED_OR_ERROR,
            }
        }
        _ => classify(stream, true),
    };
    observation.observed(kind);
    std::thread::sleep(duration.saturating_sub(start.elapsed()));
}
