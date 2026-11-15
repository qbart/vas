use core::cell::Cell;
use std::vec::Vec;
use vas::binary_tree::{InsertError, NodeId, Slot};
use vas::{BinaryTree, Memory, Rng};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn values(tree: &BinaryTree<'_, u32>, ids: impl Iterator<Item = NodeId<u32>>) -> Vec<u32> {
    ids.filter_map(|id| tree.get(id).copied()).collect()
}

#[test]
fn new_is_empty() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let tree = BinaryTree::new(&mut memory);
    assert_eq!(tree.len(), 0);
    assert_eq!(tree.capacity(), 4);
    assert!(tree.is_empty());
    assert!(!tree.is_full());
    assert_eq!(tree.root(), None);
}

#[test]
fn builds_and_walks() {
    let mut memory = Memory::<Slot<u32>, 8>::new();
    let mut tree = BinaryTree::new(&mut memory);
    //        1
    //       / \
    //      2   3
    //     / \   \
    //    4   5   6
    let n1 = tree.set_root(1).ok();
    assert!(n1.is_some());
    if let Some(n1) = n1 {
        let n2 = tree.insert_left(n1, 2).ok();
        let n3 = tree.insert_right(n1, 3).ok();
        assert!(n2.is_some() && n3.is_some());
        if let (Some(n2), Some(n3)) = (n2, n3) {
            let n4 = tree.insert_left(n2, 4).ok();
            assert!(tree.insert_right(n2, 5).is_ok());
            assert!(tree.insert_right(n3, 6).is_ok());

            assert_eq!(tree.root(), Some(n1));
            assert_eq!(tree.left(n1), Some(n2));
            assert_eq!(tree.right(n1), Some(n3));
            assert_eq!(tree.left(n3), None);
            assert_eq!(tree.parent(n2), Some(n1));
            assert_eq!(tree.parent(n1), None);
            assert_eq!(tree.depth(n1), Some(0));
            assert_eq!(n4.and_then(|n4| tree.depth(n4)), Some(2));
            assert_eq!(tree.len(), 6);

            assert_eq!(values(&tree, tree.pre_order(n1)), [1, 2, 4, 5, 3, 6]);
            assert_eq!(values(&tree, tree.in_order(n1)), [4, 2, 5, 1, 3, 6]);
            assert_eq!(values(&tree, tree.post_order(n1)), [4, 5, 2, 6, 3, 1]);
            // Walks can start at any node and stay inside its subtree.
            assert_eq!(values(&tree, tree.pre_order(n2)), [2, 4, 5]);
            assert_eq!(values(&tree, tree.in_order(n2)), [4, 2, 5]);
            assert_eq!(values(&tree, tree.post_order(n2)), [4, 5, 2]);
            assert_eq!(values(&tree, tree.in_order(n3)), [3, 6]);

            if let Some(value) = tree.get_mut(n3) {
                *value = 30;
            }
            assert_eq!(tree.get(n3), Some(&30));
        }
    }
}

#[test]
fn insert_errors_return_the_value() {
    let mut memory = Memory::<Slot<u32>, 2>::new();
    let mut tree = BinaryTree::new(&mut memory);
    let root = tree.set_root(1).ok();
    assert!(root.is_some());
    assert_eq!(tree.set_root(9), Err(InsertError::Occupied(9)));
    if let Some(root) = root {
        let left = tree.insert_left(root, 2).ok();
        assert_eq!(tree.insert_left(root, 9), Err(InsertError::Occupied(9)));
        assert_eq!(tree.insert_right(root, 9), Err(InsertError::Full(9)));
        if let Some(left) = left {
            assert_eq!(tree.remove(left), Some(2));
            assert_eq!(tree.insert_left(left, 9), Err(InsertError::Stale(9)));
            assert_eq!(tree.insert_right(left, 9), Err(InsertError::Stale(9)));
        }
        assert_eq!(tree.left(root), None);
        assert!(tree.insert_left(root, 3).is_ok());
    }
}

#[test]
fn remove_takes_the_whole_subtree() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Slot<Counted>, 8>::new();
    let mut tree = BinaryTree::new(&mut memory);
    let root = tree.set_root(Counted(&drops)).ok();
    assert!(root.is_some());
    if let Some(root) = root {
        let left = tree.insert_left(root, Counted(&drops)).ok();
        assert!(tree.insert_right(root, Counted(&drops)).is_ok());
        if let Some(left) = left {
            let deep = tree.insert_right(left, Counted(&drops)).ok();
            assert!(tree.insert_left(left, Counted(&drops)).is_ok());
            let removed = tree.remove(left);
            assert_eq!(drops.get(), 2);
            drop(removed);
            assert_eq!(drops.get(), 3);
            assert!(!tree.contains(left));
            assert!(deep.is_some_and(|deep| !tree.contains(deep)));
            assert!(tree.remove(left).is_none());
            assert_eq!(tree.left(root), None);
        }
        assert_eq!(tree.len(), 2);
        // Removing the root empties the tree.
        drop(tree.remove(root));
        assert_eq!(drops.get(), 5);
        assert!(tree.is_empty());
        assert_eq!(tree.root(), None);
        assert!(tree.set_root(Counted(&drops)).is_ok());
    }
    drop(tree);
    assert_eq!(drops.get(), 6);
}

#[test]
fn stale_ids_and_clear() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let mut tree = BinaryTree::new(&mut memory);
    let root = tree.set_root(1).ok();
    if let Some(root) = root {
        assert!(tree.insert_left(root, 2).is_ok());
        assert_eq!(tree.iter().map(|(_, value)| *value).sum::<u32>(), 3);
        for (_, value) in tree.iter_mut() {
            *value += 1;
        }
        assert_eq!(tree.iter().map(|(_, value)| *value).sum::<u32>(), 5);
        tree.clear();
        assert!(tree.is_empty());
        assert_eq!(tree.root(), None);
        assert_eq!(tree.get(root), None);
        assert_eq!(tree.depth(root), None);
        assert_eq!(tree.pre_order(root).count(), 0);
        assert_eq!(tree.in_order(root).count(), 0);
        assert_eq!(tree.post_order(root).count(), 0);
    }
    assert!(tree.set_root(7).is_ok());
}

/// Reference model with recursive walks.
struct Model {
    left: Vec<Option<usize>>,
    right: Vec<Option<usize>>,
}

impl Model {
    fn child(list: &[Option<usize>], key: usize) -> Option<usize> {
        list.get(key).copied().flatten()
    }

    fn walk(&self, key: usize, order: u8, out: &mut Vec<usize>) {
        if order == 0 {
            out.push(key);
        }
        if let Some(left) = Self::child(&self.left, key) {
            self.walk(left, order, out);
        }
        if order == 1 {
            out.push(key);
        }
        if let Some(right) = Self::child(&self.right, key) {
            self.walk(right, order, out);
        }
        if order == 2 {
            out.push(key);
        }
    }
}

#[test]
fn walks_match_recursive_reference() {
    let mut rng = Rng::new(31);
    let rounds = if cfg!(miri) { 4 } else { 200 };
    for _ in 0..rounds {
        let mut memory = Memory::<Slot<usize>, 40>::new();
        let mut tree = BinaryTree::new(&mut memory);
        let mut model = Model {
            left: Vec::new(),
            right: Vec::new(),
        };
        let mut ids: Vec<NodeId<usize>> = Vec::new();
        if let Ok(root) = tree.set_root(0) {
            ids.push(root);
            model.left.push(None);
            model.right.push(None);
        }
        for _ in 0..rng.u32_below(60) {
            let parent = rng.usize_below(ids.len());
            let key = ids.len();
            let go_left = rng.bool();
            let Some(parent_id) = ids.get(parent).copied() else {
                continue;
            };
            let result = if go_left {
                tree.insert_left(parent_id, key)
            } else {
                tree.insert_right(parent_id, key)
            };
            if let Ok(id) = result {
                ids.push(id);
                model.left.push(None);
                model.right.push(None);
                let side = if go_left {
                    &mut model.left
                } else {
                    &mut model.right
                };
                if let Some(slot) = side.get_mut(parent) {
                    *slot = Some(key);
                }
            }
        }
        assert_eq!(tree.len(), ids.len());
        for (key, id) in ids.iter().enumerate() {
            let walks = [
                tree.pre_order(*id)
                    .filter_map(|n| tree.get(n).copied())
                    .collect::<Vec<_>>(),
                tree.in_order(*id)
                    .filter_map(|n| tree.get(n).copied())
                    .collect::<Vec<_>>(),
                tree.post_order(*id)
                    .filter_map(|n| tree.get(n).copied())
                    .collect::<Vec<_>>(),
            ];
            for (order, walk) in walks.iter().enumerate() {
                let mut expected = Vec::new();
                model.walk(key, order as u8, &mut expected);
                assert_eq!(walk, &expected);
            }
        }
    }
}
