---
name: adr
description: Write an architecture decision record. Use when making a choice that is expensive to reverse, when changing a pgprox-core contract, or when a future reader would otherwise ask "why on earth is it done this way".
---

# Architecture decision record

An ADR captures why, at a moment when the alternatives were still live. Six
months later the code shows what was chosen and nothing shows what was rejected,
which is the part people actually need.

## When to write one

- A choice that is expensive to reverse
- Any `pgprox-core` contract change
- A decision that will look wrong without its context
- Rejecting an obvious approach for a non-obvious reason

Not for choices with a conventional default, and not for anything a code comment
covers.

## Location and numbering

`docs/internal/product/decisions/NNNN-kebab-case-title.md`, numbers never reused. A superseded
record stays and gets `Status: superseded by [NNNN](...)`. Deleting it destroys
the reasoning.

## Structure

```markdown
# NNNN. Title as a statement, not a question

Status: accepted | superseded by [NNNN](...) | proposed

## Context
## Decision
## Consequences
## Alternatives rejected
```

`## Consequences` is checked by `scripts/m-1-complete.sh`. A record without it
is incomplete by definition.

## Writing each section

**Context.** The forces, not the answer. What made this a decision rather than
an obvious call. Include the numbers that mattered: "100k connections at 32 KiB
of buffer each is 3.2 GB" is the whole argument in one line.

**Decision.** Active voice, present tense. "TLS is required on the frontend",
not "we decided that TLS should probably be required."

**Consequences.** Both directions, honestly. The good ones are easy. The ones
worth writing are the costs you are accepting:

> Kernel socket memory does not go away. At 100k sockets, expect 1 to 3 GB
> depending on tcp_rmem and tcp_wmem minimums.

An ADR listing only benefits is marketing. The next person needs to know what
was traded away, because they will hit it.

**Alternatives rejected.** One paragraph each: what it was, its genuine
advantage, and why it lost. State the advantage honestly. An alternative
described as having no upside was never really considered, and the reader will
know.

## The test of a good ADR

Someone arriving in a year, who disagrees with the decision, should be able to
read it and understand why a reasonable person chose otherwise. If they finish
thinking "they clearly didn't consider X", the record failed, whether or not X
was considered.
