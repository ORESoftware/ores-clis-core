#![forbid(unsafe_code)]
//! Shared runtime policy for ORESoftware Rust command-line tools.
//!
//! The original runtime primitives remain isolated in `runtime`; parser and
//! protocol hardening live in small companion modules so consumers can adopt
//! the shared behavior without taking a dependency on a specific CLI parser or
//! logging backend.

#[path = "lib.rs"]
mod runtime;

pub use runtime::*;

mod args;
mod lifecycle;
mod protocol;
mod reserved;
#[path = "runtime_config_registry.rs"]
mod root_config_registry;
mod shutdown;

pub use args::{ParsedSharedArgs, SharedArgError, parse_shared_argv};
pub use lifecycle::{ShutdownLifecycleGate, ShutdownLifecyclePhase};
pub use protocol::{EmitDisposition, FlushPolicy, ProtocolEmitter, StreamRole, top_level_io};
pub use reserved::parse_shared_argv_with_reserved;
pub use root_config_registry::{
    CLI_FLAGS_CONFIG, OPTO_SYNC_CONFIG, ORES_LRU_CONFIG, ORES_MW_CONFIG, ORES_OTEL_CONFIG,
    ORES_RL_CONFIG, REGISTERED_RUNTIME_CONFIGS, ROOT_CONFIG_IDENTITIES, RootConfigIdentity,
    SHARED_AUTH_COMPAT_CONFIG, SHARED_AUTH_CONFIG, is_registered_runtime_config,
    root_config_identity,
};
pub use shutdown::{
    SIGNAL_HANDLERS_ENV, SIGNAL_TTY_REQUIREMENT_ENV, ShutdownAction, ShutdownReason,
    SignalHandlerError, SignalHandlerOptions, SignalHandlerStatus, TtyRequirement,
    setup_signal_handlers, setup_signal_handlers_with, setup_signal_handlers_with_lifecycle,
};
