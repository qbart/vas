use core::mem::MaybeUninit;
use vas::{Full, Memory, Vector};

#[test]
fn memory_backs_a_vector() {
    let mut memory = Memory::<f32, 4>::new();
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.capacity(), 4);
    assert_eq!(v.push(1.0), Ok(()));
    assert_eq!(v.extend_from_slice(&[2.0, 3.0, 4.0]), Ok(()));
    assert_eq!(v.push(5.0), Err(Full(5.0)));
    assert_eq!(v.as_slice(), &[1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn memory_reports_capacity_and_is_reusable() {
    let mut memory = Memory::<u8, 3>::new();
    assert_eq!(memory.capacity(), 3);
    {
        let mut v = Vector::new(&mut memory);
        assert_eq!(v.push(1), Ok(()));
    }
    let v = Vector::new(&mut memory);
    assert!(v.is_empty());
}

#[test]
fn memory_works_for_non_copy_types_and_statics() {
    struct NotCopy(#[allow(dead_code)] u32);
    static _STATIC: Memory<u32, 8> = Memory::new();
    let mut memory = Memory::<NotCopy, 2>::default();
    let slots: &mut [MaybeUninit<NotCopy>] = &mut memory;
    assert_eq!(slots.len(), 2);
    let mut v = Vector::new(slots);
    assert!(v.push(NotCopy(1)).is_ok());
}

#[test]
fn memory_has_no_overhead() {
    assert_eq!(size_of::<Memory<f32, 16>>(), 16 * size_of::<f32>());
    assert_eq!(align_of::<Memory<f32, 16>>(), align_of::<f32>());
}
