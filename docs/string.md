# String

`String<S>` is a UTF-8 string with a
[storage parameter](storage.md#storage-parameter). See [storage](storage.md)
and [errors](errors.md).

Use it wherever text is needed without the heap: entity and item names,
UI labels, chat lines, text input, HUD text formatted every frame,
`Map` keys. It is `vas::String`; import it under another name
(`use vas::String as Text;`) when std's `String` is also in use.

```rust
use core::fmt::Write;
use vas::{Inline, String};

let mut hud = String::<Inline<32>>::inline();
write!(hud, "HP {}/{}", hp, max_hp)?;
renderer.text(&hud); // a &str
```

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(buffer)`, `String::<Inline<N>>::inline()`, `from_str_in(buffer, text)` | `inline` is `const` |
| sizes | `len()`, `is_empty()`, `capacity()`, `remaining()`, `is_full()` | in bytes |
| read | `as_str()`, `as_bytes()`; every `str` method through `Deref` | |
| append | `push(ch) -> Result<(), Full<char>>`, `push_str(text) -> Result<(), Full<()>>` | all or nothing |
| append what fits | `push_str_truncating(text) -> usize` | whole characters; returns the bytes written |
| format | `write!(s, ...)`, `writeln!` (`fmt::Write`) | `fmt::Error` when full; pieces before it stay |
| edit | `insert(i, ch)`, `insert_str(i, text)`, `remove(i) -> Option<char>`, `pop()` | `i` is a byte index on a character boundary; O(len) |
| shorten | `truncate(len)`, `clear()` | `truncate` cuts before a character it would split |
| filter | `retain(\|ch\| keep)` | O(len) |
| compare | `==` with `str`, `&str` and strings of any storage; `Ord`, `Hash` | as the `str` |

`Borrow<str>` lets a [`Map`](map.md) keyed by `String` be looked up with a
`&str`: `prices.get("sword")`.

## Guarantees

- Always valid UTF-8. Writes that do not fit fail without changing the
  string (except `push_str_truncating` and `write!`, as above).
- Indices inside a character are `InsertError::OutOfBounds` for `insert`,
  `None` for `remove`; nothing panics.
- `retain` leaves valid text even when `keep` panics: the characters not
  judged yet stay.
- `unsafe` only in `as_str` / `as_mut_str`, which skip re-checking the
  UTF-8 the string already guarantees, so reading is O(1).

## Storage

| Type | Memory |
|---|---|
| `String<Inline<N>>` | `N` bytes inside the string: no lifetime, a plain struct field; can be cloned |
| `String<Borrowed<'a>>` | provided by the caller: `&mut Memory<u8, N>`, an arena piece |
| `String<Dynamic<A>>` | grows through the allocator `A` |

## Not included

- Counting or indexing by character or grapheme: use `chars()` and
  `char_indices()` from `str`.
- Small-string optimisation: an inline string is already on the stack.
