# pgprox-config

The configuration document, where it comes from, and how a change reaches a
running node.

## Config is pulled, and drain is desired state

A node reads its configuration rather than being told. Draining is a field in
the document rather than a command sent to a process.

That is the difference between a node that comes back from a restart still
draining and one that quietly starts serving again because the thing that
drained it was a signal somebody sent once. The intent lives wherever the
config lives. See
[ADR 0006](../../docs/internal/product/decisions/0006-pluggable-config-declarative-drain.md).

## Validation happens once

`document` owns the file format. `provider` owns where the file comes from.
Validation sits in the shared path between them, so every provider behaves
identically and a new one cannot accidentally accept something the file
provider rejects.

A document that fails validation is rejected and the running configuration
stays, so a bad ConfigMap does not take a node down. Every validation error
names the offending field, because a config error at startup with no field name
means reading the whole file to guess.

## Where it sits

Depends on `pgprox-core`, which declares the `ConfigSource` trait this crate
implements. Used only by `bin/pgprox`.

What reloads and what does not is a real distinction: the document reloads
without a restart, command-line arguments do not. Anything you might want to
change during an incident belongs in the document, which is why draining is
there.
