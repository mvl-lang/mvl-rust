#!/usr/bin/env python3
"""mvl-rust Assurance Checker — scenario-level ISPE traceability.

ISPE (Intent, Specification, Program, Evidence). Intent is tickets,
Specification is `.openspec/specs/`, Program is `crates/`, Evidence is tests.

**The unit of measurement is the scenario, not the requirement.** A requirement
is an umbrella claim; a scenario is a falsifiable one, and GIVEN/WHEN/THEN maps
onto arrange/act/assert closely enough that a scenario can have a 1:1 test.
Measuring at requirement level let one test stand in for five scenarios, which
inflated coverage to 100% while nothing tied any individual scenario to
anything at all.

A scenario is **covered** when it carries its own `**Tests:**` link and every
file and test function named there actually exists. Nothing else counts: a
requirement-level test link is ignored, and so is a link that does not resolve.

There is deliberately no "planned" exclusion. A requirement written into a spec
is defined, and its scenarios are obligations. Letting a marker remove them from
the denominator meant declaring intent improved the score.

Reported:
  - scenarios covered / total  (the headline)
  - per-spec covered fraction
  - specs fully covered

Line coverage and raw test counts are deliberately absent — they measure the
program against itself and say nothing about S, E, or the links between them.
Use `make coverage` for those.

Usage:
    python3 tools/assurance.py            # dashboard
    python3 tools/assurance.py --verbose  # per-scenario detail
    python3 tools/assurance.py --min 0.75 # CI gate on scenario coverage
"""

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
SPEC_DIR = REPO_ROOT / ".openspec" / "specs"

REQ_HEADER = re.compile(r"^### Requirement (\d+): (.+?) \[([A-Z][A-Z ]*)\]", re.M)
SCEN_SPLIT = re.compile(r"(?=^#### Scenario:)", re.M)
SCEN_TITLE = re.compile(r"^#### Scenario:\s*(.+)", re.M)
TESTS_LINE = re.compile(r"\*\*Tests:\*\*\s*(.+)")
IMPL_LINE = re.compile(r"\*\*Implementation:\*\*\s*(.+)")


def resolve_tests(raw):
    """Resolve a **Tests:** line to concrete files and test functions.

    Returns (resolved, missing). A link is evidence only if it points at
    something that exists, so both the file and each named `fn` are checked.
    A bare `::name` continues the previously named file. A file named with no
    test function is not scenario-level evidence and is reported as missing.
    """
    if not raw:
        return [], []
    resolved, missing, current = [], [], None
    for ref in re.findall(r"`([^`]+)`", raw):
        ref = ref.strip()
        head = ref.split("::")[0]
        if "::" in ref and "/" not in head and not head.endswith(".rs"):
            path, fns = current, ref.split("::")
        elif "::" in ref:
            parts = ref.split("::")
            path, fns = parts[0], parts[1:]
            current = path
        else:
            path, fns, current = ref, [], ref
        if path is None:
            missing.append(ref)
            continue
        target = (REPO_ROOT / path).resolve()
        if not (target.is_relative_to(REPO_ROOT.resolve()) and target.exists()):
            missing.append(path)
            continue
        if not fns:
            missing.append(f"{path} (no test fn named)")
            continue
        body = target.read_text(errors="replace")
        for fn in (f.strip() for f in fns):
            if not fn or fn == "tests":
                continue
            if re.search(r"\bfn\s+" + re.escape(fn) + r"\s*\(", body):
                resolved.append(f"{path}::{fn}")
            else:
                missing.append(f"{path}::{fn}")
    return resolved, list(dict.fromkeys(missing))


def parse_specs():
    """Return (specs, scenarios). One scenario dict per `#### Scenario:`."""
    specs, scenarios = [], []
    for spec_dir in sorted(SPEC_DIR.iterdir()):
        spec_file = spec_dir / "spec.md"
        if not spec_file.exists():
            continue
        text = spec_file.read_text()
        n_reqs = 0
        for block in re.split(r"(?=^### Requirement \d+)", text, flags=re.M):
            m = REQ_HEADER.match(block)
            if not m:
                continue
            n_reqs += 1
            req_num, req_title = int(m.group(1)), m.group(2)
            impl = IMPL_LINE.search(block)
            impl_paths = re.findall(r"`([^`]+)`", impl.group(1)) if impl else []
            for sb in SCEN_SPLIT.split(block)[1:]:
                t = SCEN_TITLE.match(sb)
                title = t.group(1).strip() if t else "(untitled)"
                tline = TESTS_LINE.search(sb)
                resolved, missing = resolve_tests(tline.group(1) if tline else None)
                scenarios.append({
                    "spec": spec_dir.name,
                    "req": req_num,
                    "req_title": req_title,
                    "title": title,
                    "linked": tline is not None,
                    "resolved": resolved,
                    "missing": missing,
                    "covered": bool(resolved) and not missing,
                    "impl_paths": impl_paths,
                })
        specs.append({"name": spec_dir.name, "reqs": n_reqs})
    return specs, scenarios


def report(specs, scenarios, verbose=False):
    total = len(scenarios)
    if total == 0:
        print("No scenarios found in .openspec/specs/")
        return 0.0

    covered = [s for s in scenarios if s["covered"]]
    broken = [s for s in scenarios if s["missing"]]
    unlinked = [s for s in scenarios if not s["linked"]]
    coverage = len(covered) / total

    per_spec = {}
    for s in scenarios:
        d = per_spec.setdefault(s["spec"], [0, 0])
        d[1] += 1
        if s["covered"]:
            d[0] += 1
    fully = [n for n, (c, t) in per_spec.items() if t and c == t]
    n_reqs = sum(sp["reqs"] for sp in specs)

    print("=" * 68)
    print("mvl-rust Assurance Dashboard (ISPE — scenario level)")
    print("=" * 68)
    print(f"Specs:                 {len(specs)}")
    print(f"Requirements:          {n_reqs}")
    print(f"Scenarios:             {total}")
    print()
    print(f"Scenarios covered:     {len(covered)}/{total}  ({coverage:.0%})")
    print(f"  - no test link:      {len(unlinked)}")
    print(f"  - link unresolved:   {len(broken)}")
    print()
    print(f"Specs fully covered:   {len(fully)}/{len(specs)}")
    print()
    print("Per spec:")
    for name in sorted(per_spec):
        c, t = per_spec[name]
        filled = round(20 * c / t) if t else 0
        bar = "#" * filled + "." * (20 - filled)
        mark = " *" if t and c == t else ""
        print(f"  {name:<30} {c:>3}/{t:<3} {bar} {c / t:>4.0%}{mark}")
    print("=" * 68)

    if broken:
        print()
        print(f"UNRESOLVED LINKS ({sum(len(s['missing']) for s in broken)}):")
        for s in broken:
            for miss in s["missing"]:
                print(f"  {s['spec']}/Req {s['req']} — {s['title'][:40]}: {miss}")
        print("=" * 68)

    if verbose:
        print()
        print("  Legend: [x]=covered  [ ]=no test link  [!]=link unresolved")
        last = None
        for s in scenarios:
            if s["spec"] != last:
                print(f"\n  {s['spec']}")
                last = s["spec"]
            mark = "x" if s["covered"] else "!" if s["missing"] else " "
            print(f"    [{mark}] Req {s['req']}: {s['title'][:56]}")
            for r in s["resolved"]:
                print(f"          -> {r}")
        print()

    return coverage


def main():
    ap = argparse.ArgumentParser(description="mvl-rust Assurance Checker")
    ap.add_argument("-v", "--verbose", action="store_true", help="Per-scenario detail")
    ap.add_argument("--min", type=float, default=0.0, help="CI gate on scenario coverage")
    args = ap.parse_args()

    specs, scenarios = parse_specs()
    coverage = report(specs, scenarios, verbose=args.verbose)

    if any(s["missing"] for s in scenarios):
        n = sum(len(s["missing"]) for s in scenarios)
        print(f"\nFAIL: {n} unresolved Tests: link(s) — see above")
        sys.exit(1)

    if args.min > 0:
        if coverage < args.min:
            print(f"\nFAIL: scenario coverage {coverage:.0%} below threshold {args.min:.0%}")
            sys.exit(1)
        print(f"\nPASS: scenario coverage {coverage:.0%} above threshold {args.min:.0%}")


if __name__ == "__main__":
    main()
