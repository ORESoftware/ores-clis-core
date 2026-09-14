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

pub use args::{parse_shared_argv, ParsedSharedArgs, SharedArgError};
pub use protocol::{
    top_level_io, EmitDisposition, FlushPolicy, ProtocolEmitter, StreamRole,
};
