//! Contracts shared by every `pgprox` crate.
//!
//! This crate holds traits, DTOs, error types, and ID newtypes, plus a working
//! in-memory fake for every trait. It performs no I/O and depends on no other
//! workspace crate.
//!
//! That constraint is what lets several tracks develop in parallel: a track
//! codes against the traits and tests against the fakes, never against another
//! track's half-finished crate.
//!
//! # The contracts
//!
//! | Trait | Implemented by | Fake |
//! | --- | --- | --- |
//! | [`Clock`] | `pgprox-core` | [`clock::FakeClock`] |
//! | [`CredentialResolver`] | `pgprox-auth` | [`auth::FakeCredentialResolver`] |
//! | [`UpstreamPool`] | `pgprox-pool` | [`pool::FakeUpstreamPool`] |
//! | [`ClusterCoordinator`] | `pgprox-cluster` | [`cluster::FakeClusterCoordinator`] |
//! | [`ConfigSource`] | `pgprox-config` | [`config::FakeConfigSource`] |
//! | [`Router`] | `pgprox-route` | [`route::FakeRouter`] |
//! | [`QueryCache`] | `pgprox-cache` (M9) | [`cache::FakeQueryCache`] |
//!
//! Fakes are behind the `test-fakes` feature for downstream crates, and are
//! always compiled during this crate's own tests. They behave like the real
//! thing rather than recording calls: the pool refuses past its cap, the
//! resolver refuses unknown tokens, the config source validates on publish.
//!
//! # Changing anything here
//!
//! A contract change is one atomic commit covering the trait, every fake, every
//! implementation, every call site, and an ADR. If it touches more than one
//! track, stop and escalate first. See `standards/contracts.md`.
//!
//! # Conventions that are easy to miss
//!
//! - [`UpstreamGuard`] discards its connection unless told otherwise, so a
//!   cancelled future cannot recycle a connection sitting mid-transaction.
//! - [`QuotaLease::count`] reports zero once expired, so a caller that forgets
//!   to check expiry cannot over-subscribe a cap.
//! - [`StmtClass::Unknown`] is the default and never reaches a replica.
//! - Nothing holding a credential derives `Debug`.
//!
//! # No unsafe, and not by the workspace's leave
//!
//! `#![forbid]` rather than the workspace's `deny`, so no `#[allow]` anywhere
//! in this crate can reach it. This crate holds `sql::Lexer`, which decides which text in an untrusted
//! statement is SQL and which is data, and `SecretString`, whose whole purpose
//! is that a credential cannot be read out by accident.
//!
//! `M27.1` opened the door elsewhere and left it shut here on purpose. See ADR
//! 0026 and `scripts/check-unsafe.sh`, which holds the list.

#![forbid(unsafe_code)]
pub mod admin;
pub mod auth;
pub mod buf;
pub mod cache;
pub mod clock;
pub mod cluster;
pub mod config;
pub mod error;
pub mod ids;
pub mod pool;
pub mod route;
pub mod secret;
pub mod sql;

pub use auth::{
    AuthError, AuthRequest, Backend, ClaimSet, CredentialResolver, Grant, PoolHints, PoolMode,
    TlsMode,
};
pub use buf::{BufferSlab, PooledBuf};
pub use cache::{CacheKey, CachedResult, QueryCache};
pub use clock::{Clock, SystemClock};
pub use cluster::{
    ClusterCoordinator, ClusterDigest, Member, MembershipView, NodeMode, QuotaError, QuotaLease,
};
pub use config::{Config, ConfigError, ConfigSource, NodeOverride, ServerConfig};
pub use error::{AuthRejection, ClientError, SqlState};
pub use ids::{ConnId, Lsn, NodeId, PoolKey, ServerId, TenantId};
pub use pool::{
    ConnectionRelease, PoolError, PoolStats, ReleaseOutcome, UpstreamGuard, UpstreamId,
    UpstreamPool,
};
pub use route::{ReplicaState, RouteCtx, RouteHint, RouteTarget, Router, StmtClass};
pub use secret::SecretString;
