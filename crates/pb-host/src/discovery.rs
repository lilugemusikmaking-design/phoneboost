//! Production Linux DNS-SD discovery.
//!
//! Avahi only supplies untrusted C04 transport hints. C05 Noise IK and the
//! committed peer pin remain the sole path to authenticated session authority.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use dbus::Path;
use dbus::blocking::Connection;
use dbus::message::MatchRule;

use crate::{DeviceDiscovery, DiscoveryError, TransportCandidate};

pub const DNS_SD_SERVICE_TYPE: &str = "_phoneboost._tcp";
pub const DISCOVERY_CANDIDATE_LIFETIME: Duration = Duration::from_secs(30);

const AVAHI_DESTINATION: &str = "org.freedesktop.Avahi";
const AVAHI_SERVER_PATH: &str = "/";
const AVAHI_SERVER_INTERFACE: &str = "org.freedesktop.Avahi.Server";
const AVAHI_BROWSER_INTERFACE: &str = "org.freedesktop.Avahi.ServiceBrowser";
const AVAHI_INTERFACE_UNSPEC: i32 = -1;
const AVAHI_PROTOCOL_UNSPEC: i32 = -1;
const AVAHI_LOOKUP_FLAGS_NONE: u32 = 0;
const DBUS_TIMEOUT: Duration = Duration::from_secs(2);
const DBUS_PROCESS_TIMEOUT: Duration = Duration::from_millis(100);
const RESOLVE_REFRESH: Duration = Duration::from_secs(10);
const MAX_TRACKED_SERVICES: usize = 64;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ServiceKey {
    interface: i32,
    protocol: i32,
    name: String,
    service_type: String,
    domain: String,
}

#[derive(Debug)]
enum BrowserEvent {
    Added {
        browser_path: String,
        service: ServiceKey,
    },
    Removed {
        browser_path: String,
        service: ServiceKey,
    },
    Failed {
        browser_path: String,
    },
}

#[derive(Default)]
struct BrowserSignals {
    events: Vec<BrowserEvent>,
}

struct KnownService {
    last_resolve: Option<Instant>,
}

struct ResolvedCandidate {
    service: ServiceKey,
    candidate: TransportCandidate,
    refreshed_at: Instant,
}

#[derive(Default)]
struct CandidateCache {
    candidates: Vec<ResolvedCandidate>,
    next_candidate: usize,
}

struct AvahiBackend {
    connection: Connection,
    browser_path: Path<'static>,
    signals: Arc<Mutex<BrowserSignals>>,
    services: HashMap<ServiceKey, KnownService>,
    cache: CandidateCache,
}

#[derive(Default)]
struct AvahiState {
    backend: Option<AvahiBackend>,
}

/// Debian-family production discovery through Avahi's system D-Bus API.
///
/// Returned endpoints are attacker-controlled hints until the secure initiator
/// completes Noise IK and verifies a committed peer pin.
#[derive(Default)]
pub struct AvahiDiscovery {
    state: Mutex<AvahiState>,
}

impl AvahiDiscovery {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(AvahiState { backend: None }),
        }
    }
}

impl DeviceDiscovery for AvahiDiscovery {
    fn start(&self) -> Result<(), DiscoveryError> {
        let mut state = lock(&self.state);
        if state.backend.is_some() {
            return Ok(());
        }
        state.backend = Some(AvahiBackend::start()?);
        Ok(())
    }

    fn discover(&self) -> Result<Option<TransportCandidate>, DiscoveryError> {
        let mut state = lock(&self.state);
        let backend = state
            .backend
            .as_mut()
            .ok_or(DiscoveryError::BackendUnavailable)?;
        if backend.poll().is_err() {
            state.backend = None;
            return Err(DiscoveryError::BackendUnavailable);
        }
        Ok(backend.next_candidate(Instant::now()))
    }

    fn stop(&self) {
        let backend = lock(&self.state).backend.take();
        if let Some(backend) = backend {
            backend.free_browser();
        }
    }
}

impl AvahiBackend {
    fn start() -> Result<Self, DiscoveryError> {
        let connection =
            Connection::new_system().map_err(|_| DiscoveryError::BackendUnavailable)?;
        let signals = Arc::new(Mutex::new(BrowserSignals::default()));

        add_item_match(&connection, Arc::clone(&signals), "ItemNew", true)?;
        add_item_match(&connection, Arc::clone(&signals), "ItemRemove", false)?;
        add_failure_match(&connection, Arc::clone(&signals))?;

        let proxy = connection.with_proxy(AVAHI_DESTINATION, AVAHI_SERVER_PATH, DBUS_TIMEOUT);
        let (browser_path,): (Path<'static>,) = proxy
            .method_call(
                AVAHI_SERVER_INTERFACE,
                "ServiceBrowserNew",
                (
                    AVAHI_INTERFACE_UNSPEC,
                    AVAHI_PROTOCOL_UNSPEC,
                    DNS_SD_SERVICE_TYPE,
                    "",
                    AVAHI_LOOKUP_FLAGS_NONE,
                ),
            )
            .map_err(|_| DiscoveryError::BackendUnavailable)?;

        Ok(Self {
            connection,
            browser_path,
            signals,
            services: HashMap::new(),
            cache: CandidateCache::default(),
        })
    }

    fn poll(&mut self) -> Result<(), ()> {
        self.connection
            .process(DBUS_PROCESS_TIMEOUT)
            .map_err(|_| ())?;
        self.apply_events()?;

        let now = Instant::now();
        let due = self.services.iter().find_map(|(service, known)| {
            known
                .last_resolve
                .is_none_or(|last| now.saturating_duration_since(last) >= RESOLVE_REFRESH)
                .then_some(service.clone())
        });

        if let Some(service) = due {
            if let Some(known) = self.services.get_mut(&service) {
                known.last_resolve = Some(now);
            }
            if let Some(candidate) = resolve_service(&self.connection, &service, now) {
                self.cache.refresh(service, candidate, now);
            }
        }
        self.cache.expire(now);
        Ok(())
    }

    fn apply_events(&mut self) -> Result<(), ()> {
        let events = std::mem::take(&mut lock(&self.signals).events);
        let browser_path = self.browser_path.to_string();
        for event in events {
            match event {
                BrowserEvent::Added {
                    browser_path: event_path,
                    service,
                } if event_path == browser_path && service.service_type == DNS_SD_SERVICE_TYPE => {
                    if self.services.contains_key(&service)
                        || self.services.len() < MAX_TRACKED_SERVICES
                    {
                        self.services
                            .entry(service)
                            .or_insert(KnownService { last_resolve: None });
                    }
                }
                BrowserEvent::Removed {
                    browser_path: event_path,
                    service,
                } if event_path == browser_path => {
                    self.services.remove(&service);
                    self.cache
                        .candidates
                        .retain(|candidate| candidate.service != service);
                }
                BrowserEvent::Failed {
                    browser_path: event_path,
                } if event_path == browser_path => return Err(()),
                _ => {}
            }
        }
        Ok(())
    }

    fn next_candidate(&mut self, now: Instant) -> Option<TransportCandidate> {
        self.cache.next(now)
    }

    fn free_browser(self) {
        let proxy = self
            .connection
            .with_proxy(AVAHI_DESTINATION, self.browser_path, DBUS_TIMEOUT);
        let _: Result<(), _> = proxy.method_call(AVAHI_BROWSER_INTERFACE, "Free", ());
    }
}

impl CandidateCache {
    fn refresh(&mut self, service: ServiceKey, candidate: TransportCandidate, now: Instant) {
        if let Some(existing) = self.candidates.iter_mut().find(|existing| {
            existing.service == service && existing.candidate.endpoint() == candidate.endpoint()
        }) {
            existing.candidate = candidate;
            existing.refreshed_at = now;
            return;
        }
        self.candidates.push(ResolvedCandidate {
            service,
            candidate,
            refreshed_at: now,
        });
    }

    fn expire(&mut self, now: Instant) {
        self.candidates.retain(|candidate| {
            now.saturating_duration_since(candidate.refreshed_at) < DISCOVERY_CANDIDATE_LIFETIME
        });
        if self.candidates.is_empty() {
            self.next_candidate = 0;
        } else {
            self.next_candidate %= self.candidates.len();
        }
    }

    fn next(&mut self, now: Instant) -> Option<TransportCandidate> {
        self.expire(now);
        let candidate = self.candidates.get(self.next_candidate)?.candidate;
        self.next_candidate = (self.next_candidate + 1) % self.candidates.len();
        Some(candidate)
    }
}

fn add_item_match(
    connection: &Connection,
    signals: Arc<Mutex<BrowserSignals>>,
    member: &'static str,
    added: bool,
) -> Result<(), DiscoveryError> {
    connection
        .add_match(
            MatchRule::new_signal(AVAHI_BROWSER_INTERFACE, member),
            move |args: (i32, i32, String, String, String, u32), _, message| {
                let Some(path) = message.path().map(|path| path.to_string()) else {
                    return true;
                };
                let service = ServiceKey {
                    interface: args.0,
                    protocol: args.1,
                    name: args.2,
                    service_type: args.3,
                    domain: args.4,
                };
                let event = if added {
                    BrowserEvent::Added {
                        browser_path: path,
                        service,
                    }
                } else {
                    BrowserEvent::Removed {
                        browser_path: path,
                        service,
                    }
                };
                lock(&signals).events.push(event);
                true
            },
        )
        .map(|_| ())
        .map_err(|_| DiscoveryError::BackendUnavailable)
}

fn add_failure_match(
    connection: &Connection,
    signals: Arc<Mutex<BrowserSignals>>,
) -> Result<(), DiscoveryError> {
    connection
        .add_match(
            MatchRule::new_signal(AVAHI_BROWSER_INTERFACE, "Failure"),
            move |_: (String,), _, message| {
                if let Some(path) = message.path() {
                    lock(&signals).events.push(BrowserEvent::Failed {
                        browser_path: path.to_string(),
                    });
                }
                true
            },
        )
        .map(|_| ())
        .map_err(|_| DiscoveryError::BackendUnavailable)
}

fn resolve_service(
    connection: &Connection,
    service: &ServiceKey,
    discovered_at: Instant,
) -> Option<TransportCandidate> {
    type ResolveReply = (
        i32,
        i32,
        String,
        String,
        String,
        String,
        i32,
        String,
        u16,
        Vec<Vec<u8>>,
        u32,
    );

    let proxy = connection.with_proxy(AVAHI_DESTINATION, AVAHI_SERVER_PATH, DBUS_TIMEOUT);
    let reply: ResolveReply = proxy
        .method_call(
            AVAHI_SERVER_INTERFACE,
            "ResolveService",
            (
                service.interface,
                service.protocol,
                service.name.as_str(),
                service.service_type.as_str(),
                service.domain.as_str(),
                service.protocol,
                AVAHI_LOOKUP_FLAGS_NONE,
            ),
        )
        .ok()?;

    if reply.3 != DNS_SD_SERVICE_TYPE || reply.8 == 0 {
        return None;
    }
    // TXT data is intentionally ignored. It never contributes protocol,
    // capability, peer identity, key, lease, or authority truth.
    let address: IpAddr = reply.7.parse().ok()?;
    let interface_index = u32::try_from(reply.0).ok();
    let endpoint = match address {
        IpAddr::V4(address) => SocketAddr::new(IpAddr::V4(address), reply.8),
        IpAddr::V6(address) => SocketAddr::V6(SocketAddrV6::new(
            address,
            reply.8,
            0,
            ipv6_scope_id(address, interface_index),
        )),
    };
    Some(TransportCandidate::discovered(
        endpoint,
        interface_index,
        discovered_at,
    ))
}

const fn ipv6_scope_id(address: Ipv6Addr, interface_index: Option<u32>) -> u32 {
    if address.is_unicast_link_local() {
        match interface_index {
            Some(index) => index,
            None => 0,
        }
    } else {
        0
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str) -> ServiceKey {
        ServiceKey {
            interface: 7,
            protocol: AVAHI_PROTOCOL_UNSPEC,
            name: name.to_owned(),
            service_type: DNS_SD_SERVICE_TYPE.to_owned(),
            domain: "local".to_owned(),
        }
    }

    #[test]
    fn candidate_expires_thirty_seconds_without_refresh() {
        let now = Instant::now();
        let mut cache = CandidateCache::default();
        cache.refresh(
            service("PhoneBoost-a1b2c3d4"),
            TransportCandidate::discovered("192.0.2.8:48100".parse().unwrap(), Some(7), now),
            now,
        );

        assert!(
            cache
                .next(now + DISCOVERY_CANDIDATE_LIFETIME - Duration::from_nanos(1))
                .is_some()
        );
        assert!(cache.next(now + DISCOVERY_CANDIDATE_LIFETIME).is_none());
    }

    #[test]
    fn link_local_ipv6_preserves_interface_scope() {
        let address: Ipv6Addr = "fe80::1234".parse().unwrap();
        assert_eq!(ipv6_scope_id(address, Some(19)), 19);
        assert_eq!(ipv6_scope_id(address, None), 0);
        assert_eq!(ipv6_scope_id("2001:db8::1".parse().unwrap(), Some(19)), 0);
    }

    #[test]
    fn candidates_are_round_robin_without_address_family_priority() {
        let now = Instant::now();
        let mut cache = CandidateCache::default();
        let first: SocketAddr = "[2001:db8::1]:48100".parse().unwrap();
        let second: SocketAddr = "192.0.2.8:48100".parse().unwrap();
        cache.refresh(
            service("PhoneBoost-v6"),
            TransportCandidate::discovered(first, Some(7), now),
            now,
        );
        cache.refresh(
            service("PhoneBoost-v4"),
            TransportCandidate::discovered(second, Some(7), now),
            now,
        );

        assert_eq!(cache.next(now).unwrap().endpoint(), first);
        assert_eq!(cache.next(now).unwrap().endpoint(), second);
        assert_eq!(cache.next(now).unwrap().endpoint(), first);
    }
}
