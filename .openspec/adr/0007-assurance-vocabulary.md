---
status: Accepted
date: 2026-07-29
---

# ADR-0007: Assurance Vocabulary — the Case and Three Levels Below It

## Context

"Assurance" had drifted into naming four incompatible things at once. All four were live simultaneously in this workspace:

| Usage | What it actually was |
|---|---|
| `tools/assurance.py`, `make assurance` | a **traceability** measurement |
| `AssuranceReport` / `AssuranceSection` / `AssuranceLeaf` | the report envelope and the case |
| `--emit-assurance-json`, `run_assurance_mode` | one tool's **verification** verdicts |
| spec `008-assurance-reporting` (now `008-reporting`) | the reporting concern across all of them |

The ISPE model makes it worse rather than better: it uses "assurance" for one specific ratio, the E→S link. So the word named both the whole idea and a third of one part of it.

The cost is not aesthetic. When a reader sees "assurance: 100%" they cannot tell whether that means the program was verified, the artefacts are connected, or evidence exists — and those are different claims with different consequences for a certification audience.

## Decision

### 1. Assurance is the argument; three levels support it

Each level answers a distinct question and has its own verb and its own artefact. That is the test for whether something *is* a level: two things sharing a verb are one level.

| Level | Question | Verb | Artefact |
|---|---|---|---|
| **Assurance case** | why should you believe this is fit for purpose? | argue | a case (claim → argument → evidence) |
| **Verification** | does the program satisfy its specification? | verify | verdicts |
| **Traceability** | do intent, spec, program and evidence connect? | trace | link ratios |
| **Evidence** | what artefacts back the claims? | collect | records |

Assurance is the only one of the four that is an **argument**. The other three are measurements or artefacts. That is why it sits on top, and why it must not name any of them.

### 2. Compliance is downstream, not a fourth level

Compliance asks whether a case satisfies a named standard — DO-178C, ISO 26262, EU CRA. One case maps onto N standards; compliance **consumes** the case rather than composing it.

```
Assurance case ──map──> Compliance report (DO-178C)
               ──map──> Compliance report (ISO 26262)
```

Consequence worth recording: compliance being thin here is **not a gap**. It is a mapping layer nothing has needed yet, and building it early would mean writing a translator for a document nobody has requested.

### 3. The dashboard reports all three levels, and each level is runnable

`tools/assurance.py` keeps its name, because its scope grew to match it: it now reports verification, traceability and evidence under one heading, which *is* assembling the case. It prints compliance's absence as a deliberate boundary rather than an omission.

Within it, traceability's unit is the **scenario** rather than the requirement (see spec 001 and the trace section of the dashboard) — a requirement is an umbrella claim, a scenario is a falsifiable one.

**The taxonomy is executable, not just documented.** Each level has its own `make` target, so it can be run and can fail on its own:

| Target | Level | Artefact |
|---|---|---|
| `make verify` | VERIFICATION — `compile` + `test` + `examples` | verdicts |
| `make traceability` | TRACEABILITY — scenario→test links | ratios |
| `make evidence` | EVIDENCE — `coverage` | records |
| `make assurance` | the case — all three under one heading | a case |
| `make assurance-gate` | the case, as a gate | — |
| `make all` | hygiene + all three levels + the gate | — |

That the levels fail independently is the point: verification green with traceability red is working code nobody can trace to a requirement, and the reverse is perfect paperwork over broken code. A single ratio reports them identically.

`make traceability` deliberately needs neither cargo nor a coverage cache — it is the fastest check and the one most likely to break in a PR that renames a test.

**Hygiene (`fmt-check`, `clippy`) is not a level.** It gates whether the code is well-formed enough to be worth verifying. Folding it into verification would make "verification: pass" mean two unrelated things.

**There is no `compliance` target**, for the reason in §2. The previous one was a synonym for "run everything" — now `make all` — and naming that "compliance" was exactly the overloading this ADR exists to stop.

CI mirrors the same structure: a `hygiene` job, a `verification` matrix job (stable + MSRV, with `examples` on stable only), and an `assurance` job that `needs: [verification]` — because a green traceability ratio over code that does not build looks like progress.

### 4. Renames applied

| Before | After | Why |
|---|---|---|
| `--emit-assurance-json` | `--emit-verification-json` | each tool emits its own `check` section — diagnostics are **verdicts**, not a case and not evidence |
| `run_assurance_mode` | `run_verification_mode` | same |
| `crates/*/tests/assurance_mode.rs` | `tests/verification_mode.rs` | same, ×5 crates |
| `assurance_mode_*` / `emits_valid_assurance_json_*` test fns | `verification_mode_*` / `emits_valid_verification_json_*` | same |

### 5. Names deliberately kept

These were already correct, and renaming them would have made the vocabulary worse:

- **`AssuranceReport` keeps its name.** It is the envelope carrying `check` (verification), `prove` (verification), `test`/`mcdc`/`coverage` (evidence) and `assurance` (the case). It is not a per-tool record — it is the case document.
- **`AssuranceSection` / `AssuranceLeaf` keep their names.** `claim` + `argument_tree` + `leaves` is precisely the case.
- **`cargo mvl assurance` keeps its name.** It aggregates the sections into a case — the one subcommand that genuinely earns the word.

> **Correction to an earlier proposal.** The first draft of this vocabulary
> (recorded in `~/wc/my-brain/projects/mvl/assurance-vocabulary.md`) proposed
> renaming `AssuranceReport` → `EvidenceReport` and `tools/assurance.py` →
> `traceability.py`. Both were wrong, and reading `schema.rs` is what showed
> why: `AssuranceReport` is the envelope over all three levels plus the case,
> not a per-tool evidence record, and the dashboard's scope is all three levels
> rather than traceability alone. The misnamed thing was the per-tool emitter
> (§4), which the first draft left untouched. Recorded because the reasoning
> that produced the wrong answer — inferring a type's role from its name rather
> than its fields — is the same reasoning this ADR exists to stop.

## Consequences

- **`--emit-assurance-json` is gone with no alias.** A breaking CLI change for anyone scripting it. Acceptable under ADR-0001 §5 (greenfield, no grandfathering) and at v0.1.x, but it is a real break and belongs in release notes.
- **The dashboard cannot fully measure verification.** It reports compile status when asked (`--with-compile`, which `make assurance-gate` passes) and otherwise names the targets that do the measuring — `make compile`, `make examples`, `make test`. Reporting a verification ratio it did not compute would be exactly the overclaiming this ADR is against.
- **A non-compiling workspace now fails the gate before any ratio is read.** A green traceability number over code that will not build is worse than no number, because it looks like progress.
- **"Assurance" now has exactly one meaning in this workspace**, and the ISPE E→S ratio is scoped as a traceability sub-metric rather than owning the word.
- **The same collision exists upstream** in `mvl-lang/mvl` — `mvl assurance` CLI, `make assurance`, `tools/assurance.py`, spec `023-assurance`. Applying this vocabulary there too is preferable to letting the two implementations diverge on what the word means; not attempted here.
- **`ProvenObligationRecord` was still misnamed** — it held non-proven outcomes. Was out of scope here; fixed in #56, which renamed it `ObligationRecord` and added the `kind` discriminator that separates coherence, entailment, and residual.

## Links

- `mvl-lang/mvl-rust`#56 (obligation-kind labelling — the remaining misnomer)
- ADR-0001 §5 (greenfield rule, which permits the un-aliased CLI break), ADR-0006 §5 (provenance must distinguish enforced from proven — the same honesty concern at the record level)
- Spec `001-system-overview`, spec `008-reporting`
- `~/wc/my-brain/projects/mvl/assurance-vocabulary.md` (the working note, including the corrected proposal)
