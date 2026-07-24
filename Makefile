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
	@echo "  examples           build+run hello-world and rust-limit-demo, check all (summary only)"
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

fmt-check:
	cargo fmt --check
	cd examples/hello-world && cargo fmt --check
	cd examples/rust-limit-demo/compliant && cargo fmt --check
	cd examples/rust-limit-demo/violating && cargo fmt --check

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
	@echo "compliant example: OK (0 diagnostics, as expected)"
	@output=$$(./target/debug/cargo-mvl-limit examples/rust-limit-demo/violating/src/main.rs 2>&1); \
	status=$$?; \
	if [ $$status -eq 0 ]; then \
		echo "FAIL: violating example unexpectedly passed with no diagnostics" >&2; \
		exit 1; \
	fi; \
	count=$$(printf '%s\n' "$$output" | grep -c '^error:'); \
	echo "violating example: OK ($$count diagnostics correctly rejected -- run 'make examples-verbose' to see them)"

examples-verbose: build
	cargo build -p rust-limit --bin cargo-mvl-limit
	cd examples/hello-world && cargo run --quiet
	./target/debug/cargo-mvl-limit examples/hello-world/src/main.rs
	@echo "hello-world: OK (0 diagnostics)"
	./target/debug/cargo-mvl-limit examples/rust-limit-demo/compliant/src/main.rs
	@echo "compliant example: OK (0 diagnostics, as expected)"
	@echo "--- violating example (expect: exit 1; diagnostics below are INTENTIONAL) ---"
	! ./target/debug/cargo-mvl-limit examples/rust-limit-demo/violating/src/main.rs

check: fmt-check clippy test examples

clean:
	cargo clean
	cd examples/hello-world && cargo clean
	cd examples/rust-limit-demo/compliant && cargo clean
	cd examples/rust-limit-demo/violating && cargo clean
