.PHONY: build release test lint fmt doc examples check miri bench bench-crates publish clean

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

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps

examples:
	for example in arena containers hierarchy pathfinding spatial; do cargo run -q --all-features --example $$example || exit 1; done

check: lint test examples doc
	cargo fmt --check

# One doc example (`Arena::from_heap`) leaks its block on purpose, so the
# leak check is off for doc tests only.
miri:
	cargo +nightly miri test --all-features --lib --tests
	MIRIFLAGS=-Zmiri-ignore-leaks cargo +nightly miri test --all-features --doc

bench:
	cargo bench --all-features

bench-crates:
	cd bench && cargo run --release

publish: check
	cargo publish

clean:
	cargo clean
