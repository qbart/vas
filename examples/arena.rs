//! One block of memory for the whole game. Run with
//! `cargo run --example arena`.

use std::error::Error;

use vas::arena::StaticBlock;
use vas::pool::Slot;
use vas::{Arena, Full, Map, Pool, Vector};

/// 20 MB for everything, living as long as the program.
static GAME_MEMORY: StaticBlock<{ 20 << 20 }> = StaticBlock::new();

/// A struct of arrays. The block is `'static`, so no lifetime parameter.
struct Particles {
    x: Vector<'static, f32>,
    y: Vector<'static, f32>,
    life: Vector<'static, u16>,
}

impl Particles {
    fn new(arena: &Arena<'static>, count: usize) -> Result<Self, Full<()>> {
        Ok(Self {
            x: Vector::new(arena.alloc(count)?),
            y: Vector::new(arena.alloc(count)?),
            life: Vector::new(arena.alloc(count)?),
        })
    }
}

/// A struct of structs, all packed in the same block.
struct World {
    particles: Particles,
    enemies: Pool<'static, &'static str>,
    item_names: Map<'static, u32, &'static str>,
    score: &'static mut u32,
}

impl World {
    fn new(arena: &Arena<'static>) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            particles: Particles::new(arena, 100_000)?,
            enemies: Pool::new(arena.alloc::<Slot<&str>>(1024)?),
            item_names: Map::new(arena.alloc(4096)?),
            score: arena.alloc_value(0).map_err(|_| "no room for the score")?,
        })
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let arena = Arena::new(GAME_MEMORY.take().ok_or("game memory already taken")?);

    // Give one system a fixed budget of its own.
    let audio = arena.sub(1 << 20)?;
    let samples = Vector::<i16>::new(audio.alloc(48_000)?);
    assert_eq!(samples.capacity(), 48_000);

    let mut world = World::new(&arena)?;
    println!("used {} of {} bytes", arena.used(), arena.capacity());
    // The arena is only needed for setup.
    drop(arena);

    world.particles.x.push(1.0)?;
    world.particles.y.push(2.0)?;
    world.particles.life.push(60)?;
    let goblin = world.enemies.insert("goblin")?;
    world.item_names.insert(7, "sword")?;
    *world.score += 10;

    assert_eq!(world.particles.x.capacity(), 100_000);
    assert_eq!(world.enemies.get(goblin), Some(&"goblin"));
    assert_eq!(world.item_names.get(&7), Some(&"sword"));
    assert_eq!(*world.score, 10);

    println!("arena: ok");
    Ok(())
}
