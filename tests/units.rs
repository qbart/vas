use vas::arena::StaticBlock;
use vas::{Arena, GB, KB, MB, Memory};

static BLOCK: StaticBlock<{ 2 * MB }> = StaticBlock::new();

#[test]
fn units_are_powers_of_1024() {
    assert_eq!(KB, 1024);
    assert_eq!(MB, 1024 * 1024);
    assert_eq!(GB, 1024 * 1024 * 1024);
    assert_eq!(20 * MB, 20_971_520);
    assert_eq!(MB / 2, 512 * KB);
}

#[test]
fn units_size_blocks_at_compile_time() {
    assert_eq!(BLOCK.capacity(), 2_097_152);
    let mut memory = Memory::<u8, { 4 * KB }>::new();
    assert_eq!(memory.capacity(), 4096);
    let arena = Arena::new(&mut memory);
    assert!(arena.sub(KB).is_ok());
    assert_eq!(arena.remaining(), 3 * KB);
}

#[cfg(feature = "alloc")]
#[test]
fn units_size_heap_blocks() {
    let block = vas::arena::HeapBlock::new(64 * KB);
    assert_eq!(block.map(|block| block.capacity()).ok(), Some(65_536));
}
