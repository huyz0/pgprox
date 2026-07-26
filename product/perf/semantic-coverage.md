# Semantic coverage

What the reference workload actually runs, by execution count rather than by
hit or miss. Written by `scripts/profile.sh`; do not edit by hand.

The replay: 200 connections for 25s against the local
one-node stack, workload version 2, seed
1. It completed 14479
transactions with 0 errors, p50
4334us and p99 711899us.

2407 functions in this workspace's own crates were compiled into the
profiled binary. 393 of them ran at least once and are big enough to be
worth a line here.

## Hot and under-tested

High execution count, under 80% of their regions
covered by this replay. The highest-risk code in the repository: it runs
constantly and the run did not exercise all of it. Tests go here first.

| Function | File | Count | Covered |
| --- | --- | --- | --- |
| `skip_trivia` | crates/pgprox-core/src/sql.rs | 690,480 | 71% |
| `next` | crates/pgprox-core/src/sql.rs | 650,555 | 42% |
| `pgprox_pool::pin::is_session_advisory_lock` | crates/pgprox-pool/src/pin.rs | 272,978 | 75% |
| `pgprox_proto::backend::decode` | crates/pgprox-proto/src/backend.rs | 128,976 | 32% |
| `queue` | crates/pgprox-session/src/shell.rs | 127,464 | 79% |
| `poll_read` | bin/pgprox/src/dial.rs | 115,912 | 56% |
| `flush::{closure#0}` | crates/pgprox-session/src/shell.rs | 59,384 | 71% |
| `fill::{closure#0}` | crates/pgprox-session/src/shell.rs | 57,956 | 75% |
| `borrow::{closure#0}` | crates/pgprox-session/src/shell.rs | 57,956 | 58% |
| `poll_flush` | bin/pgprox/src/dial.rs | 57,956 | 57% |
| `poll_write` | bin/pgprox/src/dial.rs | 57,956 | 56% |
| `fill::{closure#0}` | crates/pgprox-session/src/shell.rs | 40,525 | 75% |
| `borrow::{closure#0}` | crates/pgprox-session/src/shell.rs | 40,525 | 58% |
| `flush::{closure#0}` | crates/pgprox-session/src/shell.rs | 40,325 | 57% |
| `pgprox_app::serve::map_statement_name` | bin/pgprox/src/serve.rs | 40,125 | 8% |
| `pgprox_proto::frontend::decode` | crates/pgprox-proto/src/frontend.rs | 40,125 | 17% |
| `from_byte` | crates/pgprox-proto/src/backend.rs | 40,009 | 62% |
| `queue` | crates/pgprox-session/src/shell.rs | 39,925 | 79% |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 39,925 | 67% |
| `pgprox_app::serve::statement_of` | bin/pgprox/src/serve.rs | 39,925 | 29% |
| `pgprox_app::serve::ready_statement` | bin/pgprox/src/serve.rs | 39,925 | 18% |
| `pgprox_core::sql::statement_words` | crates/pgprox-core/src/sql.rs | 39,925 | 59% |
| `observe_statement` | crates/pgprox-pool/src/params.rs | 39,925 | 14% |
| `observe_statement` | crates/pgprox-pool/src/pin.rs | 39,925 | 56% |
| `parse` | crates/pgprox-pool/src/params.rs | 39,925 | 18% |

## Hot and expensive

Execution count times region count: the optimization queue, ordered by total
contribution rather than by which code looks interesting. A number here is not
a defect. It is where a saved instruction is worth the most.

| Function | File | Count | Regions |
| --- | --- | --- | --- |
| `next` | crates/pgprox-core/src/sql.rs | 650,555 | 81 |
| `skip_trivia` | crates/pgprox-core/src/sql.rs | 690,480 | 35 |
| `pgprox_core::sql::is_word_char` | crates/pgprox-core/src/sql.rs | 3,641,955 | 6 |
| `pgprox_proto::backend::decode` | crates/pgprox-proto/src/backend.rs | 128,976 | 76 |
| `pgprox_proto::frame::decode` | crates/pgprox-proto/src/frame.rs | 310,412 | 26 |
| `parse` | crates/pgprox-pool/src/params.rs | 39,925 | 98 |
| `read_tagged::{closure#0}` | crates/pgprox-session/src/shell.rs | 171,806 | 22 |
| `advance` | crates/pgprox-core/src/sql.rs | 613,710 | 6 |
| `on_server` | crates/pgprox-session/src/relay.rs | 127,464 | 25 |
| `try_borrow` | crates/pgprox-core/src/buf.rs | 196,762 | 16 |
| `pgprox_route::hints::parse_route_assignment` | crates/pgprox-route/src/hints.rs | 39,925 | 61 |
| `give_back` | crates/pgprox-core/src/buf.rs | 196,762 | 12 |
| `pgprox_proto::frame::check_length` | crates/pgprox-proto/src/frame.rs | 212,331 | 11 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 39,925 | 58 |
| `pgprox_pool::pin::is_session_advisory_lock` | crates/pgprox-pool/src/pin.rs | 272,978 | 8 |
| `pgprox_proto::frontend::decode` | crates/pgprox-proto/src/frontend.rs | 40,125 | 53 |
| `flush::{closure#0}` | crates/pgprox-session/src/shell.rs | 59,384 | 35 |
| `reclaim` | crates/pgprox-session/src/shell.rs | 171,806 | 12 |
| `lock_free_list` | crates/pgprox-core/src/buf.rs | 393,524 | 5 |
| `route` | crates/pgprox-route/src/router.rs | 39,925 | 48 |
| `poll_read` | bin/pgprox/src/dial.rs | 115,912 | 16 |
| `pgprox_core::sql::is_string_introducer` | crates/pgprox-core/src/sql.rs | 459,772 | 4 |
| `pgprox_core::sql::statement_words` | crates/pgprox-core/src/sql.rs | 39,925 | 46 |
| `queue` | crates/pgprox-session/src/shell.rs | 127,464 | 14 |
| `serve::forward::{closure#0}` | bin/pgprox/src/serve.rs | 127,464 | 14 |

## Cold and complex

Never ran during the replay, and large. Speculative optimization and dead
paths both look like this. Each one is either a case the workload does not
cover, which is a gap in the workload, or code nobody needs, which is a
deletion.

| Function | File | Regions |
| --- | --- | --- |
| `serve::relay::{closure#0}` | bin/pgprox/src/serve.rs | 154 |
| `serve::relay::{closure#0}` | bin/pgprox/src/serve.rs | 154 |
| `serve::relay::{closure#0}` | bin/pgprox/src/serve.rs | 154 |
| `serve::serve_client::{closure#0}` | bin/pgprox/src/serve.rs | 112 |
| `serve::serve_client::{closure#0}` | bin/pgprox/src/serve.rs | 112 |
| `serve::serve_client::{closure#0}` | bin/pgprox/src/serve.rs | 112 |
| `connect::drive::{closure#0}` | crates/pgprox-session/src/connect.rs | 100 |
| `connect::drive::{closure#0}` | crates/pgprox-session/src/connect.rs | 100 |
| `parse` | bin/pgprox/src/entry.rs | 89 |
| `next` | crates/pgprox-core/src/sql.rs | 81 |
| `serve::session::{closure#0}` | bin/pgprox/src/serve.rs | 73 |
| `gossip::request_over::{closure#0}` | bin/pgprox/src/gossip.rs | 60 |
| `pgprox_app::metrics::samples` | bin/pgprox/src/metrics.rs | 60 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 58 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 58 |
| `serve::pump::{closure#0}` | bin/pgprox/src/serve.rs | 58 |
| `probe::run_replica_query::{closure#0}` | crates/pgprox-session/src/probe.rs | 58 |
| `probe::run_replica_query::{closure#0}` | crates/pgprox-session/src/probe.rs | 58 |
| `pgprox_auth::scram::parse_server_first` | crates/pgprox-auth/src/scram.rs | 55 |
| `gossip::answer::{closure#0}` | bin/pgprox/src/gossip.rs | 55 |
| `gossip::answer::{closure#0}` | bin/pgprox/src/gossip.rs | 55 |
| `pgprox_auth::scram::parse_server_first` | crates/pgprox-auth/src/scram.rs | 55 |
| `on_initial` | crates/pgprox-session/src/auth.rs | 54 |
| `on_initial` | crates/pgprox-session/src/auth.rs | 54 |
| `on_initial` | crates/pgprox-session/src/auth.rs | 54 |
