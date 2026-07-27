#!/usr/bin/env python3
"""Turn one instrumented replay into the three lists.

Reads `cargo llvm-cov report --json`, which carries an execution count per
function rather than the hit/miss booleans a coverage percentage is made of,
and writes the report `scripts/profile.sh` commits.

The lists come from `standards/testing.md`:

* hot and under-tested   ran a lot, poorly covered. The highest-risk code here.
* hot and expensive       ran a lot, and is big. The optimization queue.
* cold and complex        never ran, and is big. Candidates for deletion.

Nothing here decides anything. It sorts, and a human reads.
"""

import json
import subprocess
import sys

# How many entries each list carries. Enough to act on, few enough to read.
TOP = 25

# A function this small is a getter, and a getter running a million times is
# not a finding. Applied to the hot lists only: a big cold function is exactly
# what the third list is for.
MIN_REGIONS = 4

# Below this, a function that runs constantly is worth a test before anything
# else is done to it.
UNDER_TESTED_PERCENT = 80.0


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def functions(coverage):
    """Every function in the workspace's own crates, with its count."""
    out = []
    for export in coverage.get("data", []):
        for function in export.get("functions", []):
            filenames = function.get("filenames", [])
            if not filenames:
                continue
            path = filenames[0]
            # Dependencies are not this project's to optimize, and the
            # registry path is how they are told apart.
            if "/.cargo/" in path or "/rustc/" in path:
                continue
            if "/tests/" in path or "/benches/" in path:
                continue

            regions = function.get("regions", [])
            covered = sum(1 for region in regions if region[4] > 0)
            out.append(
                {
                    "name": function.get("name", "?"),
                    "file": trim(path),
                    "count": function.get("count", 0),
                    "regions": len(regions),
                    "percent": 100.0 * covered / len(regions) if regions else 0.0,
                }
            )
    names = demangle([f["name"] for f in out])
    for entry, name in zip(out, names):
        entry["name"] = shorten(name)
    return out


def demangle(names):
    """Readable names, through rustfilt when it is installed.

    Mangled v0 symbols are unreadable enough that a report full of them is a
    report nobody acts on, so this is worth a subprocess. Without rustfilt the
    names pass through unchanged rather than the report failing: a mangled name
    plus a file is still actionable.
    """
    try:
        result = subprocess.run(
            ["rustfilt"],
            input="\n".join(names),
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return names
    out = result.stdout.split("\n")
    return out if len(out) >= len(names) else names


def shorten(name):
    """Drop the generic arguments and the trailing hash, keep the path."""
    depth = 0
    kept = []
    for char in name:
        if char == "<":
            depth += 1
        elif char == ">":
            depth = max(0, depth - 1)
        elif depth == 0:
            kept.append(char)
    name = "".join(kept)
    parts = [part for part in name.split("::") if part]
    if parts and parts[-1].startswith("h") and len(parts[-1]) == 17:
        parts = parts[:-1]
    return "::".join(parts[-3:]) if parts else name


def coverage_by_function(report):
    """Percent of regions covered per function, from a tier-1 coverage run."""
    out = {}
    for export in report.get("data", []):
        for function in export.get("functions", []):
            regions = function.get("regions", [])
            if not regions:
                continue
            covered = sum(1 for region in regions if region[4] > 0)
            name = shorten(demangle([function.get("name", "?")])[0])
            percent = 100.0 * covered / len(regions)
            # The best coverage any instantiation of the name achieves: a
            # generic function compiled twice should not look untested because
            # one instantiation was not exercised.
            out[name] = max(out.get(name, 0.0), percent)
    return out


def trim(path):
    for marker in ("/crates/", "/bin/"):
        if marker in path:
            return path[path.index(marker) + 1 :]
    return path


def table(rows, columns):
    lines = ["| " + " | ".join(name for name, _ in columns) + " |"]
    lines.append("| " + " | ".join("---" for _ in columns) + " |")
    for row in rows:
        lines.append("| " + " | ".join(render(row) for _, render in columns) + " |")
    return "\n".join(lines)


def main():
    coverage = load(sys.argv[1])
    load_report = load(sys.argv[2])
    connections, seconds = sys.argv[3], sys.argv[4]
    # Optional: what tier 1 covers, so "under-tested" can mean what
    # `standards/testing.md` says rather than "this replay did not reach it".
    tested = coverage_by_function(load(sys.argv[5])) if len(sys.argv) > 5 else {}

    every = functions(coverage)
    for entry in every:
        entry["tested"] = tested.get(entry["name"])
    ran = [f for f in every if f["count"] > 0 and f["regions"] >= MIN_REGIONS]
    ran.sort(key=lambda f: f["count"], reverse=True)

    # Hot, this replay did not cover it, *and* the test suite does not either.
    # A function at 18% here may be fully tested and merely not exercised by
    # this workload, and every crate holds 95% from tier 1, so without the
    # second condition this list is mostly a list of workload gaps.
    def under(entry):
        if entry["percent"] >= UNDER_TESTED_PERCENT:
            return False
        return entry["tested"] is None or entry["tested"] < UNDER_TESTED_PERCENT

    under_tested = [f for f in ran if under(f)][:TOP]
    # Count times size: a function that runs a million times and is one branch
    # contributes less than one that runs a hundred thousand times and is fifty.
    expensive = sorted(ran, key=lambda f: f["count"] * f["regions"], reverse=True)[:TOP]
    cold = [f for f in every if f["count"] == 0 and f["regions"] >= 10]
    cold.sort(key=lambda f: f["regions"], reverse=True)
    cold = cold[:TOP]

    latency = load_report.get("latency", {})
    print(f"""# Semantic coverage

What the reference workload actually runs, by execution count rather than by
hit or miss. Written by `scripts/profile.sh`; do not edit by hand.

The replay: {connections} connections for {seconds}s against the local
one-node stack, workload version {load_report.get('workload_version')}, seed
{load_report.get('seed')}. It completed {load_report.get('transactions')}
transactions with {load_report.get('errors')} errors, p50
{latency.get('p50_us')}us and p99 {latency.get('p99_us')}us.

{len(every)} functions in this workspace's own crates were compiled into the
profiled binary. {len(ran)} of them ran at least once and are big enough to be
worth a line here.

## Hot and under-tested

High execution count, and under {UNDER_TESTED_PERCENT:.0f}% of their regions
covered *both* by this replay and by the tier-1 test suite. The second column
is the workload's reach and the third is the suite's; a function the suite
covers is not under-tested, however little of it this particular replay
touched. What is left runs constantly and nothing exercises it, which is the
highest-risk code in the repository. A dash means the suite never compiled
that instantiation at all.

{table(under_tested, [
    ("Function", lambda f: f"`{f['name']}`"),
    ("File", lambda f: f["file"]),
    ("Count", lambda f: f"{f['count']:,}"),
    ("Replay", lambda f: f"{f['percent']:.0f}%"),
    ("Tier 1", lambda f: "-" if f["tested"] is None else f"{f['tested']:.0f}%"),
])}

## Hot and expensive

Execution count times region count: the optimization queue, ordered by total
contribution rather than by which code looks interesting. A number here is not
a defect. It is where a saved instruction is worth the most.

{table(expensive, [
    ("Function", lambda f: f"`{f['name']}`"),
    ("File", lambda f: f["file"]),
    ("Count", lambda f: f"{f['count']:,}"),
    ("Regions", lambda f: str(f["regions"])),
])}

## Cold and complex

Never ran during the replay, and large. Speculative optimization and dead
paths both look like this. Each one is either a case the workload does not
cover, which is a gap in the workload, or code nobody needs, which is a
deletion.

{table(cold, [
    ("Function", lambda f: f"`{f['name']}`"),
    ("File", lambda f: f["file"]),
    ("Regions", lambda f: str(f["regions"])),
])}
""")


if __name__ == "__main__":
    main()
