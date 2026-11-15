# Deque

`Deque<'a, T>` is a fixed-capacity double-ended queue (ring buffer) over
user-provided memory. See [storage](storage.md) and [errors](errors.md).

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
| create | `new(&'a mut [MaybeUninit<T>]) -> Deque<'a, T>` | O(1); capacity = slice length |
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
