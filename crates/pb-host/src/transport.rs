//! C04 local-IP byte transport. This module owns bytes and metrics only.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const RETRY_BASE_MS: [u64; 5] = [1_000, 2_000, 4_000, 8_000, 15_000];
pub const MAX_CONCURRENT_ATTEMPTS_PER_DEVICE: usize = 2;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// C05 alone may eventually promote beyond this C04 state machine.
pub const AUTHENTICATED_BACKOFF_RESET: &str = "DEFERRED_TO_C05";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportType {
    LocalIp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionState {
    NotApplicableHost,
    Granted,
    Denied,
    NotExecutedEnvLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportState {
    Unavailable,
    Discovered,
    Connecting,
    ConnectedUnauthenticated,
    Lost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportCandidate {
    endpoint: SocketAddr,
}

impl TransportCandidate {
    pub const fn manual(endpoint: SocketAddr) -> Self {
        Self { endpoint }
    }

    pub const fn endpoint(self) -> SocketAddr {
        self.endpoint
    }

    pub const fn grants_trust(self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportMetrics {
    pub transport_type: TransportType,
    pub rtt_ms: Option<u64>,
    pub sustained_tx_bps: Option<u64>,
    pub sustained_rx_bps: Option<u64>,
    pub reconnect_count: u64,
    pub stability_score: u8,
    pub permission_state: PermissionState,
}

#[derive(Debug)]
pub enum TransportError {
    AttemptLimit,
    NotConnected,
    Io(io::Error),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AttemptLimit => formatter.write_str("C04 connect attempt limit reached"),
            Self::NotConnected => formatter.write_str("C04 byte stream is not connected"),
            Self::Io(error) => write!(formatter, "C04 byte stream I/O failed: {error}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone)]
pub struct ConnectAttemptLimiter {
    active: Arc<AtomicUsize>,
}

impl ConnectAttemptLimiter {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn try_begin(&self) -> Result<ConnectAttemptPermit, TransportError> {
        loop {
            let current = self.active.load(Ordering::Acquire);
            if current >= MAX_CONCURRENT_ATTEMPTS_PER_DEVICE {
                return Err(TransportError::AttemptLimit);
            }
            if self
                .active
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(ConnectAttemptPermit {
                    active: Arc::clone(&self.active),
                });
            }
        }
    }

    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

impl Default for ConnectAttemptLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ConnectAttemptPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for ConnectAttemptPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Returns the canonical retry delay using a caller-provided OS/test sample.
pub fn retry_delay_ms(attempt: usize, jitter_sample: u16) -> u64 {
    let base = RETRY_BASE_MS[attempt.min(RETRY_BASE_MS.len() - 1)];
    let span = base / 5;
    let scaled = u64::from(jitter_sample) * (span * 2 + 1) / (u64::from(u16::MAX) + 1);
    base.saturating_sub(span).saturating_add(scaled)
}

pub fn os_jitter_sample() -> io::Result<u16> {
    let mut bytes = [0_u8; 2];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(u16::from_be_bytes(bytes))
}

/// Single-owner host-side C04 transport manager. It never authenticates.
pub struct TransportManager {
    candidate: TransportCandidate,
    state: TransportState,
    stream: Option<TcpStream>,
    limiter: ConnectAttemptLimiter,
    created_at: Instant,
    connected_at: Option<Instant>,
    accumulated_connected: Duration,
    successful_connections: u64,
    reconnect_count: u64,
    rtt_ms: Option<u64>,
    tx_bytes: u64,
    rx_bytes: u64,
}

impl TransportManager {
    pub fn new(candidate: TransportCandidate) -> Self {
        Self {
            candidate,
            state: TransportState::Discovered,
            stream: None,
            limiter: ConnectAttemptLimiter::new(),
            created_at: Instant::now(),
            connected_at: None,
            accumulated_connected: Duration::ZERO,
            successful_connections: 0,
            reconnect_count: 0,
            rtt_ms: None,
            tx_bytes: 0,
            rx_bytes: 0,
        }
    }

    pub const fn candidate(&self) -> TransportCandidate {
        self.candidate
    }

    pub const fn state(&self) -> TransportState {
        self.state
    }

    pub fn connect(&mut self) -> Result<(), TransportError> {
        self.finish_connected_period();
        self.stream = None;
        self.state = TransportState::Connecting;
        let _permit = self.limiter.try_begin()?;
        let started = Instant::now();
        let result = TcpStream::connect_timeout(&self.candidate.endpoint(), CONNECT_TIMEOUT);
        let stream = match result {
            Ok(stream) => stream,
            Err(error) => {
                self.state = TransportState::Lost;
                return Err(TransportError::Io(error));
            }
        };
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        self.rtt_ms = Some(duration_ms_ceil(started.elapsed()));
        if self.successful_connections > 0 {
            self.reconnect_count = self.reconnect_count.saturating_add(1);
        }
        self.successful_connections = self.successful_connections.saturating_add(1);
        self.connected_at = Some(Instant::now());
        self.stream = Some(stream);
        self.state = TransportState::ConnectedUnauthenticated;
        Ok(())
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        let stream = self.stream.as_mut().ok_or(TransportError::NotConnected)?;
        if let Err(error) = stream.write_all(bytes) {
            self.mark_lost();
            return Err(TransportError::Io(error));
        }
        self.tx_bytes = self.tx_bytes.saturating_add(bytes.len() as u64);
        Ok(())
    }

    pub fn recv(&mut self, output: &mut [u8]) -> Result<usize, TransportError> {
        let stream = self.stream.as_mut().ok_or(TransportError::NotConnected)?;
        match stream.read(output) {
            Ok(0) => {
                self.mark_lost();
                Ok(0)
            }
            Ok(read) => {
                self.rx_bytes = self.rx_bytes.saturating_add(read as u64);
                Ok(read)
            }
            Err(error) => {
                self.mark_lost();
                Err(TransportError::Io(error))
            }
        }
    }

    pub fn recv_exact(&mut self, output: &mut [u8]) -> Result<(), TransportError> {
        let mut offset = 0;
        while offset < output.len() {
            let read = self.recv(&mut output[offset..])?;
            if read == 0 {
                return Err(TransportError::NotConnected);
            }
            offset += read;
        }
        Ok(())
    }

    pub fn poll_loss(&mut self) -> Result<bool, TransportError> {
        let stream = self.stream.as_ref().ok_or(TransportError::NotConnected)?;
        stream.set_nonblocking(true)?;
        let mut byte = [0_u8; 1];
        let result = stream.peek(&mut byte);
        let restore = stream.set_nonblocking(false);
        restore?;
        match result {
            Ok(0) => {
                self.mark_lost();
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(error) => {
                self.mark_lost();
                Err(TransportError::Io(error))
            }
        }
    }

    pub fn close(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _shutdown = stream.shutdown(Shutdown::Both);
        }
        self.finish_connected_period();
        self.state = TransportState::Lost;
    }

    pub fn take_connected_stream(&mut self) -> Result<TcpStream, TransportError> {
        if self.state != TransportState::ConnectedUnauthenticated {
            return Err(TransportError::NotConnected);
        }
        self.finish_connected_period();
        self.state = TransportState::Lost;
        self.stream.take().ok_or(TransportError::NotConnected)
    }

    pub fn metrics(&self) -> TransportMetrics {
        let total = self.created_at.elapsed();
        let connected = self.accumulated_connected
            + self.connected_at.map_or(Duration::ZERO, |at| at.elapsed());
        let elapsed_ms = duration_ms_ceil(total).max(1);
        let connected_ms = duration_ms_ceil(connected).max(1);
        let stability = (connected_ms.saturating_mul(100) / elapsed_ms).min(100);
        TransportMetrics {
            transport_type: TransportType::LocalIp,
            rtt_ms: self.rtt_ms,
            sustained_tx_bps: (self.successful_connections > 0)
                .then_some(byte_rate(self.tx_bytes, connected_ms)),
            sustained_rx_bps: (self.successful_connections > 0)
                .then_some(byte_rate(self.rx_bytes, connected_ms)),
            reconnect_count: self.reconnect_count,
            stability_score: stability as u8,
            permission_state: PermissionState::NotApplicableHost,
        }
    }

    fn mark_lost(&mut self) {
        self.stream = None;
        self.finish_connected_period();
        self.state = TransportState::Lost;
    }

    fn finish_connected_period(&mut self) {
        if let Some(started) = self.connected_at.take() {
            self.accumulated_connected =
                self.accumulated_connected.saturating_add(started.elapsed());
        }
    }
}

impl Drop for TransportManager {
    fn drop(&mut self) {
        self.close();
    }
}

fn duration_ms_ceil(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .max(u64::from(!duration.is_zero()))
}

fn byte_rate(bytes: u64, connected_ms: u64) -> u64 {
    if bytes == 0 {
        0
    } else {
        (bytes.saturating_mul(1_000) / connected_ms).max(1)
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn echo_twice() -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("loopback listener");
        let endpoint = listener.local_addr().expect("loopback endpoint");
        let worker = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut bytes = [0_u8; 16];
                stream.read_exact(&mut bytes).expect("raw bytes");
                stream.write_all(&bytes).expect("raw echo");
            }
        });
        (endpoint, worker)
    }

    #[test]
    fn c04_t03_t05_t06_t11_t12_real_raw_connect_loss_and_reconnect() {
        let (endpoint, worker) = echo_twice();
        let candidate = TransportCandidate::manual(endpoint);
        assert!(!candidate.grants_trust());
        let mut manager = TransportManager::new(candidate);
        for expected_reconnects in 0..=1 {
            manager.connect().expect("real loopback connect");
            assert_eq!(manager.state(), TransportState::ConnectedUnauthenticated);
            let sent = [0_u8, 0xff, 1, 2, 3, 0x80, 7, 6, 5, 4, 0, 9, 8, 7, 6, 5];
            manager.send(&sent).expect("raw send");
            let mut received = [0_u8; 16];
            manager.recv_exact(&mut received).expect("raw receive");
            assert_eq!(received, sent);
            let mut eof = [0_u8; 1];
            assert_eq!(manager.recv(&mut eof).expect("physical EOF"), 0);
            assert_eq!(manager.state(), TransportState::Lost);
            assert_eq!(manager.metrics().reconnect_count, expected_reconnects);
        }
        worker.join().expect("bounded echo worker");
        let metrics = manager.metrics();
        assert!(metrics.sustained_tx_bps.is_some_and(|rate| rate > 0));
        assert!(metrics.sustained_rx_bps.is_some_and(|rate| rate > 0));
    }

    #[test]
    fn c04_t08_connect_timeout_is_exactly_five_seconds() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(5));
    }

    #[test]
    fn c04_t09_retry_schedule_and_jitter_are_exactly_bounded() {
        for (attempt, base) in RETRY_BASE_MS.into_iter().enumerate() {
            assert_eq!(retry_delay_ms(attempt, 0), base - base / 5);
            assert!(retry_delay_ms(attempt, u16::MAX) <= base + base / 5);
            assert!(retry_delay_ms(attempt, u16::MAX) >= base);
        }
        assert!(retry_delay_ms(99, 0) >= 12_000);
        assert!(retry_delay_ms(99, u16::MAX) <= 18_000);
        assert_eq!(AUTHENTICATED_BACKOFF_RESET, "DEFERRED_TO_C05");
    }

    #[test]
    fn c04_t10_attempt_limiter_never_exceeds_two() {
        let limiter = ConnectAttemptLimiter::new();
        let first = limiter.try_begin().expect("first");
        let second = limiter.try_begin().expect("second");
        assert_eq!(limiter.active(), 2);
        assert!(matches!(
            limiter.try_begin(),
            Err(TransportError::AttemptLimit)
        ));
        drop(first);
        assert!(limiter.try_begin().is_ok());
        drop(second);
    }

    #[test]
    fn c04_t13_t14_manual_hint_is_untrusted_and_metrics_are_observed() {
        let endpoint: SocketAddr = "127.0.0.1:9".parse().expect("endpoint");
        let candidate = TransportCandidate::manual(endpoint);
        let manager = TransportManager::new(candidate);
        assert!(!manager.candidate().grants_trust());
        assert_eq!(manager.state(), TransportState::Discovered);
        let metrics = manager.metrics();
        assert_eq!(metrics.transport_type, TransportType::LocalIp);
        assert_eq!(metrics.permission_state, PermissionState::NotApplicableHost);
        assert_eq!(metrics.reconnect_count, 0);
    }
}
