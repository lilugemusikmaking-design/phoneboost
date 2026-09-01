use std::fmt;

use serde_json::{Map, Value};

/// Canonical LC-007 method classification. It carries no dispatch authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMethodClass {
    ReadOnly,
    Mutating,
}

/// Closed, case-sensitive canonical local-method allowlist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMethod {
    SystemStatus,
    DevicesList,
    DevicesGet,
    JobsGet,
    DiagnosticsRedacted,
    EventsTail,
    PairingBegin,
    PairingConfirm,
    PairingCancel,
    PeerUnpair,
    ControllerAcquire,
    ControllerRelease,
    ComputeSubmit,
    ComputeCancel,
    BenchmarkRun,
}

impl LocalMethod {
    pub const ALL: [Self; 15] = [
        Self::SystemStatus,
        Self::DevicesList,
        Self::DevicesGet,
        Self::JobsGet,
        Self::DiagnosticsRedacted,
        Self::EventsTail,
        Self::PairingBegin,
        Self::PairingConfirm,
        Self::PairingCancel,
        Self::PeerUnpair,
        Self::ControllerAcquire,
        Self::ControllerRelease,
        Self::ComputeSubmit,
        Self::ComputeCancel,
        Self::BenchmarkRun,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemStatus => "system.status",
            Self::DevicesList => "devices.list",
            Self::DevicesGet => "devices.get",
            Self::JobsGet => "jobs.get",
            Self::DiagnosticsRedacted => "diagnostics.redacted",
            Self::EventsTail => "events.tail",
            Self::PairingBegin => "pairing.begin",
            Self::PairingConfirm => "pairing.confirm",
            Self::PairingCancel => "pairing.cancel",
            Self::PeerUnpair => "peer.unpair",
            Self::ControllerAcquire => "controller.acquire",
            Self::ControllerRelease => "controller.release",
            Self::ComputeSubmit => "compute.submit",
            Self::ComputeCancel => "compute.cancel",
            Self::BenchmarkRun => "benchmark.run",
        }
    }

    pub const fn class(self) -> LocalMethodClass {
        match self {
            Self::SystemStatus
            | Self::DevicesList
            | Self::DevicesGet
            | Self::JobsGet
            | Self::DiagnosticsRedacted
            | Self::EventsTail => LocalMethodClass::ReadOnly,
            Self::PairingBegin
            | Self::PairingConfirm
            | Self::PairingCancel
            | Self::PeerUnpair
            | Self::ControllerAcquire
            | Self::ControllerRelease
            | Self::ComputeSubmit
            | Self::ComputeCancel
            | Self::BenchmarkRun => LocalMethodClass::Mutating,
        }
    }

    fn parse_exact(method: &str) -> Option<Self> {
        match method {
            "system.status" => Some(Self::SystemStatus),
            "devices.list" => Some(Self::DevicesList),
            "devices.get" => Some(Self::DevicesGet),
            "jobs.get" => Some(Self::JobsGet),
            "diagnostics.redacted" => Some(Self::DiagnosticsRedacted),
            "events.tail" => Some(Self::EventsTail),
            "pairing.begin" => Some(Self::PairingBegin),
            "pairing.confirm" => Some(Self::PairingConfirm),
            "pairing.cancel" => Some(Self::PairingCancel),
            "peer.unpair" => Some(Self::PeerUnpair),
            "controller.acquire" => Some(Self::ControllerAcquire),
            "controller.release" => Some(Self::ControllerRelease),
            "compute.submit" => Some(Self::ComputeSubmit),
            "compute.cancel" => Some(Self::ComputeCancel),
            "benchmark.run" => Some(Self::BenchmarkRun),
            _ => None,
        }
    }
}

/// The canonical public LC-007 failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalValidationErrorKind {
    LocalBadRequest,
    LocalUnsupportedMethod,
}

/// Internal typed cause. Only a recoverable opaque correlation id may be retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalValidationCause {
    InvalidUtf8,
    InvalidJson,
    RootNotObject,
    MissingApi,
    InvalidApi,
    MissingId,
    MissingMethod,
    InvalidMethodType,
    MissingParams,
    UnknownMethod,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalValidationScope {
    Request,
}

#[derive(Clone, Eq, PartialEq)]
pub struct LocalValidationError {
    cause: LocalValidationCause,
    correlation_id: Option<Value>,
}

impl fmt::Debug for LocalValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalValidationError")
            .field("cause", &self.cause)
            .field("has_correlation_id", &self.correlation_id.is_some())
            .finish()
    }
}

impl LocalValidationError {
    pub const fn kind(&self) -> LocalValidationErrorKind {
        match self.cause {
            LocalValidationCause::UnknownMethod => LocalValidationErrorKind::LocalUnsupportedMethod,
            LocalValidationCause::InvalidUtf8
            | LocalValidationCause::InvalidJson
            | LocalValidationCause::RootNotObject
            | LocalValidationCause::MissingApi
            | LocalValidationCause::InvalidApi
            | LocalValidationCause::MissingId
            | LocalValidationCause::MissingMethod
            | LocalValidationCause::InvalidMethodType
            | LocalValidationCause::MissingParams => LocalValidationErrorKind::LocalBadRequest,
        }
    }

    pub const fn cause(&self) -> LocalValidationCause {
        self.cause
    }

    pub const fn scope(&self) -> LocalValidationScope {
        LocalValidationScope::Request
    }

    pub const fn state_changed(&self) -> bool {
        false
    }

    pub fn correlation_id(&self) -> Option<&Value> {
        self.correlation_id.as_ref()
    }
}

impl fmt::Display for LocalValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            LocalValidationErrorKind::LocalBadRequest => formatter.write_str("LOCAL_BAD_REQUEST"),
            LocalValidationErrorKind::LocalUnsupportedMethod => {
                formatter.write_str("LOCAL_UNSUPPORTED_METHOD")
            }
        }
    }
}

impl std::error::Error for LocalValidationError {}

/// A fully LC-007-validated request. `id` and `params` remain uninterpreted.
pub struct ValidatedLocalRequest {
    api: u64,
    method: LocalMethod,
    id: Value,
    params: Value,
    object: Map<String, Value>,
    source_len: usize,
}

impl fmt::Debug for ValidatedLocalRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedLocalRequest")
            .field("api", &self.api)
            .field("method", &self.method)
            .field("has_id", &true)
            .field("has_params", &true)
            .field("source_len", &self.source_len)
            .finish()
    }
}

impl ValidatedLocalRequest {
    pub const fn api(&self) -> u64 {
        self.api
    }

    pub const fn method(&self) -> LocalMethod {
        self.method
    }

    pub const fn source_len(&self) -> usize {
        self.source_len
    }

    /// Preserve the parsed object for a later canonical pass without interpreting it here.
    pub fn object(&self) -> &Map<String, Value> {
        &self.object
    }

    pub fn id(&self) -> &Value {
        &self.id
    }

    pub fn params(&self) -> &Value {
        &self.params
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValidationStage {
    Utf8Accepted,
    JsonStarted,
    JsonAccepted,
    ObjectAccepted,
    ApiAccepted,
    MethodAccepted,
    RequestBuilt,
}

pub(crate) fn validate_local_request(
    line: Vec<u8>,
) -> Result<ValidatedLocalRequest, LocalValidationError> {
    validate_with_observer(line, |_| {})
}

fn validate_with_observer(
    line: Vec<u8>,
    mut observe: impl FnMut(ValidationStage),
) -> Result<ValidatedLocalRequest, LocalValidationError> {
    let source_len = line.len();
    let utf8 = String::from_utf8(line).map_err(|_| invalid(LocalValidationCause::InvalidUtf8))?;
    observe(ValidationStage::Utf8Accepted);

    observe(ValidationStage::JsonStarted);
    let value: Value =
        serde_json::from_str(&utf8).map_err(|_| invalid(LocalValidationCause::InvalidJson))?;
    observe(ValidationStage::JsonAccepted);

    let object = match value {
        Value::Object(object) => object,
        _ => return Err(invalid(LocalValidationCause::RootNotObject)),
    };
    observe(ValidationStage::ObjectAccepted);

    let recoverable_id = object.get("id").cloned();
    let api = match object.get("api") {
        None => {
            return Err(invalid_with_id(
                LocalValidationCause::MissingApi,
                recoverable_id,
            ));
        }
        Some(Value::Number(number)) if number.is_u64() && number.as_u64() == Some(1) => 1,
        Some(_) => {
            return Err(invalid_with_id(
                LocalValidationCause::InvalidApi,
                recoverable_id,
            ));
        }
    };
    observe(ValidationStage::ApiAccepted);

    let mut object = object;
    let id = match object.remove("id") {
        Some(id) => id,
        None => return Err(invalid(LocalValidationCause::MissingId)),
    };
    let params = match object.remove("params") {
        Some(params) => params,
        None => {
            return Err(invalid_with_id(
                LocalValidationCause::MissingParams,
                Some(id),
            ));
        }
    };

    let method = match object.get("method") {
        None => {
            return Err(invalid_with_id(
                LocalValidationCause::MissingMethod,
                Some(id),
            ));
        }
        Some(Value::String(method)) => match LocalMethod::parse_exact(method) {
            Some(method) => method,
            None => {
                return Err(invalid_with_id(
                    LocalValidationCause::UnknownMethod,
                    Some(id),
                ));
            }
        },
        Some(_) => {
            return Err(invalid_with_id(
                LocalValidationCause::InvalidMethodType,
                Some(id),
            ));
        }
    };
    observe(ValidationStage::MethodAccepted);

    let request = ValidatedLocalRequest {
        api,
        method,
        id,
        params,
        object,
        source_len,
    };
    observe(ValidationStage::RequestBuilt);
    Ok(request)
}

const fn invalid(cause: LocalValidationCause) -> LocalValidationError {
    LocalValidationError {
        cause,
        correlation_id: None,
    }
}

const fn invalid_with_id(
    cause: LocalValidationCause,
    correlation_id: Option<Value>,
) -> LocalValidationError {
    LocalValidationError {
        cause,
        correlation_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const READ_METHODS: [&str; 6] = [
        "system.status",
        "devices.list",
        "devices.get",
        "jobs.get",
        "diagnostics.redacted",
        "events.tail",
    ];
    const MUTATING_METHODS: [&str; 9] = [
        "pairing.begin",
        "pairing.confirm",
        "pairing.cancel",
        "peer.unpair",
        "controller.acquire",
        "controller.release",
        "compute.submit",
        "compute.cancel",
        "benchmark.run",
    ];

    fn request(method: &str) -> Vec<u8> {
        format!(r#"{{"api":1,"id":"test","method":"{method}","params":{{}}}}"#).into_bytes()
    }

    fn validate(bytes: impl Into<Vec<u8>>) -> Result<ValidatedLocalRequest, LocalValidationError> {
        validate_local_request(bytes.into())
    }

    fn assert_bad_request(bytes: impl Into<Vec<u8>>, expected_cause: LocalValidationCause) {
        let error = validate(bytes).expect_err("LC-007 must reject request");
        assert_eq!(error.kind(), LocalValidationErrorKind::LocalBadRequest);
        assert_eq!(error.cause(), expected_cause);
        assert_eq!(error.scope(), LocalValidationScope::Request);
        assert!(!error.state_changed());
        assert_eq!(error.to_string(), "LOCAL_BAD_REQUEST");
    }

    fn assert_unsupported_method(bytes: impl Into<Vec<u8>>) {
        let error = validate(bytes).expect_err("unknown method must be rejected");
        assert_eq!(
            error.kind(),
            LocalValidationErrorKind::LocalUnsupportedMethod
        );
        assert_ne!(error.kind(), LocalValidationErrorKind::LocalBadRequest);
        assert_eq!(error.cause(), LocalValidationCause::UnknownMethod);
        assert_eq!(error.scope(), LocalValidationScope::Request);
        assert!(!error.state_changed());
        assert_eq!(error.to_string(), "LOCAL_UNSUPPORTED_METHOD");
    }

    #[test]
    fn val_t01_valid_minimal_request_builds_typed_request() {
        let bytes = request("system.status");
        let expected_len = bytes.len();
        let validated = validate(bytes).expect("minimal request validates");
        assert_eq!(validated.api(), 1);
        assert_eq!(validated.method(), LocalMethod::SystemStatus);
        assert_eq!(validated.source_len(), expected_len);
        assert_eq!(validated.object().len(), 2);
        assert_eq!(validated.id(), "test");
        assert_eq!(validated.params(), &serde_json::json!({}));
    }

    #[test]
    fn val_t02_invalid_utf8_is_request_scoped_local_bad_request() {
        assert_bad_request(vec![0xff, 0xfe], LocalValidationCause::InvalidUtf8);
    }

    #[test]
    fn val_t03_invalid_json_is_rejected() {
        assert_bad_request(b"not-json".to_vec(), LocalValidationCause::InvalidJson);
    }

    #[test]
    fn val_t04_null_root_is_rejected() {
        assert_bad_request(b"null".to_vec(), LocalValidationCause::RootNotObject);
    }

    #[test]
    fn val_t05_boolean_root_is_rejected() {
        assert_bad_request(b"true".to_vec(), LocalValidationCause::RootNotObject);
    }

    #[test]
    fn val_t06_number_root_is_rejected() {
        assert_bad_request(b"1".to_vec(), LocalValidationCause::RootNotObject);
    }

    #[test]
    fn val_t07_string_root_is_rejected() {
        assert_bad_request(
            br#""request""#.to_vec(),
            LocalValidationCause::RootNotObject,
        );
    }

    #[test]
    fn val_t08_array_root_is_rejected() {
        assert_bad_request(b"[]".to_vec(), LocalValidationCause::RootNotObject);
    }

    #[test]
    fn val_t09_missing_api_is_rejected() {
        assert_bad_request(
            br#"{"method":"system.status"}"#.to_vec(),
            LocalValidationCause::MissingApi,
        );
    }

    #[test]
    fn val_t10_null_api_is_rejected() {
        assert_bad_request(
            br#"{"api":null,"method":"system.status"}"#.to_vec(),
            LocalValidationCause::InvalidApi,
        );
    }

    #[test]
    fn val_t11_boolean_api_is_rejected() {
        assert_bad_request(
            br#"{"api":true,"method":"system.status"}"#.to_vec(),
            LocalValidationCause::InvalidApi,
        );
    }

    #[test]
    fn val_t12_string_api_is_rejected() {
        assert_bad_request(
            br#"{"api":"1","method":"system.status"}"#.to_vec(),
            LocalValidationCause::InvalidApi,
        );
    }

    #[test]
    fn val_t13_float_api_one_is_rejected_without_coercion() {
        assert_bad_request(
            br#"{"api":1.0,"method":"system.status"}"#.to_vec(),
            LocalValidationCause::InvalidApi,
        );
    }

    #[test]
    fn val_t14_other_api_values_and_container_types_are_rejected() {
        for api in ["0", "2", "-1", "[]", "{}"] {
            let bytes = format!(r#"{{"api":{api},"method":"system.status"}}"#);
            assert_bad_request(bytes.into_bytes(), LocalValidationCause::InvalidApi);
        }
    }

    #[test]
    fn val_t15_integer_one_passes_and_leading_zero_json_fails() {
        assert!(validate(request("system.status")).is_ok());
        assert_bad_request(
            br#"{"api":01,"method":"system.status"}"#.to_vec(),
            LocalValidationCause::InvalidJson,
        );
    }

    #[test]
    fn val_t16_missing_method_is_rejected() {
        assert_bad_request(
            br#"{"api":1,"id":"test","params":{}}"#.to_vec(),
            LocalValidationCause::MissingMethod,
        );
    }

    #[test]
    fn val_t17_non_string_method_types_are_rejected() {
        for method in ["null", "true", "1", "[]", "{}"] {
            let bytes = format!(r#"{{"api":1,"id":"test","method":{method},"params":{{}}}}"#);
            assert_bad_request(bytes.into_bytes(), LocalValidationCause::InvalidMethodType);
        }
    }

    #[test]
    fn val_t18_all_six_read_only_methods_are_accepted_and_classified() {
        for method in READ_METHODS {
            let validated = validate(request(method)).expect("read method validates");
            assert_eq!(validated.method().as_str(), method);
            assert_eq!(validated.method().class(), LocalMethodClass::ReadOnly);
        }
    }

    #[test]
    fn val_t19_all_nine_mutating_methods_are_accepted_and_classified() {
        for method in MUTATING_METHODS {
            let validated = validate(request(method)).expect("mutating method validates");
            assert_eq!(validated.method().as_str(), method);
            assert_eq!(validated.method().class(), LocalMethodClass::Mutating);
        }
    }

    #[test]
    fn val_t20_unknown_method_with_valid_shape_is_rejected() {
        assert_unsupported_method(request("devices.unknown"));
    }

    #[test]
    fn val_t21_method_matching_is_case_sensitive() {
        assert_unsupported_method(request("System.status"));
    }

    #[test]
    fn val_t22_method_matching_does_not_trim_whitespace() {
        for method in [" system.status", "system.status "] {
            assert_unsupported_method(request(method));
        }
    }

    #[test]
    fn val_t23_method_matching_rejects_prefix_suffix_and_raw_variants() {
        for method in [
            "system",
            "system.status.extra",
            "system.status\\n",
            "SYSTEM.STATUS",
        ] {
            assert_unsupported_method(request(method));
        }
    }

    #[test]
    fn val_t24_no_invalid_path_reaches_request_construction() {
        let invalid_cases = [
            vec![0xff],
            b"{".to_vec(),
            b"null".to_vec(),
            br#"{"method":"system.status"}"#.to_vec(),
            br#"{"api":2,"method":"system.status"}"#.to_vec(),
            b"{\"api\":1}".to_vec(),
            br#"{"api":1,"method":null}"#.to_vec(),
            request("unknown.method"),
        ];
        for bytes in invalid_cases {
            let mut stages = Vec::new();
            assert!(validate_with_observer(bytes, |stage| stages.push(stage)).is_err());
            assert!(!stages.contains(&ValidationStage::RequestBuilt));
        }
    }

    #[test]
    fn val_t25_non_object_never_reaches_api_or_method_acceptance() {
        for bytes in [b"null".as_slice(), b"[]", b"true", b"1", br#""text""#] {
            let mut stages = Vec::new();
            assert!(validate_with_observer(bytes.to_vec(), |stage| stages.push(stage)).is_err());
            assert!(stages.contains(&ValidationStage::JsonAccepted));
            assert!(!stages.contains(&ValidationStage::ObjectAccepted));
            assert!(!stages.contains(&ValidationStage::ApiAccepted));
            assert!(!stages.contains(&ValidationStage::MethodAccepted));
        }
    }

    #[test]
    fn val_t26_invalid_utf8_never_reaches_serde_json() {
        let mut stages = Vec::new();
        let error = validate_with_observer(vec![0xf0, 0x28, 0x8c, 0x28], |stage| {
            stages.push(stage);
        })
        .expect_err("invalid UTF-8 rejected before JSON");
        assert_eq!(error.cause(), LocalValidationCause::InvalidUtf8);
        assert!(stages.is_empty());
    }

    #[test]
    fn val_t27_id_and_params_are_preserved_uninterpreted_and_redacted_from_debug() {
        let secret = "D4_PRIVATE_SECRET_DO_NOT_LOG";
        let bytes = format!(
            r#"{{"api":1,"method":"compute.submit","id":{{"opaque":true}},"params":{{"token":"{secret}"}}}}"#
        );
        let validated = validate(bytes.into_bytes()).expect("opaque fields do not block LC-007");
        assert_eq!(validated.id(), &serde_json::json!({"opaque": true}));
        assert_eq!(
            validated.params()["token"],
            Value::String(secret.to_owned())
        );
        let debug = format!("{validated:?}");
        assert!(!debug.contains(secret));
        assert!(!debug.contains("token"));
    }

    #[test]
    fn val_t28_allowlist_structural_oracle_proves_all_fifteen_exact_names() {
        let expected: Vec<&str> = READ_METHODS.into_iter().chain(MUTATING_METHODS).collect();
        let actual: Vec<&str> = LocalMethod::ALL
            .into_iter()
            .map(LocalMethod::as_str)
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 15);
        for (name, method) in actual.into_iter().zip(LocalMethod::ALL) {
            assert_eq!(LocalMethod::parse_exact(name), Some(method));
            assert_eq!(
                validate(request(name))
                    .expect("allowlisted name validates")
                    .method(),
                method
            );
        }
    }

    #[test]
    fn fix_t01_invalid_json_remains_local_bad_request() {
        assert_bad_request(b"{".to_vec(), LocalValidationCause::InvalidJson);
    }

    #[test]
    fn fix_t02_invalid_api_remains_local_bad_request() {
        assert_bad_request(
            br#"{"api":2,"method":"system.status"}"#.to_vec(),
            LocalValidationCause::InvalidApi,
        );
    }

    #[test]
    fn fix_t03_non_string_method_remains_local_bad_request() {
        assert_bad_request(
            br#"{"api":1,"id":"test","method":null,"params":{}}"#.to_vec(),
            LocalValidationCause::InvalidMethodType,
        );
    }

    #[test]
    fn fix_t04_unknown_string_method_is_local_unsupported_method() {
        assert_unsupported_method(request("unknown"));
    }

    #[test]
    fn fix_t05_case_changed_method_is_local_unsupported_method() {
        assert_unsupported_method(request("System.status"));
    }

    #[test]
    fn fix_t06_prefix_suffix_whitespace_and_raw_names_are_unsupported() {
        for method in [
            "system",
            "system.status.extra",
            "system.status ",
            "PBMUX_CONTROL_8",
            "rpc.system.status",
        ] {
            assert_unsupported_method(request(method));
        }
    }

    #[test]
    fn fix_t07_all_fifteen_canonical_methods_remain_accepted() {
        for method in LocalMethod::ALL {
            let validated = validate(request(method.as_str())).expect("canonical method accepted");
            assert_eq!(validated.method(), method);
        }
    }

    #[test]
    fn c12_id_and_params_are_required_but_remain_opaque() {
        assert_bad_request(
            br#"{"api":1,"method":"system.status","params":{}}"#.to_vec(),
            LocalValidationCause::MissingId,
        );
        let missing_params =
            validate(br#"{"api":1,"id":{"opaque":true},"method":"system.status"}"#.to_vec())
                .expect_err("params is mandatory");
        assert_eq!(missing_params.cause(), LocalValidationCause::MissingParams);
        assert_eq!(
            missing_params.correlation_id(),
            Some(&serde_json::json!({"opaque": true}))
        );

        let request = validate(
            br#"{"api":1,"id":[1,null],"method":"system.status","params":"opaque"}"#.to_vec(),
        )
        .expect("id and params types are handler-owned");
        assert_eq!(request.id(), &serde_json::json!([1, null]));
        assert_eq!(request.params(), "opaque");
    }

    #[test]
    fn malformed_object_error_recovers_id_without_exposing_it_in_debug() {
        let secret_id = "PRIVATE_CORRELATION_VALUE";
        let bytes = format!(r#"{{"api":2,"id":"{secret_id}"}}"#);
        let error = validate(bytes.into_bytes()).expect_err("invalid api rejected");
        assert_eq!(
            error.correlation_id(),
            Some(&Value::String(secret_id.to_owned()))
        );
        assert!(!format!("{error:?}").contains(secret_id));
    }
}
