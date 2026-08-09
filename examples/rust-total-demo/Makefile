.PHONY: help build fmt fmt-check run-compliant run-violating build-tool check clean

WORKSPACE := ../..
CARGO_MVL_TOTAL := $(WORKSPACE)/target/debug/cargo-mvl-total

help:
	@echo "Targets (run from examples/rust-total-demo/):"
	@echo "  build            cargo build --all-targets in compliant/ and violating/"
	@echo "  fmt              cargo fmt in compliant/ and violating/"
	@echo "  fmt-check        cargo fmt --check in compliant/ and violating/"
	@echo "  run-compliant    cargo-mvl-total's diagnostics, THEN cargo run -- what the tool says, then what the code does"
	@echo "  run-violating    same, for violating/ (builds+runs cleanly -- see main.rs's module doc)"
	@echo "  check            build cargo-mvl-total, then verify: exit 0 on compliant/, exit 1 on violating/"
	@echo "  clean            cargo clean in compliant/ and violating/"

build:
	cd compliant && cargo build --all-targets
	cd violating && cargo build --all-targets

fmt:
	cd compliant && cargo fmt
	cd violating && cargo fmt

fmt-check:
	cd compliant && cargo fmt --check
	cd violating && cargo fmt --check

# Both targets tell the same two-part story: what `cargo-mvl-total` says
# about the source (statically, no execution), then what the code actually
# does when it runs. `violating/` is expected to exit 1 from the tool step
# -- `-` tells make not to abort the target because of it.

run-compliant: build-tool
	@echo "--- cargo-mvl-total compliant/src/main.rs (expect: exit 0, no diagnostics) ---"
	$(CARGO_MVL_TOTAL) compliant/src/main.rs
	@echo
	@echo "--- cargo run (compliant/) ---"
	cd compliant && cargo run --quiet

run-violating: build-tool
	@echo "--- cargo-mvl-total violating/src/main.rs (expect: exit 1, diagnostics below are INTENTIONAL) ---"
	-$(CARGO_MVL_TOTAL) violating/src/main.rs
	@echo
	@echo "--- cargo run (violating/ -- builds+runs cleanly despite the diagnostics above; see main.rs's module doc) ---"
	cd violating && cargo run --quiet

build-tool:
	cd $(WORKSPACE) && cargo build -p rust-total --bin cargo-mvl-total

# Mirrors the root Makefile's `examples` target, scoped to just this demo --
# useful for iterating on rust-total itself without rebuilding/rerunning
# every other tool's paired fixtures.
check: build-tool
	$(CARGO_MVL_TOTAL) compliant/src/main.rs
	@echo "compliant: OK (0 diagnostics, as expected)"
	@output=$$($(CARGO_MVL_TOTAL) violating/src/main.rs 2>&1); \
	status=$$?; \
	if [ $$status -eq 0 ]; then \
		echo "FAIL: violating unexpectedly passed with no diagnostics" >&2; \
		exit 1; \
	fi; \
	count=$$(printf '%s\n' "$$output" | grep -c '^error:'); \
	echo "violating: OK ($$count diagnostics correctly rejected -- run '$(CARGO_MVL_TOTAL) violating/src/main.rs' to see them)"

clean:
	cd compliant && cargo clean
	cd violating && cargo clean
