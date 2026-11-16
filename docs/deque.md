# Deque

`Deque<T, S>` is a fixed-capacity double-ended queue (ring buffer) with a [storage parameter](storage.md#storage-parameter). See [storage](storage.md) and [errors](errors.md).

Use it for queues (events, commands, input) and for "keep the last N"
history buffers (frame times, positions for a trail).

## Layout

The contents start at a moving `head` and wrap around the end of the block.
Pushing and popping at either end is O(1) and never moves other elements.
Because of the wrap, the contents are exposed as **up to two slices**, not one
contiguous block; use `Vector` when a single block is required.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(buffer)`, `Deque::<T, Inline<N>>::inline()` | O(1); capacity = slice length |
| sizes | `len`, `capacity`, `remaining`, `is_empty`, `is_full` | O(1) |
| push | `push_back(T)`, `push_front(T) -> Result<(), Full<T>>` | returns the value when full |
| push, evicting | `push_overwrite(T) -> Option<T>` | when full, removes and returns the front element to make room |
| pop | `pop_front()`, `pop_back() -> Option<T>` | `None` when empty |
| access | `front`, `back`, `get(i)` and `_mut` variants | index counted from the front |
| view | `as_slices`, `as_mut_slices` | front-to-back; second slice empty unless wrapped |
| iterate | `iter`, `iter_mut`, `&deque`, `&mut deque` | front to back, double-ended |
| clear | `clear()` | drops all elements |

## Guarantees

- No allocation, no panics.
- Elements still in the deque are dropped when it is dropped.
- Zero-sized types and zero-capacity storage are supported.

## Storage

The parameter `S` says where the memory lives; see
[storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `Deque<T, Inline<64>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `Deque<T, Borrowed<'a>>` | provided by the caller, sized in `T`: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept either kind are generic over `S`.
