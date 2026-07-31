# The null-byte scan

`M15.4`. The Postgres wire protocol carries its strings null-terminated, so
every string field this proxy reads ends with a search for a zero byte.
`Reader::cstr` did that search with `iter().position(|b| *b == 0)`, which is one
byte per iteration.

```bash
scripts/bench.sh            # instruction counts, callgrind, against the baseline
```

## The two shapes, measured separately

They behave differently enough that one number would have hidden the answer.

`decode_query` is one long scan: a `Query` body carrying a 256-byte generated
`SELECT` with its column list spelled out, which is what an ORM emits. One
`cstr` call, 256 bytes of scanning.

`decode_error_response` is many short ones: an `ErrorResponse` with the eight
fields a unique-constraint violation actually carries, so eight `cstr` calls
over strings of five to sixty bytes.

| | before | after | change |
| --- | --- | --- | --- |
| `decode_query` | 2168 | 460 | **-78.8%** |
| `decode_error_response` | 2222 | 1905 | -14.3% |
| `scan_frame` | 20 | 20 | none |
| `decode_backend_message` | 162 | 162 | none |
| `relay_frame` | 197 | 197 | none |

The long scan is 4.7 times cheaper. The short one is not, and that is the
expected shape rather than a disappointment: at five bytes a vector load reads
past the end of the string and the work is dominated by call overhead and by
the UTF-8 validation that follows, which was already using the same technique.
It still improves, so nothing regressed at short lengths, which was the thing
worth checking before taking the dependency.

The three unchanged rows are the control. `scan_frame` reads a header and never
touches a string, and `relay_frame` never parses at all, so a change in either
would have meant the measurement was picking up something other than the scan.

## What this is worth in context

`decode_query` runs once per simple query and once per `Parse`. Against
`route_point_select` at 6982 instructions, 1708 saved is roughly a quarter of
what routing costs, on a path every statement takes.

It is worth saying what it is not. `M7.58` took CPU per statement from 687us to
43.7us, and 1708 instructions is not in that league. This is a small, certain
win on a hot path, taken because the code was doing a byte-at-a-time scan for no
reason, not because a profile said the proxy was slow here.

## The dependency

`memchr`, which has no dependencies of its own, is MIT so the licence gate has
nothing to say about it, and was already in `Cargo.lock` transitively.

Default features, meaning `std`. That is deliberate: without `std`, memchr falls
back to SSE2 alone, because choosing AVX2 needs the runtime feature detection
that lives in `std`. This crate is `std` already, so there was nothing to buy by
turning it off.
