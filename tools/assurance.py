#!/usr/bin/env python3
"""mvl-rust Assurance Dashboard — the case, and the three levels under it.

**Assurance** is the argument that this software is fit for its purpose. It is
not a measurement; it is what the measurements are marshalled into. Three levels
support it, each answering a distinct question with its own verb and artefact
(ADR-0007):

  VERIFICATION   does the program satisfy its specification?   verdicts
  TRACEABILITY   do intent, spec, program and evidence connect? link ratios
  EVIDENCE       what artefacts back the claims?                records

**Compliance is not a fourth level.** It is downstream: you build one assurance
case and map it onto N standards (DO-178C, ISO 26262, CRA). Compliance consumes
the case; it is not part of it.

ISPE (Intent, Specification, Program, Evidence) supplies the traceability layer.
Intent is tickets, Specification is `.openspec/specs/`, Program is `crates/`,
Evidence is tests.

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

Line coverage is **not** an ISPE link — it measures the program against itself.
It is reported anyway because its interaction with scenario coverage says what
work to do next, which a single ratio cannot:

    scenario LOW  + line LOW   -> the tests do not exist. WRITE TESTS.
                                  Linking cannot help; there is nothing to link.
    scenario LOW  + line HIGH  -> the tests exist but are not tied to scenarios.
                                  LINK TESTS. This is traceability work, not
                                  engineering work.
    scenario HIGH + line LOW   -> scenarios are linked but much of the program is
                                  unexercised. Either the linked tests are
                                  shallow, or there is code no scenario covers.
    scenario HIGH + line HIGH  -> healthy.

Two further gates, both hard failures independent of any ratio:
  - the workspace must compile (`make compile`, run before this script)
  - any `**Tests:**` link that does not resolve

Usage:
    python3 tools/assurance.py            # dashboard
    python3 tools/assurance.py --verbose  # per-scenario detail
    python3 tools/assurance.py --min 0.75 --min-coverage 0.80
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


def line_coverage():
    """Read cached llvm-cov line coverage. Returns (pct, covered, total) or None.

    Deliberately does not run coverage itself -- `make coverage` produces the
    cache. A missing cache means the diagnostic below cannot be computed, which
    is reported rather than silently treated as passing.
    """
    cache = REPO_ROOT / "target" / "llvm-cov.json"
    if not cache.exists():
        return None
    try:
        import json
        lines = json.loads(cache.read_text())["data"][0]["totals"]["lines"]
        return lines["percent"] / 100.0, lines["covered"], lines["count"]
    except (ValueError, KeyError, IndexError):
        return None


def compile_status(run):
    """VERIFICATION, cheapest signal: does the workspace compile at all?

    If it does not, every downstream number is meaningless -- a green
    traceability ratio over code that will not build is worse than no number.
    Only run when asked (the gate asks); the dashboard defaults to naming the
    target rather than paying the subprocess.
    """
    if not run:
        return None
    import subprocess
    try:
        r = subprocess.run(
            ["cargo", "check", "--workspace", "--all-targets", "--quiet"],
            capture_output=True, text=True, timeout=300, cwd=REPO_ROOT,
        )
        return r.returncode == 0
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None


def test_count():
    """EVIDENCE: how many tests exist. Read from the coverage cache's sibling
    marker if present, else counted via cargo. None when unavailable."""
    import subprocess
    try:
        r = subprocess.run(
            ["cargo", "test", "--workspace", "--", "--list"],
            capture_output=True, text=True, timeout=180, cwd=REPO_ROOT,
        )
        if r.returncode == 0:
            n = sum(1 for ln in r.stdout.splitlines() if ": test" in ln)
            return n or None
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    return None


def verdict(scen, line, scen_min, line_min):
    """The 2x2: what work does this state actually call for?"""
    if line is None:
        return ("UNKNOWN", "no coverage cache — run `make coverage` to get the diagnostic")
    lo_s, lo_l = scen < scen_min, line < line_min
    if lo_s and lo_l:
        return ("WRITE TESTS",
                "evidence does not exist yet; linking cannot help because there is "
                "nothing to link")
    if lo_s and not lo_l:
        return ("LINK TESTS",
                "the tests exist but scenarios are not tied to them — traceability "
                "work, not engineering work")
    if not lo_s and lo_l:
        return ("BROADEN TESTS",
                "scenarios are linked but much of the program is unexercised: either "
                "the linked tests are shallow, or there is code no scenario covers")
    return ("HEALTHY", "scenarios are evidenced and the program is exercised")


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


def report(specs, scenarios, verbose=False, scen_min=0.75, line_min=0.80,
           with_compile=False):
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

    cov = line_coverage()
    compiles = compile_status(with_compile)

    print("=" * 68)
    print("mvl-rust Assurance Dashboard")
    print("=" * 68)
    print("The case: is this software fit for its purpose? Three levels below it.")
    print("Compliance is downstream — it consumes this case, it is not part of it.")
    print()

    print("VERIFICATION   does the program satisfy its specification?")
    if compiles is True:
        print("  Compiles:            yes")
    elif compiles is False:
        print("  Compiles:            NO — every number below is meaningless")
    else:
        print("  Compiles:            not checked here — `make compile`")
    print(f"  Tool verdicts:       `make examples` (paired compliant/violating per tool)")
    print()

    print("TRACEABILITY   do intent, spec, program and evidence connect?")
    print(f"  Specs:               {len(specs)}")
    print(f"  Requirements:        {n_reqs}")
    print(f"  Scenarios:           {total}")
    print(f"  Scenarios covered:   {len(covered)}/{total}  ({coverage:.0%})")
    print(f"    no test link:      {len(unlinked)}")
    print(f"    link unresolved:   {len(broken)}")
    print(f"  Specs fully covered: {len(fully)}/{len(specs)}")
    print()

    print("EVIDENCE       what artefacts back the claims?")
    if cov:
        pct, c, t = cov
        print(f"  Line coverage:       {c}/{t}  ({pct:.0%})")
    else:
        print("  Line coverage:       no cache — run `make coverage`")
    tests = test_count() if with_compile else None
    if tests:
        print(f"  Tests:               {tests}")
    print(f"  Per-tool records:    --emit-verification-json (five tools)")
    print()

    v, why = verdict(coverage, cov[0] if cov else None, scen_min, line_min)
    print(f"  NEXT WORK: {v}")
    print(f"    {why}")
    print()
    print("Traceability per spec:")
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

    return coverage, cov[0] if cov else None, compiles


def main():
    ap = argparse.ArgumentParser(description="mvl-rust Assurance Checker")
    ap.add_argument("-v", "--verbose", action="store_true", help="Per-scenario detail")
    ap.add_argument("--min", type=float, default=0.0, help="CI gate on scenario coverage")
    ap.add_argument("--min-coverage", type=float, default=0.0,
                    help="CI gate on line coverage (requires `make coverage` cache)")
    ap.add_argument("--with-compile", action="store_true",
                    help="run cargo check and count tests (the gate does; the dashboard doesn't)")
    args = ap.parse_args()

    specs, scenarios = parse_specs()
    coverage, line, compiles = report(specs, scenarios, verbose=args.verbose,
                                      scen_min=args.min or 0.75,
                                      line_min=args.min_coverage or 0.80,
                                      with_compile=args.with_compile)

    if any(s["missing"] for s in scenarios):
        n = sum(len(s["missing"]) for s in scenarios)
        print(f"\nFAIL: {n} unresolved Tests: link(s) — see above")
        sys.exit(1)

    failed = False
    if compiles is False:
        print("\nFAIL: workspace does not compile — fix that before reading any ratio")
        sys.exit(1)
    if args.min > 0 and coverage < args.min:
        print(f"\nFAIL: scenario coverage {coverage:.0%} below threshold {args.min:.0%}")
        failed = True
    if args.min_coverage > 0:
        if line is None:
            print("\nFAIL: line-coverage gate requested but no cache — run `make coverage`")
            failed = True
        elif line < args.min_coverage:
            print(f"\nFAIL: line coverage {line:.0%} below threshold {args.min_coverage:.0%}")
            failed = True
    if failed:
        sys.exit(1)
    if args.min > 0 or args.min_coverage > 0:
        parts = [f"scenario coverage {coverage:.0%}"]
        if line is not None:
            parts.append(f"line coverage {line:.0%}")
        print(f"\nPASS: {', '.join(parts)}")


if __name__ == "__main__":
    main()
