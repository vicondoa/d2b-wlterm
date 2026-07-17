//! Canonical d2b client adapter boundary.
//!
//! The legacy public-JSON transport is intentionally absent. Target discovery,
//! session setup, persistent-shell streams, and Wayland control remain
//! unavailable until their canonical service contracts are content-frozen.

use std::fmt;

use wlterm_core::WorkloadSummary;

pub use d2b_client_toolkit::{
    D2B_SOURCE_FINGERPRINT as CLIENT_SOURCE_FINGERPRINT,
    D2B_SOURCE_REVISION as CLIENT_SOURCE_REVISION,
};

const SERVICES_UNAVAILABLE: &str =
    "canonical terminal and desktop services are not available in this source cut";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bClientConfig {
    pub public_socket_path: String,
    pub operation_timeout_ms: u64,
}

impl Default for D2bClientConfig {
    fn default() -> Self {
        Self {
            public_socket_path: std::env::var("D2B_PUBLIC_SOCKET")
                .unwrap_or_else(|_| "/run/d2b/public.sock".to_owned()),
            operation_timeout_ms: 5_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bActionBoundary {
    config: D2bClientConfig,
}

impl D2bActionBoundary {
    pub fn new(config: D2bClientConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &D2bClientConfig {
        &self.config
    }

    pub fn inventory_blocking(&self) -> Result<Vec<WorkloadSummary>, D2bClientError> {
        Err(D2bClientError::services_unavailable())
    }

    pub fn discover_blocking(&self) -> Result<Vec<WorkloadSummary>, D2bClientError> {
        Err(D2bClientError::services_unavailable())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum D2bClientErrorKind {
    FeatureUnavailable(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bClientError {
    kind: D2bClientErrorKind,
}

impl D2bClientError {
    fn services_unavailable() -> Self {
        Self {
            kind: D2bClientErrorKind::FeatureUnavailable(SERVICES_UNAVAILABLE),
        }
    }

    pub fn kind(&self) -> &D2bClientErrorKind {
        &self.kind
    }
}

impl fmt::Display for D2bClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(SERVICES_UNAVAILABLE)
    }
}

impl std::error::Error for D2bClientError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_the_exact_canonical_source() {
        assert_eq!(
            CLIENT_SOURCE_REVISION,
            "4018d9c9652bd826c2e6a9abccdcdcafb832d944"
        );
        assert_eq!(
            CLIENT_SOURCE_FINGERPRINT,
            "c2c99bdd77ba66948fce81161dcc3efde608eefefb96f28fa934c9f58d96d838"
        );
    }

    #[test]
    fn blocked_services_fail_without_protocol_fallback() {
        let boundary = D2bActionBoundary::new(D2bClientConfig::default());
        let error = boundary
            .discover_blocking()
            .expect_err("blocked discovery must fail closed");
        assert_eq!(
            error.kind(),
            &D2bClientErrorKind::FeatureUnavailable(SERVICES_UNAVAILABLE)
        );
    }
}
