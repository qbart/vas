//! Trees. Run with `cargo run --example hierarchy`.

use std::error::Error;

use vas::{BinaryTree, Memory, Tree};

fn main() -> Result<(), Box<dyn Error>> {
    // Tree: a scene hierarchy.
    let mut memory = Memory::<vas::tree::Slot<&str>, 64>::new();
    let mut scene = Tree::new(&mut memory);
    let body = scene.insert_root("body")?;
    let arm = scene.insert_child(body, "arm")?;
    let hand = scene.insert_child(arm, "hand")?;
    scene.insert_child(body, "leg")?;

    assert_eq!(scene.parent(hand), Some(arm));
    assert_eq!(scene.children(body).count(), 2);
    let below: Vec<&str> = scene
        .descendants(body)
        .filter_map(|id| scene.get(id).copied())
        .collect();
    assert_eq!(below, ["arm", "hand", "leg"]);

    // Move a subtree, then remove one: removal takes everything under it.
    scene.set_parent(hand, Some(body))?;
    scene.remove(arm);
    assert_eq!(scene.len(), 3);
    assert!(scene.contains(hand));

    // BinaryTree: an expression, 1 + 2 * 3.
    let mut memory = Memory::<vas::binary_tree::Slot<&str>, 8>::new();
    let mut expression = BinaryTree::new(&mut memory);
    let plus = expression.set_root("+")?;
    expression.insert_left(plus, "1")?;
    let times = expression.insert_right(plus, "*")?;
    expression.insert_left(times, "2")?;
    expression.insert_right(times, "3")?;

    let infix: Vec<&str> = expression
        .in_order(plus)
        .filter_map(|id| expression.get(id).copied())
        .collect();
    assert_eq!(infix, ["1", "+", "2", "*", "3"]);
    let postfix: Vec<&str> = expression
        .post_order(plus)
        .filter_map(|id| expression.get(id).copied())
        .collect();
    assert_eq!(postfix, ["1", "2", "3", "*", "+"]);

    println!("hierarchy: ok");
    Ok(())
}
