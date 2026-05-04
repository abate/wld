.PHONY: build build-release test fmt fmt-check clippy check clean install

build:
	cargo build --locked

build-release:
	cargo build --locked --release

test:
	cargo test --locked

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --locked --workspace --all-features --all-targets -- -D warnings

check: fmt-check clippy test

clean:
	cargo clean

install:
	cargo install --locked --path .
