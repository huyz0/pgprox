//! The one place a tenant may become a metric label.
//!
//! # Why there is an exception at all
//!
//! `metrics` forbids a `tenant` label because five thousand tenants is a series
//! count that takes a Prometheus down. That is right for five thousand tenants
//! and wrong for three.
//!
//! Most fleets have a handful of tenants that matter more than the rest: the
//! one paying for the platform, the one that keeps filing incidents, the one
//! being migrated this week. Wanting a dashboard for those is reasonable, and
//! refusing outright means somebody builds it by scraping the admin API on a
//! timer, which is the same series in a worse place.
//!
//! So it is allowed, for a named list, with a ceiling.
//!
//! # The ceiling is the whole design
//!
//! An allowlist without one does not stay small. It grows one incident at a
//! time, each addition individually reasonable, until it is the unbounded label
//! it was meant to avoid. Nobody ever decides to break Prometheus; they add a
//! seventeenth tenant during an outage.
//!
//! So [`TenantAllowlist::add`] refuses past the ceiling and says what to remove.
//! Refusing is the useful behaviour: an operator who has to take one out has to
//! decide which of the two matters, and that decision is exactly the one the
//! ceiling exists to force.
//!
//! # Everything else is aggregated, not dropped
//!
//! A tenant not on the list still appears in every metric; it is counted under
//! [`OTHER`] rather than under its own name. The totals stay correct, which
//! matters because the first thing anyone does with a per-tenant panel is check
//! it against the fleet total.

use std::collections::BTreeSet;

use pgprox_core::ids::TenantId;

/// The label value used for every tenant not on the allowlist.
///
/// A single bucket rather than omission. Dropping them would make the sum of
/// the per-tenant series disagree with the fleet total, and an operator who
/// notices that stops trusting both numbers.
pub const OTHER: &str = "other";

/// The most tenants that may have their own series.
///
/// Small on purpose. The point is not the exact number, it is that adding the
/// next one requires taking one out, so the list cannot grow by degrees.
pub const MAX_ALLOWLISTED: usize = 16;

/// Why a tenant could not be added.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AllowlistError {
    /// The allowlist is full.
    #[error(
        "the per-tenant metric allowlist is full at {max} tenants; \
         remove one of [{current}] before adding {tenant}"
    )]
    Full {
        /// The ceiling.
        max: usize,
        /// The tenant that could not be added.
        tenant: String,
        /// Who is on the list, so the choice can be made without another call.
        current: String,
    },
}

/// Tenants that may appear as a metric label.
#[derive(Clone, Debug, Default)]
pub struct TenantAllowlist {
    allowed: BTreeSet<TenantId>,
}

impl TenantAllowlist {
    /// An empty allowlist, which is the default.
    ///
    /// Everything aggregates under [`OTHER`] until somebody deliberately opts a
    /// tenant in, because a deployment that has not thought about this should
    /// get the safe behaviour rather than the convenient one.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds one from a configured list, refusing if it is too long.
    ///
    /// Refusing at startup rather than truncating: a configuration listing
    /// twenty tenants and silently getting sixteen is one where nobody finds
    /// out which four were dropped until they go looking for a panel.
    ///
    /// # Errors
    ///
    /// [`AllowlistError::Full`] if there are more than [`MAX_ALLOWLISTED`].
    pub fn from_configured(
        tenants: impl IntoIterator<Item = TenantId>,
    ) -> Result<Self, AllowlistError> {
        let mut allowlist = Self::new();
        for tenant in tenants {
            allowlist.add(tenant)?;
        }
        Ok(allowlist)
    }

    /// Adds a tenant.
    ///
    /// # Errors
    ///
    /// [`AllowlistError::Full`] past [`MAX_ALLOWLISTED`], naming who is already
    /// on the list so the caller can decide what to remove.
    pub fn add(&mut self, tenant: TenantId) -> Result<(), AllowlistError> {
        // Re-adding an existing entry is not growth, so it never fails. An
        // idempotent config reload must not start erroring at the ceiling.
        if self.allowed.contains(&tenant) {
            return Ok(());
        }
        if self.allowed.len() >= MAX_ALLOWLISTED {
            return Err(AllowlistError::Full {
                max: MAX_ALLOWLISTED,
                tenant: tenant.as_str().to_owned(),
                current: self
                    .allowed
                    .iter()
                    .map(|t| t.as_str().to_owned())
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        }
        self.allowed.insert(tenant);
        Ok(())
    }

    /// Removes a tenant, which is how room is made.
    pub fn remove(&mut self, tenant: &TenantId) -> bool {
        self.allowed.remove(tenant)
    }

    /// Whether a tenant has its own series.
    #[must_use]
    pub fn is_allowed(&self, tenant: &TenantId) -> bool {
        self.allowed.contains(tenant)
    }

    /// The label value for a tenant.
    ///
    /// Its own name if it is on the list, [`OTHER`] otherwise. This is the
    /// function a call site uses, so a caller cannot accidentally pass a raw
    /// tenant ID as a label.
    #[must_use]
    pub fn label_for<'a>(&self, tenant: &'a TenantId) -> &'a str
    where
        'a: 'a,
    {
        if self.is_allowed(tenant) {
            tenant.as_str()
        } else {
            OTHER
        }
    }

    /// How many tenants have their own series.
    #[must_use]
    pub fn len(&self) -> usize {
        self.allowed.len()
    }

    /// Whether every tenant aggregates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// The distinct label values this allowlist can produce.
    ///
    /// The allowlisted tenants plus [`OTHER`]. This is the cardinality a
    /// `tenant` label would actually have, and it is bounded by construction.
    #[must_use]
    pub fn cardinality(&self) -> usize {
        self.allowed.len() + 1
    }

    /// Who is on the list.
    pub fn iter(&self) -> impl Iterator<Item = &TenantId> {
        self.allowed.iter()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::metrics::MAX_LABEL_VALUES;

    fn tenant(name: &str) -> TenantId {
        TenantId::new(name)
    }

    #[test]
    fn the_default_aggregates_everything() {
        // A deployment that has not thought about this should get the safe
        // behaviour rather than the convenient one.
        let allowlist = TenantAllowlist::new();

        assert!(allowlist.is_empty());
        assert_eq!(allowlist.label_for(&tenant("acme")), OTHER);
        assert_eq!(allowlist.cardinality(), 1);
    }

    #[test]
    fn an_allowlisted_tenant_gets_its_own_label() {
        let mut allowlist = TenantAllowlist::new();
        allowlist.add(tenant("acme")).unwrap();

        assert_eq!(allowlist.label_for(&tenant("acme")), "acme");
        assert!(allowlist.is_allowed(&tenant("acme")));
        assert_eq!(allowlist.len(), 1);
    }

    #[test]
    fn everyone_else_is_aggregated_rather_than_dropped() {
        // Dropping them would make the per-tenant series disagree with the
        // fleet total, and an operator who notices that stops trusting both.
        let mut allowlist = TenantAllowlist::new();
        allowlist.add(tenant("acme")).unwrap();

        assert_eq!(allowlist.label_for(&tenant("globex")), OTHER);
        assert_eq!(allowlist.label_for(&tenant("initech")), OTHER);
        assert!(!allowlist.is_allowed(&tenant("globex")));
    }

    #[test]
    fn the_allowlist_cannot_grow_past_its_ceiling() {
        // The whole design. Without one it grows an incident at a time, each
        // addition individually reasonable, until it is the unbounded label it
        // was meant to avoid.
        let mut allowlist = TenantAllowlist::new();
        for i in 0..MAX_ALLOWLISTED {
            allowlist.add(tenant(&format!("tenant-{i}"))).unwrap();
        }

        let err = allowlist.add(tenant("one-too-many")).unwrap_err();
        assert!(matches!(err, AllowlistError::Full { .. }));
        assert_eq!(allowlist.len(), MAX_ALLOWLISTED);
    }

    #[test]
    fn a_full_allowlist_says_who_is_on_it() {
        // An operator has to decide which of the two matters, and that decision
        // is the one the ceiling exists to force. Making them go and look it up
        // first is how they decide to raise the ceiling instead.
        let mut allowlist = TenantAllowlist::new();
        for i in 0..MAX_ALLOWLISTED {
            allowlist.add(tenant(&format!("tenant-{i}"))).unwrap();
        }

        let err = allowlist.add(tenant("acme")).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("acme"), "got {message}");
        assert!(message.contains("tenant-0"), "got {message}");
        assert!(message.contains("remove"), "got {message}");
    }

    #[test]
    fn re_adding_an_existing_tenant_is_not_growth() {
        // An idempotent config reload must not start erroring at the ceiling.
        let mut allowlist = TenantAllowlist::new();
        for i in 0..MAX_ALLOWLISTED {
            allowlist.add(tenant(&format!("tenant-{i}"))).unwrap();
        }

        allowlist.add(tenant("tenant-0")).unwrap();
        assert_eq!(allowlist.len(), MAX_ALLOWLISTED);
    }

    #[test]
    fn removing_one_makes_room_for_another() {
        let mut allowlist = TenantAllowlist::new();
        for i in 0..MAX_ALLOWLISTED {
            allowlist.add(tenant(&format!("tenant-{i}"))).unwrap();
        }
        assert!(allowlist.add(tenant("acme")).is_err());

        assert!(allowlist.remove(&tenant("tenant-0")));
        allowlist.add(tenant("acme")).unwrap();
        assert!(allowlist.is_allowed(&tenant("acme")));
        assert!(!allowlist.is_allowed(&tenant("tenant-0")));

        assert!(!allowlist.remove(&tenant("never-added")));
    }

    #[test]
    fn an_over_long_configuration_is_refused_at_startup() {
        // Truncating silently means nobody finds out which entries were dropped
        // until they go looking for a panel that is not there.
        let too_many: Vec<TenantId> = (0..=MAX_ALLOWLISTED)
            .map(|i| tenant(&format!("tenant-{i}")))
            .collect();

        let err = TenantAllowlist::from_configured(too_many).unwrap_err();
        assert!(matches!(err, AllowlistError::Full { .. }));
    }

    #[test]
    fn a_configuration_at_the_ceiling_is_accepted() {
        let exactly: Vec<TenantId> = (0..MAX_ALLOWLISTED)
            .map(|i| tenant(&format!("tenant-{i}")))
            .collect();

        let allowlist = TenantAllowlist::from_configured(exactly).unwrap();
        assert_eq!(allowlist.len(), MAX_ALLOWLISTED);
    }

    #[test]
    fn the_ceiling_keeps_the_label_inside_the_cardinality_rule() {
        // The exception must not quietly break the rule it is an exception to.
        // A full allowlist plus the `other` bucket is still under the ceiling
        // `metrics` enforces on every other label.
        let mut allowlist = TenantAllowlist::new();
        for i in 0..MAX_ALLOWLISTED {
            allowlist.add(tenant(&format!("tenant-{i}"))).unwrap();
        }

        assert!(
            u32::try_from(allowlist.cardinality()).unwrap() <= MAX_LABEL_VALUES,
            "a full allowlist produces {} label values, which is more than the {MAX_LABEL_VALUES} \
             every other label is held to",
            allowlist.cardinality()
        );
    }

    #[test]
    fn the_list_can_be_read_back_in_a_stable_order() {
        // So a config dump and an admin response agree, and a diff between two
        // nodes means something.
        let allowlist =
            TenantAllowlist::from_configured([tenant("globex"), tenant("acme")]).unwrap();

        let names: Vec<&str> = allowlist.iter().map(TenantId::as_str).collect();
        assert_eq!(names, vec!["acme", "globex"]);
    }
}
