//! Postgres wire protocol codec, both directions.
//!
//! Sans-I/O: decoding is a pure function of the bytes that have arrived, so a
//! byte sequence captured from a trace becomes a unit test directly, with no
//! runtime and no Postgres.
//!
//! # Two rules that shape everything here
//!
//! **Never parse `DataRow`.** Result rows are forwarded as opaque frames.
//! Parsing them is the difference between a proxy and a bottleneck.
//!
//! **Validate length before allocating.** This code reads bytes sent by anyone
//! who can reach the listener, so a declared length is untrusted until checked
//! against a maximum. Nothing here allocates at all: frames borrow from the
//! caller's buffer.

pub mod backend;
pub mod encode;
pub mod frame;
pub mod frontend;
pub mod read;
pub mod session;
pub mod startup;

pub use backend::{BackendMessage, TxStatus};
pub use frame::{
    DEFAULT_MAX_FRAME, DEFAULT_MAX_INSPECT, DecodeError, Decoded, Direction, Frame, FrameHeader,
    Inspect, Tag, decode_header, inspect_policy,
};
pub use frontend::{FrontendMessage, Target};
pub use read::{FieldError, Reader};
pub use session::{CopyDirection, HoldReason, SessionState};
pub use startup::{Startup, VersionResponse, negotiate_version};
