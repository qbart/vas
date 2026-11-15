# vas

vas is a collection of data containers, structures and algorithms for game development in Rust focused on speed and memory control.

- `no_std`, zero dependencies
- never allocates: every container works on memory you provide
- never panics: fallible operations return `Result` / `Option`

```rust
use vas::{Memory, Vector};

let mut memory = Memory::<f32, 1024>::new();
let mut v = Vector::new(&mut memory);

assert!(v.push(1.0).is_ok());
assert!(v.extend_from_slice(&[2.0, 3.0]).is_ok());
assert_eq!(v.as_slice(), &[1.0, 2.0, 3.0]); // one contiguous block
```

Containers so far:

- `Vector`: fixed-capacity contiguous vector
- `Pool`: fixed-capacity store with generational handles
- `Map`: fixed-capacity unordered map
- `Deque`: fixed-capacity double-ended queue (ring buffer)
- `Heap`: fixed-capacity priority queue
- `Bitset`: fixed-capacity set of bits

Algorithms so far:

- `Rng`: seedable pseudo-random number generator
- `PathFinder`: A* / Dijkstra shortest path

Specs are in [docs](docs), progress in [ROADMAP.md](ROADMAP.md).
