.PHONY: build release test lint fmt check miri publish clean

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

miri:
	cargo +nightly miri test

publish: check
	cargo publish

clean:
	cargo clean
