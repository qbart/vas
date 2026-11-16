use core::mem::MaybeUninit;
use vas::arena::StaticBlock;
use vas::pool::Slot;
use vas::{Arena, Borrowed, Full, Map, Memory, Pool, Vector};

#[test]
fn hands_out_separate_pieces() {
    let mut block = Memory::<u8, 64>::new();
    let arena = Arena::new(&mut block);
    assert_eq!(arena.capacity(), 64);
    assert_eq!(arena.used(), 0);
    assert_eq!(arena.remaining(), 64);

    let a = arena.alloc::<u8>(10);
    let b = arena.alloc::<u8>(20);
    assert!(a.is_ok() && b.is_ok());
    if let (Ok(a), Ok(b)) = (a, b) {
        assert_eq!((a.len(), b.len()), (10, 20));
        // Both can be written at the same time: they do not overlap.
        for slot in a.iter_mut() {
            slot.write(1);
        }
        for slot in b.iter_mut() {
            slot.write(2);
        }
        let (a_start, b_start) = (a.as_ptr() as usize, b.as_ptr() as usize);
        assert_eq!(b_start - a_start, 10);
    }
    assert_eq!(arena.used(), 30);
    assert_eq!(arena.remaining(), 34);
}

#[test]
fn pieces_are_aligned_for_their_type() {
    let mut block = Memory::<u8, 256>::new();
    let arena = Arena::new(&mut block);
    for _ in 0..3 {
        assert!(arena.alloc::<u8>(1).is_ok());
        let wide = arena.alloc::<u64>(2);
        assert!(
            wide.is_ok_and(|piece| (piece.as_ptr() as usize).is_multiple_of(align_of::<u64>()))
        );
        let odd = arena.alloc::<u16>(1);
        assert!(odd.is_ok_and(|piece| (piece.as_ptr() as usize).is_multiple_of(align_of::<u16>())));
    }
    #[repr(align(64))]
    struct CacheLine(#[allow(dead_code)] u8);
    let line = arena.alloc::<CacheLine>(1);
    assert!(line.is_ok_and(|piece| (piece.as_ptr() as usize).is_multiple_of(64)));
}

#[test]
fn running_out_is_reported_and_changes_nothing() {
    let mut block = Memory::<u8, 16>::new();
    let arena = Arena::new(&mut block);
    assert!(arena.alloc::<u8>(10).is_ok());
    assert_eq!(arena.alloc::<u8>(7).err(), Some(Full(())));
    assert_eq!(arena.used(), 10);
    // A smaller request still fits.
    assert!(arena.alloc::<u8>(6).is_ok());
    assert_eq!(arena.remaining(), 0);
    assert_eq!(arena.alloc::<u8>(1).err(), Some(Full(())));
    // Absurd sizes do not overflow.
    assert_eq!(arena.alloc::<u64>(usize::MAX).err(), Some(Full(())));
    assert_eq!(arena.alloc::<u8>(usize::MAX).err(), Some(Full(())));
}

#[test]
fn empty_requests_and_zero_sized_types() {
    let mut block = Memory::<u8, 8>::new();
    let arena = Arena::new(&mut block);
    assert!(arena.alloc::<u32>(0).is_ok_and(|piece| piece.is_empty()));
    assert!(
        arena
            .alloc::<()>(1000)
            .is_ok_and(|piece| piece.len() == 1000)
    );
    assert_eq!(arena.used(), 0);

    let mut nothing = Memory::<u8, 0>::new();
    let arena = Arena::new(&mut nothing);
    assert_eq!(arena.capacity(), 0);
    assert!(arena.alloc::<u8>(0).is_ok());
    assert_eq!(arena.alloc::<u8>(1).err(), Some(Full(())));
}

#[test]
fn stores_single_values_and_copies_of_slices() {
    #[derive(Debug, PartialEq)]
    struct Camera {
        zoom: f32,
        position: [f32; 2],
    }
    let mut block = Memory::<u8, 64>::new();
    let arena = Arena::new(&mut block);
    let camera = arena.alloc_value(Camera {
        zoom: 2.0,
        position: [1.0, 2.0],
    });
    assert!(camera.is_ok());
    if let Ok(camera) = camera {
        camera.zoom = 3.0;
        assert_eq!(camera.zoom, 3.0);
    }
    let waypoints = arena.alloc_copy(&[1u32, 2, 3]);
    assert!(waypoints.is_ok());
    if let Ok(waypoints) = waypoints {
        assert_eq!(waypoints, [1, 2, 3]);
        waypoints.reverse();
        assert_eq!(waypoints, [3, 2, 1]);
    }
    // When it does not fit, the value comes back.
    let big = arena.alloc_value([0u8; 100]);
    assert!(big.is_err_and(|Full(value)| value.len() == 100));
    assert_eq!(arena.alloc_copy(&[0u64; 100]).err(), Some(Full(())));
}

#[test]
fn sub_arenas_give_each_system_a_budget() {
    let mut block = Memory::<u8, 100>::new();
    let arena = Arena::new(&mut block);
    let physics = arena.sub(40);
    let audio = arena.sub(40);
    assert!(physics.is_ok() && audio.is_ok());
    assert_eq!(arena.sub(40).err(), Some(Full(())));
    if let (Ok(physics), Ok(audio)) = (physics, audio) {
        assert_eq!(physics.capacity(), 40);
        assert!(physics.alloc::<u8>(40).is_ok());
        // One system running out does not touch the other's budget.
        assert_eq!(physics.alloc::<u8>(1).err(), Some(Full(())));
        assert!(audio.alloc::<u8>(40).is_ok());
    }
    assert_eq!(arena.remaining(), 20);
}

/// A struct of arrays: every field is a container, all packed in one block.
struct Particles<'a> {
    x: Vector<f32, Borrowed<'a>>,
    y: Vector<f32, Borrowed<'a>>,
    life: Vector<u16, Borrowed<'a>>,
}

impl<'a> Particles<'a> {
    fn new(arena: &Arena<'a>, count: usize) -> Result<Self, Full<()>> {
        Ok(Self {
            x: Vector::new(arena.alloc(count)?),
            y: Vector::new(arena.alloc(count)?),
            life: Vector::new(arena.alloc(count)?),
        })
    }

    fn spawn(&mut self, x: f32, y: f32, life: u16) -> bool {
        if self.x.is_full() {
            return false;
        }
        self.x.push(x).is_ok() && self.y.push(y).is_ok() && self.life.push(life).is_ok()
    }
}

#[test]
fn packs_a_struct_of_arrays_and_other_containers_in_one_block() {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);

    let particles = Particles::new(&arena, 100);
    let enemies = arena.alloc::<Slot<&str>>(16).map(Pool::new);
    let names = arena.alloc::<vas::map::Slot<u32, &str>>(32).map(Map::new);
    assert!(particles.is_ok() && enemies.is_ok() && names.is_ok());
    if let (Ok(mut particles), Ok(mut enemies), Ok(mut names)) = (particles, enemies, names) {
        for i in 0..100 {
            assert!(particles.spawn(i as f32, -(i as f32), i));
        }
        assert!(!particles.spawn(0.0, 0.0, 0));
        let goblin = enemies.insert("goblin").ok();
        assert_eq!(names.insert(7, "sword"), Ok(None));

        // Filling one container did not disturb the others.
        assert_eq!(particles.x.len(), 100);
        assert_eq!(particles.y.last(), Some(&-99.0));
        assert_eq!(
            particles.life.iter().map(|life| *life as u32).sum::<u32>(),
            4950
        );
        assert_eq!(goblin.and_then(|id| enemies.get(id)), Some(&"goblin"));
        assert_eq!(names.get(&7), Some(&"sword"));
    }
    assert!(arena.used() <= arena.capacity());
}

#[test]
fn block_can_be_reused_once_everything_is_gone() {
    let mut block = Memory::<u8, 32>::new();
    {
        let arena = Arena::new(&mut block);
        let piece = arena.alloc::<u8>(32);
        assert!(piece.is_ok());
        assert_eq!(arena.remaining(), 0);
    }
    // A new arena on the same block starts empty: this is how to "reset".
    let arena = Arena::new(&mut block);
    assert_eq!(arena.remaining(), 32);
    assert!(arena.alloc::<u8>(32).is_ok());
}

static BLOCK: StaticBlock<1024> = StaticBlock::new();

/// With a block that lives for the whole program, containers are `'static`
/// and need no lifetime parameter on the structs that hold them.
struct World {
    positions: Vector<[f32; 2], Borrowed<'static>>,
    score: &'static mut u32,
}

#[test]
fn static_block_is_taken_once_and_gives_static_containers() {
    assert_eq!(BLOCK.capacity(), 1024);
    let block: Option<&'static mut [MaybeUninit<u8>]> = BLOCK.take();
    assert!(block.is_some());
    // It can only be taken once.
    assert!(BLOCK.take().is_none());
    if let Some(block) = block {
        // The arena itself is only needed for setup: it goes out of scope
        // here, and the pieces stay valid.
        let (positions, score) = {
            let arena = Arena::new(block);
            (arena.alloc(64).map(Vector::new), arena.alloc_value(0u32))
        };
        assert!(positions.is_ok() && score.is_ok());
        if let (Ok(positions), Ok(score)) = (positions, score) {
            let mut world = World { positions, score };
            assert!(world.positions.push([1.0, 2.0]).is_ok());
            *world.score += 10;
            assert_eq!(world.positions.len(), 1);
            assert_eq!(*world.score, 10);
        }
    }
}
