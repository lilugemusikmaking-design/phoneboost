use std::collections::BTreeMap;

use crate::lease::{AuthenticatedSession, ControllerLeaseManager, LeaseId};
use crate::{WorkerIncarnationId, random_nonzero_128};

pub const POC_CAP_BYTES: u64 = 128 * 1024 * 1024;
pub const MIN_AVAILABLE_BYTES: u64 = 768 * 1024 * 1024;
pub const RECOVERY_AVAILABLE_BYTES: u64 = 896 * 1024 * 1024;
pub const MEMORY_MARGIN_BYTES: u64 = 512 * 1024 * 1024;
pub const RESERVATION_TTL_MS: u64 = 30_000;
pub const HEALTH_INTERVAL_MS: u64 = 2_000;
pub const HEALTH_STALE_AFTER_MS: u64 = 6_000;
pub const TERMINAL_RETENTION_MS: u64 = 5 * 60_000;
const IDEMPOTENCE_CAPACITY: usize = 1_024;
pub const NATIVE_OPERATION_THREADS: usize = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(i32)]
pub enum ThermalBand {
    None = 0,
    Light = 1,
    Moderate = 2,
    Severe = 3,
    Critical = 4,
    Emergency = 5,
    Shutdown = 6,
    Unknown = 7,
}

impl ThermalBand {
    pub const fn from_android_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::None),
            1 => Some(Self::Light),
            2 => Some(Self::Moderate),
            3 => Some(Self::Severe),
            4 => Some(Self::Critical),
            5 => Some(Self::Emergency),
            6 => Some(Self::Shutdown),
            _ => None,
        }
    }

    const fn safety(self) -> SafetyBand {
        match self {
            Self::None | Self::Light => SafetyBand::Nominal,
            Self::Moderate => SafetyBand::Throttle,
            Self::Severe | Self::Critical | Self::Emergency | Self::Shutdown | Self::Unknown => {
                SafetyBand::RefusedThermal
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(i32)]
pub enum BatteryBand {
    Nominal = 0,
    Throttle = 1,
    Refused = 2,
    Unknown = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(i32)]
pub enum SafetyBand {
    Nominal = 0,
    Throttle = 1,
    RefusedMemoryPressure = 2,
    RefusedThermal = 3,
    RefusedBattery = 4,
    RefusedStaleState = 5,
}

impl SafetyBand {
    pub const fn is_refused(self) -> bool {
        matches!(
            self,
            Self::RefusedMemoryPressure
                | Self::RefusedThermal
                | Self::RefusedBattery
                | Self::RefusedStaleState
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthSample {
    pub available_memory_bytes: u64,
    pub low_memory: bool,
    pub thermal: ThermalBand,
    pub battery_percent: u8,
    pub charging: bool,
    pub power_save: bool,
    pub monotonic_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthStatus {
    pub safety: SafetyBand,
    pub thermal: ThermalBand,
    pub battery: BatteryBand,
    pub available_memory_bytes: u64,
    pub low_memory: bool,
    pub charging: bool,
    pub power_save: bool,
    pub sample_age_ms: Option<u64>,
    pub fresh: bool,
    pub budget_bytes: u64,
}

impl HealthStatus {
    const fn unavailable() -> Self {
        Self {
            safety: SafetyBand::RefusedStaleState,
            thermal: ThermalBand::Unknown,
            battery: BatteryBand::Unknown,
            available_memory_bytes: 0,
            low_memory: true,
            charging: false,
            power_save: false,
            sample_age_ms: None,
            fresh: false,
            budget_bytes: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum ResourceGuardState {
    Active = 1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReservationId([u8; 16]);

impl ReservationId {
    pub fn is_nonzero(self) -> bool {
        self.0.iter().any(|byte| *byte != 0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RequestId([u8; 16]);

impl RequestId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[cfg(test)]
    pub(crate) const fn test_only(value: u128) -> Self {
        Self(value.to_be_bytes())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceClass {
    Poc,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReserveRequest {
    pub request_id: RequestId,
    pub class: ResourceClass,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReservationDecision {
    Reserved { reservation_id: ReservationId },
    RefusedSafety,
    RefusedBudget,
    UnknownResourceClass,
    StaleLease,
    IdempotenceConflict,
    IdempotenceTableFull,
    EntropyUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitDecision {
    Committed,
    RefusedSafety,
    StaleLease,
    UnknownOrExpiredReservation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseDecision {
    Released,
    AlreadyTerminal,
    StaleLease,
    UnknownReservation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReservationState {
    Reserved,
    Committed,
    Released,
    Expired,
    RefusedSafety,
}

#[derive(Clone, Copy)]
struct Reservation {
    lease_id: LeaseId,
    incarnation: WorkerIncarnationId,
    request_id: RequestId,
    bytes: u64,
    expires_at_ms: u64,
    state: ReservationState,
    terminal_at_ms: Option<u64>,
}

#[derive(Clone, Copy)]
struct IdempotenceEntry {
    lease_id: LeaseId,
    incarnation: WorkerIncarnationId,
    class: ResourceClass,
    bytes: u64,
    decision: ReservationDecision,
    terminal_at_ms: Option<u64>,
}

#[derive(Clone, Copy)]
struct Recovery<T> {
    target: T,
    since_ms: u64,
    samples: u8,
}

/// C08 single-writer actor. It owns all reservation and safety state.
pub struct ResourceGuard {
    latest: Option<HealthSample>,
    sample_count: u64,
    memory_blocked: bool,
    memory_recovery_since_ms: Option<u64>,
    effective_thermal: ThermalBand,
    thermal_recovery: Option<Recovery<ThermalBand>>,
    effective_battery: BatteryBand,
    battery_recovery: Option<Recovery<BatteryBand>>,
    allocated_bytes: u64,
    reservations: BTreeMap<ReservationId, Reservation>,
    idempotence: BTreeMap<RequestId, IdempotenceEntry>,
}

impl ResourceGuard {
    pub const fn new() -> Self {
        Self {
            latest: None,
            sample_count: 0,
            memory_blocked: true,
            memory_recovery_since_ms: None,
            effective_thermal: ThermalBand::Unknown,
            thermal_recovery: None,
            effective_battery: BatteryBand::Unknown,
            battery_recovery: None,
            allocated_bytes: 0,
            reservations: BTreeMap::new(),
            idempotence: BTreeMap::new(),
        }
    }

    pub const fn state(&self) -> ResourceGuardState {
        ResourceGuardState::Active
    }

    pub const fn health_sample_count(&self) -> u64 {
        self.sample_count
    }

    pub fn record_health(&mut self, sample: HealthSample) -> HealthStatus {
        self.update_memory(sample);
        self.update_thermal(sample);
        self.update_battery(sample);
        self.latest = Some(sample);
        self.sample_count = self.sample_count.saturating_add(1);
        self.health_status(sample.monotonic_ms)
    }

    pub fn health_status(&self, now_ms: u64) -> HealthStatus {
        let Some(sample) = self.latest else {
            return HealthStatus::unavailable();
        };
        let age = now_ms.saturating_sub(sample.monotonic_ms);
        let fresh = now_ms >= sample.monotonic_ms && age <= HEALTH_STALE_AFTER_MS;
        let memory_refused = self.memory_blocked
            || sample.low_memory
            || sample.available_memory_bytes < MIN_AVAILABLE_BYTES;
        let mut safety = if memory_refused {
            SafetyBand::RefusedMemoryPressure
        } else if self.effective_thermal.safety().is_refused() {
            SafetyBand::RefusedThermal
        } else if self.effective_battery == BatteryBand::Refused {
            SafetyBand::RefusedBattery
        } else if self.effective_thermal.safety() == SafetyBand::Throttle
            || self.effective_battery == BatteryBand::Throttle
            || sample.power_save
        {
            SafetyBand::Throttle
        } else {
            SafetyBand::Nominal
        };
        if !fresh {
            safety = SafetyBand::RefusedStaleState;
        }
        HealthStatus {
            safety,
            thermal: self.effective_thermal,
            battery: self.effective_battery,
            available_memory_bytes: sample.available_memory_bytes,
            low_memory: sample.low_memory,
            charging: sample.charging,
            power_save: sample.power_save,
            sample_age_ms: Some(age),
            fresh,
            budget_bytes: if fresh && !memory_refused {
                poc_budget(sample.available_memory_bytes)
            } else {
                0
            },
        }
    }

    pub fn reserve(
        &mut self,
        leases: &mut ControllerLeaseManager,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        request: ReserveRequest,
        now_ms: u64,
    ) -> ReservationDecision {
        let proof = match leases.validate(session, lease_id, now_ms) {
            Ok(proof) => proof,
            Err(_) => return ReservationDecision::StaleLease,
        };
        self.expire_and_purge(now_ms, lease_id, proof.incarnation);
        if let Some(existing) = self.idempotence.get(&request.request_id) {
            if existing.lease_id != proof.lease_id || existing.incarnation != proof.incarnation {
                return ReservationDecision::IdempotenceConflict;
            }
            if existing.class == request.class && existing.bytes == request.bytes {
                return existing.decision;
            }
            return ReservationDecision::IdempotenceConflict;
        }
        if self.idempotence.len() >= IDEMPOTENCE_CAPACITY {
            return ReservationDecision::IdempotenceTableFull;
        }
        let health = self.health_status(now_ms);
        let decision = if request.class == ResourceClass::Unknown {
            ReservationDecision::UnknownResourceClass
        } else if health.safety.is_refused() {
            ReservationDecision::RefusedSafety
        } else {
            let remaining = health.budget_bytes.saturating_sub(self.allocated_bytes);
            if request.bytes == 0 || request.bytes > remaining {
                ReservationDecision::RefusedBudget
            } else {
                match random_nonzero_128() {
                    Ok(bytes) => {
                        let reservation_id = ReservationId(bytes);
                        self.allocated_bytes = self.allocated_bytes.saturating_add(request.bytes);
                        self.reservations.insert(
                            reservation_id,
                            Reservation {
                                lease_id: proof.lease_id,
                                incarnation: proof.incarnation,
                                request_id: request.request_id,
                                bytes: request.bytes,
                                expires_at_ms: now_ms.saturating_add(RESERVATION_TTL_MS),
                                state: ReservationState::Reserved,
                                terminal_at_ms: None,
                            },
                        );
                        ReservationDecision::Reserved { reservation_id }
                    }
                    Err(_) => ReservationDecision::EntropyUnavailable,
                }
            }
        };
        let terminal_at_ms = match decision {
            ReservationDecision::Reserved { .. } => None,
            _ => Some(now_ms),
        };
        self.idempotence.insert(
            request.request_id,
            IdempotenceEntry {
                lease_id: proof.lease_id,
                incarnation: proof.incarnation,
                class: request.class,
                bytes: request.bytes,
                decision,
                terminal_at_ms,
            },
        );
        decision
    }

    pub fn commit(
        &mut self,
        leases: &mut ControllerLeaseManager,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        reservation_id: ReservationId,
        now_ms: u64,
    ) -> CommitDecision {
        let proof = match leases.validate(session, lease_id, now_ms) {
            Ok(proof) => proof,
            Err(_) => return CommitDecision::StaleLease,
        };
        self.expire_and_purge(now_ms, lease_id, proof.incarnation);
        let Some(reservation) = self.reservations.get(&reservation_id).copied() else {
            return CommitDecision::UnknownOrExpiredReservation;
        };
        if reservation.lease_id != proof.lease_id || reservation.incarnation != proof.incarnation {
            return CommitDecision::StaleLease;
        }
        match reservation.state {
            ReservationState::Committed => return CommitDecision::Committed,
            ReservationState::Reserved => {}
            ReservationState::Released
            | ReservationState::Expired
            | ReservationState::RefusedSafety => {
                return CommitDecision::UnknownOrExpiredReservation;
            }
        }
        if self.health_status(now_ms).safety.is_refused() {
            self.release_allocation(reservation.bytes);
            if let Some(entry) = self.reservations.get_mut(&reservation_id) {
                entry.state = ReservationState::RefusedSafety;
                entry.terminal_at_ms = Some(now_ms);
            }
            self.mark_idempotence_terminal(reservation.request_id, now_ms);
            return CommitDecision::RefusedSafety;
        }
        if let Some(entry) = self.reservations.get_mut(&reservation_id) {
            entry.state = ReservationState::Committed;
        }
        CommitDecision::Committed
    }

    pub fn release(
        &mut self,
        leases: &mut ControllerLeaseManager,
        session: &AuthenticatedSession,
        lease_id: LeaseId,
        reservation_id: ReservationId,
        now_ms: u64,
    ) -> ReleaseDecision {
        let proof = match leases.validate(session, lease_id, now_ms) {
            Ok(proof) => proof,
            Err(_) => return ReleaseDecision::StaleLease,
        };
        self.expire_and_purge(now_ms, lease_id, proof.incarnation);
        let Some(reservation) = self.reservations.get(&reservation_id).copied() else {
            return ReleaseDecision::UnknownReservation;
        };
        if reservation.lease_id != proof.lease_id || reservation.incarnation != proof.incarnation {
            return ReleaseDecision::StaleLease;
        }
        match reservation.state {
            ReservationState::Reserved | ReservationState::Committed => {
                self.release_allocation(reservation.bytes);
                if let Some(entry) = self.reservations.get_mut(&reservation_id) {
                    entry.state = ReservationState::Released;
                    entry.terminal_at_ms = Some(now_ms);
                }
                self.mark_idempotence_terminal(reservation.request_id, now_ms);
                ReleaseDecision::Released
            }
            ReservationState::Released
            | ReservationState::Expired
            | ReservationState::RefusedSafety => ReleaseDecision::AlreadyTerminal,
        }
    }

    fn update_memory(&mut self, sample: HealthSample) {
        if sample.low_memory || sample.available_memory_bytes < MIN_AVAILABLE_BYTES {
            self.memory_blocked = true;
            self.memory_recovery_since_ms = None;
        } else if self.memory_blocked && sample.available_memory_bytes >= RECOVERY_AVAILABLE_BYTES {
            let since = self
                .memory_recovery_since_ms
                .get_or_insert(sample.monotonic_ms);
            if sample.monotonic_ms.saturating_sub(*since) >= 10_000 {
                self.memory_blocked = false;
                self.memory_recovery_since_ms = None;
            }
        } else if self.memory_blocked {
            self.memory_recovery_since_ms = None;
        }
    }

    fn update_thermal(&mut self, sample: HealthSample) {
        let observed = sample.thermal;
        if self.latest.is_none() || observed > self.effective_thermal {
            self.effective_thermal = observed;
            self.thermal_recovery = None;
        } else if observed < self.effective_thermal {
            match &mut self.thermal_recovery {
                Some(recovery) if recovery.target == observed => {
                    recovery.samples = recovery.samples.saturating_add(1);
                    if recovery.samples >= 2
                        && sample.monotonic_ms.saturating_sub(recovery.since_ms) >= 10_000
                    {
                        self.effective_thermal = observed;
                        self.thermal_recovery = None;
                    }
                }
                _ => {
                    self.thermal_recovery = Some(Recovery {
                        target: observed,
                        since_ms: sample.monotonic_ms,
                        samples: 1,
                    });
                }
            }
        } else {
            self.thermal_recovery = None;
        }
    }

    fn update_battery(&mut self, sample: HealthSample) {
        let observed = raw_battery_band(sample.battery_percent, sample.charging);
        if sample.charging {
            self.effective_battery = BatteryBand::Nominal;
            self.battery_recovery = None;
        } else if self.latest.is_none() || observed > self.effective_battery {
            self.effective_battery = observed;
            self.battery_recovery = None;
        } else if observed < self.effective_battery {
            let threshold_met = match self.effective_battery {
                BatteryBand::Refused => sample.battery_percent >= 20,
                BatteryBand::Throttle => sample.battery_percent >= 30,
                BatteryBand::Nominal | BatteryBand::Unknown => false,
            };
            if !threshold_met {
                self.battery_recovery = None;
                return;
            }
            let target = if self.effective_battery == BatteryBand::Refused {
                BatteryBand::Throttle
            } else {
                BatteryBand::Nominal
            };
            match &mut self.battery_recovery {
                Some(recovery) if recovery.target == target => {
                    recovery.samples = recovery.samples.saturating_add(1);
                    if sample.monotonic_ms.saturating_sub(recovery.since_ms) >= 30_000 {
                        self.effective_battery = target;
                        self.battery_recovery = None;
                    }
                }
                _ => {
                    self.battery_recovery = Some(Recovery {
                        target,
                        since_ms: sample.monotonic_ms,
                        samples: 1,
                    });
                }
            }
        } else {
            self.battery_recovery = None;
        }
    }

    fn expire_and_purge(
        &mut self,
        now_ms: u64,
        active_lease: LeaseId,
        active_incarnation: WorkerIncarnationId,
    ) {
        let superseded: Vec<_> = self
            .reservations
            .iter()
            .filter_map(|(id, reservation)| {
                (reservation.lease_id != active_lease
                    || reservation.incarnation != active_incarnation)
                    .then_some((*id, reservation.bytes, reservation.state))
            })
            .collect();
        for (id, bytes, state) in superseded {
            if matches!(
                state,
                ReservationState::Reserved | ReservationState::Committed
            ) {
                self.release_allocation(bytes);
            }
            self.reservations.remove(&id);
        }
        let expired: Vec<_> = self
            .reservations
            .iter()
            .filter_map(|(id, reservation)| {
                (reservation.state == ReservationState::Reserved
                    && now_ms >= reservation.expires_at_ms)
                    .then_some((*id, reservation.bytes, reservation.request_id))
            })
            .collect();
        for (id, bytes, request_id) in expired {
            self.release_allocation(bytes);
            if let Some(reservation) = self.reservations.get_mut(&id) {
                reservation.state = ReservationState::Expired;
                reservation.terminal_at_ms = Some(now_ms);
            }
            self.mark_idempotence_terminal(request_id, now_ms);
        }
        self.idempotence.retain(|_, entry| {
            let belongs_to_active =
                entry.lease_id == active_lease && entry.incarnation == active_incarnation;
            let retained_terminal = entry
                .terminal_at_ms
                .is_none_or(|terminal| now_ms.saturating_sub(terminal) <= TERMINAL_RETENTION_MS);
            belongs_to_active && retained_terminal
        });
        self.reservations.retain(|_, reservation| {
            reservation
                .terminal_at_ms
                .is_none_or(|terminal| now_ms.saturating_sub(terminal) <= TERMINAL_RETENTION_MS)
        });
    }

    fn mark_idempotence_terminal(&mut self, request_id: RequestId, now_ms: u64) {
        if let Some(entry) = self.idempotence.get_mut(&request_id) {
            entry.terminal_at_ms = Some(now_ms);
        }
    }

    fn release_allocation(&mut self, bytes: u64) {
        self.allocated_bytes = self.allocated_bytes.saturating_sub(bytes);
    }
}

impl Default for ResourceGuard {
    fn default() -> Self {
        Self::new()
    }
}

const fn raw_battery_band(percent: u8, charging: bool) -> BatteryBand {
    if charging || percent > 25 {
        BatteryBand::Nominal
    } else if percent < 15 {
        BatteryBand::Refused
    } else {
        BatteryBand::Throttle
    }
}

pub const fn poc_budget(available_bytes: u64) -> u64 {
    let after_margin = available_bytes.saturating_sub(MEMORY_MARGIN_BYTES);
    let quarter = after_margin / 4;
    if quarter < POC_CAP_BYTES {
        quarter
    } else {
        POC_CAP_BYTES
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn incarnation() -> WorkerIncarnationId {
        WorkerIncarnationId([1; 16])
    }

    fn healthy(now_ms: u64) -> HealthSample {
        HealthSample {
            available_memory_bytes: 1_024 * MIB,
            low_memory: false,
            thermal: ThermalBand::None,
            battery_percent: 80,
            charging: false,
            power_save: false,
            monotonic_ms: now_ms,
        }
    }

    fn ready_guard(now_ms: u64) -> ResourceGuard {
        let mut guard = ResourceGuard::new();
        guard.record_health(healthy(now_ms.saturating_sub(10_000)));
        guard.record_health(healthy(now_ms));
        guard
    }

    fn authority(now_ms: u64) -> (ControllerLeaseManager, AuthenticatedSession, LeaseId) {
        let session = AuthenticatedSession::test_only(7);
        let mut leases = ControllerLeaseManager::new(incarnation());
        let lease = leases.acquire(&session, now_ms).expect("lease");
        (leases, session, lease)
    }

    fn request(id: u128, bytes: u64) -> ReserveRequest {
        ReserveRequest {
            request_id: RequestId::test_only(id),
            class: ResourceClass::Poc,
            bytes,
        }
    }

    fn reservation_id(decision: ReservationDecision) -> ReservationId {
        match decision {
            ReservationDecision::Reserved { reservation_id } => reservation_id,
            other => panic!("expected reservation, got {other:?}"),
        }
    }

    #[test]
    fn rg_t12_budget_formula_and_128_mib_cap_are_exact() {
        assert_eq!(poc_budget(512 * MIB), 0);
        assert_eq!(poc_budget(768 * MIB), 64 * MIB);
        assert_eq!(poc_budget(1_024 * MIB), 128 * MIB);
        assert_eq!(poc_budget(4_096 * MIB), 128 * MIB);
    }

    #[test]
    fn rg_t05_no_health_or_stale_health_refuses_closed() {
        let guard = ResourceGuard::new();
        assert_eq!(guard.health_status(0).safety, SafetyBand::RefusedStaleState);
        let guard = ready_guard(10_000);
        assert!(guard.health_status(16_000).fresh);
        assert!(!guard.health_status(16_001).fresh);
        assert_eq!(
            guard.health_status(16_001).safety,
            SafetyBand::RefusedStaleState
        );
    }

    #[test]
    fn rg_t06_memory_threshold_and_ten_second_hysteresis() {
        let mut guard = ready_guard(10_000);
        let mut low = healthy(11_000);
        low.available_memory_bytes = 767 * MIB;
        assert_eq!(
            guard.record_health(low).safety,
            SafetyBand::RefusedMemoryPressure
        );
        let mut recovery = healthy(12_000);
        recovery.available_memory_bytes = 896 * MIB;
        assert_eq!(
            guard.record_health(recovery).safety,
            SafetyBand::RefusedMemoryPressure
        );
        recovery.monotonic_ms = 21_999;
        assert_eq!(
            guard.record_health(recovery).safety,
            SafetyBand::RefusedMemoryPressure
        );
        recovery.monotonic_ms = 22_000;
        assert_ne!(
            guard.record_health(recovery).safety,
            SafetyBand::RefusedMemoryPressure
        );
    }

    #[test]
    fn rg_t07_low_memory_flag_refuses_regardless_of_bytes() {
        let mut guard = ready_guard(10_000);
        let mut sample = healthy(11_000);
        sample.low_memory = true;
        assert_eq!(
            guard.record_health(sample).safety,
            SafetyBand::RefusedMemoryPressure
        );
    }

    #[test]
    fn rg_t08_t09_thermal_bands_and_recovery_are_exact() {
        let mut guard = ready_guard(10_000);
        let mut sample = healthy(11_000);
        sample.thermal = ThermalBand::Moderate;
        assert_eq!(guard.record_health(sample).safety, SafetyBand::Throttle);
        sample.monotonic_ms = 12_000;
        sample.thermal = ThermalBand::Severe;
        assert_eq!(
            guard.record_health(sample).safety,
            SafetyBand::RefusedThermal
        );
        sample.monotonic_ms = 13_000;
        sample.thermal = ThermalBand::Light;
        assert_eq!(
            guard.record_health(sample).safety,
            SafetyBand::RefusedThermal
        );
        sample.monotonic_ms = 23_000;
        assert_eq!(guard.record_health(sample).safety, SafetyBand::Nominal);
    }

    #[test]
    fn rg_t10_t11_battery_bands_and_recovery_are_exact() {
        let mut guard = ready_guard(10_000);
        let mut sample = healthy(11_000);
        sample.battery_percent = 14;
        assert_eq!(
            guard.record_health(sample).safety,
            SafetyBand::RefusedBattery
        );
        sample.monotonic_ms = 12_000;
        sample.battery_percent = 20;
        assert_eq!(
            guard.record_health(sample).safety,
            SafetyBand::RefusedBattery
        );
        sample.monotonic_ms = 42_000;
        assert_eq!(guard.record_health(sample).safety, SafetyBand::Throttle);
        sample.monotonic_ms = 43_000;
        sample.battery_percent = 30;
        assert_eq!(guard.record_health(sample).safety, SafetyBand::Throttle);
        sample.monotonic_ms = 73_000;
        assert_eq!(guard.record_health(sample).safety, SafetyBand::Nominal);
    }

    #[test]
    fn rg_t07_charging_recovers_battery_immediately() {
        let mut guard = ready_guard(10_000);
        let mut sample = healthy(11_000);
        sample.battery_percent = 5;
        guard.record_health(sample);
        sample.monotonic_ms = 12_000;
        sample.charging = true;
        assert_eq!(guard.record_health(sample).battery, BatteryBand::Nominal);
    }

    #[test]
    fn rg_t08_power_save_throttles_without_invented_policy() {
        let mut guard = ready_guard(10_000);
        let mut sample = healthy(11_000);
        sample.power_save = true;
        assert_eq!(guard.record_health(sample).safety, SafetyBand::Throttle);
    }

    #[test]
    fn rg_t09_reserve_requires_live_authority_before_other_checks() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ResourceGuard::new();
        let wrong = LeaseId::test_only(9);
        assert_eq!(
            guard.reserve(&mut leases, &session, wrong, request(1, 1), 10_001),
            ReservationDecision::StaleLease
        );
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(1, 1), 10_001),
            ReservationDecision::RefusedSafety
        );
    }

    #[test]
    fn rg_t01_reserve_holds_budget_and_returns_random_id() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let id = reservation_id(guard.reserve(
            &mut leases,
            &session,
            lease,
            request(1, 64 * MIB),
            10_001,
        ));
        assert!(id.is_nonzero());
        assert_eq!(guard.allocated_bytes, 64 * MIB);
    }

    #[test]
    fn rg_t13_total_never_exceeds_cap_and_zero_is_refused() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(1, 129 * MIB), 10_001),
            ReservationDecision::RefusedBudget
        );
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(2, 0), 10_001),
            ReservationDecision::RefusedBudget
        );
        assert!(matches!(
            guard.reserve(
                &mut leases,
                &session,
                lease,
                request(3, POC_CAP_BYTES),
                10_001,
            ),
            ReservationDecision::Reserved { .. }
        ));
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(4, 1), 10_001),
            ReservationDecision::RefusedBudget
        );
        assert_eq!(guard.allocated_bytes, POC_CAP_BYTES);
    }

    #[test]
    fn rg_t14_t15_same_request_replays_and_changed_parameters_conflict() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let first = guard.reserve(&mut leases, &session, lease, request(1, MIB), 10_001);
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(1, MIB), 10_002),
            first
        );
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(1, 2 * MIB), 10_002),
            ReservationDecision::IdempotenceConflict
        );
    }

    #[test]
    fn rg_t04_uncommitted_reservation_expires_and_releases_hold() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let id =
            reservation_id(guard.reserve(&mut leases, &session, lease, request(1, MIB), 10_001));
        assert_eq!(
            guard.commit(&mut leases, &session, lease, id, 40_001),
            CommitDecision::UnknownOrExpiredReservation
        );
        assert_eq!(guard.allocated_bytes, 0);
    }

    #[test]
    fn rg_t02_commit_is_only_provider_authorizing_state() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let id =
            reservation_id(guard.reserve(&mut leases, &session, lease, request(1, MIB), 10_001));
        assert_eq!(guard.reservations[&id].state, ReservationState::Reserved);
        assert_eq!(
            guard.commit(&mut leases, &session, lease, id, 10_002),
            CommitDecision::Committed
        );
        assert_eq!(guard.reservations[&id].state, ReservationState::Committed);
    }

    #[test]
    fn rg_t17_commit_safety_failure_releases_hold() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let id =
            reservation_id(guard.reserve(&mut leases, &session, lease, request(1, MIB), 10_001));
        let mut unsafe_sample = healthy(10_002);
        unsafe_sample.low_memory = true;
        guard.record_health(unsafe_sample);
        assert_eq!(
            guard.commit(&mut leases, &session, lease, id, 10_003),
            CommitDecision::RefusedSafety
        );
        assert_eq!(guard.allocated_bytes, 0);
    }

    #[test]
    fn rg_t03_release_is_idempotent_and_releases_accounting() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let id =
            reservation_id(guard.reserve(&mut leases, &session, lease, request(1, MIB), 10_001));
        assert_eq!(
            guard.release(&mut leases, &session, lease, id, 10_002),
            ReleaseDecision::Released
        );
        assert_eq!(
            guard.release(&mut leases, &session, lease, id, 10_003),
            ReleaseDecision::AlreadyTerminal
        );
        assert_eq!(guard.allocated_bytes, 0);
    }

    #[test]
    fn rg_t16_idempotence_table_is_bounded_without_live_eviction() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        for id in 0..1_024 {
            assert_eq!(
                guard.reserve(&mut leases, &session, lease, request(id, 129 * MIB), 10_001),
                ReservationDecision::RefusedBudget
            );
        }
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(1_025, MIB), 10_001),
            ReservationDecision::IdempotenceTableFull
        );
    }

    #[test]
    fn rg_t18_thirty_two_concurrent_eight_mib_attempts_never_exceed_cap() {
        struct Harness {
            leases: ControllerLeaseManager,
            session: AuthenticatedSession,
            lease: LeaseId,
            guard: ResourceGuard,
        }
        let (leases, session, lease) = authority(10_000);
        let harness = Arc::new(Mutex::new(Harness {
            leases,
            session,
            lease,
            guard: ready_guard(10_000),
        }));
        let mut threads = Vec::new();
        for id in 0..32 {
            let harness = Arc::clone(&harness);
            threads.push(thread::spawn(move || {
                let mut locked = harness.lock().expect("single writer");
                let lease = locked.lease;
                let Harness {
                    leases,
                    session,
                    guard,
                    ..
                } = &mut *locked;
                guard.reserve(leases, session, lease, request(id, 8 * MIB), 10_001)
            }));
        }
        let decisions: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("retry"))
            .collect();
        let accepted: Vec<_> = decisions
            .iter()
            .filter_map(|decision| match decision {
                ReservationDecision::Reserved { reservation_id } => Some(*reservation_id),
                ReservationDecision::RefusedBudget => None,
                other => panic!("unexpected concurrent decision: {other:?}"),
            })
            .collect();
        assert_eq!(accepted.len(), 16);
        let mut locked = harness.lock().expect("single writer");
        assert_eq!(locked.guard.allocated_bytes, POC_CAP_BYTES);
        assert!(locked.guard.allocated_bytes <= POC_CAP_BYTES);
        for reservation_id in accepted {
            let lease = locked.lease;
            let Harness {
                leases,
                session,
                guard,
                ..
            } = &mut *locked;
            assert_eq!(
                guard.commit(leases, session, lease, reservation_id, 10_002),
                CommitDecision::Committed
            );
        }
        assert_eq!(locked.guard.allocated_bytes, POC_CAP_BYTES);
    }

    #[test]
    fn auth_t01_to_t04_no_auth_no_lease_wrong_peer_and_expiry_fail_closed() {
        let mut leases = ControllerLeaseManager::new(incarnation());
        let unauth = AuthenticatedSession::test_unauthenticated(7);
        assert_eq!(
            leases.acquire(&unauth, 0),
            Err(crate::LeaseError::Unauthenticated)
        );
        let session = AuthenticatedSession::test_only(7);
        let lease = leases.acquire(&session, 0).expect("lease");
        let other = AuthenticatedSession::test_only(8);
        let mut guard = ready_guard(0);
        assert_eq!(
            guard.reserve(&mut leases, &other, lease, request(1, MIB), 1),
            ReservationDecision::StaleLease
        );
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, request(2, MIB), 60_000),
            ReservationDecision::StaleLease
        );
    }

    #[test]
    fn unknown_resource_class_is_rejected_after_authority_validation() {
        let (mut leases, session, lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let mut unknown = request(1, MIB);
        unknown.class = ResourceClass::Unknown;
        assert_eq!(
            guard.reserve(&mut leases, &session, lease, unknown, 10_001),
            ReservationDecision::UnknownResourceClass
        );
    }

    #[test]
    fn lease_end_releases_old_committed_accounting_before_new_lease_admission() {
        let (mut leases, session, first_lease) = authority(10_000);
        let mut guard = ready_guard(10_000);
        let first = reservation_id(guard.reserve(
            &mut leases,
            &session,
            first_lease,
            request(1, POC_CAP_BYTES),
            10_001,
        ));
        assert_eq!(
            guard.commit(&mut leases, &session, first_lease, first, 10_002),
            CommitDecision::Committed
        );
        leases
            .release(&session, first_lease, 10_003)
            .expect("release first lease");
        let second_lease = leases.acquire(&session, 10_004).expect("second lease");
        assert!(matches!(
            guard.reserve(
                &mut leases,
                &session,
                second_lease,
                request(2, POC_CAP_BYTES),
                10_005,
            ),
            ReservationDecision::Reserved { .. }
        ));
        assert_eq!(guard.allocated_bytes, POC_CAP_BYTES);
    }

    #[test]
    fn phy_t01_to_t05_sampler_contract_constants_and_local_fields() {
        assert_eq!(HEALTH_INTERVAL_MS, 2_000);
        assert_eq!(HEALTH_STALE_AFTER_MS, 6_000);
        assert_eq!(NATIVE_OPERATION_THREADS, 1);
        let guard = ready_guard(10_000);
        let status = guard.health_status(10_000);
        assert!(status.fresh);
        assert_eq!(status.available_memory_bytes, 1_024 * MIB);
    }
}
