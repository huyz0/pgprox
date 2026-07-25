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
- Validation happens once, in the shared path, so all three providers behave
  identically.

See ADR [0006](../../product/decisions/0006-pluggable-config-declarative-drain.md).
