.PHONY: build release test lint fmt check publish clean

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt

check: lint test
	cargo fmt --check

publish: check
	cargo publish

clean:
	cargo clean
