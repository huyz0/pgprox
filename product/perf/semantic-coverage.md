# Semantic coverage

What the reference workload actually runs, by execution count rather than by
hit or miss. Written by `scripts/profile.sh`; do not edit by hand.

The replay: 200 connections for 20s against the local
one-node stack, workload version 2, seed
1. It completed 9860
transactions with 0 errors, p50
28899us and p99 925299us.

2421 functions in this workspace's own crates were compiled into the
profiled binary. 421 of them ran at least once and are big enough to be
worth a line here.

## Hot and under-tested

High execution count, and under 80% of their regions
covered *both* by this replay and by the tier-1 test suite. The second column
is the workload's reach and the third is the suite's; a function the suite
covers is not under-tested, however little of it this particular replay
touched. What is left runs constantly and nothing exercises it, which is the
highest-risk code in the repository. A dash means the suite never compiled
that instantiation at all.

| Function | File | Count | Replay | Tier 1 |
| --- | --- | --- | --- | --- |
| `queue` | crates/pgprox-session/src/shell.rs | 86,852 | 79% | 79% |
| `pgprox_app::serve::map_statement_name` | bin/pgprox/src/serve.rs | 27,423 | 8% | 61% |
| `queue` | crates/pgprox-session/src/shell.rs | 27,223 | 79% | 79% |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 27,223 | 65% | 75% |
| `pgprox_app::serve::ready_statement` | bin/pgprox/src/serve.rs | 27,223 | 18% | 71% |
| `queue` | crates/pgprox-session/src/shell.rs | 9,860 | 79% | 79% |
| `serve::told::{closure#0}` | bin/pgprox/src/serve.rs | 9,860 | 36% | 36% |
| `serve::told::{closure#0}` | bin/pgprox/src/serve.rs | 9,860 | 36% | 36% |
| `guard` | crates/pgprox-pool/src/live.rs | 9,860 | 75% | 75% |
| `queue` | crates/pgprox-session/src/shell.rs | 2,392 | 79% | 79% |
| `queue` | crates/pgprox-session/src/shell.rs | 200 | 79% | 79% |
| `queue` | crates/pgprox-session/src/shell.rs | 200 | 79% | 79% |
| `serve::session::{closure#0}` | bin/pgprox/src/serve.rs | 200 | 36% | 74% |
| `shell::authenticate_token::{closure#0}` | crates/pgprox-session/src/shell.rs | 200 | 57% | 74% |
| `queue` | crates/pgprox-session/src/shell.rs | 82 | 79% | 79% |
| `run::follow_drain::{closure#0}` | bin/pgprox/src/run.rs | 22 | 37% | 70% |
| `static_admin` | bin/pgprox/src/entry.rs | 1 | 14% | 14% |
| `tls` | bin/pgprox/src/entry.rs | 1 | 22% | 22% |
| `tls_posture` | bin/pgprox/src/wiring.rs | 1 | 71% | 71% |
| `pgprox_app::run::bind_client` | bin/pgprox/src/run.rs | 1 | 71% | 76% |

## Hot and expensive

Execution count times region count: the optimization queue, ordered by total
contribution rather than by which code looks interesting. A number here is not
a defect. It is where a saved instruction is worth the most.

| Function | File | Count | Regions |
| --- | --- | --- | --- |
| `next` | crates/pgprox-core/src/sql.rs | 443,593 | 78 |
| `pgprox_core::sql::trim_leading_space` | crates/pgprox-core/src/sql.rs | 833,901 | 26 |
| `skip_trivia` | crates/pgprox-core/src/sql.rs | 470,816 | 35 |
| `pgprox_core::sql::word_end` | crates/pgprox-core/src/sql.rs | 313,444 | 28 |
| `pgprox_proto::backend::decode` | crates/pgprox-proto/src/backend.rs | 88,328 | 76 |
| `pgprox_proto::frame::decode` | crates/pgprox-proto/src/frame.rs | 212,419 | 26 |
| `acquire` | crates/pgprox-pool/src/pool.rs | 174,672 | 19 |
| `pgprox_core::sql::is_word_char` | crates/pgprox-core/src/sql.rs | 396,650 | 8 |
| `total` | crates/pgprox-pool/src/pool.rs | 164,862 | 17 |
| `lock` | crates/pgprox-pool/src/live.rs | 534,032 | 5 |
| `parse` | crates/pgprox-pool/src/params.rs | 27,223 | 98 |
| `read_tagged::{closure#0}` | crates/pgprox-session/src/shell.rs | 117,616 | 22 |
| `advance` | crates/pgprox-core/src/sql.rs | 418,410 | 6 |
| `on_server` | crates/pgprox-session/src/relay.rs | 86,852 | 25 |
| `try_borrow` | crates/pgprox-core/src/buf.rs | 134,560 | 16 |
| `with_pool` | crates/pgprox-pool/src/live.rs | 174,672 | 11 |
| `with_pool` | crates/pgprox-pool/src/live.rs | 164,812 | 11 |
| `with_pool` | crates/pgprox-pool/src/live.rs | 164,812 | 11 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 27,223 | 65 |
| `pgprox_route::hints::parse_route_assignment` | crates/pgprox-route/src/hints.rs | 27,223 | 61 |
| `give_back` | crates/pgprox-core/src/buf.rs | 134,560 | 12 |
| `pgprox_proto::frame::check_length` | crates/pgprox-proto/src/frame.rs | 145,439 | 11 |
| `pgprox_core::sql::is_string_introducer` | crates/pgprox-core/src/sql.rs | 313,444 | 5 |
| `pgprox_pool::pin::is_session_advisory_lock` | crates/pgprox-pool/src/pin.rs | 186,202 | 8 |
| `pgprox_proto::frontend::decode` | crates/pgprox-proto/src/frontend.rs | 27,423 | 53 |

## Cold and complex

Never ran during the replay, and large. Speculative optimization and dead
paths both look like this. Each one is either a case the workload does not
cover, which is a gap in the workload, or code nobody needs, which is a
deletion.

| Function | File | Regions |
| --- | --- | --- |
| `serve::relay::{closure#0}` | bin/pgprox/src/serve.rs | 160 |
| `serve::relay::{closure#0}` | bin/pgprox/src/serve.rs | 160 |
| `serve::relay::{closure#0}` | bin/pgprox/src/serve.rs | 160 |
| `serve::serve_client::{closure#0}` | bin/pgprox/src/serve.rs | 116 |
| `serve::serve_client::{closure#0}` | bin/pgprox/src/serve.rs | 116 |
| `serve::serve_client::{closure#0}` | bin/pgprox/src/serve.rs | 116 |
| `connect::drive::{closure#0}` | crates/pgprox-session/src/connect.rs | 100 |
| `connect::drive::{closure#0}` | crates/pgprox-session/src/connect.rs | 100 |
| `parse` | bin/pgprox/src/entry.rs | 89 |
| `next` | crates/pgprox-core/src/sql.rs | 78 |
| `serve::session::{closure#0}` | bin/pgprox/src/serve.rs | 73 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 65 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 65 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 65 |
| `gossip::request_over::{closure#0}` | bin/pgprox/src/gossip.rs | 60 |
| `pgprox_app::metrics::samples` | bin/pgprox/src/metrics.rs | 60 |
| `probe::run_replica_query::{closure#0}` | crates/pgprox-session/src/probe.rs | 58 |
| `probe::run_replica_query::{closure#0}` | crates/pgprox-session/src/probe.rs | 58 |
| `pgprox_auth::scram::parse_server_first` | crates/pgprox-auth/src/scram.rs | 55 |
| `gossip::answer::{closure#0}` | bin/pgprox/src/gossip.rs | 55 |
| `gossip::answer::{closure#0}` | bin/pgprox/src/gossip.rs | 55 |
| `pgprox_auth::scram::parse_server_first` | crates/pgprox-auth/src/scram.rs | 55 |
| `on_initial` | crates/pgprox-session/src/auth.rs | 54 |
| `on_initial` | crates/pgprox-session/src/auth.rs | 54 |
| `on_initial` | crates/pgprox-session/src/auth.rs | 54 |
