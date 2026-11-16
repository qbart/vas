.PHONY: build release test lint fmt examples check miri bench publish clean

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

examples:
	for example in containers hierarchy pathfinding spatial; do cargo run -q --example $$example || exit 1; done

check: lint test examples
	cargo fmt --check

miri:
	cargo +nightly miri test

bench:
	cargo bench

publish: check
	cargo publish

clean:
	cargo clean
