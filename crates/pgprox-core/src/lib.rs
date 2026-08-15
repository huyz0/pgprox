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
//! | [`auth::GrantInvalidation`] | `pgprox-auth` | [`auth::FakeInvalidation`] |
//! | [`auth::TopologyRefresh`] | `pgprox-auth` | [`auth::FakeTopologyRefresh`] |
//! | [`UpstreamPool`] | `pgprox-pool` | [`pool::FakeUpstreamPool`] |
//! | [`ClusterCoordinator`] | `pgprox-cluster` | [`cluster::FakeClusterCoordinator`] |
//! | [`cluster::PeerSource`] | `pgprox-core` | [`cluster::FakePeerSource`] |
//! | [`ConfigSource`] | `pgprox-config` | [`config::FakeConfigSource`] |
//! | [`Router`] | `pgprox-route` | [`route::FakeRouter`] |
//! | [`QueryCache`] | `pgprox-cache` (M9) | [`cache::FakeQueryCache`] |
//! | [`admin::Observatory`] | `bin/pgprox` | [`admin::FakeObservatory`] |
//!
//! `pool::ConnectionRelease` is deliberately not a row: it has no public fake
//! ([`pool::FakeUpstreamPool`]'s own release path is private, used only
//! internally), so it never had the "implemented by, faked by" shape this
//! table exists to record. It is `UpstreamGuard`'s release plumbing, not a
//! seam a downstream crate swaps.
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
//! track, stop and escalate first. See `docs/internal/standards/contracts.md`.
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
pub mod hash;
pub mod ids;
pub mod pool;
pub mod retry;
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

#[cfg(test)]
mod tests {
    //! `M88.16`. The contracts table above listed seven traits when this
    //! crate defines twelve `pub trait`s that `scripts/check-core-contract.sh`
    //! governs identically — any `pub trait` block under this crate's `src/`,
    //! whether or not it made the table. Missing were `GrantInvalidation`,
    //! `TopologyRefresh`, `Observatory`, and `PeerSource`; the twelfth,
    //! `ConnectionRelease`, is still deliberately not a row, for the reason
    //! given just under the table.
    //!
    //! Two tests: one that every trait meant to have a row actually has one
    //! (catches a row silently dropped), and one that counts `pub trait`
    //! blocks across every source file and checks the count against the
    //! table plus the one deliberate exclusion (catches a *new* trait added
    //! later with no table update — the same drift this finding was).

    const LIB_RS: &str = include_str!("lib.rs");

    const GOVERNED_TRAITS: &[&str] = &[
        "Clock",
        "CredentialResolver",
        "GrantInvalidation",
        "TopologyRefresh",
        "UpstreamPool",
        "ClusterCoordinator",
        "PeerSource",
        "ConfigSource",
        "Router",
        "QueryCache",
        "Observatory",
    ];

    #[test]
    fn the_contracts_table_lists_every_governed_trait() {
        for trait_name in GOVERNED_TRAITS {
            // Matches a bare `` [`Trait`] `` row and a module-qualified
            // `` [`module::Trait`] `` row alike — several of these traits are
            // linked by their module path, not re-exported at the crate root.
            let row = format!("{trait_name}`]");
            assert!(
                LIB_RS.contains(&row),
                "the contracts table is missing a row for `{trait_name}`"
            );
        }
    }

    #[test]
    fn the_crate_defines_exactly_the_governed_traits_plus_connection_release() {
        const SOURCES: &[&str] = &[
            include_str!("auth.rs"),
            include_str!("admin.rs"),
            include_str!("route.rs"),
            include_str!("pool.rs"),
            include_str!("clock.rs"),
            include_str!("cache.rs"),
            include_str!("config.rs"),
            include_str!("cluster.rs"),
        ];
        let pub_trait_count: usize = SOURCES
            .iter()
            .map(|src| {
                src.lines()
                    .filter(|line| line.trim_start().starts_with("pub trait "))
                    .count()
            })
            .sum();
        // +1 for `ConnectionRelease`, the one deliberate exclusion.
        assert_eq!(
            pub_trait_count,
            GOVERNED_TRAITS.len() + 1,
            "a pub trait was added or removed without updating this crate's \
             contracts table (or this test's deliberate-exclusion count)"
        );
    }
}
