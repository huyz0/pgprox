//! Configuration providers, schema validation and hot reload.
//!
//! Config is pulled, not pushed, and drain is desired state rather than a
//! command. A drained node stays drained across a restart, and the intent is
//! visible in whatever the config lives in rather than being a side effect
//! somebody ran once. See ADR 0006.
//!
//! [`document`] owns the file format, [`provider`] owns where the file comes
//! from, and validation happens once in the shared path so every provider
//! behaves identically.

pub mod document;
pub mod drain;
pub mod provider;

pub use document::{ConfigDocument, parse};
pub use drain::{DrainConfig, DrainState, ModeSource};
pub use provider::{FileConfig, FileSource};

#[cfg(test)]
mod tests {
    //! `M88.15`. `AGENTS.md` and ADR 0006 said "three providers" plainly enough
    //! to read as three built things, when this crate has always had exactly
    //! one `ConfigSource`, `FileSource`. Nothing else in the repo can catch
    //! that kind of drift mechanically — `scripts/check-drift.sh` checks that
    //! an ADR's *named libraries* are real dependencies, not that its prose
    //! about a type's implementation count matches the type. These two tests
    //! are the mechanical half of that finding's fix, `include_str!`-ing both
    //! documents at compile time so the overclaim cannot come back silently.

    const AGENTS_MD: &str = include_str!("../AGENTS.md");
    const ADR_0006: &str = include_str!(
        "../../../docs/internal/product/decisions/0006-pluggable-config-declarative-drain.md"
    );

    #[test]
    fn agents_md_names_the_one_provider_that_exists_rather_than_three() {
        assert!(
            !AGENTS_MD.contains("all three providers"),
            "AGENTS.md claimed three ConfigSource providers behave identically; \
             only FileSource exists"
        );
        assert!(
            AGENTS_MD.contains("just `FileSource`"),
            "AGENTS.md should say plainly that FileSource is the only provider \
             implemented today"
        );
    }

    #[test]
    fn the_adr_records_that_only_one_provider_is_implemented() {
        assert!(
            ADR_0006.contains("one of three providers implemented"),
            "ADR 0006's Status line should say plainly that etcd-watch and \
             HTTP-poll were designed for, not built"
        );
    }
}
