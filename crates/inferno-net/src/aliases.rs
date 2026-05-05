//! Friendly model-name aliasing.
//!
//! Devices report opaque names like `Y001-Yamaha-Tio1608-D-1533c8`; users
//! think of them as "Yamaha 1608 Stagebox". This module loads a list of
//! substring rules (bundled defaults + optional user overrides) and resolves
//! a device's reported strings to a friendly alias.
//!
//! Rule precedence: user-supplied rules are checked before bundled defaults,
//! so users can override (e.g.) the default `NY64-D` mapping without editing
//! the binary.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const BUNDLED_ALIASES: &str = include_str!("../data/aliases.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasRule {
    /// Case-insensitive substring matched against the device's reported
    /// name, model_name, and model_code.
    pub pattern: String,
    /// Friendly name to display.
    pub alias: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AliasFile {
    #[serde(default, rename = "alias")]
    aliases: Vec<AliasRule>,
}

#[derive(Debug, Clone, Default)]
pub struct Aliases {
    /// User rules (checked first), then bundled.
    rules: Vec<AliasRule>,
}

impl Aliases {
    /// Load only the bundled defaults baked into the binary.
    pub fn bundled() -> Self {
        let parsed: AliasFile =
            toml::from_str(BUNDLED_ALIASES).expect("bundled aliases.toml must parse");
        Self { rules: parsed.aliases }
    }

    /// Bundled defaults plus user rules from `path` (user rules take precedence).
    pub fn load_with_overrides(path: Option<&Path>) -> Result<Self> {
        let mut a = Self::bundled();
        if let Some(p) = path {
            let text = std::fs::read_to_string(p)
                .map_err(|e| Error::Protocol(format!("read aliases {}: {e}", p.display())))?;
            let user: AliasFile = toml::from_str(&text)
                .map_err(|e| Error::Protocol(format!("parse aliases {}: {e}", p.display())))?;
            // user rules first so they override the bundled ones
            let mut prefixed = user.aliases;
            prefixed.extend(std::mem::take(&mut a.rules));
            a.rules = prefixed;
        }
        Ok(a)
    }

    /// Resolve an alias by checking each candidate string against the rules
    /// in order. None of the candidates need to be non-empty.
    pub fn lookup(&self, candidates: &[&str]) -> Option<&str> {
        for rule in &self.rules {
            let pat = rule.pattern.to_ascii_lowercase();
            for hay in candidates {
                if hay.is_empty() {
                    continue;
                }
                if hay.to_ascii_lowercase().contains(&pat) {
                    return Some(&rule.alias);
                }
            }
        }
        None
    }

    pub fn rules(&self) -> &[AliasRule] {
        &self.rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_includes_known_models() {
        let a = Aliases::bundled();
        assert_eq!(a.lookup(&["Y001-Yamaha-Tio1608-D-1533c8"]), Some("Yamaha 1608 Stagebox"));
        assert_eq!(a.lookup(&["AA-4202524000109"]), Some("Galaxy32"));
        assert_eq!(a.lookup(&["x16D--293bb0"]), Some("Apollo x16D"));
        assert_eq!(a.lookup(&["Y000-Yamaha-NY64-D-152434"]), Some("Yamaha TF-1"));
    }

    #[test]
    fn lookup_is_case_insensitive_and_searches_all_candidates() {
        let a = Aliases::bundled();
        assert_eq!(a.lookup(&["", "tio1608"]), Some("Yamaha 1608 Stagebox"));
    }

    #[test]
    fn lookup_returns_none_when_no_pattern_matches() {
        let a = Aliases::bundled();
        assert_eq!(a.lookup(&["UnknownDevice", "", ""]), None);
    }
}
