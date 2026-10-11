//! C04 local-IP byte transport. This module owns bytes and metrics only.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use std::path::Path;

use pb_types::PeerId;

#[cfg(target_os = "linux")]
use nix::ifaddrs::getifaddrs;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const RETRY_BASE_MS: [u64; 5] = [1_000, 2_000, 4_000, 8_000, 15_000];
pub const MAX_CONCURRENT_ATTEMPTS_PER_DEVICE: usize = 2;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
pub const LIVE_LINK_FRESHNESS: Duration = Duration::from_secs(6);
pub const MINIMUM_RTT_SAMPLES: u16 = 3;
pub const MINIMUM_GOODPUT_WINDOW: Duration = Duration::from_secs(1);

/// C05 alone may eventually promote beyond this C04 state machine.
pub const AUTHENTICATED_BACKOFF_RESET: &str = "DEFERRED_TO_C05";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportType {
    LocalIp,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NormalizedLinkClass {
    WifiLan,
    UsbNetwork,
    EthernetIp,
    LocalIpUnknown,
}

impl NormalizedLinkClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WifiLan => "wifi_lan",
            Self::UsbNetwork => "usb_network",
            Self::EthernetIp => "ethernet_ip",
            Self::LocalIpUnknown => "local_ip_unknown",
        }
    }

    pub const fn transport_signature(self) -> &'static str {
        match self {
            Self::WifiLan => "pb-link-v1:wifi_lan",
            Self::UsbNetwork => "pb-link-v1:usb_network",
            Self::EthernetIp => "pb-link-v1:ethernet_ip",
            Self::LocalIpUnknown => "pb-link-v1:local_ip_unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkClassSource {
    OsRouteInterfaceVerified,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkClassObservation {
    class: NormalizedLinkClass,
    source: LinkClassSource,
}

impl LinkClassObservation {
    pub const fn unknown() -> Self {
        Self {
            class: NormalizedLinkClass::LocalIpUnknown,
            source: LinkClassSource::Unknown,
        }
    }

    const fn os_route_interface_verified(class: NormalizedLinkClass) -> Option<Self> {
        match class {
            NormalizedLinkClass::LocalIpUnknown => None,
            _ => Some(Self {
                class,
                source: LinkClassSource::OsRouteInterfaceVerified,
            }),
        }
    }

    pub const fn class(self) -> NormalizedLinkClass {
        self.class
    }

    pub const fn source(self) -> LinkClassSource {
        self.source
    }

    pub const fn transport_signature(self) -> &'static str {
        self.class.transport_signature()
    }

    pub const fn grants_trust(self) -> bool {
        false
    }
}

fn classify_linux_interface(interface_index: Option<u32>) -> NormalizedLinkClass {
    #[cfg(target_os = "linux")]
    {
        let Some(interface_index) = interface_index else {
            return NormalizedLinkClass::LocalIpUnknown;
        };
        classify_linux_sysfs_interface(Path::new("/sys/class/net"), interface_index)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = interface_index;
        NormalizedLinkClass::LocalIpUnknown
    }
}

#[cfg(target_os = "linux")]
fn connected_route_interface_index(stream: &TcpStream) -> Option<u32> {
    let local_ip = stream.local_addr().ok()?.ip();
    let interfaces = getifaddrs().ok()?;
    let mut matched_index = None;
    for interface in interfaces {
        let Some(address) = interface.address else {
            continue;
        };
        let interface_ip = if let Some(address) = address.as_sockaddr_in() {
            std::net::IpAddr::V4(address.ip())
        } else if let Some(address) = address.as_sockaddr_in6() {
            std::net::IpAddr::V6(address.ip())
        } else {
            continue;
        };
        if interface_ip != local_ip {
            continue;
        }
        let index =
            rustix::net::netdevice::name_to_index(stream, &interface.interface_name).ok()?;
        if matched_index.is_some_and(|matched| matched != index) {
            return None;
        }
        matched_index = Some(index);
    }
    matched_index
}

#[cfg(not(target_os = "linux"))]
fn connected_route_interface_index(_stream: &TcpStream) -> Option<u32> {
    None
}

#[cfg(target_os = "linux")]
fn classify_linux_sysfs_interface(root: &Path, interface_index: u32) -> NormalizedLinkClass {
    let Ok(entries) = std::fs::read_dir(root) else {
        return NormalizedLinkClass::LocalIpUnknown;
    };
    for entry in entries.flatten() {
        let interface = entry.path();
        let Ok(raw_index) = std::fs::read_to_string(interface.join("ifindex")) else {
            continue;
        };
        if raw_index.trim().parse::<u32>().ok() != Some(interface_index) {
            continue;
        }
        if interface.join("wireless").is_dir() {
            return NormalizedLinkClass::WifiLan;
        }
        let is_ethernet_framed = std::fs::read_to_string(interface.join("type"))
            .ok()
            .is_some_and(|value| value.trim() == "1");
        let device = std::fs::canonicalize(interface.join("device")).ok();
        if is_ethernet_framed
            && device.as_ref().is_some_and(|path| {
                path.components()
                    .any(|component| component.as_os_str().to_string_lossy().starts_with("usb"))
            })
        {
            return NormalizedLinkClass::UsbNetwork;
        }
        let is_pci_ethernet_controller = std::fs::read_to_string(interface.join("device/class"))
            .ok()
            .is_some_and(|value| value.trim().starts_with("0x0200"));
        if is_ethernet_framed && is_pci_ethernet_controller && !interface.join("bridge").exists() {
            return NormalizedLinkClass::EthernetIp;
        }
        return NormalizedLinkClass::LocalIpUnknown;
    }
    NormalizedLinkClass::LocalIpUnknown
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
    interface_index: Option<u32>,
    discovered_at: Instant,
    link: LinkClassObservation,
}

impl TransportCandidate {
    pub fn manual(endpoint: SocketAddr) -> Self {
        Self {
            endpoint,
            interface_index: None,
            discovered_at: Instant::now(),
            link: LinkClassObservation::unknown(),
        }
    }

    pub const fn discovered(
        endpoint: SocketAddr,
        interface_index: Option<u32>,
        discovered_at: Instant,
    ) -> Self {
        Self {
            endpoint,
            interface_index,
            discovered_at,
            link: LinkClassObservation::unknown(),
        }
    }

    pub const fn endpoint(self) -> SocketAddr {
        self.endpoint
    }

    pub const fn interface_index(self) -> Option<u32> {
        self.interface_index
    }

    pub const fn discovered_at(self) -> Instant {
        self.discovered_at
    }

    pub const fn link_class(self) -> NormalizedLinkClass {
        self.link.class()
    }

    pub const fn link_class_source(self) -> LinkClassSource {
        self.link.source()
    }

    pub const fn transport_signature(self) -> &'static str {
        self.link.transport_signature()
    }

    pub const fn grants_trust(self) -> bool {
        false
    }

    fn observe_connected_route(&mut self, interface_index: Option<u32>) {
        let class = classify_linux_interface(interface_index);
        self.link = LinkClassObservation::os_route_interface_verified(class)
            .unwrap_or_else(LinkClassObservation::unknown);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportMetrics {
    pub transport_type: TransportType,
    pub link_class: NormalizedLinkClass,
    pub transport_signature: &'static str,
    /// TCP connect elapsed time. It is never an RTT input to C11.
    pub connect_latency_ms: Option<u64>,
    /// Raw pre-authentication transport bytes over connected lifetime.
    pub raw_tx_bps: Option<u64>,
    /// Raw pre-authentication transport bytes over connected lifetime.
    pub raw_rx_bps: Option<u64>,
    pub reconnect_count: u64,
    pub stability_score: u8,
    pub permission_state: PermissionState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedRttMeasurement {
    sample_count: u16,
    median_millis: u64,
}

impl AuthenticatedRttMeasurement {
    pub const fn from_authenticated_request_response(
        sample_count: u16,
        median_millis: u64,
    ) -> Option<Self> {
        if sample_count < MINIMUM_RTT_SAMPLES || median_millis == 0 {
            return None;
        }
        Some(Self {
            sample_count,
            median_millis,
        })
    }

    pub const fn sample_count(self) -> u16 {
        self.sample_count
    }

    pub const fn median_millis(self) -> u64 {
        self.median_millis
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedGoodputMeasurement {
    payload_bytes: u64,
    active_millis: u64,
    bytes_per_second: u64,
}

impl AuthenticatedGoodputMeasurement {
    pub fn from_completed_authenticated_payload(
        payload_bytes: u64,
        active_duration: Duration,
    ) -> Option<Self> {
        if payload_bytes == 0 || active_duration < MINIMUM_GOODPUT_WINDOW {
            return None;
        }
        let active_millis = duration_ms_ceil(active_duration);
        Some(Self {
            payload_bytes,
            active_millis,
            bytes_per_second: byte_rate(payload_bytes, active_millis),
        })
    }

    pub const fn payload_bytes(self) -> u64 {
        self.payload_bytes
    }

    pub const fn active_millis(self) -> u64 {
        self.active_millis
    }

    pub const fn bytes_per_second(self) -> u64 {
        self.bytes_per_second
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkCostRequirements {
    pub tx: bool,
    pub rx: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedLinkCostInput {
    pub rtt_millis: u64,
    pub tx_bytes_per_second: Option<u64>,
    pub rx_bytes_per_second: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveLinkSnapshot {
    secure_session_generation: u64,
    peer_id: PeerId,
    transport_signature: String,
    observed_at: Instant,
    rtt: AuthenticatedRttMeasurement,
    tx_goodput: Option<AuthenticatedGoodputMeasurement>,
    rx_goodput: Option<AuthenticatedGoodputMeasurement>,
}

impl LiveLinkSnapshot {
    pub fn from_authenticated_session(
        secure_session_generation: u64,
        peer_id: PeerId,
        link_class: NormalizedLinkClass,
        observed_at: Instant,
        rtt: AuthenticatedRttMeasurement,
        tx_goodput: Option<AuthenticatedGoodputMeasurement>,
        rx_goodput: Option<AuthenticatedGoodputMeasurement>,
    ) -> Self {
        Self {
            secure_session_generation,
            peer_id,
            transport_signature: link_class.transport_signature().to_owned(),
            observed_at,
            rtt,
            tx_goodput,
            rx_goodput,
        }
    }

    pub fn validated_cost_input(
        &self,
        now: Instant,
        current_session_generation: u64,
        current_peer_id: PeerId,
        current_link_class: NormalizedLinkClass,
        requirements: LinkCostRequirements,
    ) -> Option<ValidatedLinkCostInput> {
        let age = now.checked_duration_since(self.observed_at)?;
        if age >= LIVE_LINK_FRESHNESS
            || self.secure_session_generation != current_session_generation
            || self.peer_id != current_peer_id
            || self.transport_signature != current_link_class.transport_signature()
            || (requirements.tx && self.tx_goodput.is_none())
            || (requirements.rx && self.rx_goodput.is_none())
        {
            return None;
        }
        Some(ValidatedLinkCostInput {
            rtt_millis: self.rtt.median_millis(),
            tx_bytes_per_second: self.tx_goodput.map(|value| value.bytes_per_second()),
            rx_bytes_per_second: self.rx_goodput.map(|value| value.bytes_per_second()),
        })
    }

    pub const fn secure_session_generation(&self) -> u64 {
        self.secure_session_generation
    }

    pub const fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub fn transport_signature(&self) -> &str {
        &self.transport_signature
    }

    pub const fn observed_at(&self) -> Instant {
        self.observed_at
    }
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
    connect_latency_ms: Option<u64>,
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
            connect_latency_ms: None,
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
        self.candidate.link = LinkClassObservation::unknown();
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
        self.candidate
            .observe_connected_route(connected_route_interface_index(&stream));
        self.connect_latency_ms = Some(duration_ms_ceil(started.elapsed()));
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
            link_class: self.candidate.link_class(),
            transport_signature: self.candidate.transport_signature(),
            connect_latency_ms: self.connect_latency_ms,
            raw_tx_bps: (self.successful_connections > 0)
                .then_some(byte_rate(self.tx_bytes, connected_ms)),
            raw_rx_bps: (self.successful_connections > 0)
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

    #[cfg(target_os = "linux")]
    use std::path::{Path, PathBuf};
    #[cfg(target_os = "linux")]
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    use super::*;

    #[cfg(target_os = "linux")]
    static NEXT_SYSFS_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[cfg(target_os = "linux")]
    struct SysfsFixture {
        root: PathBuf,
    }

    #[cfg(target_os = "linux")]
    impl SysfsFixture {
        fn new() -> Self {
            let id = NEXT_SYSFS_FIXTURE.fetch_add(1, AtomicOrdering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("phoneboost-link-class-{}-{id}", std::process::id()));
            std::fs::create_dir_all(root.join("class/net")).expect("sysfs fixture root");
            Self { root }
        }

        fn net_root(&self) -> PathBuf {
            self.root.join("class/net")
        }

        fn add_interface(
            &self,
            name: &str,
            index: u32,
            link_type: u16,
            device: Option<(&Path, Option<&str>)>,
            wireless: bool,
            bridge: bool,
        ) {
            let interface = self.net_root().join(name);
            std::fs::create_dir_all(&interface).expect("interface fixture");
            std::fs::write(interface.join("ifindex"), format!("{index}\n"))
                .expect("interface index fixture");
            std::fs::write(interface.join("type"), format!("{link_type}\n"))
                .expect("interface type fixture");
            if wireless {
                std::fs::create_dir(interface.join("wireless")).expect("wireless marker");
            }
            if bridge {
                std::fs::create_dir(interface.join("bridge")).expect("bridge marker");
            }
            if let Some((target, pci_class)) = device {
                std::fs::create_dir_all(target).expect("device target fixture");
                if let Some(pci_class) = pci_class {
                    std::fs::write(target.join("class"), format!("{pci_class}\n"))
                        .expect("PCI class fixture");
                }
                std::os::unix::fs::symlink(target, interface.join("device"))
                    .expect("device symlink fixture");
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for SysfsFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

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
        assert!(metrics.raw_tx_bps.is_some_and(|rate| rate > 0));
        assert!(metrics.raw_rx_bps.is_some_and(|rate| rate > 0));
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
        assert_eq!(metrics.link_class, NormalizedLinkClass::LocalIpUnknown);
        assert_eq!(metrics.transport_signature, "pb-link-v1:local_ip_unknown");
        assert_eq!(metrics.permission_state, PermissionState::NotApplicableHost);
        assert_eq!(metrics.reconnect_count, 0);
    }

    #[test]
    fn d1_link_classes_have_exact_non_aliasing_signatures_and_no_authority() {
        let classes = [
            NormalizedLinkClass::WifiLan,
            NormalizedLinkClass::UsbNetwork,
            NormalizedLinkClass::EthernetIp,
            NormalizedLinkClass::LocalIpUnknown,
        ];
        let signatures = classes.map(NormalizedLinkClass::transport_signature);
        assert_eq!(
            signatures,
            [
                "pb-link-v1:wifi_lan",
                "pb-link-v1:usb_network",
                "pb-link-v1:ethernet_ip",
                "pb-link-v1:local_ip_unknown",
            ]
        );
        for (index, signature) in signatures.iter().enumerate() {
            assert!(!signatures[..index].contains(signature));
        }

        let unknown = LinkClassObservation::unknown();
        assert_eq!(unknown.class(), NormalizedLinkClass::LocalIpUnknown);
        assert_eq!(unknown.source(), LinkClassSource::Unknown);
        assert!(!unknown.grants_trust());
        assert!(
            LinkClassObservation::os_route_interface_verified(NormalizedLinkClass::LocalIpUnknown)
                .is_none()
        );

        for class in classes[..3].iter().copied() {
            let verified = LinkClassObservation::os_route_interface_verified(class)
                .expect("known class can be positively observed");
            assert_eq!(verified.source(), LinkClassSource::OsRouteInterfaceVerified);
            assert_eq!(verified.transport_signature(), class.transport_signature());
            assert!(!verified.grants_trust());
        }
    }

    #[test]
    fn d1_candidates_default_unknown_without_connected_route_evidence() {
        let endpoint: SocketAddr = "192.0.2.8:48100".parse().expect("endpoint");
        let now = Instant::now();
        let mut unknown = TransportCandidate::discovered(endpoint, Some(7), now);
        assert_eq!(unknown.link_class(), NormalizedLinkClass::LocalIpUnknown);
        assert_eq!(unknown.link_class_source(), LinkClassSource::Unknown);
        unknown.observe_connected_route(None);
        assert_eq!(unknown.link_class(), NormalizedLinkClass::LocalIpUnknown);
        assert_eq!(unknown.link_class_source(), LinkClassSource::Unknown);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn d1_linux_sysfs_classification_is_positive_and_fail_closed() {
        let fixture = SysfsFixture::new();
        let pci = fixture.root.join("devices/pci0000/0000:00:01.0");
        let usb = fixture.root.join("devices/pci0000/usb1/1-2");
        let bridge_device = fixture.root.join("devices/pci0000/0000:00:02.0");
        let other_network = fixture.root.join("devices/pci0000/0000:00:03.0");

        fixture.add_interface("radio", 7, 1, None, true, false);
        fixture.add_interface("rndis", 8, 1, Some((&usb, None)), false, false);
        fixture.add_interface("wired", 9, 1, Some((&pci, Some("0x020000"))), false, false);
        fixture.add_interface("usb_name_is_not_evidence", 10, 1, None, false, false);
        fixture.add_interface(
            "bridge",
            11,
            1,
            Some((&bridge_device, Some("0x020000"))),
            false,
            true,
        );
        fixture.add_interface(
            "other_network",
            12,
            1,
            Some((&other_network, Some("0x028000"))),
            false,
            false,
        );

        let root = fixture.net_root();
        assert_eq!(
            classify_linux_sysfs_interface(&root, 7),
            NormalizedLinkClass::WifiLan
        );
        assert_eq!(
            classify_linux_sysfs_interface(&root, 8),
            NormalizedLinkClass::UsbNetwork
        );
        assert_eq!(
            classify_linux_sysfs_interface(&root, 9),
            NormalizedLinkClass::EthernetIp
        );
        assert_eq!(
            classify_linux_sysfs_interface(&root, 10),
            NormalizedLinkClass::LocalIpUnknown
        );
        assert_eq!(
            classify_linux_sysfs_interface(&root, 11),
            NormalizedLinkClass::LocalIpUnknown
        );
        assert_eq!(
            classify_linux_sysfs_interface(&root, 12),
            NormalizedLinkClass::LocalIpUnknown
        );
        assert_eq!(
            classify_linux_sysfs_interface(&root, 99),
            NormalizedLinkClass::LocalIpUnknown
        );
        assert_eq!(
            classify_linux_interface(None),
            NormalizedLinkClass::LocalIpUnknown
        );

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("loopback listener");
        let endpoint = listener.local_addr().expect("loopback endpoint");
        let worker = thread::spawn(move || listener.accept().expect("route probe accept").0);
        let stream = TcpStream::connect(endpoint).expect("route probe connect");
        assert!(connected_route_interface_index(&stream).is_some());
        assert_eq!(
            classify_linux_interface(connected_route_interface_index(&stream)),
            NormalizedLinkClass::LocalIpUnknown
        );
        drop(stream);
        drop(worker.join().expect("route probe worker"));
    }

    #[test]
    fn d2_measurement_constructors_enforce_sample_and_window_minima() {
        assert!(AuthenticatedRttMeasurement::from_authenticated_request_response(2, 8).is_none());
        assert!(AuthenticatedRttMeasurement::from_authenticated_request_response(3, 0).is_none());
        let rtt = AuthenticatedRttMeasurement::from_authenticated_request_response(3, 8)
            .expect("minimum authenticated RTT evidence");
        assert_eq!(rtt.sample_count(), 3);
        assert_eq!(rtt.median_millis(), 8);

        assert!(
            AuthenticatedGoodputMeasurement::from_completed_authenticated_payload(
                1,
                MINIMUM_GOODPUT_WINDOW - Duration::from_millis(1),
            )
            .is_none()
        );
        assert!(
            AuthenticatedGoodputMeasurement::from_completed_authenticated_payload(
                0,
                MINIMUM_GOODPUT_WINDOW,
            )
            .is_none()
        );
        let goodput = AuthenticatedGoodputMeasurement::from_completed_authenticated_payload(
            2_000,
            Duration::from_secs(2),
        )
        .expect("completed authenticated payload");
        assert_eq!(goodput.payload_bytes(), 2_000);
        assert_eq!(goodput.active_millis(), 2_000);
        assert_eq!(goodput.bytes_per_second(), 1_000);
    }

    #[test]
    fn d2_live_snapshot_is_strictly_fresh_and_bound_to_session_peer_and_link() {
        let observed_at = Instant::now();
        let peer_id = PeerId::from([7; 32]);
        let rtt = AuthenticatedRttMeasurement::from_authenticated_request_response(3, 8)
            .expect("RTT evidence");
        let tx = AuthenticatedGoodputMeasurement::from_completed_authenticated_payload(
            25_000,
            Duration::from_secs(1),
        );
        let rx = AuthenticatedGoodputMeasurement::from_completed_authenticated_payload(
            30_000,
            Duration::from_secs(1),
        );
        let snapshot = LiveLinkSnapshot::from_authenticated_session(
            11,
            peer_id,
            NormalizedLinkClass::UsbNetwork,
            observed_at,
            rtt,
            tx,
            rx,
        );
        let requirements = LinkCostRequirements { tx: true, rx: true };
        let fresh = snapshot
            .validated_cost_input(
                observed_at + LIVE_LINK_FRESHNESS - Duration::from_millis(1),
                11,
                peer_id,
                NormalizedLinkClass::UsbNetwork,
                requirements,
            )
            .expect("strictly younger snapshot");
        assert_eq!(fresh.rtt_millis, 8);
        assert_eq!(fresh.tx_bytes_per_second, Some(25_000));
        assert_eq!(fresh.rx_bytes_per_second, Some(30_000));
        assert!(
            snapshot
                .validated_cost_input(
                    observed_at + LIVE_LINK_FRESHNESS,
                    11,
                    peer_id,
                    NormalizedLinkClass::UsbNetwork,
                    requirements,
                )
                .is_none()
        );
        assert!(
            snapshot
                .validated_cost_input(
                    observed_at + Duration::from_secs(1),
                    12,
                    peer_id,
                    NormalizedLinkClass::UsbNetwork,
                    requirements,
                )
                .is_none()
        );
        assert!(
            snapshot
                .validated_cost_input(
                    observed_at + Duration::from_secs(1),
                    11,
                    PeerId::from([8; 32]),
                    NormalizedLinkClass::UsbNetwork,
                    requirements,
                )
                .is_none()
        );
        assert!(
            snapshot
                .validated_cost_input(
                    observed_at + Duration::from_secs(1),
                    11,
                    peer_id,
                    NormalizedLinkClass::WifiLan,
                    requirements,
                )
                .is_none()
        );
        assert!(
            snapshot
                .validated_cost_input(
                    observed_at - Duration::from_millis(1),
                    11,
                    peer_id,
                    NormalizedLinkClass::UsbNetwork,
                    requirements,
                )
                .is_none()
        );
    }

    #[test]
    fn d2_required_goodput_direction_never_defaults_to_zero() {
        let observed_at = Instant::now();
        let peer_id = PeerId::from([9; 32]);
        let rtt = AuthenticatedRttMeasurement::from_authenticated_request_response(3, 5)
            .expect("RTT evidence");
        let snapshot = LiveLinkSnapshot::from_authenticated_session(
            3,
            peer_id,
            NormalizedLinkClass::WifiLan,
            observed_at,
            rtt,
            None,
            None,
        );
        assert!(
            snapshot
                .validated_cost_input(
                    observed_at,
                    3,
                    peer_id,
                    NormalizedLinkClass::WifiLan,
                    LinkCostRequirements {
                        tx: true,
                        rx: false,
                    },
                )
                .is_none()
        );
        let rtt_only = snapshot
            .validated_cost_input(
                observed_at,
                3,
                peer_id,
                NormalizedLinkClass::WifiLan,
                LinkCostRequirements {
                    tx: false,
                    rx: false,
                },
            )
            .expect("RTT-only consumer");
        assert_eq!(rtt_only.tx_bytes_per_second, None);
        assert_eq!(rtt_only.rx_bytes_per_second, None);
    }
}
