# Heap

`Heap<'a, T, O = Max>` is a fixed-capacity binary heap (priority queue) over
user-provided memory. See [storage](storage.md) and [errors](errors.md).

Use it wherever "the most urgent item next" is needed: the open set of a
pathfinder, timers, scheduled events.

## Order

The heap pops first the element its order ranks greatest.

| Constructor | Pops first | Requires |
|---|---|---|
| `Heap::new(storage)` | largest | `T: Ord` |
| `Heap::min(storage)` | smallest | `T: Ord` |
| `Heap::with_order(storage, order)` | whatever `order` ranks greatest | any `Fn(&T, &T) -> Ordering`, or an `Order<T>` implementation |

`Max` and `Min` are zero-sized, so they cost nothing. A custom order is how
types that are not `Ord` are handled, for example `f32` costs:

```rust
let mut open = Heap::with_order(&mut memory, |a: &Node, b: &Node| b.cost.total_cmp(&a.cost));
```

Elements that compare equal are popped in no particular order.

## API

| Operation | Signature | Notes |
|---|---|---|
| sizes | `len`, `capacity`, `remaining`, `is_empty`, `is_full` | O(1) |
| push | `push(T) -> Result<(), Full<T>>` | O(log n); returns the value when full |
| pop | `pop() -> Option<T>` | O(log n); `None` when empty |
| peek | `peek() -> Option<&T>` | O(1) |
| view | `as_slice() -> &[T]` | heap order: first is next to pop |
| clear | `clear()` | drops all elements |

There is no way to change the priority of an element already in the heap.
Push the element again with its new priority and ignore the outdated copy
when it is popped (the usual approach in A*).

## Implementation

Built on `Vector`. Moving an element up or down the heap takes it out of the
slice, shifts the others into the gap one by one, and writes it back at the
end, which halves the memory writes compared with swapping. This uses
`unsafe`; the gap is always closed again, even if a comparison panics.

## Guarantees

- No allocation, no panics.
- Elements still in the heap are dropped when it is dropped.
