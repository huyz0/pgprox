//! The per-client state machine and the relay loop.
//!
//! One of two crates permitted to compose others. Everything before it was
//! built against fakes on purpose; this is where the pieces meet.
//!
//! # The shape
//!
//! Every stage of a client connection is a sans-I/O state machine, and the I/O
//! shell that drives them is generic over `AsyncRead + AsyncWrite + Unpin`. A
//! test drives a whole session over `tokio::io::duplex` without binding a
//! port, which is what makes the error cases reachable at all: a client that
//! sends `SSLRequest` twice or disconnects mid-frame is a function call here
//! and a piece of theatre in an integration test.
//!
//! # The hazard
//!
//! A read can pull in bytes belonging to the next stage. The buffer therefore
//! belongs to the connection, never to the function handling the current
//! stage. This has already caused one bug in this project, in the SCRAM tests,
//! and the crate's `AGENTS.md` says so at length.

pub mod auth;
pub mod state;

pub use auth::{
    Progress, SCRAM_SHA_256, SaslProgress, ScramAuth, ScramChallenge, ScramConfig,
    StaticCredentials, TokenAuth,
};
pub use state::{Action, Credential, Handshake, HandshakeConfig, Reply, StartupInfo, TlsPosture};
