use std::time::Duration;

use pb_types::PeerId;

pub const PERFORMANCE_PROFILE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const SPEED_MINIMUM_GAIN_BASIS_POINTS: i32 = 500;
const LOCALITY_MAXIMUM_LOSS_BASIS_POINTS: i32 = -500;
const MINIMUM_MEASURED_RUNS: u16 = 15;
const MINIMUM_SUSTAINED_CHECK: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlacementMode {
    LocalOnly,
    Capacity,
    Speed,
    Balanced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlacementTarget {
    LocalOnly,
    RemoteCapacity,
    RemoteSpeed,
    RemoteLocality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlacementReason {
    LocalOnlyRequested,
    RemoteNotReady,
    RemoteNotAuthenticated,
    LeaseInactive,
    MetricsStale,
    ProviderUnavailable,
    ResourceEstimateZero,
    ResourceGuardRefused,
    ThermalDisallowed,
    BatteryPolicyDisallowed,
    WorkloadNotPhoneBoostAware,
    RemoteCapacityInsufficient,
    ProfileUnknown,
    ProfileInvalid,
    ProfileNotSpeedQualified,
    RemoteCapacityRequired,
    SpeedEvidenceQualified,
    RemoteDataLocality,
    LocalPreferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlacementDecision {
    target: PlacementTarget,
    reason: PlacementReason,
}

impl PlacementDecision {
    pub const fn target(self) -> PlacementTarget {
        self.target
    }

    pub const fn reason(self) -> PlacementReason {
        self.reason
    }

    const fn local(reason: PlacementReason) -> Self {
        Self {
            target: PlacementTarget::LocalOnly,
            reason,
        }
    }

    const fn remote(target: PlacementTarget, reason: PlacementReason) -> Self {
        Self { target, reason }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ThermalClass {
    Nominal,
    Light,
    Moderate,
    Severe,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BatteryPolicy {
    ExternalPower,
    BatteryAllowed,
    PowerSave,
    RemoteDisallowed,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PerformanceProfileKey {
    pub peer_id: PeerId,
    pub worker_build_id: String,
    pub provider_id: String,
    pub operation_id: String,
    pub core_protocol_version: u16,
    pub provider_protocol_version: u16,
    pub transport_signature: String,
    pub workload_signature: String,
    pub thermal_class: ThermalClass,
    pub battery_policy: BatteryPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PerformanceEvidence {
    pub local_warmup_complete: bool,
    pub remote_warmup_complete: bool,
    pub measured_local_runs: u16,
    pub measured_remote_runs: u16,
    /// Relative remote median improvement in basis points. Negative means slower.
    pub remote_median_gain_basis_points: i32,
    /// Lower bound of the bootstrap 95% confidence interval in basis points.
    pub bootstrap_95_lower_bound_basis_points: i32,
    pub sustained_check_duration: Duration,
    pub thermal_severe_observed: bool,
}

impl PerformanceEvidence {
    pub const fn has_minimum_measurements(self) -> bool {
        self.local_warmup_complete
            && self.remote_warmup_complete
            && self.measured_local_runs >= MINIMUM_MEASURED_RUNS
            && self.measured_remote_runs >= MINIMUM_MEASURED_RUNS
    }

    pub const fn is_speed_qualified(self) -> bool {
        self.has_minimum_measurements()
            && self.remote_median_gain_basis_points >= SPEED_MINIMUM_GAIN_BASIS_POINTS
            && self.bootstrap_95_lower_bound_basis_points > 0
            && self.sustained_check_duration.as_secs() >= MINIMUM_SUSTAINED_CHECK.as_secs()
            && !self.thermal_severe_observed
    }

    pub const fn is_locality_acceptable(self) -> bool {
        self.has_minimum_measurements()
            && self.remote_median_gain_basis_points >= LOCALITY_MAXIMUM_LOSS_BASIS_POINTS
            && !self.thermal_severe_observed
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerformanceProfile {
    pub schema_version: u16,
    pub key: PerformanceProfileKey,
    pub created_wall_ms: u64,
    pub evidence: PerformanceEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileInvalidationReason {
    SchemaUnsupported,
    KeyChanged,
    ClockMovedBackwards,
    TtlExpired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileStatus {
    Missing,
    Valid,
    Invalid(ProfileInvalidationReason),
}

impl PerformanceProfile {
    pub fn status(&self, current_key: &PerformanceProfileKey, now_wall_ms: u64) -> ProfileStatus {
        if self.schema_version != 1 {
            return ProfileStatus::Invalid(ProfileInvalidationReason::SchemaUnsupported);
        }
        if &self.key != current_key {
            return ProfileStatus::Invalid(ProfileInvalidationReason::KeyChanged);
        }
        let Some(age_ms) = now_wall_ms.checked_sub(self.created_wall_ms) else {
            return ProfileStatus::Invalid(ProfileInvalidationReason::ClockMovedBackwards);
        };
        if age_ms >= PERFORMANCE_PROFILE_TTL.as_millis() as u64 {
            return ProfileStatus::Invalid(ProfileInvalidationReason::TtlExpired);
        }
        ProfileStatus::Valid
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CostInput {
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub resident_remote_bytes: u64,
    pub expected_compute_units: u64,
    pub link_bandwidth_bytes_per_second: u64,
    pub link_rtt_millis: u64,
    pub local_score: u64,
    pub remote_score: u64,
    pub thermal_class: ThermalClass,
    pub battery_policy: BatteryPolicy,
    pub queue_depth: u32,
}

impl CostInput {
    /// C11 data locality: bytes already resident in a READY RemoteBuffer cost no upload.
    pub const fn estimated_upload_bytes(self) -> u64 {
        self.bytes_up.saturating_sub(self.resident_remote_bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteGates {
    pub authenticated: bool,
    pub ready: bool,
    pub active_lease: bool,
    pub fresh_metrics: bool,
    pub provider_available: bool,
    pub resource_estimate_nonzero: bool,
    pub resource_guard_admitted: bool,
}

impl RemoteGates {
    const fn refusal_reason(self) -> Option<PlacementReason> {
        if !self.authenticated {
            Some(PlacementReason::RemoteNotAuthenticated)
        } else if !self.ready {
            Some(PlacementReason::RemoteNotReady)
        } else if !self.active_lease {
            Some(PlacementReason::LeaseInactive)
        } else if !self.fresh_metrics {
            Some(PlacementReason::MetricsStale)
        } else if !self.provider_available {
            Some(PlacementReason::ProviderUnavailable)
        } else if !self.resource_estimate_nonzero {
            Some(PlacementReason::ResourceEstimateZero)
        } else if !self.resource_guard_admitted {
            Some(PlacementReason::ResourceGuardRefused)
        } else {
            None
        }
    }
}

pub struct PlannerInput<'a> {
    pub mode: PlacementMode,
    pub gates: RemoteGates,
    pub cost: CostInput,
    pub workload_phoneboost_aware: bool,
    pub local_execution_fits: bool,
    pub remote_capacity_fits: bool,
    pub current_profile_key: Option<&'a PerformanceProfileKey>,
    pub profile: Option<&'a PerformanceProfile>,
    pub now_wall_ms: u64,
}

impl PlannerInput<'_> {
    pub fn decide(&self) -> PlacementDecision {
        if self.mode == PlacementMode::LocalOnly {
            return PlacementDecision::local(PlacementReason::LocalOnlyRequested);
        }
        if let Some(reason) = self.gates.refusal_reason() {
            return PlacementDecision::local(reason);
        }
        if self.cost.thermal_class == ThermalClass::Severe {
            return PlacementDecision::local(PlacementReason::ThermalDisallowed);
        }
        if self.cost.battery_policy == BatteryPolicy::RemoteDisallowed {
            return PlacementDecision::local(PlacementReason::BatteryPolicyDisallowed);
        }
        if !self.workload_phoneboost_aware {
            return PlacementDecision::local(PlacementReason::WorkloadNotPhoneBoostAware);
        }
        if !self.remote_capacity_fits {
            return PlacementDecision::local(PlacementReason::RemoteCapacityInsufficient);
        }

        match self.mode {
            PlacementMode::LocalOnly => unreachable!("handled before remote gates"),
            PlacementMode::Capacity => PlacementDecision::remote(
                PlacementTarget::RemoteCapacity,
                PlacementReason::RemoteCapacityRequired,
            ),
            PlacementMode::Speed => self.speed_decision(),
            PlacementMode::Balanced => self.balanced_decision(),
        }
    }

    pub fn profile_status(&self) -> ProfileStatus {
        match (self.current_profile_key, self.profile) {
            (Some(key), Some(profile)) => profile.status(key, self.now_wall_ms),
            _ => ProfileStatus::Missing,
        }
    }

    fn speed_decision(&self) -> PlacementDecision {
        match (self.profile_status(), self.profile) {
            (ProfileStatus::Missing, _) => {
                PlacementDecision::local(PlacementReason::ProfileUnknown)
            }
            (ProfileStatus::Invalid(_), _) => {
                PlacementDecision::local(PlacementReason::ProfileInvalid)
            }
            (ProfileStatus::Valid, Some(profile)) if profile.evidence.is_speed_qualified() => {
                PlacementDecision::remote(
                    PlacementTarget::RemoteSpeed,
                    PlacementReason::SpeedEvidenceQualified,
                )
            }
            (ProfileStatus::Valid, Some(_)) => {
                PlacementDecision::local(PlacementReason::ProfileNotSpeedQualified)
            }
            (ProfileStatus::Valid, None) => {
                PlacementDecision::local(PlacementReason::ProfileUnknown)
            }
        }
    }

    fn balanced_decision(&self) -> PlacementDecision {
        if !self.local_execution_fits {
            return PlacementDecision::remote(
                PlacementTarget::RemoteCapacity,
                PlacementReason::RemoteCapacityRequired,
            );
        }

        if let Some(profile) = self.profile
            && self.profile_status() == ProfileStatus::Valid
        {
            if profile.evidence.is_speed_qualified() {
                return PlacementDecision::remote(
                    PlacementTarget::RemoteSpeed,
                    PlacementReason::SpeedEvidenceQualified,
                );
            }
            if self.cost.resident_remote_bytes > 0 && profile.evidence.is_locality_acceptable() {
                return PlacementDecision::remote(
                    PlacementTarget::RemoteLocality,
                    PlacementReason::RemoteDataLocality,
                );
            }
        }

        PlacementDecision::local(PlacementReason::LocalPreferred)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000_000;

    fn key() -> PerformanceProfileKey {
        PerformanceProfileKey {
            peer_id: PeerId::from([7; 32]),
            worker_build_id: "worker-a".to_owned(),
            provider_id: "native".to_owned(),
            operation_id: "pb.native.blake3/1".to_owned(),
            core_protocol_version: 1,
            provider_protocol_version: 1,
            transport_signature: "pb-link-v1:wifi_lan".to_owned(),
            workload_signature: "blake3/64mib".to_owned(),
            thermal_class: ThermalClass::Nominal,
            battery_policy: BatteryPolicy::ExternalPower,
        }
    }

    fn evidence(gain: i32) -> PerformanceEvidence {
        PerformanceEvidence {
            local_warmup_complete: true,
            remote_warmup_complete: true,
            measured_local_runs: 15,
            measured_remote_runs: 15,
            remote_median_gain_basis_points: gain,
            bootstrap_95_lower_bound_basis_points: 1,
            sustained_check_duration: MINIMUM_SUSTAINED_CHECK,
            thermal_severe_observed: false,
        }
    }

    fn profile(gain: i32) -> PerformanceProfile {
        PerformanceProfile {
            schema_version: 1,
            key: key(),
            created_wall_ms: NOW - 1,
            evidence: evidence(gain),
        }
    }

    fn input<'a>(
        mode: PlacementMode,
        current_key: &'a PerformanceProfileKey,
        profile: Option<&'a PerformanceProfile>,
    ) -> PlannerInput<'a> {
        PlannerInput {
            mode,
            gates: RemoteGates {
                authenticated: true,
                ready: true,
                active_lease: true,
                fresh_metrics: true,
                provider_available: true,
                resource_estimate_nonzero: true,
                resource_guard_admitted: true,
            },
            cost: CostInput {
                bytes_up: 1_000,
                bytes_down: 32,
                resident_remote_bytes: 0,
                expected_compute_units: 1,
                link_bandwidth_bytes_per_second: 1_000_000,
                link_rtt_millis: 10,
                local_score: 100,
                remote_score: 110,
                thermal_class: ThermalClass::Nominal,
                battery_policy: BatteryPolicy::ExternalPower,
                queue_depth: 0,
            },
            workload_phoneboost_aware: true,
            local_execution_fits: true,
            remote_capacity_fits: true,
            current_profile_key: Some(current_key),
            profile,
            now_wall_ms: NOW,
        }
    }

    #[test]
    fn local_only_never_dispatches_remote() {
        let key = key();
        let profile = profile(1_000);
        let decision = input(PlacementMode::LocalOnly, &key, Some(&profile)).decide();
        assert_eq!(decision.target(), PlacementTarget::LocalOnly);
        assert_eq!(decision.reason(), PlacementReason::LocalOnlyRequested);
    }

    #[test]
    fn every_remote_gate_fails_closed() {
        let key = key();
        let cases = [
            (0, PlacementReason::RemoteNotAuthenticated),
            (1, PlacementReason::RemoteNotReady),
            (2, PlacementReason::LeaseInactive),
            (3, PlacementReason::MetricsStale),
            (4, PlacementReason::ProviderUnavailable),
            (5, PlacementReason::ResourceEstimateZero),
            (6, PlacementReason::ResourceGuardRefused),
        ];
        for (gate, reason) in cases {
            let mut input = input(PlacementMode::Capacity, &key, None);
            match gate {
                0 => input.gates.authenticated = false,
                1 => input.gates.ready = false,
                2 => input.gates.active_lease = false,
                3 => input.gates.fresh_metrics = false,
                4 => input.gates.provider_available = false,
                5 => input.gates.resource_estimate_nonzero = false,
                6 => input.gates.resource_guard_admitted = false,
                _ => unreachable!(),
            }
            assert_eq!(input.decide(), PlacementDecision::local(reason));
        }
    }

    #[test]
    fn current_thermal_and_battery_safety_override_remote_placement() {
        let key = key();
        let mut candidate = input(PlacementMode::Capacity, &key, None);
        candidate.cost.thermal_class = ThermalClass::Severe;
        assert_eq!(
            candidate.decide().reason(),
            PlacementReason::ThermalDisallowed
        );

        candidate.cost.thermal_class = ThermalClass::Nominal;
        candidate.cost.battery_policy = BatteryPolicy::RemoteDisallowed;
        assert_eq!(
            candidate.decide().reason(),
            PlacementReason::BatteryPolicyDisallowed
        );
    }

    #[test]
    fn capacity_requires_explicit_phoneboost_workload_and_budget() {
        let key = key();
        let mut candidate = input(PlacementMode::Capacity, &key, None);
        candidate.workload_phoneboost_aware = false;
        assert_eq!(
            candidate.decide().reason(),
            PlacementReason::WorkloadNotPhoneBoostAware
        );

        let mut candidate = input(PlacementMode::Capacity, &key, None);
        candidate.remote_capacity_fits = false;
        assert_eq!(
            candidate.decide().reason(),
            PlacementReason::RemoteCapacityInsufficient
        );
    }

    #[test]
    fn capacity_does_not_require_a_speed_profile() {
        let key = key();
        assert_eq!(
            input(PlacementMode::Capacity, &key, None).decide(),
            PlacementDecision::remote(
                PlacementTarget::RemoteCapacity,
                PlacementReason::RemoteCapacityRequired
            )
        );
    }

    #[test]
    fn speed_without_profile_is_local_profile_unknown() {
        let key = key();
        assert_eq!(
            input(PlacementMode::Speed, &key, None).decide().reason(),
            PlacementReason::ProfileUnknown
        );
    }

    #[test]
    fn speed_requires_five_percent_gain_and_positive_confidence_bound() {
        let key = key();
        let below = profile(499);
        assert_eq!(
            input(PlacementMode::Speed, &key, Some(&below))
                .decide()
                .target(),
            PlacementTarget::LocalOnly
        );

        let mut ambiguous = profile(500);
        ambiguous.evidence.bootstrap_95_lower_bound_basis_points = 0;
        assert_eq!(
            input(PlacementMode::Speed, &key, Some(&ambiguous))
                .decide()
                .target(),
            PlacementTarget::LocalOnly
        );

        let qualified = profile(500);
        assert_eq!(
            input(PlacementMode::Speed, &key, Some(&qualified))
                .decide()
                .target(),
            PlacementTarget::RemoteSpeed
        );
    }

    #[test]
    fn severe_thermal_or_short_sustain_disqualifies_speed() {
        let key = key();
        let mut severe = profile(1_000);
        severe.evidence.thermal_severe_observed = true;
        assert!(!severe.evidence.is_speed_qualified());

        let mut short = profile(1_000);
        short.evidence.sustained_check_duration = Duration::from_secs(299);
        assert!(!short.evidence.is_speed_qualified());
        assert_eq!(
            input(PlacementMode::Speed, &key, Some(&short))
                .decide()
                .target(),
            PlacementTarget::LocalOnly
        );
    }

    #[test]
    fn balanced_prefers_capacity_when_local_does_not_fit() {
        let key = key();
        let mut candidate = input(PlacementMode::Balanced, &key, None);
        candidate.local_execution_fits = false;
        assert_eq!(candidate.decide().target(), PlacementTarget::RemoteCapacity);
    }

    #[test]
    fn balanced_prefers_qualified_speed_before_locality() {
        let key = key();
        let profile = profile(700);
        let mut candidate = input(PlacementMode::Balanced, &key, Some(&profile));
        candidate.cost.resident_remote_bytes = 1_000;
        assert_eq!(candidate.decide().target(), PlacementTarget::RemoteSpeed);
    }

    #[test]
    fn balanced_uses_locality_when_remote_is_not_more_than_five_percent_slower() {
        let key = key();
        let locality_profile = profile(-500);
        let mut candidate = input(PlacementMode::Balanced, &key, Some(&locality_profile));
        candidate.cost.resident_remote_bytes = 1_000;
        assert_eq!(candidate.decide().target(), PlacementTarget::RemoteLocality);

        let too_slow = profile(-501);
        candidate.profile = Some(&too_slow);
        assert_eq!(candidate.decide().target(), PlacementTarget::LocalOnly);
    }

    #[test]
    fn resident_bytes_reduce_upload_without_underflow() {
        let key = key();
        let mut candidate = input(PlacementMode::Balanced, &key, None);
        candidate.cost.bytes_up = 4_096;
        candidate.cost.resident_remote_bytes = 1_024;
        assert_eq!(candidate.cost.estimated_upload_bytes(), 3_072);
        candidate.cost.resident_remote_bytes = 8_192;
        assert_eq!(candidate.cost.estimated_upload_bytes(), 0);
    }

    #[test]
    fn profile_invalidates_on_every_key_change() {
        let profile = profile(1_000);
        let mut changed = key();
        changed.worker_build_id = "worker-b".to_owned();
        assert_eq!(
            profile.status(&changed, NOW),
            ProfileStatus::Invalid(ProfileInvalidationReason::KeyChanged)
        );
    }

    #[test]
    fn profile_expires_at_24_hours_and_rejects_clock_rollback() {
        let key = key();
        let profile = PerformanceProfile {
            created_wall_ms: NOW,
            ..profile(1_000)
        };
        assert_eq!(
            profile.status(&key, NOW + PERFORMANCE_PROFILE_TTL.as_millis() as u64 - 1),
            ProfileStatus::Valid
        );
        assert_eq!(
            profile.status(&key, NOW + PERFORMANCE_PROFILE_TTL.as_millis() as u64),
            ProfileStatus::Invalid(ProfileInvalidationReason::TtlExpired)
        );
        assert_eq!(
            profile.status(&key, NOW - 1),
            ProfileStatus::Invalid(ProfileInvalidationReason::ClockMovedBackwards)
        );
    }
}
