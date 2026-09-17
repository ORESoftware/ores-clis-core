use crate::TerminalState;
use std::env;
use std::fmt;
use std::io::{self, Read, Write};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Environment switch read by [`setup_signal_handlers`].
///
/// Accepted enabled values are `1`, `true`, `yes`, and `on`. Accepted disabled
/// values are `0`, `false`, `no`, and `off`. The setup function is enabled by
/// default when this variable is absent, but no signal behavior changes unless
/// the setup function is actually called by the consumer.
pub const SIGNAL_HANDLERS_ENV: &str = "ORES_CLIS_SIGNAL_HANDLERS";

/// Optional environment override for the TTY requirement used by SIGINT.
///
/// Accepted values are `stdin`, `stdin+stdout`, `stdin+stderr`, and `all`.
/// Every mode deliberately requires stdin because Ctrl-D is an stdin EOF
/// gesture and must never be advertised when stdin itself is redirected.
pub const SIGNAL_TTY_REQUIREMENT_ENV: &str = "ORES_CLIS_SIGNAL_TTY_REQUIREMENT";

static SIGNAL_HANDLERS_INSTALLED: AtomicBool = AtomicBool::new(false);

type ShutdownCallback = Arc<dyn Fn(ShutdownReason) + Send + Sync + 'static>;
type LifecycleCallback = Arc<dyn Fn(ShutdownAction) + Send + Sync + 'static>;

#[derive(Clone)]
enum HandlerCallback {
    Legacy(ShutdownCallback),
    Lifecycle(LifecycleCallback),
}

#[derive(Default)]
struct HandlerState {
    ctrl_d_armed: AtomicBool,
    shutdown_started: AtomicBool,
    drain_started: AtomicBool,
    force_started: AtomicBool,
}

/// Additional TTYs that may be required before SIGINT enters interactive mode.
///
/// stdin is always required. stdout/stderr can be added when a CLI wants the
/// Ctrl-D confirmation behavior only while those streams are terminal-backed
/// too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TtyRequirement {
    /// stdin must be a TTY. This is the default and is normally the right choice.
    #[default]
    Stdin,
    /// stdin and stdout must both be TTYs.
    StdinAndStdout,
    /// stdin and stderr must both be TTYs.
    StdinAndStderr,
    /// stdin, stdout, and stderr must all be TTYs.
    All,
}

impl TtyRequirement {
    fn is_satisfied(self, terminals: TerminalState) -> bool {
        match self {
            Self::Stdin => terminals.stdin_tty,
            Self::StdinAndStdout => terminals.stdin_tty && terminals.stdout_tty,
            Self::StdinAndStderr => terminals.stdin_tty && terminals.stderr_tty,
            Self::All => terminals.stdin_tty && terminals.stdout_tty && terminals.stderr_tty,
        }
    }
}

impl FromStr for TtyRequirement {
    type Err = SignalHandlerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "stdin" => Ok(Self::Stdin),
            "stdin+stdout" => Ok(Self::StdinAndStdout),
            "stdin+stderr" => Ok(Self::StdinAndStderr),
            "all" | "stdin+stdout+stderr" => Ok(Self::All),
            _ => Err(SignalHandlerError::InvalidEnvironment {
                variable: SIGNAL_TTY_REQUIREMENT_ENV,
                value: value.to_owned(),
                expected: "stdin, stdin+stdout, stdin+stderr, or all",
            }),
        }
    }
}

/// Reason associated with a signal/terminal shutdown event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// SIGINT, or the platform's Ctrl-C equivalent, requested shutdown.
    SigInt,
    /// SIGTERM requested shutdown.
    SigTerm,
    /// Ctrl-D/EOF on stdin confirmed or forced an interactive shutdown.
    CtrlD,
}

impl ShutdownReason {
    /// Conventional process exit code used by [`setup_signal_handlers`].
    ///
    /// Interactive Ctrl-D is treated as an intentional clean shutdown; direct
    /// SIGINT and SIGTERM use the conventional `128 + signal` Unix codes.
    #[must_use]
    pub const fn conventional_exit_code(self) -> i32 {
        match self {
            Self::SigInt => 130,
            Self::SigTerm => 143,
            Self::CtrlD => 0,
        }
    }
}

/// Two-phase process lifecycle event for graceful servers and long-running CLIs.
///
/// `Drain` means stop accepting new work and begin graceful cleanup. `Force`
/// means the operator explicitly requested escalation after draining started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownAction {
    /// Begin graceful draining. This event is emitted at most once.
    Drain(ShutdownReason),
    /// Escalate an already-started drain. This event is emitted at most once.
    Force(ShutdownReason),
}

impl ShutdownAction {
    /// Underlying operator/platform reason.
    #[must_use]
    pub const fn reason(self) -> ShutdownReason {
        match self {
            Self::Drain(reason) | Self::Force(reason) => reason,
        }
    }

    /// Whether this action requests force escalation rather than graceful drain.
    #[must_use]
    pub const fn is_force(self) -> bool {
        matches!(self, Self::Force(_))
    }
}

/// Snapshot and switches used when signal handling is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalHandlerOptions {
    enabled: bool,
    tty_requirement: TtyRequirement,
    terminals: TerminalState,
}

impl SignalHandlerOptions {
    /// Construct enabled options from an already-captured terminal snapshot.
    #[must_use]
    pub const fn new(terminals: TerminalState) -> Self {
        Self {
            enabled: true,
            tty_requirement: TtyRequirement::Stdin,
            terminals,
        }
    }

    /// Enable or disable installation explicitly.
    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Require additional TTY-backed streams before SIGINT becomes interactive.
    #[must_use]
    pub const fn with_tty_requirement(mut self, tty_requirement: TtyRequirement) -> Self {
        self.tty_requirement = tty_requirement;
        self
    }

    /// Build options from ambient stdio and the ORES signal environment switches.
    ///
    /// `ORES_CLIS_SIGNAL_HANDLERS` controls whether a call installs anything.
    /// `ORES_CLIS_SIGNAL_TTY_REQUIREMENT` optionally tightens the default stdin
    /// requirement.
    pub fn from_env() -> Result<Self, SignalHandlerError> {
        let mut options = Self::default();

        if let Some(value) = read_env(SIGNAL_HANDLERS_ENV)? {
            options.enabled = parse_enabled(value.as_str())?;
        }

        if let Some(value) = read_env(SIGNAL_TTY_REQUIREMENT_ENV)? {
            options.tty_requirement = value.parse()?;
        }

        Ok(options)
    }

    /// Return whether SIGINT should use Ctrl-D confirmation/force for this snapshot.
    #[must_use]
    pub fn interactive_sigint(self) -> bool {
        self.tty_requirement.is_satisfied(self.terminals)
    }
}

impl Default for SignalHandlerOptions {
    fn default() -> Self {
        Self::new(TerminalState::detect())
    }
}

/// Result of attempting to install the shared signal policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalHandlerStatus {
    /// Handlers were installed for this process.
    Installed,
    /// Installation was disabled by configuration/environment.
    Disabled,
    /// The shared handlers were already installed by an earlier call.
    AlreadyInstalled,
}

/// Failure while parsing configuration or installing platform signal handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalHandlerError {
    /// An ORES signal environment variable had an unsupported value.
    InvalidEnvironment {
        /// Environment variable name.
        variable: &'static str,
        /// Rejected value.
        value: String,
        /// Human-readable accepted-value description.
        expected: &'static str,
    },
    /// The operating-system signal handler or its worker thread could not start.
    Install {
        /// Platform/runtime error description.
        message: String,
    },
    /// The target platform is neither Unix nor Windows.
    UnsupportedPlatform,
}

impl fmt::Display for SignalHandlerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEnvironment {
                variable,
                value,
                expected,
            } => write!(
                formatter,
                "invalid {variable} value {value:?}; expected {expected}"
            ),
            Self::Install { message } => {
                write!(formatter, "failed to install signal handlers: {message}")
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("signal handlers are unsupported on this target platform")
            }
        }
    }
}

impl std::error::Error for SignalHandlerError {}

/// Install the default ORES CLI signal policy.
///
/// This is the backward-compatible one-shot policy. Importing the crate does not
/// change signal behavior. When explicitly installed, interactive SIGINT warns
/// and arms Ctrl-D without exiting; Ctrl-D performs the one shutdown callback.
/// Non-interactive SIGINT and SIGTERM shut down directly.
pub fn setup_signal_handlers() -> Result<SignalHandlerStatus, SignalHandlerError> {
    let options = SignalHandlerOptions::from_env()?;
    setup_signal_handlers_with(options, |reason| {
        std::process::exit(reason.conventional_exit_code());
    })
}

/// Install the backward-compatible one-shot signal policy with a callback.
///
/// The callback is invoked at most once. Interactive SIGINT is warning-only and
/// Ctrl-D/EOF performs the callback, preserving the original CLI behavior.
/// Graceful servers should use [`setup_signal_handlers_with_lifecycle`] instead.
pub fn setup_signal_handlers_with<F>(
    options: SignalHandlerOptions,
    on_shutdown: F,
) -> Result<SignalHandlerStatus, SignalHandlerError>
where
    F: Fn(ShutdownReason) + Send + Sync + 'static,
{
    install_handlers(options, HandlerCallback::Legacy(Arc::new(on_shutdown)))
}

/// Install the two-phase graceful lifecycle policy with a consumer-owned callback.
///
/// Semantics:
/// - interactive first SIGINT emits `Drain(SigInt)`, logs that Ctrl-D forces,
///   and arms exactly one stdin Ctrl-D/EOF waiter;
/// - repeated interactive SIGINT never escalates a drain;
/// - Ctrl-D/EOF after that drain emits `Force(CtrlD)` at most once;
/// - non-interactive SIGINT emits `Drain(SigInt)` directly;
/// - SIGTERM emits `Drain(SigTerm)` on Unix and never implicitly forces;
/// - failure to attach the Ctrl-D waiter leaves the already-started drain intact
///   rather than converting an implementation failure into force termination.
///
/// This API is the intended bridge to HTTP/server drain coordinators such as
/// `ores-middleware::ShutdownCoordinator`.
pub fn setup_signal_handlers_with_lifecycle<F>(
    options: SignalHandlerOptions,
    on_action: F,
) -> Result<SignalHandlerStatus, SignalHandlerError>
where
    F: Fn(ShutdownAction) + Send + Sync + 'static,
{
    install_handlers(options, HandlerCallback::Lifecycle(Arc::new(on_action)))
}

fn install_handlers(
    options: SignalHandlerOptions,
    callback: HandlerCallback,
) -> Result<SignalHandlerStatus, SignalHandlerError> {
    if !options.enabled {
        return Ok(SignalHandlerStatus::Disabled);
    }

    if SIGNAL_HANDLERS_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(SignalHandlerStatus::AlreadyInstalled);
    }

    if let Err(error) = install_platform_handler(options.interactive_sigint(), callback) {
        SIGNAL_HANDLERS_INSTALLED.store(false, Ordering::Release);
        return Err(error);
    }

    Ok(SignalHandlerStatus::Installed)
}

fn read_env(name: &'static str) -> Result<Option<String>, SignalHandlerError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(value)) => Err(SignalHandlerError::InvalidEnvironment {
            variable: name,
            value: value.to_string_lossy().into_owned(),
            expected: "valid UTF-8 configuration",
        }),
    }
}

fn parse_enabled(value: &str) -> Result<bool, SignalHandlerError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(SignalHandlerError::InvalidEnvironment {
            variable: SIGNAL_HANDLERS_ENV,
            value: value.to_owned(),
            expected: "1/true/yes/on or 0/false/no/off",
        }),
    }
}

fn log_signal_message(message: &str) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "[ores-clis-core] {message}");
    let _ = stderr.flush();
}

fn request_legacy_shutdown(
    state: &HandlerState,
    callback: &ShutdownCallback,
    reason: ShutdownReason,
) -> bool {
    if state
        .shutdown_started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        callback(reason);
        true
    } else {
        false
    }
}

fn request_drain(
    state: &HandlerState,
    callback: &LifecycleCallback,
    reason: ShutdownReason,
) -> bool {
    if state
        .drain_started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        callback(ShutdownAction::Drain(reason));
        true
    } else {
        false
    }
}

fn request_force(
    state: &HandlerState,
    callback: &LifecycleCallback,
    reason: ShutdownReason,
) -> bool {
    if !state.drain_started.load(Ordering::Acquire) {
        return false;
    }
    if state
        .force_started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        callback(ShutdownAction::Force(reason));
        true
    } else {
        false
    }
}

fn handle_sigint(interactive: bool, state: &Arc<HandlerState>, callback: &HandlerCallback) {
    match callback {
        HandlerCallback::Legacy(callback) => {
            if !interactive {
                log_signal_message("SIGINT received; shutting down process.");
                request_legacy_shutdown(state, callback, ShutdownReason::SigInt);
                return;
            }

            log_signal_message("SIGINT received; use Ctrl-D to shutdown process.");
            arm_ctrl_d_waiter(state, HandlerCallback::Legacy(Arc::clone(callback)));
        }
        HandlerCallback::Lifecycle(callback) => {
            if !interactive {
                log_signal_message("SIGINT received; beginning graceful shutdown.");
                request_drain(state, callback, ShutdownReason::SigInt);
                return;
            }

            let started = request_drain(state, callback, ShutdownReason::SigInt);
            if started {
                log_signal_message(
                    "SIGINT received; beginning graceful shutdown; use Ctrl-D to force shutdown.",
                );
            } else {
                log_signal_message("shutdown already draining; use Ctrl-D to force shutdown.");
            }
            arm_ctrl_d_waiter(state, HandlerCallback::Lifecycle(Arc::clone(callback)));
        }
    }
}

fn arm_ctrl_d_waiter(state: &Arc<HandlerState>, callback: HandlerCallback) {
    if state
        .ctrl_d_armed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    if let Err(error) = spawn_ctrl_d_waiter(callback.clone(), Arc::clone(state)) {
        match callback {
            HandlerCallback::Legacy(callback) => {
                log_signal_message(&format!(
                    "could not attach Ctrl-D shutdown waiter ({error}); shutting down process."
                ));
                request_legacy_shutdown(state, &callback, ShutdownReason::SigInt);
            }
            HandlerCallback::Lifecycle(_) => {
                log_signal_message(&format!(
                    "could not attach Ctrl-D force waiter ({error}); graceful drain remains active."
                ));
            }
        }
    }
}

#[cfg(unix)]
fn handle_sigterm(state: &Arc<HandlerState>, callback: &HandlerCallback) {
    match callback {
        HandlerCallback::Legacy(callback) => {
            log_signal_message("SIGTERM received; shutting down process.");
            request_legacy_shutdown(state, callback, ShutdownReason::SigTerm);
        }
        HandlerCallback::Lifecycle(callback) => {
            log_signal_message("SIGTERM received; beginning graceful shutdown.");
            request_drain(state, callback, ShutdownReason::SigTerm);
        }
    }
}

fn spawn_ctrl_d_waiter(callback: HandlerCallback, state: Arc<HandlerState>) -> io::Result<()> {
    thread::Builder::new()
        .name("ores-cli-ctrl-d".to_owned())
        .spawn(move || {
            let stdin = io::stdin();
            let mut input = stdin.lock();
            let mut byte = [0_u8; 1];

            loop {
                match input.read(&mut byte) {
                    Ok(0) => {
                        handle_ctrl_d(&state, &callback, true);
                        return;
                    }
                    Ok(_) if byte[0] == 0x04 => {
                        handle_ctrl_d(&state, &callback, false);
                        return;
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        match &callback {
                            HandlerCallback::Legacy(callback) => {
                                log_signal_message(&format!(
                                    "stdin failed while waiting for Ctrl-D ({error}); shutting down process."
                                ));
                                request_legacy_shutdown(&state, callback, ShutdownReason::SigInt);
                            }
                            HandlerCallback::Lifecycle(_) => {
                                log_signal_message(&format!(
                                    "stdin failed while waiting for Ctrl-D ({error}); graceful drain remains active."
                                ));
                            }
                        }
                        return;
                    }
                }
            }
        })
        .map(|_| ())
}

fn handle_ctrl_d(state: &HandlerState, callback: &HandlerCallback, eof: bool) {
    match callback {
        HandlerCallback::Legacy(callback) => {
            log_signal_message(if eof {
                "Ctrl-D/EOF received; shutting down process."
            } else {
                "Ctrl-D received; shutting down process."
            });
            request_legacy_shutdown(state, callback, ShutdownReason::CtrlD);
        }
        HandlerCallback::Lifecycle(callback) => {
            log_signal_message(if eof {
                "Ctrl-D/EOF received; forcing shutdown process."
            } else {
                "Ctrl-D received; forcing shutdown process."
            });
            request_force(state, callback, ShutdownReason::CtrlD);
        }
    }
}

#[cfg(unix)]
fn install_platform_handler(
    interactive: bool,
    callback: HandlerCallback,
) -> Result<(), SignalHandlerError> {
    use signal_hook::consts::signal::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals =
        Signals::new([SIGINT, SIGTERM]).map_err(|error| SignalHandlerError::Install {
            message: error.to_string(),
        })?;
    let state = Arc::new(HandlerState::default());

    thread::Builder::new()
        .name("ores-cli-signals".to_owned())
        .spawn(move || {
            for signal in signals.forever() {
                match signal {
                    SIGINT => handle_sigint(interactive, &state, &callback),
                    SIGTERM => handle_sigterm(&state, &callback),
                    _ => {}
                }
            }
        })
        .map(|_| ())
        .map_err(|error| SignalHandlerError::Install {
            message: error.to_string(),
        })
}

#[cfg(windows)]
fn install_platform_handler(
    interactive: bool,
    callback: HandlerCallback,
) -> Result<(), SignalHandlerError> {
    let state = Arc::new(HandlerState::default());

    ctrlc::try_set_handler(move || {
        handle_sigint(interactive, &state, &callback);
    })
    .map_err(|error| SignalHandlerError::Install {
        message: error.to_string(),
    })
}

#[cfg(not(any(unix, windows)))]
fn install_platform_handler(
    _interactive: bool,
    _callback: HandlerCallback,
) -> Result<(), SignalHandlerError> {
    Err(SignalHandlerError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn default_interactive_policy_is_driven_by_stdin() {
        let options = SignalHandlerOptions::new(TerminalState::new(true, false, false));
        assert!(options.interactive_sigint());
    }

    #[test]
    fn ctrl_d_is_never_advertised_without_tty_stdin() {
        let terminals = TerminalState::new(false, true, true);
        for requirement in [
            TtyRequirement::Stdin,
            TtyRequirement::StdinAndStdout,
            TtyRequirement::StdinAndStderr,
            TtyRequirement::All,
        ] {
            assert!(
                !SignalHandlerOptions::new(terminals)
                    .with_tty_requirement(requirement)
                    .interactive_sigint()
            );
        }
    }

    #[test]
    fn stdout_and_stderr_can_tighten_interactive_policy() {
        let terminals = TerminalState::new(true, false, true);
        assert!(
            !SignalHandlerOptions::new(terminals)
                .with_tty_requirement(TtyRequirement::StdinAndStdout)
                .interactive_sigint()
        );
        assert!(
            SignalHandlerOptions::new(terminals)
                .with_tty_requirement(TtyRequirement::StdinAndStderr)
                .interactive_sigint()
        );
    }

    #[test]
    fn enabled_switch_accepts_common_boolean_spellings_and_whitespace() {
        for enabled in ["1", "true", "yes", "on", " TRUE "] {
            assert!(parse_enabled(enabled).expect("enabled value"));
        }
        for disabled in ["0", "false", "no", "off", " OFF "] {
            assert!(!parse_enabled(disabled).expect("disabled value"));
        }
        assert!(parse_enabled("maybe").is_err());
    }

    #[test]
    fn tty_requirement_parser_preserves_stdin_requirement() {
        assert_eq!(
            "stdin".parse::<TtyRequirement>().expect("stdin"),
            TtyRequirement::Stdin
        );
        assert_eq!(
            "stdin+stdout".parse::<TtyRequirement>().expect("stdout"),
            TtyRequirement::StdinAndStdout
        );
        assert_eq!(
            "stdin+stderr".parse::<TtyRequirement>().expect("stderr"),
            TtyRequirement::StdinAndStderr
        );
        assert_eq!(
            "all".parse::<TtyRequirement>().expect("all"),
            TtyRequirement::All
        );
        assert_eq!(
            " stdin+stdout+stderr "
                .parse::<TtyRequirement>()
                .expect("all alias"),
            TtyRequirement::All
        );
        assert!("stdout".parse::<TtyRequirement>().is_err());
    }

    #[test]
    fn shutdown_action_exposes_reason_and_force_semantics() {
        let drain = ShutdownAction::Drain(ShutdownReason::SigInt);
        assert_eq!(drain.reason(), ShutdownReason::SigInt);
        assert!(!drain.is_force());
        let force = ShutdownAction::Force(ShutdownReason::CtrlD);
        assert_eq!(force.reason(), ShutdownReason::CtrlD);
        assert!(force.is_force());
    }

    #[test]
    fn lifecycle_drain_is_emitted_once() {
        let state = HandlerState::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let callback: LifecycleCallback = Arc::new(move |action| {
            captured.lock().expect("events lock").push(action);
        });

        assert!(request_drain(&state, &callback, ShutdownReason::SigInt));
        assert!(!request_drain(&state, &callback, ShutdownReason::SigTerm));
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![ShutdownAction::Drain(ShutdownReason::SigInt)]
        );
    }

    #[test]
    fn lifecycle_force_requires_prior_drain_and_is_emitted_once() {
        let state = HandlerState::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let callback: LifecycleCallback = Arc::new(move |action| {
            captured.lock().expect("events lock").push(action);
        });

        assert!(!request_force(&state, &callback, ShutdownReason::CtrlD));
        assert!(request_drain(&state, &callback, ShutdownReason::SigInt));
        assert!(request_force(&state, &callback, ShutdownReason::CtrlD));
        assert!(!request_force(&state, &callback, ShutdownReason::CtrlD));
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                ShutdownAction::Drain(ShutdownReason::SigInt),
                ShutdownAction::Force(ShutdownReason::CtrlD),
            ]
        );
    }

    #[test]
    fn noninteractive_lifecycle_sigint_starts_drain_without_force() {
        let state = Arc::new(HandlerState::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let callback = HandlerCallback::Lifecycle(Arc::new(move |action| {
            captured.lock().expect("events lock").push(action);
        }));

        handle_sigint(false, &state, &callback);
        assert!(state.drain_started.load(Ordering::Acquire));
        assert!(!state.force_started.load(Ordering::Acquire));
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![ShutdownAction::Drain(ShutdownReason::SigInt)]
        );
    }

    #[test]
    fn ctrl_d_force_path_does_not_repeat() {
        let state = HandlerState::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let callback = HandlerCallback::Lifecycle(Arc::new(move |action| {
            captured.lock().expect("events lock").push(action);
        }));
        let lifecycle = match &callback {
            HandlerCallback::Lifecycle(callback) => callback,
            HandlerCallback::Legacy(_) => unreachable!(),
        };

        request_drain(&state, lifecycle, ShutdownReason::SigInt);
        handle_ctrl_d(&state, &callback, false);
        handle_ctrl_d(&state, &callback, true);
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                ShutdownAction::Drain(ShutdownReason::SigInt),
                ShutdownAction::Force(ShutdownReason::CtrlD),
            ]
        );
    }

    #[test]
    fn legacy_shutdown_callback_remains_one_shot() {
        let state = HandlerState::default();
        let reasons = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&reasons);
        let callback: ShutdownCallback = Arc::new(move |reason| {
            captured.lock().expect("reasons lock").push(reason);
        });

        assert!(request_legacy_shutdown(
            &state,
            &callback,
            ShutdownReason::SigInt
        ));
        assert!(!request_legacy_shutdown(
            &state,
            &callback,
            ShutdownReason::CtrlD
        ));
        assert_eq!(
            *reasons.lock().expect("reasons lock"),
            vec![ShutdownReason::SigInt]
        );
    }
}
