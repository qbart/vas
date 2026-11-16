use vas::arena::StaticBlock;
use vas::{Arena, GB, KB, MB, Memory};

static BLOCK: StaticBlock<{ MB(2) }> = StaticBlock::new();

#[test]
fn units_are_powers_of_1024() {
    assert_eq!(KB(1), 1024);
    assert_eq!(MB(1), 1024 * 1024);
    assert_eq!(GB(1), 1024 * 1024 * 1024);
    assert_eq!(MB(20), 20_971_520);
    assert_eq!(MB(1) / 2, KB(512));
    assert_eq!(KB(0), 0);
}

#[test]
fn too_large_sizes_saturate_instead_of_panicking() {
    assert_eq!(KB(usize::MAX), usize::MAX);
    assert_eq!(MB(usize::MAX / 2), usize::MAX);
    assert_eq!(GB(usize::MAX / 1024), usize::MAX);
}

#[test]
fn units_size_blocks_at_compile_time() {
    assert_eq!(BLOCK.capacity(), 2_097_152);
    let mut memory = Memory::<u8, { KB(4) }>::new();
    assert_eq!(memory.capacity(), 4096);
    let arena = Arena::new(&mut memory);
    assert!(arena.sub(KB(1)).is_ok());
    assert_eq!(arena.remaining(), KB(3));
}

#[cfg(feature = "alloc")]
#[test]
fn units_size_heap_blocks() {
    let block = vas::arena::HeapBlock::new(KB(64));
    assert_eq!(block.map(|block| block.capacity()).ok(), Some(65_536));
}
