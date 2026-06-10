#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    ConnectTimeout,
    ConnectRefused,
    TunnelUnavailable,
    Upstream429,
    QuotaExhausted,
    ModelUnsupported,
    AuthInvalid,
    Upstream5xx,
    BadGatewayResponse,
    BadRequest,
    StreamInterrupted,
    StreamUsageMissing,
    ClientDisconnected,
    SettlementFailed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureScope {
    Request,
    Share,
    Owner,
    #[allow(dead_code)]
    App,
    Model,
    MarketPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailurePolicy {
    pub scope: FailureScope,
    pub base_cooldown_secs: i64,
    pub report_to_router: bool,
    pub counts_against_share: bool,
    pub retryable_hint: bool,
}

impl FailureKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::ConnectTimeout => "connect_timeout",
            Self::ConnectRefused => "connect_refused",
            Self::TunnelUnavailable => "tunnel_unavailable",
            Self::Upstream429 => "upstream_429",
            Self::QuotaExhausted => "quota_exhausted",
            Self::ModelUnsupported => "model_unsupported",
            Self::AuthInvalid => "auth_invalid",
            Self::Upstream5xx => "upstream_5xx",
            Self::BadGatewayResponse => "bad_gateway_response",
            Self::BadRequest => "bad_request",
            Self::StreamInterrupted => "stream_interrupted",
            Self::StreamUsageMissing => "stream_usage_missing",
            Self::ClientDisconnected => "client_disconnected",
            Self::SettlementFailed => "settlement_failed",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "connect_timeout" | "timeout" => Self::ConnectTimeout,
            "connect_refused" => Self::ConnectRefused,
            "tunnel_unavailable" | "network" => Self::TunnelUnavailable,
            "upstream_429" | "rate_limited" => Self::Upstream429,
            "quota_exhausted" => Self::QuotaExhausted,
            "model_unsupported" => Self::ModelUnsupported,
            "auth_invalid" | "auth_failed" => Self::AuthInvalid,
            "upstream_5xx" | "upstream_unavailable" => Self::Upstream5xx,
            "bad_gateway_response" | "upstream_error" => Self::BadGatewayResponse,
            "bad_request" => Self::BadRequest,
            "stream_interrupted" => Self::StreamInterrupted,
            "stream_usage_missing" => Self::StreamUsageMissing,
            "client_disconnected" => Self::ClientDisconnected,
            "settlement_failed" => Self::SettlementFailed,
            _ => Self::Unknown,
        }
    }

    pub fn policy(self) -> FailurePolicy {
        match self {
            Self::ConnectTimeout => FailurePolicy {
                scope: FailureScope::MarketPath,
                base_cooldown_secs: 15,
                report_to_router: false,
                counts_against_share: true,
                retryable_hint: true,
            },
            Self::ConnectRefused | Self::TunnelUnavailable => FailurePolicy {
                scope: FailureScope::MarketPath,
                base_cooldown_secs: 30,
                report_to_router: false,
                counts_against_share: true,
                retryable_hint: true,
            },
            Self::Upstream429 => FailurePolicy {
                scope: FailureScope::Owner,
                base_cooldown_secs: 120,
                report_to_router: true,
                counts_against_share: true,
                retryable_hint: true,
            },
            Self::QuotaExhausted => FailurePolicy {
                scope: FailureScope::Owner,
                base_cooldown_secs: 900,
                report_to_router: true,
                counts_against_share: true,
                retryable_hint: true,
            },
            Self::ModelUnsupported => FailurePolicy {
                scope: FailureScope::Model,
                base_cooldown_secs: 3600,
                report_to_router: true,
                counts_against_share: false,
                retryable_hint: false,
            },
            Self::AuthInvalid => FailurePolicy {
                scope: FailureScope::Owner,
                base_cooldown_secs: 1800,
                report_to_router: true,
                counts_against_share: true,
                retryable_hint: false,
            },
            Self::Upstream5xx | Self::BadGatewayResponse => FailurePolicy {
                scope: FailureScope::Share,
                base_cooldown_secs: 30,
                report_to_router: false,
                counts_against_share: true,
                retryable_hint: true,
            },
            Self::BadRequest => FailurePolicy {
                scope: FailureScope::Request,
                base_cooldown_secs: 0,
                report_to_router: false,
                counts_against_share: false,
                retryable_hint: false,
            },
            Self::StreamInterrupted => FailurePolicy {
                scope: FailureScope::Share,
                base_cooldown_secs: 10,
                report_to_router: false,
                counts_against_share: true,
                retryable_hint: true,
            },
            Self::StreamUsageMissing => FailurePolicy {
                scope: FailureScope::Request,
                base_cooldown_secs: 10,
                report_to_router: false,
                counts_against_share: false,
                retryable_hint: false,
            },
            Self::ClientDisconnected | Self::SettlementFailed => FailurePolicy {
                scope: FailureScope::Request,
                base_cooldown_secs: 0,
                report_to_router: false,
                counts_against_share: false,
                retryable_hint: false,
            },
            Self::Unknown => FailurePolicy {
                scope: FailureScope::Share,
                base_cooldown_secs: 30,
                report_to_router: false,
                counts_against_share: true,
                retryable_hint: true,
            },
        }
    }
}

impl FailureScope {
    pub fn code(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Share => "share",
            Self::Owner => "owner",
            Self::App => "app",
            Self::Model => "model",
            Self::MarketPath => "market_path",
        }
    }
}

pub fn cooldown_secs(kind: FailureKind, failure_count: i64) -> i64 {
    let base = kind.policy().base_cooldown_secs;
    if base <= 0 {
        return 0;
    }
    let exponent = failure_count.saturating_sub(1).clamp(0, 4) as u32;
    let multiplier = 1_i64.checked_shl(exponent).unwrap_or(16).clamp(1, 16);
    (base * multiplier).min(60 * 60)
}

pub fn append_failure_audit_flags(
    flags: &mut serde_json::Value,
    kind: FailureKind,
    policy: FailurePolicy,
) {
    append_audit_flag(flags, &format!("failure:{}", kind.code()));
    append_audit_flag(flags, &format!("failure_scope:{}", policy.scope.code()));
    append_audit_flag(
        flags,
        if policy.retryable_hint {
            "failure_retryable:true"
        } else {
            "failure_retryable:false"
        },
    );
}

pub fn append_audit_flag(flags: &mut serde_json::Value, flag: &str) {
    if !flags.is_array() {
        *flags = serde_json::json!([]);
    }
    let Some(array) = flags.as_array_mut() else {
        return;
    };
    if !array.iter().any(|item| item.as_str() == Some(flag)) {
        array.push(serde_json::Value::String(flag.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_policies_keep_non_health_failures_out_of_share_counts() {
        assert!(!FailureKind::SettlementFailed.policy().counts_against_share);
        assert!(
            !FailureKind::ClientDisconnected
                .policy()
                .counts_against_share
        );
        assert!(FailureKind::ConnectTimeout.policy().counts_against_share);
    }

    #[test]
    fn cooldown_uses_bounded_exponential_backoff() {
        assert_eq!(cooldown_secs(FailureKind::ConnectTimeout, 1), 15);
        assert_eq!(cooldown_secs(FailureKind::ConnectTimeout, 2), 30);
        assert_eq!(cooldown_secs(FailureKind::ConnectTimeout, 5), 240);
        assert_eq!(cooldown_secs(FailureKind::QuotaExhausted, 99), 3600);
        assert_eq!(cooldown_secs(FailureKind::ClientDisconnected, 99), 0);
    }

    #[test]
    fn audit_flags_are_stable_and_deduplicated() {
        let mut flags = serde_json::json!(["existing"]);
        append_failure_audit_flags(
            &mut flags,
            FailureKind::Upstream429,
            FailureKind::Upstream429.policy(),
        );
        append_failure_audit_flags(
            &mut flags,
            FailureKind::Upstream429,
            FailureKind::Upstream429.policy(),
        );
        assert_eq!(
            flags,
            serde_json::json!([
                "existing",
                "failure:upstream_429",
                "failure_scope:owner",
                "failure_retryable:true"
            ])
        );
    }
}
