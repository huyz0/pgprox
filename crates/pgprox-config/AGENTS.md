# pgprox-config

`ConfigSource` providers, schema validation, hot reload.

## Rules specific to this crate

- **Watch the directory, not the file.** ConfigMap updates swap a symlink, so
  watching the file itself misses every change. This is the bug that makes hot
  reload appear to work in testing and fail in the cluster.
- Drain is desired state, not a command, so it survives a pod restart and shows
  up in git.
- The imperative `POST /v1/drain` path writes the same state with a TTL. Without
  the TTL, a node drained at 2am stays drained forever and nobody can tell
  whether that was intentional.
- Validation happens once, in the shared path, so every `ConfigSource` — today
  just `FileSource`, with the trait shaped to add another without a rewrite —
  behaves identically. It is `Config::validate` from `pgprox-core`, the same
  function the fake calls, so a document this crate accepts and one the fake
  accepts cannot diverge.
- **`deny_unknown_fields` on every document type.** A misspelled key that is
  silently ignored is the worst configuration bug there is: the operator sees
  their edit in git, the node reports nothing, and the setting never took
  effect.
- Durations take a unit and servers take a port. Both could have a default and
  neither does, because a configuration that silently means something other
  than what it looks like is worse than one that refuses to start.
- The document format is this crate's to own, which is why it is a separate type
  from `Config` rather than `Deserialize` on it. A field can be renamed in
  `pgprox-core` without every deployment's ConfigMap becoming invalid.

See ADR [0006](../../docs/internal/product/decisions/0006-pluggable-config-declarative-drain.md).
