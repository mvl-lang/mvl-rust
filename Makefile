.PHONY: help build test check fmt fmt-check clippy examples examples-verbose clean \
	test-core test-limit test-total test-refine test-effect test-ifc test-cargo-mvl \
	coverage assurance assurance-gate compile verify evidence traceability all

help:
	@echo "Targets:"
	@echo "  build              cargo build --workspace --all-targets"
	@echo "  test               cargo test --workspace"
	@echo "  test-core          test mvl-rust-core only"
	@echo "  test-limit         test rust-limit only"
	@echo "  test-total         test rust-total only"
	@echo "  test-refine        test rust-refine only"
	@echo "  test-effect        test rust-effect only"
	@echo "  test-ifc           test rust-ifc only"
	@echo "  test-cargo-mvl     test cargo-mvl only"
	@echo "  fmt                cargo fmt (workspace + example crates)"
	@echo "  fmt-check          cargo fmt --check (workspace + example crates)"
	@echo "  clippy             cargo clippy --workspace --all-targets"
	@echo "  examples           build+run hello-world, rust-limit-demo, rust-total-demo, rust-refine-demo, rust-effect-demo, rust-ifc-demo, check all (summary only)"
	@echo "  examples-verbose   same, but print the violating example's full diagnostics"
	@echo "  check              fmt-check + clippy + test + examples (mirrors CI)"
	@echo ""
	@echo "Assurance (the case) and the three levels below it — see ADR-0007:"
	@echo "  assurance          dashboard across all three levels (VERBOSE=true for per-scenario)"
	@echo "  assurance-gate     the same as a CI gate (thresholds + hard failures)"
	@echo "  verify             VERIFICATION  compile + test + examples -> verdicts"
	@echo "  evidence           EVIDENCE      coverage -> records"
	@echo "  traceability       TRACEABILITY  scenario->test links -> ratios (fast, no cargo)"
	@echo "  all                everything: hygiene + all three levels + gate"
	@echo ""
	@echo "  compile            fail fast: does the workspace compile at all?"
	@echo "  coverage           cargo llvm-cov line/function coverage (cached in target/llvm-cov.json)"
	@echo "  clean              cargo clean (workspace + example crates)"

build:
	cargo build --workspace --all-targets

test:
	cargo test --workspace

test-core:
	cargo test -p mvl-rust-core

test-limit:
	cargo test -p rust-limit

test-total:
	cargo test -p rust-total

test-refine:
	cargo test -p rust-refine

test-effect:
	cargo test -p rust-effect

test-ifc:
	cargo test -p rust-ifc

test-cargo-mvl:
	cargo test -p cargo-mvl

fmt:
	cargo fmt
	cd examples/hello-world && cargo fmt
	cd examples/rust-limit-demo/compliant && cargo fmt
	cd examples/rust-limit-demo/violating && cargo fmt
	cd examples/rust-total-demo/compliant && cargo fmt
	cd examples/rust-total-demo/violating && cargo fmt
	cd examples/rust-refine-demo/compliant && cargo fmt
	cd examples/rust-refine-demo/violating && cargo fmt
	cd examples/rust-effect-demo/compliant && cargo fmt
	cd examples/rust-effect-demo/violating && cargo fmt
	cd examples/rust-ifc-demo/compliant && cargo fmt
	cd examples/rust-ifc-demo/violating && cargo fmt

fmt-check:
	cargo fmt --check
	cd examples/hello-world && cargo fmt --check
	cd examples/rust-limit-demo/compliant && cargo fmt --check
	cd examples/rust-limit-demo/violating && cargo fmt --check
	cd examples/rust-total-demo/compliant && cargo fmt --check
	cd examples/rust-total-demo/violating && cargo fmt --check
	cd examples/rust-refine-demo/compliant && cargo fmt --check
	cd examples/rust-refine-demo/violating && cargo fmt --check
	cd examples/rust-effect-demo/compliant && cargo fmt --check
	cd examples/rust-effect-demo/violating && cargo fmt --check
	cd examples/rust-ifc-demo/compliant && cargo fmt --check
	cd examples/rust-ifc-demo/violating && cargo fmt --check

clippy:
	cargo clippy --workspace --all-targets

examples: build
	cargo build -p rust-limit --bin cargo-mvl-limit
	@output=$$(cd examples/hello-world && cargo run --quiet); \
	if [ "$$output" != "Hello, world!" ]; then \
		echo "FAIL: hello-world printed unexpected output: $$output" >&2; \
		exit 1; \
	fi
	./target/debug/cargo-mvl-limit examples/hello-world/src/main.rs
	@echo "hello-world: OK (prints \"Hello, world!\", 0 diagnostics)"
	./target/debug/cargo-mvl-limit examples/rust-limit-demo/compliant/src/main.rs
	@echo "rust-limit compliant example: OK (0 diagnostics, as expected)"
	@output=$$(./target/debug/cargo-mvl-limit examples/rust-limit-demo/violating/src/main.rs 2>&1); \
	status=$$?; \
	if [ $$status -eq 0 ]; then \
		echo "FAIL: rust-limit violating example unexpectedly passed with no diagnostics" >&2; \
		exit 1; \
	fi; \
	count=$$(printf '%s\n' "$$output" | grep -c '^error:'); \
	echo "rust-limit violating example: OK ($$count diagnostics correctly rejected -- run 'make examples-verbose' to see them)"
	cargo build -p rust-total --bin cargo-mvl-total
	./target/debug/cargo-mvl-total examples/rust-total-demo/compliant/src/main.rs
	@echo "rust-total compliant example: OK (0 diagnostics, as expected)"
	@output=$$(./target/debug/cargo-mvl-total examples/rust-total-demo/violating/src/main.rs 2>&1); \
	status=$$?; \
	if [ $$status -eq 0 ]; then \
		echo "FAIL: rust-total violating example unexpectedly passed with no diagnostics" >&2; \
		exit 1; \
	fi; \
	count=$$(printf '%s\n' "$$output" | grep -c '^error:'); \
	echo "rust-total violating example: OK ($$count diagnostics correctly rejected -- run 'make examples-verbose' to see them)"
	cargo build -p rust-refine --bin cargo-mvl-refine
	./target/debug/cargo-mvl-refine examples/rust-refine-demo/compliant/src/main.rs
	@echo "rust-refine compliant example: OK (proven at L2, 0 violations, as expected)"
	@output=$$(./target/debug/cargo-mvl-refine examples/rust-refine-demo/violating/src/main.rs 2>&1); \
	status=$$?; \
	if [ $$status -eq 0 ]; then \
		echo "FAIL: rust-refine violating example unexpectedly passed with no diagnostics" >&2; \
		exit 1; \
	fi; \
	count=$$(printf '%s\n' "$$output" | grep -c '^error:'); \
	echo "rust-refine violating example: OK ($$count diagnostics correctly rejected -- run 'make examples-verbose' to see them)"
	cargo build -p rust-effect --bin cargo-mvl-effect
	./target/debug/cargo-mvl-effect examples/rust-effect-demo/compliant/src/main.rs
	@echo "rust-effect compliant example: OK (0 diagnostics, as expected)"
	@output=$$(./target/debug/cargo-mvl-effect examples/rust-effect-demo/violating/src/main.rs 2>&1); \
	status=$$?; \
	if [ $$status -eq 0 ]; then \
		echo "FAIL: rust-effect violating example unexpectedly passed with no diagnostics" >&2; \
		exit 1; \
	fi; \
	count=$$(printf '%s\n' "$$output" | grep -c '^error:'); \
	echo "rust-effect violating example: OK ($$count diagnostics correctly rejected -- run 'make examples-verbose' to see them)"
	cargo build -p rust-ifc --bin cargo-mvl-ifc
	./target/debug/cargo-mvl-ifc examples/rust-ifc-demo/compliant/src/main.rs
	@echo "rust-ifc compliant example: OK (0 diagnostics, as expected)"
	@output=$$(./target/debug/cargo-mvl-ifc examples/rust-ifc-demo/violating/src/main.rs 2>&1); \
	status=$$?; \
	if [ $$status -eq 0 ]; then \
		echo "FAIL: rust-ifc violating example unexpectedly passed with no diagnostics" >&2; \
		exit 1; \
	fi; \
	count=$$(printf '%s\n' "$$output" | grep -c '^error:'); \
	echo "rust-ifc violating example: OK ($$count diagnostics correctly rejected -- run 'make examples-verbose' to see them)"
	cargo build -p cargo-mvl --bin cargo-mvl
	./target/debug/cargo-mvl mvl check examples/hello-world/src/main.rs
	@echo "cargo mvl check (hello-world): OK (0 diagnostics -- all five tools real)"
	@if ./target/debug/cargo-mvl mvl check examples/rust-limit-demo/violating/src/main.rs >/dev/null 2>&1; then \
		echo "FAIL: cargo mvl check unexpectedly passed against the rust-limit violating example" >&2; \
		exit 1; \
	fi
	@echo "cargo mvl check (rust-limit violating): OK (aggregated limit diagnostics correctly rejected)"

examples-verbose: build
	cargo build -p rust-limit --bin cargo-mvl-limit
	cargo build -p rust-total --bin cargo-mvl-total
	cargo build -p rust-refine --bin cargo-mvl-refine
	cargo build -p rust-effect --bin cargo-mvl-effect
	cargo build -p rust-ifc --bin cargo-mvl-ifc
	cd examples/hello-world && cargo run --quiet
	./target/debug/cargo-mvl-limit examples/hello-world/src/main.rs
	@echo "hello-world: OK (0 diagnostics)"
	./target/debug/cargo-mvl-limit examples/rust-limit-demo/compliant/src/main.rs
	@echo "rust-limit compliant example: OK (0 diagnostics, as expected)"
	@echo "--- rust-limit violating example (expect: exit 1; diagnostics below are INTENTIONAL) ---"
	! ./target/debug/cargo-mvl-limit examples/rust-limit-demo/violating/src/main.rs
	./target/debug/cargo-mvl-total examples/rust-total-demo/compliant/src/main.rs
	@echo "rust-total compliant example: OK (0 diagnostics, as expected)"
	@echo "--- rust-total violating example (expect: exit 1; diagnostics below are INTENTIONAL) ---"
	! ./target/debug/cargo-mvl-total examples/rust-total-demo/violating/src/main.rs
	./target/debug/cargo-mvl-refine examples/rust-refine-demo/compliant/src/main.rs
	@echo "rust-refine compliant example: OK (proven at L2, as expected)"
	@echo "--- rust-refine violating example (expect: exit 1; diagnostics below are INTENTIONAL) ---"
	! ./target/debug/cargo-mvl-refine examples/rust-refine-demo/violating/src/main.rs
	./target/debug/cargo-mvl-effect examples/rust-effect-demo/compliant/src/main.rs
	@echo "rust-effect compliant example: OK (0 diagnostics, as expected)"
	@echo "--- rust-effect violating example (expect: exit 1; diagnostics below are INTENTIONAL) ---"
	! ./target/debug/cargo-mvl-effect examples/rust-effect-demo/violating/src/main.rs
	./target/debug/cargo-mvl-ifc examples/rust-ifc-demo/compliant/src/main.rs
	@echo "rust-ifc compliant example: OK (0 diagnostics, as expected)"
	@echo "--- rust-ifc violating example (expect: exit 1; diagnostics below are INTENTIONAL) ---"
	! ./target/debug/cargo-mvl-ifc examples/rust-ifc-demo/violating/src/main.rs
	cargo build -p cargo-mvl --bin cargo-mvl
	./target/debug/cargo-mvl mvl check examples/hello-world/src/main.rs
	@echo "cargo mvl check (hello-world): OK"
	@echo "--- cargo mvl check against rust-limit violating example (expect: exit 1) ---"
	! ./target/debug/cargo-mvl mvl check examples/rust-limit-demo/violating/src/main.rs

check: fmt-check clippy test examples

# === Assurance ===
#
# ASSURANCE is the ARGUMENT that this software is fit for purpose -- not a
# measurement. Three levels support it (ADR-0007), each with its own question,
# its own verb, and its own artefact. Two things sharing a verb are one level.
#
#   VERIFICATION   does the program satisfy its spec?     verify    -> verdicts
#   TRACEABILITY   do intent/spec/program/evidence link?  trace     -> ratios
#   EVIDENCE       what artefacts back the claims?        collect   -> records
#
# Each has a target below, so the taxonomy is runnable rather than just written
# down. They fail independently and that is the point: verification green with
# traceability red is working code nobody can trace to a requirement; the
# reverse is perfect paperwork over broken code.
#
# Traceability's unit is the SCENARIO, not the requirement -- a requirement is
# an umbrella claim, a scenario is a falsifiable one, and GIVEN/WHEN/THEN maps
# onto arrange/act/assert. Measuring at requirement level let one test stand in
# for five scenarios. Adapted from mvl-lang/mvl's tools/assurance.py.
#
# Hygiene (fmt-check, clippy) is deliberately NOT a level: it gates whether the
# code is well-formed enough to be worth verifying. Folding it in would make
# "verification: pass" mean two unrelated things.

compile: ## Gate 1 -- if it does not compile, nothing downstream means anything
	@cargo check --workspace --all-targets --quiet && echo "compiles"

coverage: ## Line + function coverage via cargo-llvm-cov, cached for the dashboard
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { echo "cargo-llvm-cov not installed: cargo install cargo-llvm-cov"; exit 1; }
	@mkdir -p target   # a fresh checkout has no target/, so the redirect below would fail
	@cargo llvm-cov --workspace --json --ignore-run-fail > target/llvm-cov.json 2>/dev/null
	@python3 -c "import json; d=json.load(open('target/llvm-cov.json')); t=d['data'][0]['totals']; l=t['lines']; f=t['functions']; print(f\"Lines:     {l['covered']}/{l['count']} ({l['percent']:.1f}%)\"); print(f\"Functions: {f['covered']}/{f['count']} ({f['percent']:.1f}%)\")"

assurance: ## Assurance dashboard across the three levels (VERBOSE=true for per-scenario)
	@python3 tools/assurance.py $(if $(VERBOSE),--verbose)

# Both thresholds are ratchets at the current level, not targets: raise them as
# evidence accrues, never lower them. Two failures are unconditional and ignore
# the ratios entirely -- the workspace not compiling, and any Tests: link that
# does not resolve.
assurance-gate: coverage ## CI gate: compile + unresolved links + scenario 75% + line 80%
	@python3 tools/assurance.py --with-compile --min 0.75 --min-coverage 0.80

# ── The three levels, each runnable on its own ──────────────────────────────

verify: compile test examples ## VERIFICATION: does the program satisfy its spec? -> verdicts

evidence: coverage ## EVIDENCE: what artefacts back the claims? -> records

traceability: ## TRACEABILITY: do the ISPE layers connect? -> ratios (fast: no cargo, no cache)
	@python3 tools/assurance.py --traceability-only

# ── Everything ─────────────────────────────────────────────────────────────

all: check verify evidence assurance-gate ## Hygiene + all three levels + the gate

# ── COMPLIANCE: deliberately absent ────────────────────────────────────────
#
# Compliance means mapping an assurance case onto a named standard -- DO-178C,
# ISO 26262, EU CRA. It is DOWNSTREAM of the case, not a fourth level: one case
# maps onto N standards, so compliance consumes the case rather than composing
# it (ADR-0007 §2).
#
# There is no `compliance` target because nothing needs one yet. Building it
# early would mean writing a translator for a document nobody has requested.
# The previous `compliance` target was a synonym for "run everything" -- which
# is `all` -- and naming that "compliance" was exactly the overloading ADR-0007
# exists to stop.

clean:
	cargo clean
	cd examples/hello-world && cargo clean
	cd examples/rust-limit-demo/compliant && cargo clean
	cd examples/rust-limit-demo/violating && cargo clean
	cd examples/rust-total-demo/compliant && cargo clean
	cd examples/rust-total-demo/violating && cargo clean
	cd examples/rust-refine-demo/compliant && cargo clean
	cd examples/rust-refine-demo/violating && cargo clean
	cd examples/rust-effect-demo/compliant && cargo clean
	cd examples/rust-effect-demo/violating && cargo clean
	cd examples/rust-ifc-demo/compliant && cargo clean
	cd examples/rust-ifc-demo/violating && cargo clean
