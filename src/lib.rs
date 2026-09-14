#![forbid(unsafe_code)]
//! Shared runtime policy for ORESoftware Rust command-line tools.
//!
//! This crate intentionally separates four concerns that are often conflated:
//! terminal detection, output format, color policy, and log filtering. Primary
//! command results are never suppressed by the log level; log/progress streams
//! are filtered independently.

use std::env;
use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::str::FromStr;

/// Desired color behavior for human-readable streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Enable color only for terminal streams, unless `NO_COLOR` is present.
    #[default]
    Auto,
    /// Force color for human-readable output, even when redirected.
    Always,
    /// Never emit ANSI color sequences.
    Never,
}

impl FromStr for ColorMode {
    type Err = ParsePolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "always" | "on" | "true" | "yes" | "1" => Ok(Self::Always),
            "never" | "off" | "false" | "no" | "0" => Ok(Self::Never),
            _ => Err(ParsePolicyError::new(
                "color",
                value,
                "auto, always, or never",
            )),
        }
    }
}

/// Desired primary output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Human on a terminal; JSON when stdout is redirected or piped.
    #[default]
    Auto,
    /// Force human-readable output.
    Human,
    /// Force newline-delimited JSON/structured output.
    Json,
}

impl FromStr for OutputMode {
    type Err = ParsePolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "human" | "plain" | "text" => Ok(Self::Human),
            "json" | "ndjson" | "structured" => Ok(Self::Json),
            _ => Err(ParsePolicyError::new(
                "output",
                value,
                "auto, human, or json",
            )),
        }
    }
}

/// Shared log threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum LogLevel {
    /// Suppress all log/progress records.
    Silent,
    /// Emit only errors. Kept as a distinct user-facing compatibility level.
    Quiet,
    /// Emit errors only.
    Error,
    /// Emit warnings and errors.
    Warn,
    /// Emit normal lifecycle messages, warnings, and errors.
    #[default]
    Info,
    /// Emit debug diagnostics too.
    Debug,
    /// Emit every record, including high-volume trace diagnostics.
    Trace,
}

impl LogLevel {
    /// Returns whether a record at `record_level` should be emitted.
    #[must_use]
    pub const fn allows(self, record_level: Self) -> bool {
        match self {
            Self::Silent => false,
            Self::Quiet | Self::Error => matches!(record_level, Self::Error),
            Self::Warn => matches!(record_level, Self::Error | Self::Warn),
            Self::Info => matches!(record_level, Self::Error | Self::Warn | Self::Info),
            Self::Debug => !matches!(record_level, Self::Silent | Self::Quiet | Self::Trace),
            Self::Trace => !matches!(record_level, Self::Silent | Self::Quiet),
        }
    }

    /// Canonical lowercase wire/display spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Quiet => "quiet",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

impl FromStr for LogLevel {
    type Err = ParsePolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "silent" | "off" | "none" => Ok(Self::Silent),
            "quiet" => Ok(Self::Quiet),
            "error" | "err" => Ok(Self::Error),
            "warn" | "warning" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            _ => Err(ParsePolicyError::new(
                "log-level",
                value,
                "silent, quiet, error, warn, info, debug, or trace",
            )),
        }
    }
}

/// Semantic terminal color roles shared across ORESoftware CLIs.
///
/// Consumers choose a role rather than a literal ANSI sequence so the palette
/// can evolve centrally without changing command semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    /// Successful/healthy state.
    Success,
    /// Warning or degraded state.
    Warning,
    /// Error or failed state.
    Error,
    /// Informational labels and values.
    Info,
    /// Debug diagnostics.
    Debug,
    /// High-volume trace diagnostics.
    Trace,
    /// Headings and important labels.
    Emphasis,
    /// Secondary/de-emphasized text.
    Muted,
}

impl ColorRole {
    const fn ansi_prefix(self) -> &'static str {
        match self {
            Self::Success => "\u{1b}[32m",
            Self::Warning => "\u{1b}[33m",
            Self::Error => "\u{1b}[31m",
            Self::Info => "\u{1b}[36m",
            Self::Debug => "\u{1b}[35m",
            Self::Trace => "\u{1b}[2m",
            Self::Emphasis => "\u{1b}[1m",
            Self::Muted => "\u{1b}[2m",
        }
    }
}

/// Render a semantic role with ANSI escapes when color is enabled.
///
/// This intentionally owns only styling, not output routing. Callers decide
/// whether to pass `RuntimePolicy::color_stdout()` or `color_stderr()`.
#[must_use]
pub fn paint(enabled: bool, role: ColorRole, value: impl fmt::Display) -> String {
    if enabled {
        format!("{}{}\u{1b}[0m", role.ansi_prefix(), value)
    } else {
        value.to_string()
    }
}

/// Immutable terminal capabilities captured once at process startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalState {
    /// Whether stdin is attached to a terminal.
    pub stdin_tty: bool,
    /// Whether stdout is attached to a terminal.
    pub stdout_tty: bool,
    /// Whether stderr is attached to a terminal.
    pub stderr_tty: bool,
}

impl TerminalState {
    /// Detect terminal state using stable `std::io::IsTerminal`.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            stdin_tty: io::stdin().is_terminal(),
            stdout_tty: io::stdout().is_terminal(),
            stderr_tty: io::stderr().is_terminal(),
        }
    }
}

/// Relevant environment hints for cross-tool CLI behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnvironmentHints {
    /// `NO_COLOR` is present in the process environment.
    pub no_color: bool,
    /// `CLICOLOR_FORCE` or `FORCE_COLOR` requests color when nonzero/nonempty.
    pub force_color: bool,
}

impl EnvironmentHints {
    /// Read conventional color environment variables.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            no_color: env::var_os("NO_COLOR").is_some(),
            force_color: env_truthy("CLICOLOR_FORCE") || env_truthy("FORCE_COLOR"),
        }
    }
}

fn env_truthy(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            !value.is_empty() && !matches!(value.as_str(), "0" | "false" | "no" | "off")
        })
        .unwrap_or(false)
}

/// Parser-agnostic user preferences. `--color` maps to `Always`, `--no-color`
/// maps to `Never`, `--json` maps to `Json`, and `--no-json` maps to `Human`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CliPolicy {
    /// Requested output format.
    pub output: OutputMode,
    /// Requested color policy.
    pub color: ColorMode,
    /// Requested log threshold.
    pub log_level: LogLevel,
}

impl CliPolicy {
    /// Resolve parser-level preferences against terminals and environment.
    #[must_use]
    pub fn resolve(self, terminals: TerminalState, env: EnvironmentHints) -> RuntimePolicy {
        let output = match self.output {
            OutputMode::Auto if terminals.stdout_tty => OutputMode::Human,
            OutputMode::Auto => OutputMode::Json,
            explicit => explicit,
        };

        RuntimePolicy {
            output,
            color: self.color,
            log_level: self.log_level,
            terminals,
            env,
        }
    }
}

/// Fully resolved process runtime policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePolicy {
    output: OutputMode,
    color: ColorMode,
    log_level: LogLevel,
    terminals: TerminalState,
    env: EnvironmentHints,
}

impl RuntimePolicy {
    /// Resolved primary output mode (`Human` or `Json`; never `Auto`).
    #[must_use]
    pub const fn output_mode(self) -> OutputMode {
        self.output
    }

    /// Resolved log level.
    #[must_use]
    pub const fn log_level(self) -> LogLevel {
        self.log_level
    }

    /// Whether primary stdout should be structured JSON/NDJSON.
    #[must_use]
    pub const fn json(self) -> bool {
        matches!(self.output, OutputMode::Json)
    }

    /// Whether color is allowed on primary stdout.
    #[must_use]
    pub fn color_stdout(self) -> bool {
        // Structured stdout is a wire format. Never inject ANSI escapes into it,
        // even if the caller passed --color/--color=always.
        !self.json() && self.color_for_stream(self.terminals.stdout_tty)
    }

    /// Whether color is allowed on stderr diagnostics.
    ///
    /// Stderr is intentionally independent from stdout's structured output
    /// mode, so `tool | jq` can retain colored diagnostics on an attached
    /// terminal without corrupting the JSON pipe.
    #[must_use]
    pub fn color_stderr(self) -> bool {
        self.color_for_stream(self.terminals.stderr_tty)
    }

    /// Whether a log/progress record should be emitted.
    #[must_use]
    pub const fn allows_log(self, level: LogLevel) -> bool {
        self.log_level.allows(level)
    }

    fn color_for_stream(self, stream_tty: bool) -> bool {
        match self.color {
            ColorMode::Never => false,
            ColorMode::Always => true,
            ColorMode::Auto if self.env.no_color => false,
            ColorMode::Auto if self.env.force_color => true,
            ColorMode::Auto => stream_tty,
        }
    }
}

/// Streaming line emitter that flushes each record by default.
#[derive(Debug)]
pub struct StreamEmitter<W> {
    writer: W,
    flush_each_record: bool,
}

impl<W: Write> StreamEmitter<W> {
    /// Create a line emitter suitable for progress and NDJSON streams.
    #[must_use]
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            flush_each_record: true,
        }
    }

    /// Configure whether every emitted line is flushed immediately.
    #[must_use]
    pub const fn with_flush_each_record(mut self, enabled: bool) -> Self {
        self.flush_each_record = enabled;
        self
    }

    /// Write one human-readable record and terminate it with `\n`.
    pub fn emit_line(&mut self, value: &str) -> io::Result<()> {
        self.write_line(value)
    }

    /// Write one JSON/NDJSON record, rejecting ANSI escapes to keep the wire
    /// stream machine-safe.
    pub fn emit_json_line(&mut self, value: &str) -> io::Result<()> {
        if value.contains('\u{1b}') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "JSON output must not contain ANSI escape sequences",
            ));
        }
        self.write_line(value)
    }

    /// Flush the underlying writer.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    /// Recover the wrapped writer.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer
    }

    fn write_line(&mut self, value: &str) -> io::Result<()> {
        self.writer.write_all(value.as_bytes())?;
        self.writer.write_all(b"\n")?;
        if self.flush_each_record {
            self.writer.flush()?;
        }
        Ok(())
    }
}

/// Error returned by a shared policy value parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePolicyError {
    field: &'static str,
    value: String,
    expected: &'static str,
}

impl ParsePolicyError {
    fn new(field: &'static str, value: &str, expected: &'static str) -> Self {
        Self {
            field,
            value: value.to_owned(),
            expected,
        }
    }
}

impl fmt::Display for ParsePolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid --{} value `{}`; expected {}",
            self.field, self.value, self.expected
        )
    }
}

impl std::error::Error for ParsePolicyError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tty(stdout_tty: bool, stderr_tty: bool) -> TerminalState {
        TerminalState {
            stdin_tty: true,
            stdout_tty,
            stderr_tty,
        }
    }

    #[test]
    fn auto_output_is_human_on_tty_and_json_when_piped() {
        let policy = CliPolicy::default();
        assert_eq!(
            policy
                .resolve(tty(true, true), EnvironmentHints::default())
                .output_mode(),
            OutputMode::Human
        );
        assert_eq!(
            policy
                .resolve(tty(false, false), EnvironmentHints::default())
                .output_mode(),
            OutputMode::Json
        );
    }

    #[test]
    fn auto_color_follows_destination_stream_tty() {
        let runtime = CliPolicy::default().resolve(tty(true, false), EnvironmentHints::default());
        assert!(runtime.color_stdout());
        assert!(!runtime.color_stderr());
    }

    #[test]
    fn json_stdout_is_ansi_free_but_tty_stderr_can_stay_colored() {
        let runtime = CliPolicy {
            output: OutputMode::Json,
            color: ColorMode::Always,
            log_level: LogLevel::Trace,
        }
        .resolve(tty(false, true), EnvironmentHints::default());
        assert!(!runtime.color_stdout());
        assert!(runtime.color_stderr());
    }

    #[test]
    fn no_color_disables_auto_but_not_explicit_color() {
        let env = EnvironmentHints {
            no_color: true,
            force_color: false,
        };
        let automatic = CliPolicy::default().resolve(tty(true, true), env);
        assert!(!automatic.color_stdout());

        let explicit = CliPolicy {
            color: ColorMode::Always,
            ..CliPolicy::default()
        }
        .resolve(tty(true, true), env);
        assert!(explicit.color_stdout());
    }

    #[test]
    fn trace_and_compatibility_levels_parse() {
        assert_eq!("trace".parse::<LogLevel>().unwrap(), LogLevel::Trace);
        assert_eq!("warning".parse::<LogLevel>().unwrap(), LogLevel::Warn);
        assert_eq!("off".parse::<LogLevel>().unwrap(), LogLevel::Silent);
        assert!("verbose".parse::<LogLevel>().is_err());
    }

    #[test]
    fn quiet_and_error_allow_only_errors() {
        assert!(LogLevel::Quiet.allows(LogLevel::Error));
        assert!(!LogLevel::Quiet.allows(LogLevel::Warn));
        assert!(LogLevel::Error.allows(LogLevel::Error));
        assert!(!LogLevel::Error.allows(LogLevel::Warn));
    }

    #[test]
    fn semantic_palette_is_zero_cost_when_disabled() {
        assert_eq!(paint(false, ColorRole::Success, "ok"), "ok");
        assert_eq!(
            paint(true, ColorRole::Error, "boom"),
            "\u{1b}[31mboom\u{1b}[0m"
        );
    }

    #[test]
    fn json_emitter_rejects_ansi() {
        let mut emitter = StreamEmitter::new(Vec::<u8>::new());
        assert!(emitter.emit_json_line("{\"ok\":true}").is_ok());
        assert!(emitter.emit_json_line("\u{1b}[31m{\"ok\":false}").is_err());
    }
}
