use core::sync::atomic::{AtomicUsize, Ordering};
use vas::sync::CachePadded;

#[test]
fn keeps_values_on_separate_cache_lines() {
    let pair = [
        CachePadded::new(AtomicUsize::new(1)),
        CachePadded::new(AtomicUsize::new(2)),
    ];
    let (a, b) = (
        &*pair[0] as *const AtomicUsize as usize,
        &*pair[1] as *const AtomicUsize as usize,
    );
    assert!(b - a >= 64);
    assert!(align_of::<CachePadded<u8>>() >= 64);
    assert!(a.is_multiple_of(64) && b.is_multiple_of(64));
    assert_eq!(size_of::<CachePadded<u8>>(), align_of::<CachePadded<u8>>());
}

#[test]
fn behaves_like_the_value() {
    let mut counter = CachePadded::new(AtomicUsize::new(0));
    counter.fetch_add(3, Ordering::Relaxed);
    *counter.get_mut() += 1;
    assert_eq!(counter.load(Ordering::Relaxed), 4);
    assert_eq!(CachePadded::new(7u32).into_inner(), 7);
    assert_eq!(CachePadded::<u32>::default().into_inner(), 0);
    assert_eq!(CachePadded::from(5u8), CachePadded::new(5u8));
    assert_eq!(format!("{:?}", CachePadded::new(1u8)), "CachePadded(1)");
    let original = CachePadded::new(vec![1u8, 2]);
    let copy = original.clone();
    assert_eq!(original, copy);
    assert_eq!(*copy, [1, 2]);
}

const PADDED: CachePadded<u32> = CachePadded::new(9);

#[test]
fn can_be_built_in_constants() {
    assert_eq!(*PADDED, 9);
}
