# pgprox-observe

Metrics, tracing, log initialization and the health endpoints.

## The rule that shapes the crate

An unbounded metric label is a review blocker.

At five thousand tenants, a `tenant` label is a series count that takes a
Prometheus down, and it does so at exactly the moment somebody is trying to
work out why the proxy is unhappy.

`metrics` makes that checkable rather than reviewable. Every metric is declared
in one place with the reason each of its labels is bounded, so
`no_metric_has_an_unbounded_label` is a test rather than a habit.

Per-tenant detail is not lost. It lives in the admin API and `SHOW` output,
which are pull-based and cost nothing when nobody is looking. See
[ADR 0007](../../docs/internal/product/decisions/0007-cluster-scoped-observability.md).

## The one exception, and its ceiling

`tenants` is the single place a tenant may become a label: a named allowlist
with a hard ceiling.

Refusing outright would be wrong for the fleet that has three tenants worth
watching, and somebody would build the panel by scraping the admin API on a
timer instead, which is the same series in a worse place. So it is allowed, for
a list, with a limit.

The ceiling is the design. An allowlist without one grows an incident at a
time, each addition individually reasonable. `add` refuses past the limit and
says what to remove, which forces the decision the ceiling exists to force.
Tenants not on the list still appear, counted under `other`, so fleet totals
stay correct.

## Names are code, attributes are data

`spans` holds a fixed list of span names, because a tracing backend groups by
name exactly as Prometheus groups by label.

It is also where query text is kept out of logs. Statement text routinely
carries customer data in literals, so recording it needs the log level turned
up **and** the tenant opted in. Two switches, so that raising log levels
fleet-wide during an incident does not start capturing everyone's data as a
side effect.

## Where it sits

Depends on `pgprox-core`. Used only by `bin/pgprox`.
