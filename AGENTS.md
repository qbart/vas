# vas

vas is a collection of data containers, structures and algorithms for game development in rust focused on speed and memory control.

## Rules

1. Use short commit names and follow semantic commits. Do not add co-author comments. Always work on master branch.
2. Run verifications and tests first, then commit and push. Every commit must build and pass on its own.
3. Make sure to maintain spec for features in docs/*.md
4. Do not use .unwrap / .expect / panic!. Runtime exceptions are not allowed. Lints in Cargo.toml enforce this.
5. Maintain ROADMAP.md that keeps the progress about whole project.
6. `unsafe` is denied except in places where it is unavoidable for speed.

7. Write the tests first.

8. Maintain Makefile with .PHONY rules for common operations like build, release, publish
9. Every milestone should have its own tag bump 0.x.0
10. No dependencies should be used. The only exception is `bench/`: a separate, never-published crate that compares vas against other crates. vas itself must stay dependency free.
11. The crate must be `no_std` — this is the main goal.
12. Allocation must be controlled by the library user — the library never allocates on its own.
