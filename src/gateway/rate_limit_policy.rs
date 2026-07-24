//! Per-route-class fail-open vs. fail-closed policy for the distributed rate
//! limiter.
//!
//! When Redis is unreachable the limiter cannot know whether a caller is
//! over their limit. Silently allowing every request (fail-open) is
//! reasonable for cheap public reads, but it is exactly the wrong default for
//! endpoints that exist to *stop* abuse (login, registration, password
//! reset, API key issuance) — those must fail closed so an outage doesn't
//! double as an open door for credential stuffing or account creation spam.

/// What to do with a request when the limiter is degraded (Redis error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailPolicy {
    /// Allow the request through; local in-process limiters remain a backstop.
    FailOpen,
    /// Reject the request (503) rather than risk unlimited throughput.
    FailClosed,
}

impl FailPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            FailPolicy::FailOpen => "fail_open",
            FailPolicy::FailClosed => "fail_closed",
        }
    }
}

/// Route-name segments that default to fail-closed because they gate
/// authentication or account-creation flows.
const FAIL_CLOSED_MARKERS: &[&str] = &[
    "/auth/",
    "/login",
    "/register",
    "/password",
    "/api-keys",
    "/mfa",
    "/2fa",
    "/otp",
];

/// Resolve the fail policy for `path`.
///
/// Resolution order:
/// 1. Explicit env override: `RATE_LIMIT_FAIL_POLICY_<SEGMENT>` = `open` | `closed`,
///    where `<SEGMENT>` is the upper-snake-case last path segment (mirrors
///    the existing `ROUTE_RL_<SEGMENT>_RPM` override convention).
/// 2. A global override: `RATE_LIMIT_FAIL_POLICY_DEFAULT` = `open` | `closed`.
/// 3. Built-in default: auth/account-security routes fail closed, everything
///    else fails open.
pub fn resolve(path: &str) -> FailPolicy {
    if let Some(segment) = last_path_segment(path) {
        let var = format!("RATE_LIMIT_FAIL_POLICY_{}", segment);
        if let Some(policy) = parse_policy_env(&var) {
            return policy;
        }
    }

    if let Some(policy) = parse_policy_env("RATE_LIMIT_FAIL_POLICY_DEFAULT") {
        return policy;
    }

    if FAIL_CLOSED_MARKERS.iter().any(|m| path.contains(m)) {
        FailPolicy::FailClosed
    } else {
        FailPolicy::FailOpen
    }
}

fn parse_policy_env(var: &str) -> Option<FailPolicy> {
    match std::env::var(var).ok()?.to_lowercase().as_str() {
        "closed" | "fail_closed" | "fail-closed" => Some(FailPolicy::FailClosed),
        "open" | "fail_open" | "fail-open" => Some(FailPolicy::FailOpen),
        _ => None,
    }
}

fn last_path_segment(path: &str) -> Option<String> {
    let segment = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('v'))
        .last()?;
    Some(segment.to_uppercase().replace('-', "_"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; serialize tests that touch them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn auth_routes_default_fail_closed() {
        let _g = ENV_LOCK.lock().unwrap();
        assert_eq!(resolve("/api/v1/auth/login"), FailPolicy::FailClosed);
        assert_eq!(resolve("/api/v2/auth/register"), FailPolicy::FailClosed);
        assert_eq!(resolve("/api/v1/password/reset"), FailPolicy::FailClosed);
    }

    #[test]
    fn public_reads_default_fail_open() {
        let _g = ENV_LOCK.lock().unwrap();
        assert_eq!(resolve("/api/v1/creators"), FailPolicy::FailOpen);
        assert_eq!(resolve("/api/v1/leaderboard"), FailPolicy::FailOpen);
    }

    #[test]
    fn per_route_env_override_wins() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("RATE_LIMIT_FAIL_POLICY_CREATORS", "closed");
        assert_eq!(resolve("/api/v1/creators"), FailPolicy::FailClosed);
        std::env::remove_var("RATE_LIMIT_FAIL_POLICY_CREATORS");
    }

    #[test]
    fn global_default_override_applies_when_no_route_override() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("RATE_LIMIT_FAIL_POLICY_DEFAULT", "closed");
        assert_eq!(resolve("/api/v1/creators"), FailPolicy::FailClosed);
        std::env::remove_var("RATE_LIMIT_FAIL_POLICY_DEFAULT");
    }

    #[test]
    fn route_override_takes_priority_over_global_default() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("RATE_LIMIT_FAIL_POLICY_DEFAULT", "closed");
        std::env::set_var("RATE_LIMIT_FAIL_POLICY_CREATORS", "open");
        assert_eq!(resolve("/api/v1/creators"), FailPolicy::FailOpen);
        std::env::remove_var("RATE_LIMIT_FAIL_POLICY_DEFAULT");
        std::env::remove_var("RATE_LIMIT_FAIL_POLICY_CREATORS");
    }
}
