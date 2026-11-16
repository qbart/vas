.PHONY: build release test lint fmt examples check miri bench bench-crates publish clean

build:
	cargo build

release:
	cargo build --release

test:
	cargo test --all-features

lint:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt

examples:
	for example in arena containers hierarchy pathfinding spatial; do cargo run -q --all-features --example $$example || exit 1; done

check: lint test examples
	cargo fmt --check

miri:
	cargo +nightly miri test --all-features

bench:
	cargo bench

bench-crates:
	cd bench && cargo run --release

publish: check
	cargo publish

clean:
	cargo clean
