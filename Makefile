.PHONY: build release test lint fmt doc examples check miri tsan bench bench-crates publish clean

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

# ThreadSanitizer on the tests that use threads. Needs nightly with
# `rust-src`; any report fails the run.
HOST := $(shell rustc -vV | sed -n 's/host: //p')
THREADED_TESTS := --test queue --test steal_deque --test job_counter --test arena

tsan:
	TSAN_OPTIONS=halt_on_error=1 RUSTFLAGS="-Zsanitizer=thread" CARGO_TARGET_DIR=target/tsan \
		cargo +nightly test -Zbuild-std --target $(HOST) --all-features $(THREADED_TESTS)

bench:
	cargo bench --all-features

bench-crates:
	cd bench && cargo run --release

publish: check
	cargo publish

clean:
	cargo clean
