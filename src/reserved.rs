//! Compatibility wrapper for consumers that already own a shared-looking flag.
//!
//! New CLIs should prefer the canonical shared spellings directly. Existing
//! CLIs sometimes predate this crate and may already assign one of the
//! compatibility aliases (for example `--quiet`) to domain behavior. Those
//! spellings must remain visible to the downstream parser until the consumer
//! deliberately migrates them.

use std::collections::{HashMap, HashSet};

use crate::{ParsedSharedArgs, SharedArgError, parse_shared_argv};

const SENTINEL_PREFIX: &str = "\0ores-clis-core-reserved:";

/// Parse shared runtime flags while preserving selected exact argv spellings
/// for the consumer parser.
///
/// `reserved` matches complete tokens only. A reserved `--quiet` therefore
/// protects `--quiet` but does not reserve unrelated tokens. The `--`
/// terminator itself can never be reserved.
///
/// This API is intended for compatibility collisions, not for redefining the
/// canonical shared policy. Prefer the explicit forms such as
/// `--log-level=quiet` when a consumer reserves a shorthand alias.
pub fn parse_shared_argv_with_reserved<I, S, R, T>(
    tokens: I,
    reserved: R,
) -> Result<ParsedSharedArgs, SharedArgError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
    R: IntoIterator<Item = T>,
    T: Into<String>,
{
    let reserved = reserved
        .into_iter()
        .map(Into::into)
        .filter(|token| token != "--")
        .collect::<HashSet<String>>();

    if reserved.is_empty() {
        return parse_shared_argv(tokens);
    }

    let mut originals = HashMap::new();
    let protected = tokens
        .into_iter()
        .map(Into::into)
        .enumerate()
        .map(|(index, token): (usize, String)| {
            if reserved.contains(&token) {
                // argv passed by an OS process cannot contain NUL, so this
                // sentinel cannot collide with a real command-line token.
                let sentinel = format!("{SENTINEL_PREFIX}{index}");
                originals.insert(sentinel.clone(), token);
                sentinel
            } else {
                token
            }
        })
        .collect::<Vec<_>>();

    let mut parsed = parse_shared_argv(protected)?;
    for token in &mut parsed.passthrough {
        if let Some(original) = originals.remove(token) {
            *token = original;
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LogLevel, OutputMode};

    #[test]
    fn reserved_quiet_stays_with_consumer() {
        let parsed = parse_shared_argv_with_reserved(
            ["conformance", "--quiet", "--json"],
            ["--quiet"],
        )
        .unwrap();

        assert_eq!(parsed.policy.output, OutputMode::Json);
        assert_eq!(parsed.policy.log_level, LogLevel::Info);
        assert_eq!(parsed.passthrough, vec!["conformance", "--quiet"]);
    }

    #[test]
    fn explicit_log_level_still_reaches_shared_policy() {
        let parsed = parse_shared_argv_with_reserved(
            ["conformance", "--quiet", "--log-level=quiet"],
            ["--quiet"],
        )
        .unwrap();

        assert_eq!(parsed.policy.log_level, LogLevel::Quiet);
        assert_eq!(parsed.passthrough, vec!["conformance", "--quiet"]);
    }

    #[test]
    fn empty_reservations_match_default_parser() {
        let tokens = ["status", "--json", "--color=never"];
        let regular = parse_shared_argv(tokens).unwrap();
        let reserved = parse_shared_argv_with_reserved(tokens, std::iter::empty::<&str>()).unwrap();
        assert_eq!(regular, reserved);
    }

    #[test]
    fn terminator_remains_authoritative() {
        let parsed = parse_shared_argv_with_reserved(
            ["--json", "--", "--quiet"],
            ["--quiet", "--"],
        )
        .unwrap();
        assert_eq!(parsed.passthrough, vec!["--", "--quiet"]);
    }
}
