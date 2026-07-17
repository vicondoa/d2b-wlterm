//! Friendly random names for terminal shell sessions.

use std::collections::HashSet;
use std::fmt;

use serde::{Serialize, Serializer};

pub const FRIENDLY_NAME_MAX_BYTES: usize = 64;
pub const FRIENDLY_NAME_RETRY_LIMIT: usize = 32;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FriendlyName(String);

impl FriendlyName {
    pub fn generate() -> Result<Self, FriendlyNameError> {
        Self::generate_unique(|_| false)
    }

    pub fn generate_unique(is_taken: impl FnMut(&str) -> bool) -> Result<Self, FriendlyNameError> {
        let mut source = PetnameSource;
        Self::generate_unique_with(&mut source, is_taken, FRIENDLY_NAME_RETRY_LIMIT)
    }

    pub fn generate_unique_with<S>(
        source: &mut S,
        mut is_taken: impl FnMut(&str) -> bool,
        retry_limit: usize,
    ) -> Result<Self, FriendlyNameError>
    where
        S: FriendlyNameSource + ?Sized,
    {
        for _ in 0..retry_limit {
            let Some(candidate) = source.next_candidate() else {
                continue;
            };
            let candidate = candidate.to_ascii_lowercase();
            if shell_name_valid(&candidate) && !is_taken(&candidate) {
                return Ok(Self(candidate));
            }
        }

        Err(FriendlyNameError::ExhaustedRetries)
    }

    pub fn from_candidate(value: impl Into<String>) -> Result<Self, FriendlyNameError> {
        let value = value.into().to_ascii_lowercase();
        if shell_name_valid(&value) {
            Ok(Self(value))
        } else {
            Err(FriendlyNameError::InvalidShellName)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub const fn metrics_label_value(&self) -> &'static str {
        "shell"
    }
}

impl fmt::Debug for FriendlyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FriendlyName").field(&"<redacted>").finish()
    }
}

impl Serialize for FriendlyName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FriendlyNameError {
    InvalidShellName,
    ExhaustedRetries,
}

impl fmt::Display for FriendlyNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShellName => f.write_str("invalid d2b shell name"),
            Self::ExhaustedRetries => f.write_str("friendly shell-name retry limit exhausted"),
        }
    }
}

pub trait FriendlyNameSource {
    fn next_candidate(&mut self) -> Option<String>;
}

struct PetnameSource;

impl FriendlyNameSource for PetnameSource {
    fn next_candidate(&mut self) -> Option<String> {
        petname::petname(2, "-")
    }
}

pub fn shell_name_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > FRIENDLY_NAME_MAX_BYTES {
        return false;
    }

    if !(bytes[0].is_ascii_lowercase() || bytes[0] == b'_') {
        return false;
    }

    let word_bytes = if bytes[0] == b'_' { &bytes[1..] } else { bytes };

    !word_bytes.is_empty()
        && word_bytes.split(|byte| *byte == b'-').all(|word| {
            !word.is_empty()
                && word
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

pub fn collision_set<'a>(names: impl IntoIterator<Item = &'a FriendlyName>) -> HashSet<&'a str> {
    names.into_iter().map(FriendlyName::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SequenceSource {
        candidates: Vec<Option<&'static str>>,
    }

    impl SequenceSource {
        fn new(candidates: Vec<Option<&'static str>>) -> Self {
            candidates.into_iter().rev().collect::<Vec<_>>().into()
        }
    }

    impl From<Vec<Option<&'static str>>> for SequenceSource {
        fn from(candidates: Vec<Option<&'static str>>) -> Self {
            Self { candidates }
        }
    }

    impl FriendlyNameSource for SequenceSource {
        fn next_candidate(&mut self) -> Option<String> {
            self.candidates.pop().flatten().map(str::to_string)
        }
    }

    #[test]
    fn generated_friendly_names_match_shell_grammar() {
        for _ in 0..64 {
            let name = FriendlyName::generate().expect("name");
            assert!(
                shell_name_valid(name.as_str()),
                "{} should be valid",
                name.as_str()
            );
        }
    }

    #[test]
    fn candidate_validation_rejects_bad_shell_names() {
        assert_eq!(
            FriendlyName::from_candidate("bad/name"),
            Err(FriendlyNameError::InvalidShellName)
        );
        assert_eq!(
            FriendlyName::from_candidate("123-shell"),
            Err(FriendlyNameError::InvalidShellName)
        );
        assert_eq!(
            FriendlyName::from_candidate("bad.name"),
            Err(FriendlyNameError::InvalidShellName)
        );
        assert_eq!(
            FriendlyName::from_candidate("bad--name"),
            Err(FriendlyNameError::InvalidShellName)
        );
        assert_eq!(
            FriendlyName::from_candidate("bad-name-"),
            Err(FriendlyNameError::InvalidShellName)
        );
        assert_eq!(
            FriendlyName::from_candidate("Quiet-Otter")
                .expect("valid")
                .as_str(),
            "quiet-otter"
        );
        assert_eq!(
            FriendlyName::from_candidate("_quiet-otter")
                .expect("valid")
                .as_str(),
            "_quiet-otter"
        );
    }

    #[test]
    fn candidate_validation_caps_length() {
        let valid = format!("a{}", "b".repeat(FRIENDLY_NAME_MAX_BYTES - 1));
        assert!(FriendlyName::from_candidate(valid).is_ok());

        let too_long = format!("a{}", "b".repeat(FRIENDLY_NAME_MAX_BYTES));
        assert_eq!(
            FriendlyName::from_candidate(too_long),
            Err(FriendlyNameError::InvalidShellName)
        );
    }

    #[test]
    fn generation_skips_collisions_until_free_name() {
        let mut source = SequenceSource::new(vec![
            Some("quiet-otter"),
            Some("quiet-otter"),
            Some("brave-panda"),
        ]);
        let taken = [FriendlyName::from_candidate("quiet-otter").unwrap()];
        let collisions = collision_set(&taken);

        let name = FriendlyName::generate_unique_with(
            &mut source,
            |candidate| collisions.contains(candidate),
            4,
        )
        .expect("free name");

        assert_eq!(name.as_str(), "brave-panda");
    }

    #[test]
    fn generation_fails_after_retry_limit() {
        let mut source = SequenceSource::new(vec![
            Some("bad/name"),
            Some("quiet-otter"),
            Some("quiet-otter"),
        ]);

        assert_eq!(
            FriendlyName::generate_unique_with(
                &mut source,
                |candidate| candidate == "quiet-otter",
                3
            ),
            Err(FriendlyNameError::ExhaustedRetries)
        );
    }

    #[test]
    fn shell_name_cannot_become_metric_label() {
        let name = FriendlyName::from_candidate("customer-project-shell").unwrap();
        assert_eq!(name.metrics_label_value(), "shell");
    }
}
