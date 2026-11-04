# Rust gates: the four checks CI runs on the workspace, plus rustdoc. Included by the root Makefile.
.PHONY: test clippy fmt fmt-check bench-check doc

##@ Rust gates
test: ## cargo test --workspace --locked
	cargo test --workspace --locked

clippy: ## clippy with warnings as errors
	cargo clippy --workspace --all-targets --locked -- -D warnings

fmt: ## format the workspace
	cargo fmt --all

fmt-check: ## check formatting
	cargo fmt --all -- --check

bench-check: ## compile the benches without running them
	cargo bench --workspace --no-run --locked

doc: ## rustdoc with warnings as errors
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
