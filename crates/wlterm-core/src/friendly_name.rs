//! Friendly random names for terminal sessions.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendlyName(String);

impl FriendlyName {
    pub fn generate() -> Self {
        for _ in 0..32 {
            if let Some(candidate) = petname::petname(2, "-") {
                let candidate = candidate.to_ascii_lowercase();
                if shell_name_valid(&candidate) {
                    return Self(candidate);
                }
            }
        }

        Self(format!("shell-{}", random_suffix()))
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FriendlyNameError {
    InvalidShellName,
}

fn shell_name_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn random_suffix() -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    (nanos % 10_000) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_friendly_names_match_shell_grammar() {
        for _ in 0..64 {
            let name = FriendlyName::generate();
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
            FriendlyName::from_candidate("Quiet-Otter")
                .expect("valid")
                .as_str(),
            "quiet-otter"
        );
    }
}
