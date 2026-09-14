use std::fmt;
use std::str::FromStr;

use crate::{CliPolicy, ColorMode, LogLevel, OutputMode};

/// Result of parsing only the shared ORESoftware CLI policy flags.
///
/// Arguments not owned by the shared policy layer are preserved verbatim in
/// `passthrough` for the consumer's real parser. A `--` terminator stops shared
/// parsing and is not included in the returned passthrough list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSharedArgs {
    /// Shared runtime policy derived from explicit CLI choices.
    pub policy: CliPolicy,
    /// Arguments that remain for the consumer CLI parser.
    pub passthrough: Vec<String>,
}

/// Deterministic parse/conflict failure for shared CLI flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedArgError {
    /// A flag requiring a following token did not receive one.
    MissingValue { flag: &'static str },
    /// A shared value failed canonical parsing.
    InvalidValue {
        flag: &'static str,
        value: String,
        message: String,
    },
    /// Multiple explicit choices for one field disagreed.
    Conflict {
        field: &'static str,
        first: String,
        second: String,
    },
}

impl fmt::Display for SharedArgError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue { flag } => write!(formatter, "missing value for {flag}"),
            Self::InvalidValue {
                flag,
                value,
                message,
            } => write!(formatter, "invalid value `{value}` for {flag}: {message}"),
            Self::Conflict {
                field,
                first,
                second,
            } => write!(
                formatter,
                "conflicting explicit {field} choices: `{first}` versus `{second}`"
            ),
        }
    }
}

impl std::error::Error for SharedArgError {}

/// Parse the shared ORESoftware CLI policy flags from already-tokenized argv.
///
/// The adapter is deliberately parser-agnostic: it does not invoke a shell,
/// does not replace `flags-2-env`, and preserves all unrelated arguments for
/// the consumer parser.
///
/// Supported forms:
/// - `--color`, `--color=auto|always|never`, `--no-color`, `--!color`
/// - `--json`, `--no-json`, `--!json`
/// - `--output=auto|human|json` and `--output VALUE`
/// - `--log-level=LEVEL` and `--log-level LEVEL`
/// - compatibility aliases `--quiet` and `--silent`
///
/// Duplicate identical choices are idempotent. Contradictory explicit choices
/// fail rather than depending on argument order.
pub fn parse_shared_argv<I, S>(tokens: I) -> Result<ParsedSharedArgs, SharedArgError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let tokens: Vec<String> = tokens.into_iter().map(Into::into).collect();
    let mut passthrough = Vec::new();
    let mut explicit_output: Option<(OutputMode, String)> = None;
    let mut explicit_color: Option<(ColorMode, String)> = None;
    let mut explicit_log_level: Option<(LogLevel, String)> = None;

    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];

        if token == "--" {
            passthrough.extend(tokens[index + 1..].iter().cloned());
            break;
        }

        match token.as_str() {
            "--color" => set_explicit(
                &mut explicit_color,
                ColorMode::Always,
                token,
                "color",
            )?,
            "--no-color" | "--!color" => set_explicit(
                &mut explicit_color,
                ColorMode::Never,
                token,
                "color",
            )?,
            "--json" => set_explicit(
                &mut explicit_output,
                OutputMode::Json,
                token,
                "output",
            )?,
            "--no-json" | "--!json" => set_explicit(
                &mut explicit_output,
                OutputMode::Human,
                token,
                "output",
            )?,
            "--quiet" => set_explicit(
                &mut explicit_log_level,
                LogLevel::Quiet,
                token,
                "log-level",
            )?,
            "--silent" => set_explicit(
                &mut explicit_log_level,
                LogLevel::Silent,
                token,
                "log-level",
            )?,
            "--output" => {
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or(SharedArgError::MissingValue { flag: "--output" })?;
                let parsed = parse_value::<OutputMode>("--output", value)?;
                set_explicit(&mut explicit_output, parsed, value, "output")?;
            }
            "--log-level" => {
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or(SharedArgError::MissingValue {
                        flag: "--log-level",
                    })?;
                let parsed = parse_value::<LogLevel>("--log-level", value)?;
                set_explicit(
                    &mut explicit_log_level,
                    parsed,
                    value,
                    "log-level",
                )?;
            }
            _ if token.starts_with("--color=") => {
                let value = token.trim_start_matches("--color=");
                let parsed = parse_value::<ColorMode>("--color", value)?;
                set_explicit(&mut explicit_color, parsed, value, "color")?;
            }
            _ if token.starts_with("--output=") => {
                let value = token.trim_start_matches("--output=");
                let parsed = parse_value::<OutputMode>("--output", value)?;
                set_explicit(&mut explicit_output, parsed, value, "output")?;
            }
            _ if token.starts_with("--log-level=") => {
                let value = token.trim_start_matches("--log-level=");
                let parsed = parse_value::<LogLevel>("--log-level", value)?;
                set_explicit(
                    &mut explicit_log_level,
                    parsed,
                    value,
                    "log-level",
                )?;
            }
            _ => passthrough.push(token.clone()),
        }

        index += 1;
    }

    let mut policy = CliPolicy::default();
    if let Some((output, _)) = explicit_output {
        policy.output = output;
    }
    if let Some((color, _)) = explicit_color {
        policy.color = color;
    }
    if let Some((log_level, _)) = explicit_log_level {
        policy.log_level = log_level;
    }

    Ok(ParsedSharedArgs {
        policy,
        passthrough,
    })
}

fn parse_value<T>(flag: &'static str, value: &str) -> Result<T, SharedArgError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|error| SharedArgError::InvalidValue {
            flag,
            value: value.to_owned(),
            message: error.to_string(),
        })
}

fn set_explicit<T>(
    slot: &mut Option<(T, String)>,
    value: T,
    spelling: &str,
    field: &'static str,
) -> Result<(), SharedArgError>
where
    T: Copy + Eq,
{
    if let Some((existing, existing_spelling)) = slot {
        if *existing != value {
            return Err(SharedArgError::Conflict {
                field,
                first: existing_spelling.clone(),
                second: spelling.to_owned(),
            });
        }
        return Ok(());
    }

    *slot = Some((value, spelling.to_owned()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_canonical_shared_flags() {
        let parsed = parse_shared_argv([
            "--color=always",
            "--json",
            "--log-level=trace",
            "status",
        ])
        .unwrap();

        assert_eq!(parsed.policy.color, ColorMode::Always);
        assert_eq!(parsed.policy.output, OutputMode::Json);
        assert_eq!(parsed.policy.log_level, LogLevel::Trace);
        assert_eq!(parsed.passthrough, vec!["status"]);
    }

    #[test]
    fn supports_negation_aliases() {
        let parsed = parse_shared_argv(["--!color", "--!json"]).unwrap();
        assert_eq!(parsed.policy.color, ColorMode::Never);
        assert_eq!(parsed.policy.output, OutputMode::Human);
    }

    #[test]
    fn supports_split_output_and_log_level_values() {
        let parsed = parse_shared_argv(["--output", "human", "--log-level", "debug"]).unwrap();
        assert_eq!(parsed.policy.output, OutputMode::Human);
        assert_eq!(parsed.policy.log_level, LogLevel::Debug);
    }

    #[test]
    fn duplicate_identical_choices_are_idempotent() {
        let parsed = parse_shared_argv([
            "--json",
            "--output=json",
            "--color",
            "--color=always",
            "--log-level=warning",
            "--log-level=warn",
        ])
        .unwrap();
        assert_eq!(parsed.policy.output, OutputMode::Json);
        assert_eq!(parsed.policy.color, ColorMode::Always);
        assert_eq!(parsed.policy.log_level, LogLevel::Warn);
    }

    #[test]
    fn contradictory_output_flags_fail_deterministically() {
        let error = parse_shared_argv(["--json", "--no-json"]).unwrap_err();
        assert!(matches!(error, SharedArgError::Conflict { field: "output", .. }));
    }

    #[test]
    fn contradictory_color_flags_fail_deterministically() {
        let error = parse_shared_argv(["--color", "--no-color"]).unwrap_err();
        assert!(matches!(error, SharedArgError::Conflict { field: "color", .. }));
    }

    #[test]
    fn contradictory_log_levels_fail_deterministically() {
        let error = parse_shared_argv(["--log-level=info", "--silent"]).unwrap_err();
        assert!(matches!(
            error,
            SharedArgError::Conflict {
                field: "log-level",
                ..
            }
        ));
    }

    #[test]
    fn unrelated_args_are_preserved_verbatim() {
        let parsed = parse_shared_argv(["audit", "--org", "zed-pkg", "--json"]).unwrap();
        assert_eq!(parsed.passthrough, vec!["audit", "--org", "zed-pkg"]);
        assert_eq!(parsed.policy.output, OutputMode::Json);
    }

    #[test]
    fn terminator_stops_shared_parsing() {
        let parsed = parse_shared_argv(["--json", "--", "--no-json", "payload"]).unwrap();
        assert_eq!(parsed.policy.output, OutputMode::Json);
        assert_eq!(parsed.passthrough, vec!["--no-json", "payload"]);
    }

    #[test]
    fn missing_required_value_is_an_error() {
        let error = parse_shared_argv(["--log-level"]).unwrap_err();
        assert_eq!(
            error,
            SharedArgError::MissingValue {
                flag: "--log-level"
            }
        );
    }

    #[test]
    fn unknown_shared_value_is_an_error() {
        let error = parse_shared_argv(["--log-level=verbose"]).unwrap_err();
        assert!(matches!(error, SharedArgError::InvalidValue { .. }));
    }

    #[test]
    fn defaults_remain_automatic_and_info() {
        let parsed = parse_shared_argv(["status"]).unwrap();
        assert_eq!(parsed.policy, CliPolicy::default());
        assert_eq!(parsed.passthrough, vec!["status"]);
    }
}
