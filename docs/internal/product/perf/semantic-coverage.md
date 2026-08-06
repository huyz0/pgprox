# Semantic coverage

What the reference workload actually runs, by execution count rather than by
hit or miss. Written by `scripts/profile.sh`; do not edit by hand.

The replay: 200 connections for 20s against the local
one-node stack, workload version 3, seed
1. It completed 9779
transactions with 0 errors, p50
28199us and p99 955899us.

2429 functions in this workspace's own crates were compiled into the
profiled binary. 441 of them ran at least once and are big enough to be
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
| `queue` | crates/pgprox-session/src/shell.rs | 90,705 | 79% | 79% |
| `pgprox_app::serve::map_statement_name` | bin/pgprox/src/serve.rs | 51,412 | 58% | 61% |
| `queue` | crates/pgprox-session/src/shell.rs | 50,656 | 79% | 79% |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 27,180 | 69% | 75% |
| `queue` | crates/pgprox-session/src/shell.rs | 9,779 | 79% | 79% |
| `serve::told::{closure#0}` | bin/pgprox/src/serve.rs | 9,779 | 36% | 36% |
| `serve::told::{closure#0}` | bin/pgprox/src/serve.rs | 9,779 | 36% | 36% |
| `guard` | crates/pgprox-pool/src/live.rs | 9,779 | 75% | 75% |
| `queue` | crates/pgprox-session/src/shell.rs | 1,311 | 79% | 79% |
| `queue` | crates/pgprox-session/src/shell.rs | 556 | 79% | 79% |
| `queue` | crates/pgprox-session/src/shell.rs | 300 | 79% | 79% |
| `serve::evict_for::{closure#0}` | bin/pgprox/src/serve.rs | 300 | 43% | 43% |
| `queue` | crates/pgprox-session/src/shell.rs | 200 | 79% | 79% |
| `queue` | crates/pgprox-session/src/shell.rs | 200 | 79% | 79% |
| `serve::session::{closure#0}` | bin/pgprox/src/serve.rs | 200 | 36% | 74% |
| `shell::authenticate_token::{closure#0}` | crates/pgprox-session/src/shell.rs | 200 | 57% | 74% |
| `queue` | crates/pgprox-session/src/shell.rs | 81 | 79% | 79% |
| `queue` | crates/pgprox-session/src/shell.rs | 76 | 79% | 79% |
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
| `next` | crates/pgprox-core/src/sql.rs | 249,867 | 78 |
| `pgprox_core::sql::trim_leading_space` | crates/pgprox-core/src/sql.rs | 462,443 | 26 |
| `skip_trivia` | crates/pgprox-core/src/sql.rs | 266,201 | 35 |
| `pgprox_proto::backend::decode` | crates/pgprox-proto/src/backend.rs | 92,239 | 76 |
| `pgprox_proto::frame::decode` | crates/pgprox-proto/src/frame.rs | 234,584 | 26 |
| `pgprox_core::sql::word_end` | crates/pgprox-core/src/sql.rs | 173,884 | 28 |
| `acquire` | crates/pgprox-pool/src/pool.rs | 173,194 | 19 |
| `total` | crates/pgprox-pool/src/pool.rs | 163,465 | 17 |
| `pgprox_proto::frontend::decode` | crates/pgprox-proto/src/frontend.rs | 51,412 | 53 |
| `lock` | crates/pgprox-pool/src/live.rs | 529,517 | 5 |
| `read_tagged::{closure#0}` | crates/pgprox-session/src/shell.rs | 117,041 | 22 |
| `try_borrow` | crates/pgprox-core/src/buf.rs | 155,538 | 16 |
| `on_server` | crates/pgprox-session/src/relay.rs | 90,781 | 25 |
| `flush::{closure#0}` | crates/pgprox-session/src/shell.rs | 63,204 | 35 |
| `on_client` | crates/pgprox-session/src/relay.rs | 51,412 | 40 |
| `with_pool` | crates/pgprox-pool/src/live.rs | 173,194 | 11 |
| `give_back` | crates/pgprox-core/src/buf.rs | 155,538 | 12 |
| `pgprox_proto::frame::check_length` | crates/pgprox-proto/src/frame.rs | 168,853 | 11 |
| `pgprox_app::serve::map_statement_name` | bin/pgprox/src/serve.rs | 51,412 | 36 |
| `with_pool` | crates/pgprox-pool/src/live.rs | 163,415 | 11 |
| `with_pool` | crates/pgprox-pool/src/live.rs | 163,415 | 11 |
| `pgprox_core::sql::is_word_char` | crates/pgprox-core/src/sql.rs | 221,073 | 8 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 27,180 | 65 |
| `parse` | crates/pgprox-pool/src/params.rs | 16,334 | 98 |
| `lock_free_list` | crates/pgprox-core/src/buf.rs | 311,076 | 5 |

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
