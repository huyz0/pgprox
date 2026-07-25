# pgprox-core

Contracts only. Traits, DTOs, error types, ID newtypes, `SecretString`, `Clock`,
the buffer slab, and a working fake for every trait.

**No I/O, and no dependency on any other workspace crate.** If something here
needs a socket, it belongs somewhere else.

This crate is why five tracks can run in parallel, and it is what breaks all five
at once when it changes carelessly. Use the `contract-change` skill for any edit,
and stop and escalate if the change touches more than one track.

## Rules specific to this crate

- Every trait ships with a working in-memory fake behind the `test-fakes`
  feature, and the fake has its own tests. A trait without one is not done.
- Fakes behave like the real thing. A fake pool tracks acquisitions and refuses
  past its cap. A mock that records calls lets integration bugs survive to M6.
- Prefer additive changes: a new method with a default breaks nobody, a changed
  signature breaks everyone at once.
- `#[non_exhaustive]` on public enums describing external state.
- Nothing here derives `Debug` on a type holding a credential.

See [contracts.md](../../standards/contracts.md).
