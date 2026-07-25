---
name: skill-forge
description: Create, review, or validate a skill for this repository. Use when asked to add a skill, when a procedure keeps being re-explained across sessions, or when an existing skill is not firing when it should.
---

# Building skills

A skill is a procedure an agent follows. It is not documentation and it is not a
standard.

## Skill, standard, or neither

- **A rule that is always true** goes in `standards/`. "No unwrap outside tests"
  is a rule.
- **A procedure with steps** is a skill. "Change a contract safely" is a
  procedure.
- **A fact about the system** goes in `product/`, or a code comment.

If you cannot write it as steps, it is not a skill. Skills that are really
documentation never get invoked, because the model has nothing to *do* with
them.

## Format

Portable by standard. `SKILL.md` is read unchanged by around forty tools, so
nothing here should be specific to one of them.

```
.agents/skills/<name>/SKILL.md
```

```markdown
---
name: kebab-case-name
description: What it does, and when to use it. Both halves matter.
---

# Title

Body: steps, in order.
```

## The description is the whole retrieval surface

Until a skill fires, its description is the only thing loaded. Everything about
whether it gets used lives in that one line.

Write it as **what it does plus when to use it**, and include the words someone
would actually say:

> Bad:  "Helps with testing."
> Bad:  "A comprehensive skill for test-driven development workflows."
> Good: "Write a failing test first, watch it fail, then implement. Use when
>        adding any behaviour to a pgprox crate, fixing a bug, or when a task's
>        acceptance criteria describe observable behaviour."

The second half is doing most of the work. A description with no trigger
conditions will not fire.

## Vendor neutrality is enforced

`scripts/check-drift.sh` fails a skill whose body references a tool-specific
path. Skills live in one canonical directory and each tool's discovery path is a
symlink to it.

Where a skill needs to run something, name a script in `scripts/`. Script
invocation is the one capability every coding agent has, and a skill calling a
tool-specific built-in works in exactly one tool.

## Writing the body

- **Imperative and ordered.** Steps, not prose about steps.
- **Short.** A long skill gets skimmed, and the part that gets skipped is the
  part that mattered. Push detail into `standards/` and link.
- **Say why for anything counterintuitive.** "Watch it fail" reads like
  ceremony until you explain that a test passing before the implementation
  exists is testing nothing.
- **Concrete examples over description.** One code block beats a paragraph.
- **Include the failure mode.** What goes wrong when the procedure is skipped is
  often what makes an agent follow it.

## Validate before committing

```bash
scripts/check-drift.sh
```

Checks frontmatter is present and well-formed, `name` and `description` exist,
and the body has no vendor-specific paths.

Then check it actually fires. Write down three prompts a person would plausibly
use, and confirm the description would match them. A skill nobody invokes is
worse than no skill, because it looks like the procedure is covered.

## Reviewing an existing skill

- [ ] Description states both what and when
- [ ] Description contains the words a user would actually type
- [ ] Body is steps, not documentation
- [ ] No vendor-specific paths
- [ ] Runs scripts rather than built-ins
- [ ] Nothing duplicated from `standards/`, only linked
- [ ] Short enough to be read rather than skimmed
