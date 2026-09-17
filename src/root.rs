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
mod protocol;
mod shutdown;

pub use args::{ParsedSharedArgs, SharedArgError, parse_shared_argv};
pub use protocol::{EmitDisposition, FlushPolicy, ProtocolEmitter, StreamRole, top_level_io};
pub use shutdown::{
    SIGNAL_HANDLERS_ENV, SIGNAL_TTY_REQUIREMENT_ENV, ShutdownReason, SignalHandlerError,
    SignalHandlerOptions, SignalHandlerStatus, TtyRequirement, setup_signal_handlers,
    setup_signal_handlers_with,
};
