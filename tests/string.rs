use core::fmt::Write;
use std::collections::BTreeSet;
use std::error::Error;
use vas::{Arena, Dynamic, Full, Inline, InsertError, Map, Memory, String};

type Test = Result<(), Box<dyn Error>>;

#[test]
fn push_and_read() -> Test {
    let mut memory = Memory::<u8, 16>::new();
    let mut name = String::new(&mut memory);
    assert!(name.is_empty());
    assert_eq!(name.capacity(), 16);
    name.push_str("Zoë")?;
    name.push(' ')?;
    name.push('🐉')?;
    assert_eq!(name.as_str(), "Zoë 🐉");
    assert_eq!(name.len(), 9);
    assert_eq!(name.remaining(), 7);
    assert_eq!(&*name, "Zoë 🐉");
    assert!(name.starts_with("Zo")); // str methods through Deref
    assert_eq!(name.as_bytes().len(), 9);
    assert_eq!(name.pop(), Some('🐉'));
    assert_eq!(name.pop(), Some(' '));
    assert_eq!(name, "Zoë");
    name.clear();
    assert_eq!(name.pop(), None);
    Ok(())
}

#[test]
fn full_changes_nothing() -> Test {
    let mut text = String::<Inline<5>>::inline();
    text.push_str("abc")?;
    assert_eq!(text.push_str("def"), Err(Full(())));
    assert_eq!(text, "abc");
    text.push('d')?;
    // 'é' takes two bytes; only one is left.
    assert_eq!(text.push('é'), Err(Full('é')));
    assert_eq!(text, "abcd");
    text.push('e')?;
    assert!(text.is_full());
    Ok(())
}

#[test]
fn truncating_push_keeps_whole_characters() -> Test {
    let mut label = String::<Inline<7>>::inline();
    assert_eq!(label.push_str_truncating("ab"), 2);
    assert_eq!(label.push_str_truncating("çé"), 4);
    assert_eq!(label, "abçé");
    // One byte left: "ñ" takes two, so nothing is written.
    assert_eq!(label.push_str_truncating("ñx"), 0);
    assert_eq!(label.push_str_truncating("xyz"), 1);
    assert!(label.is_full());
    assert_eq!(label, "abçéx");
    Ok(())
}

#[test]
fn write_formats_into_the_string() -> Test {
    let mut hud = String::<Inline<16>>::inline();
    write!(hud, "HP {}/{}", 42, 100)?;
    assert_eq!(hud, "HP 42/100");
    hud.clear();
    assert!(write!(hud, "{}", "x".repeat(17)).is_err());
    Ok(())
}

#[test]
fn truncate_cuts_at_a_character_boundary() -> Test {
    let mut text = String::<Inline<16>>::inline();
    text.push_str("añb")?; // ñ is bytes 1..3
    text.truncate(2);
    assert_eq!(text, "a");
    text.push_str("ñb")?;
    text.truncate(3);
    assert_eq!(text, "añ");
    text.truncate(100);
    assert_eq!(text, "añ");
    text.truncate(0);
    assert!(text.is_empty());
    Ok(())
}

#[test]
fn insert_and_remove_at_boundaries() -> Test {
    let mut input = String::<Inline<12>>::inline();
    input.push_str("hllo")?;
    input.insert(1, 'é')?;
    assert_eq!(input, "héllo");
    input.insert(input.len(), '!')?;
    assert_eq!(input, "héllo!");
    // Inside 'é' is not a boundary.
    assert_eq!(input.insert(2, 'x'), Err(InsertError::OutOfBounds('x')));
    assert_eq!(input.insert(99, 'x'), Err(InsertError::OutOfBounds('x')));
    assert_eq!(input.remove(2), None);
    assert_eq!(input.remove(1), Some('é'));
    assert_eq!(input, "hllo!");
    assert_eq!(input.remove(input.len()), None);
    input.insert_str(1, "e")?;
    assert_eq!(input, "hello!");
    input.push_str("abcdef")?;
    assert_eq!(input.insert(0, 'x'), Err(InsertError::Full('x')));
    assert_eq!(input.insert_str(0, "x"), Err(InsertError::Full(())));
    assert_eq!(input, "hello!abcdef");
    Ok(())
}

#[test]
fn retain_keeps_characters() -> Test {
    let mut text = String::<Inline<32>>::inline();
    text.push_str("a1ñ2🐉3b")?;
    text.retain(|c| !c.is_ascii_digit());
    assert_eq!(text, "añ🐉b");
    text.retain(|c| c.is_ascii());
    assert_eq!(text, "ab");
    text.retain(|_| false);
    assert!(text.is_empty());
    Ok(())
}

#[test]
fn compares_hashes_and_orders_like_str() -> Test {
    let mut a = String::<Inline<8>>::inline();
    let mut memory = Memory::<u8, 32>::new();
    let mut b = String::new(&mut memory);
    a.push_str("orc")?;
    b.push_str("orc")?;
    assert_eq!(a, b);
    assert_eq!(a, "orc");
    assert_eq!("orc", a.as_str());
    b.push('s')?;
    assert!(a < b);
    assert_ne!(a, b);
    let set: BTreeSet<_> = [a.clone()].into_iter().collect();
    assert!(set.contains("orc"));
    assert_eq!(format!("{a}|{a:?}"), "orc|\"orc\"");
    Ok(())
}

#[test]
fn works_as_a_map_key() -> Test {
    let mut slots = Memory::<vas::map::Slot<String<Inline<16>>, u32>, 8>::new();
    let mut tags = Memory::<u8, 8>::new();
    let mut prices = Map::new(&mut slots, &mut tags);
    let mut sword = String::<Inline<16>>::inline();
    sword.push_str("sword")?;
    prices.insert(sword, 30).map_err(|_| "full")?;
    assert_eq!(prices.get("sword"), Some(&30));
    Ok(())
}

#[test]
fn dynamic_storage_grows() -> Test {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut log = String::new(Dynamic(&arena));
    for line in 0..100 {
        writeln!(log, "line {line}")?;
    }
    assert_eq!(log.lines().count(), 100);
    assert!(log.ends_with("line 99\n"));
    Ok(())
}

#[test]
fn from_str_in_one_go() -> Test {
    let title = String::<Inline<8>>::from_str_in(Memory::new(), "Level 1")?;
    assert_eq!(title, "Level 1");
    let long = String::<Inline<4>>::from_str_in(Memory::new(), "Level 1");
    assert_eq!(long.err(), Some(Full(())));
    Ok(())
}

#[test]
fn retain_stays_valid_when_keep_panics() -> Test {
    let mut text = String::<Inline<32>>::inline();
    text.push_str("añ🐉bçd")?;
    let mut seen = 0;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        text.retain(|c| {
            seen += 1;
            assert!(seen < 4, "stop at the fourth");
            c != 'ñ'
        });
    }));
    assert!(result.is_err());
    // The first three were judged ("a" kept, "ñ" dropped, "🐉" kept); the
    // rest is untouched.
    assert_eq!(text, "a🐉bçd");
    assert!(core::str::from_utf8(text.as_bytes()).is_ok());
    Ok(())
}
