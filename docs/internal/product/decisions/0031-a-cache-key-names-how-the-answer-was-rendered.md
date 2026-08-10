# 0031. A cache key names how the answer was rendered, not only which rows it holds

Status: accepted

Amends ADR [0024](0024-a-cache-key-names-the-connection-that-would-have-answered.md),
which amended ADR [0021](0021-the-query-cache-promises-bounded-staleness.md).
Neither is weakened. Bounded staleness is still the whole promise, and this is
again about which question an entry is the answer to.

## Context

ADR 0024 carried one observation two fields further than ADR 0021 had: the same
SQL under two databases names different tables, and under two roles returns
different rows, so both belong in the key. Its closing table has a row for "two
sessions differing in nothing" answering "the same".

That row was not true, and the reason is a step the argument never took. What
the store holds is not rows. It is the bytes the server sent, kept verbatim for
the same reason the relay never parses a `DataRow`. So the key has to name
everything those *bytes* depend on, and rows are only half of that. Two sessions
can agree on every row of an answer and disagree on what the answer looks like.

Two things decided the bytes and were not in the key.

**The result format the `Bind` asked for.** A `Bind` carries a result format
code per column, and the server encodes what it returns accordingly. `SELECT id
FROM t` in text is `52` as two ASCII bytes; in binary it is four bytes of
big-endian integer. `bind_parameters` read the parameter values and skipped the
result format codes, so the two keyed identically. A client that asked for
binary and was served the text entry gets rows it cannot decode, and the
`RowDescription` in the stored payload tells it text while it is expecting
binary.

**The session settings that govern rendering.** Six of the parameters on
`pgprox-pool`'s replay allowlist change how a value is written down without
changing which values there are:

| Setting | What it changes in every answer holding one |
| --- | --- |
| `TimeZone` | every `timestamptz` |
| `DateStyle` | every `date` and `timestamp` |
| `IntervalStyle` | every `interval` |
| `extra_float_digits` | every `float4` and `float8` |
| `bytea_output` | every `bytea` |
| `client_encoding` | every string, which is to say most answers |

Being on the replay allowlist is exactly what makes this reachable: a session
sets one of them, is **not** pinned, keeps the setting across a connection
change, and shares a cache entry with every other session of that tenant,
database and role. `search_path` was already in the key, and it was there for
the narrower reason, which is why the other six were not noticed beside it.

`standard_conforming_strings` is a seventh and a different kind: it decides what
a backslash means inside a string literal, so it changes what the SQL text
means rather than how the answer prints.

Two settings that look like they belong here do not. `role` and
`session_authorization` would be the most serious of the lot, because they
change which rows row-level security shows. They are safe today because they are
absent from the *replay* allowlist, so a session that sets either is pinned and
a pinned session is refused a cache entry. That is two lists happening to agree
rather than a decision, and it is written down in `pgprox-cache`'s settings
module so that adding `role` to the replay allowlist is not a silent
re-opening of this.

## Decision

`CacheKey` names how the answer was rendered.

- `result_formats` carries the codes when any column is binary, and is empty
  when every column is text.
- `settings` replaces `search_path` and carries a canonical fingerprint of the
  settings above that this session has actually set.

Two normalizations do the real work.

**All-text results normalize to empty.** No codes, one code of zero standing for
every column, and a list of zeroes are one request on the wire and must be one
key. Empty is also what a simple query has, because a simple query is always
text, so a text-format `Bind` still shares an entry with the simple query of the
same SQL. That property is `M9.22`'s and this had to leave it standing.

**A session that set nothing fingerprints to empty.** Most sessions set none of
these and share the server's defaults with each other, so the common case pays
one empty `Arc<str>` and no lost hits.

The fingerprint is length-prefixed rather than delimiter-separated. The values
are a tenant's own text, so any delimiter the format uses is a delimiter a
tenant can write: joined by newlines, a session setting `TimeZone` to
`UTC\ndatestyle=ISO` produces the same string as one that set both, and the two
share an entry. The length is what makes the encoding injective. The test that
asserts this failed against the first implementation, which is how the sentence
comes to be here.

The list of settings lives in `pgprox-cache`, beside the rule about which
statements may be cached, rather than in the composition root that builds the
key. There is one rule about what reaches an answer and it should have one home.

## Consequences

The cache holds more entries for a tenant whose sessions do not agree on their
settings, and that is the correct number rather than a regression: those
sessions were never asking the same question. A fleet where every session sets
the same `TimeZone` at connect time keys identically and is unaffected, which is
the common shape.

`application_name` is deliberately excluded despite being replayable. It reaches
no answer, and half the drivers in existence set it per process, so keying on it
would give every application instance a private copy of every entry. The only
way a statement could read it is `current_setting`, which the cacheability rule
already refuses.

`statement_timeout` and `lock_timeout` are excluded because they decide whether
an answer arrives rather than what it says, and a hit does not run. The
`default_transaction_*` settings are excluded because a statement inside a
transaction is refused a key before this is consulted.

This is the third amendment to the same key in the same direction, and the
pattern is worth naming: each time, a field was omitted because the argument
stopped at "would another session see the same rows". The question the key
answers is "would another session see the same bytes", and it is a strictly
wider one. A fourth field will be found the same way if that distinction is not
the first thing a reader meets, which is why it is now the first thing
`pgprox_core::cache`'s module documentation says.
