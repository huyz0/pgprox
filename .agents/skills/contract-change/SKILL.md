---
name: contract-change
description: Change a trait, DTO, or error type in pgprox-core safely. Use whenever editing anything in pgprox-core that other crates depend on, or when a task requires a new trait method, a changed signature, or a new variant on a shared type.
---

# Changing a contract

`pgprox-core` is what lets five tracks run in parallel. It is also what breaks
all five at once when it changes carelessly.

## Stop first

Before editing anything, answer: does this change touch more than one track?

If yes, **stop and escalate**. Do not start. The cost is not the edit, it is the
rebase across five parallel branches and the half-day of everyone rebuilding
against a moved target. That is a decision for a human.

If no, continue.

## Prefer additive

In order of preference:

1. **New trait method with a default implementation.** Breaks nobody.
2. **New struct field with a `Default`.** Breaks nobody using struct update
   syntax.
3. **New enum variant**, safe if the enum is `#[non_exhaustive]`, which public
   enums describing external state already are.
4. **Changed signature.** Breaks everyone at once. Last resort.

If a breaking change is genuinely right, batch it with any others that are
pending. One painful contract change costs much less than four small ones spread
across a week, because each one costs every track a rebase.

## The atomic commit

A contract change is one commit containing all of:

1. The trait or type change
2. Every fake updated
3. Every implementation updated
4. Every call site updated
5. The ADR recording why, including what was rejected
6. Any dependent track's spec updated

Missing any of these leaves the tree red for another track, which is the failure
this procedure exists to prevent.

## Checklist

```bash
# Who implements it?
rg 'impl .*<TraitName>' crates/

# Who calls it?
rg '<method_name>' crates/

# Fakes are usually named for the trait
rg -l 'test-fakes|Fake' crates/pgprox-core/src/
```

Then:

- [ ] Trait or type changed
- [ ] Every fake updated, and its own tests still pass
- [ ] Every real implementation updated
- [ ] Every call site updated
- [ ] `cargo check --workspace` clean
- [ ] `scripts/check-coverage.sh` clean for every affected crate
- [ ] ADR written in `docs/internal/product/decisions/`
- [ ] Dependent specs updated

## The fake is not optional

A trait without a working in-memory fake is not done. The fake behaves like the
real thing: a fake pool actually tracks acquisitions and actually refuses past
its cap.

If a fake and its real implementation can diverge, write one shared test suite
and run it against both. That is cheaper than finding the divergence at
integration.

## The sidecar proto

`.proto` changes are different: that interface is not under this repo's control.
Field numbers are never reused, fields are never removed, and anything optional
is genuinely optional in the Rust type. Changes need agreement from the sidecar
owners before the Rust side moves, not after.
