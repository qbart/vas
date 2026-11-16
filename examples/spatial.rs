//! Spatial structures. Run with `cargo run --example spatial`.

use std::error::Error;

use vas::kd_tree::Neighbor;
use vas::{Aabb, Bvh, KdTree, Memory, Quadtree, SpatialGrid, Vector};

fn main() -> Result<(), Box<dyn Error>> {
    let world = Aabb::new([0.0, 0.0], [100.0, 100.0]);

    // SpatialGrid: who is near me? Best when points are evenly spread.
    let mut cells = Memory::<u32, 100>::new(); // 10 x 10 cells
    let mut entries = Memory::<vas::spatial_grid::Slot<&str, 2>, 64>::new();
    let mut grid = SpatialGrid::new(world, [10, 10], &mut cells, &mut entries)?;
    let goblin = grid.insert([12.0, 14.0], "goblin")?;
    grid.insert([80.0, 75.0], "dragon")?;

    let mut near = Vec::new();
    grid.query_radius([10.0, 10.0], 8.0, |_, name| near.push(*name));
    assert_eq!(near, ["goblin"]);

    grid.set_position(goblin, [78.0, 76.0]); // it moved
    let mut near = Vec::new();
    grid.query_radius([80.0, 75.0], 5.0, |_, name| near.push(*name));
    near.sort_unstable();
    assert_eq!(near, ["dragon", "goblin"]);

    // Quadtree: the same queries, for unevenly spread points.
    let mut nodes = Memory::<vas::orthtree::Node, 85>::new(); // 1 + 4 per split
    let mut entries = Memory::<vas::orthtree::Slot<&str, 2>, 64>::new();
    let mut tree = Quadtree::new(world, &mut nodes, &mut entries)?;
    tree.insert([12.0, 14.0], "goblin")?;
    tree.insert([80.0, 75.0], "dragon")?;
    let mut inside = Vec::new();
    tree.query(&Aabb::new([0.0, 0.0], [50.0, 50.0]), |_, name| {
        inside.push(*name)
    });
    assert_eq!(inside, ["goblin"]);

    // Bvh: objects with a size; box, point and ray queries.
    let mut items = [
        (Aabb::new([0.0, 0.0], [1.0, 1.0]), "crate"),
        (Aabb::new([5.0, 0.0], [6.0, 2.0]), "door"),
    ];
    let mut nodes = Memory::<vas::bvh::Node<2>, 3>::new(); // up to 2n - 1
    let mut bvh = Bvh::build(&mut items, &mut nodes)?;
    let mut crossed = Vec::new();
    bvh.query_ray(&[-1.0, 0.5], &[1.0, 0.0], 100.0, |_, name, distance| {
        crossed.push((distance, *name))
    });
    crossed.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(crossed, [(1.0, "crate"), (6.0, "door")]);

    // The door moved: update its box, then refit instead of rebuilding.
    for (aabb, name) in bvh.items_mut() {
        if *name == "door" {
            *aabb = Aabb::new([5.0, 10.0], [6.0, 12.0]);
        }
    }
    bvh.refit();
    let mut crossed = 0;
    bvh.query_ray(&[-1.0, 0.5], &[1.0, 0.0], 100.0, |_, _, _| crossed += 1);
    assert_eq!(crossed, 1);

    // KdTree: the nearest points. It needs no memory of its own.
    let mut places = [
        ([0.0, 0.0], "well"),
        ([8.0, 1.0], "forge"),
        ([3.0, 7.0], "inn"),
    ];
    let places = KdTree::build(&mut places);
    let found = places.nearest(&[7.0, 2.0]).ok_or("no places")?;
    assert_eq!(
        places.get(found.index).map(|(_, name)| *name),
        Some("forge")
    );

    let mut memory = Memory::<Neighbor, 2>::new(); // capacity = how many are wanted
    let mut closest = Vector::new(&mut memory);
    places.nearest_k(&[7.0, 2.0], &mut closest);
    let names: Vec<&str> = closest
        .iter()
        .filter_map(|found| places.get(found.index))
        .map(|(_, name)| *name)
        .collect();
    assert_eq!(names, ["forge", "inn"]);

    println!("spatial: ok");
    Ok(())
}
