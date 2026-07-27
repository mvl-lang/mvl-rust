.PHONY: help build test check fmt fmt-check clippy examples examples-verbose clean \
	test-core test-limit test-total test-refine test-effect test-ifc test-cargo-mvl

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
	@echo "  examples           build+run hello-world, rust-limit-demo, rust-total-demo, check all (summary only)"
	@echo "  examples-verbose   same, but print the violating example's full diagnostics"
	@echo "  check              fmt-check + clippy + test + examples (mirrors CI)"
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

fmt-check:
	cargo fmt --check
	cd examples/hello-world && cargo fmt --check
	cd examples/rust-limit-demo/compliant && cargo fmt --check
	cd examples/rust-limit-demo/violating && cargo fmt --check
	cd examples/rust-total-demo/compliant && cargo fmt --check
	cd examples/rust-total-demo/violating && cargo fmt --check

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
	cargo build -p cargo-mvl --bin cargo-mvl
	./target/debug/cargo-mvl mvl check examples/hello-world/src/main.rs
	@echo "cargo mvl check (hello-world): OK (0 diagnostics from limit+total, refine/effect/ifc reported not-yet-implemented)"
	@if ./target/debug/cargo-mvl mvl check examples/rust-limit-demo/violating/src/main.rs >/dev/null 2>&1; then \
		echo "FAIL: cargo mvl check unexpectedly passed against the rust-limit violating example" >&2; \
		exit 1; \
	fi
	@echo "cargo mvl check (rust-limit violating): OK (aggregated limit diagnostics correctly rejected)"

examples-verbose: build
	cargo build -p rust-limit --bin cargo-mvl-limit
	cargo build -p rust-total --bin cargo-mvl-total
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
	cargo build -p cargo-mvl --bin cargo-mvl
	./target/debug/cargo-mvl mvl check examples/hello-world/src/main.rs
	@echo "cargo mvl check (hello-world): OK"
	@echo "--- cargo mvl check against rust-limit violating example (expect: exit 1) ---"
	! ./target/debug/cargo-mvl mvl check examples/rust-limit-demo/violating/src/main.rs

check: fmt-check clippy test examples

clean:
	cargo clean
	cd examples/hello-world && cargo clean
	cd examples/rust-limit-demo/compliant && cargo clean
	cd examples/rust-limit-demo/violating && cargo clean
	cd examples/rust-total-demo/compliant && cargo clean
	cd examples/rust-total-demo/violating && cargo clean
