# 0006. Pluggable config sources, drain as desired state

Status: accepted

## Context

Config has to reach five pods, and operators need to drain a node for a version
upgrade without dropping client transactions. The deployment target is
kubernetes, but the design should not assume it forever.

The subtler question is whether drain is a command or a state. A command is
immediate and easy to script. A state survives a pod restart.

## Decision

A `ConfigSource` trait in `pgprox-core` with three implementations: a file
provider watching the mount directory, an etcd-watch provider, and an HTTP-poll
provider, the latter two behind features. The file provider is the default and
targets ConfigMap mounts.

The file provider watches the *directory*, not the file. ConfigMap updates swap
a symlink, so watching the file itself misses every change.

Drain is desired state, expressed declaratively:

```yaml
nodes:
  pgprox-2: { mode: drain }
```

Sequence: `/readyz` starts failing so kubernetes removes the pod from Service
endpoints; gossip announces `draining` so peers exclude it from rendezvous
hashing and reclaim its reservations; idle clients close with `57P01`; in-flight
transactions run to completion and close at their next `ReadyForQuery('I')`;
after `drain_grace` (default 60s) the remainder are force-closed. A `preStop`
hook triggers this and sleeps long enough for it to finish before SIGTERM.

`POST /v1/drain` exists for interactive use and writes the same state with a
TTL, so a manual drain does not silently persist forever.

## Consequences

- Drain survives a pod restart, and it is visible in git rather than being a
  side effect somebody ran once.
- Config is auditable and revertable by the same review process as code.
- The TTL on the imperative path is the interesting detail: without it, an
  operator draining a node at 2am leaves it drained forever, and the next person
  cannot tell whether that was intentional.
- Three providers means three things to test, mitigated by them sharing one
  trait and one validation path.
- `/readyz` becomes correctness-relevant, not just informational. Nothing else
  is allowed to make it fail transiently, because a flapping readiness probe
  under load causes exactly the connection storm the design exists to prevent.

## Alternatives rejected

**ConfigMap watch only.** Simplest, fully k8s-native. Rejected only in the sense
that it remains the default; the trait exists so a non-k8s deployment is
possible without a rewrite.

**Admin API as source of truth.** Immediate and easy for tooling. Rejected
because state is lost on pod restart unless separately persisted, which
reintroduces the problem.

**etcd as source of truth.** Strong consistency and instant propagation.
Rejected because it puts a hard external dependency in the control path for a
benefit that ConfigMap propagation delay does not actually cost us.
