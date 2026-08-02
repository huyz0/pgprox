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
//! against a maximum, and no buffer is ever sized from a number a peer sent.
//!
//! # What that does and does not promise
//!
//! Decoding does not allocate: a [`Frame`] borrows its body from the caller's
//! buffer and every accessor hands out a slice of it. That is the rule, and it
//! is what keeps `DataRow` off the heap.
//!
//! It is not the same as "nothing here allocates", which this said until
//! `M15.6` and which was never true. Four things allocate on purpose, each
//! bounded by something other than a peer's declared length:
//!
//! - [`frontend::bind_parameters`] builds a vector of borrowed values, for a
//!   caller that has already decided to key a cache on them. Its own docs
//!   explain why it is not a field on the `Bind` variant.
//! - [`startup::decode`] collects the startup parameters, once per connection.
//! - [`rewrite`] returns a new body, because rewriting a statement name changes
//!   the length of a message that has to keep its tail byte for byte.
//! - [`relay::FrameRelay`] holds the part of a body it was asked to inspect,
//!   bounded by [`frame::DEFAULT_MAX_INSPECT`] rather than by the body.
//!
//! A fifth used to: `backend::select_sasl_mechanism` collected the offered
//! mechanisms in order to search them. It no longer does, which is the only
//! one of the five that was not paying for anything.

pub mod backend;
pub mod encode;
pub mod encode_frontend;
pub mod frame;
pub mod frontend;
pub mod read;
pub mod relay;
pub mod rewrite;
pub mod session;
pub mod startup;

pub use backend::{BackendMessage, TxStatus};
pub use frame::{
    DEFAULT_MAX_FRAME, DEFAULT_MAX_INSPECT, DecodeError, Decoded, Direction, Frame, FrameHeader,
    Inspect, Tag, decode_header, inspect_policy,
};
pub use frontend::{FrontendMessage, Target};
pub use read::{FieldError, Reader};
pub use relay::{Completed, FrameRelay, RelayOutcome, inspect_budget};
pub use session::{CopyDirection, HoldReason, SessionState};
pub use startup::{Startup, VersionResponse, negotiate};
