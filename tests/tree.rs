use core::cell::Cell;
use std::vec::Vec;
use vas::tree::{InsertError, MoveError, NodeId, Slot};
use vas::{Borrowed, Full, Memory, Rng, Tree};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn values<'t>(
    tree: &'t Tree<u32, Borrowed<'_>>,
    ids: impl Iterator<Item = NodeId<u32>> + 't,
) -> Vec<u32> {
    ids.filter_map(|id| tree.get(id).copied()).collect()
}

#[test]
fn new_is_empty() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let tree = Tree::new(&mut memory);
    assert_eq!(tree.len(), 0);
    assert_eq!(tree.capacity(), 4);
    assert!(tree.is_empty());
    assert!(!tree.is_full());
    assert_eq!(tree.roots().count(), 0);
}

#[test]
fn builds_a_hierarchy() {
    let mut memory = Memory::<Slot<u32>, 16>::new();
    let mut tree = Tree::new(&mut memory);
    //        1
    //      / | \
    //     2  3  4
    //    / \     \
    //   5   6     7
    let n1 = tree.insert_root(1).ok();
    assert!(n1.is_some());
    if let Some(n1) = n1 {
        let n2 = tree.insert_child(n1, 2).ok();
        let n3 = tree.insert_child(n1, 3).ok();
        let n4 = tree.insert_child(n1, 4).ok();
        assert!(n2.is_some() && n3.is_some() && n4.is_some());
        if let (Some(n2), Some(n3), Some(n4)) = (n2, n3, n4) {
            let n5 = tree.insert_child(n2, 5).ok();
            assert!(tree.insert_child(n2, 6).is_ok());
            let n7 = tree.insert_child(n4, 7).ok();
            assert_eq!(tree.len(), 7);

            assert_eq!(values(&tree, tree.roots()), [1]);
            assert_eq!(values(&tree, tree.children(n1)), [2, 3, 4]);
            assert_eq!(values(&tree, tree.children(n2)), [5, 6]);
            assert_eq!(tree.children(n3).count(), 0);
            assert_eq!(values(&tree, tree.descendants(n1)), [2, 5, 6, 3, 4, 7]);
            assert_eq!(values(&tree, tree.descendants(n2)), [5, 6]);
            assert_eq!(tree.descendants(n3).count(), 0);

            assert_eq!(tree.parent(n1), None);
            assert_eq!(tree.parent(n2), Some(n1));
            assert_eq!(tree.first_child(n1), Some(n2));
            assert_eq!(tree.last_child(n1), Some(n4));
            assert_eq!(tree.next_sibling(n2), Some(n3));
            assert_eq!(tree.prev_sibling(n3), Some(n2));
            assert_eq!(tree.prev_sibling(n2), None);
            assert_eq!(tree.next_sibling(n4), None);
            assert_eq!(tree.first_child(n3), None);
            assert_eq!(tree.depth(n1), Some(0));
            if let (Some(n5), Some(n7)) = (n5, n7) {
                assert_eq!(tree.depth(n5), Some(2));
                assert_eq!(values(&tree, tree.ancestors(n5)), [2, 1]);
                assert_eq!(values(&tree, tree.ancestors(n7)), [4, 1]);
            }
            if let Some(value) = tree.get_mut(n3) {
                *value = 30;
            }
            assert_eq!(tree.get(n3), Some(&30));
        }
    }
}

#[test]
fn several_roots_keep_their_order() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let mut tree = Tree::new(&mut memory);
    assert!(tree.insert_root(1).is_ok());
    let middle = tree.insert_root(2).ok();
    assert!(tree.insert_root(3).is_ok());
    assert_eq!(values(&tree, tree.roots()), [1, 2, 3]);
    if let Some(middle) = middle {
        assert_eq!(tree.remove(middle), Some(2));
    }
    assert_eq!(values(&tree, tree.roots()), [1, 3]);
}

#[test]
fn insert_errors_return_the_value() {
    let mut memory = Memory::<Slot<u32>, 2>::new();
    let mut tree = Tree::new(&mut memory);
    let root = tree.insert_root(1).ok();
    assert!(root.is_some());
    if let Some(root) = root {
        let child = tree.insert_child(root, 2).ok();
        assert!(tree.is_full());
        assert_eq!(tree.insert_root(3), Err(Full(3)));
        assert_eq!(tree.insert_child(root, 3), Err(InsertError::Full(3)));
        if let Some(child) = child {
            assert_eq!(tree.remove(child), Some(2));
            assert_eq!(tree.insert_child(child, 4), Err(InsertError::Stale(4)));
        }
        assert_eq!(tree.len(), 1);
    }
}

#[test]
fn remove_takes_the_whole_subtree() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Slot<Counted>, 8>::new();
    let mut tree = Tree::new(&mut memory);
    let root = tree.insert_root(Counted(&drops)).ok();
    assert!(root.is_some());
    if let Some(root) = root {
        let a = tree.insert_child(root, Counted(&drops)).ok();
        let b = tree.insert_child(root, Counted(&drops)).ok();
        if let (Some(a), Some(b)) = (a, b) {
            let a1 = tree.insert_child(a, Counted(&drops)).ok();
            assert!(tree.insert_child(a, Counted(&drops)).is_ok());
            if let Some(a1) = a1 {
                assert!(tree.insert_child(a1, Counted(&drops)).is_ok());
                // Removes a, its two children and one grandchild.
                let removed = tree.remove(a);
                assert!(removed.is_some());
                assert_eq!(drops.get(), 3);
                drop(removed);
                assert_eq!(drops.get(), 4);
                assert!(!tree.contains(a));
                assert!(!tree.contains(a1));
                assert!(tree.remove(a).is_none());
            }
            assert_eq!(tree.len(), 2);
            assert!(tree.children(root).eq([b]));
            assert_eq!(tree.first_child(root), Some(b));
            assert_eq!(tree.last_child(root), Some(b));
            assert_eq!(tree.prev_sibling(b), None);
        }
    }
    drop(tree);
    assert_eq!(drops.get(), 6);
}

#[test]
fn set_parent_moves_a_subtree() {
    let mut memory = Memory::<Slot<u32>, 8>::new();
    let mut tree = Tree::new(&mut memory);
    let a = tree.insert_root(1).ok();
    let b = tree.insert_root(2).ok();
    assert!(a.is_some() && b.is_some());
    if let (Some(a), Some(b)) = (a, b) {
        let a1 = tree.insert_child(a, 11).ok();
        assert!(a1.is_some());
        if let Some(a1) = a1 {
            assert!(tree.insert_child(a1, 111).is_ok());
            // Move a1 (and its child) under b.
            assert_eq!(tree.set_parent(a1, Some(b)), Ok(()));
            assert_eq!(tree.children(a).count(), 0);
            assert_eq!(values(&tree, tree.descendants(b)), [11, 111]);
            assert_eq!(tree.parent(a1), Some(b));
            // A node cannot become its own descendant.
            assert_eq!(tree.set_parent(b, Some(a1)), Err(MoveError::Cycle));
            assert_eq!(tree.set_parent(b, Some(b)), Err(MoveError::Cycle));
            // Make it a root again: it goes to the end of the root list.
            assert_eq!(tree.set_parent(a1, None), Ok(()));
            assert_eq!(values(&tree, tree.roots()), [1, 2, 11]);
            assert_eq!(tree.remove(a1), Some(11));
            assert_eq!(tree.set_parent(a1, None), Err(MoveError::Stale));
            assert_eq!(tree.set_parent(a, Some(a1)), Err(MoveError::Stale));
        }
    }
}

#[test]
fn stale_ids_are_harmless() {
    let mut memory = Memory::<Slot<u32>, 2>::new();
    let mut tree = Tree::new(&mut memory);
    let id = tree.insert_root(1).ok();
    assert!(id.is_some());
    if let Some(id) = id {
        assert_eq!(tree.remove(id), Some(1));
        assert_eq!(tree.get(id), None);
        assert_eq!(tree.parent(id), None);
        assert_eq!(tree.depth(id), None);
        assert_eq!(tree.children(id).count(), 0);
        assert_eq!(tree.descendants(id).count(), 0);
        assert_eq!(tree.ancestors(id).count(), 0);
        // The slot is reused, the old id stays stale.
        assert!(tree.insert_root(2).is_ok());
        assert_eq!(tree.get(id), None);
    }
}

#[test]
fn clear_and_iterate() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let mut tree = Tree::new(&mut memory);
    let root = tree.insert_root(1).ok();
    if let Some(root) = root {
        assert!(tree.insert_child(root, 2).is_ok());
        assert!(tree.insert_child(root, 3).is_ok());
    }
    assert_eq!(tree.iter().map(|(_, value)| *value).sum::<u32>(), 6);
    for (_, value) in tree.iter_mut() {
        *value *= 2;
    }
    assert_eq!(tree.iter().map(|(_, value)| *value).sum::<u32>(), 12);
    tree.clear();
    assert!(tree.is_empty());
    assert_eq!(tree.roots().count(), 0);
    for value in 0..4 {
        assert!(tree.insert_root(value).is_ok());
    }
    assert!(tree.is_full());
}

/// Reference model: each node knows its parent and its ordered children.
#[derive(Default)]
struct Model {
    parent: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    alive: Vec<bool>,
    roots: Vec<usize>,
}

impl Model {
    fn add(&mut self, parent: Option<usize>) -> usize {
        let key = self.alive.len();
        self.parent.push(parent);
        self.children.push(Vec::new());
        self.alive.push(true);
        self.attach(key, parent);
        key
    }

    fn attach(&mut self, key: usize, parent: Option<usize>) {
        if let Some(slot) = self.parent.get_mut(key) {
            *slot = parent;
        }
        match parent.and_then(|p| self.children.get_mut(p)) {
            Some(list) => list.push(key),
            None => self.roots.push(key),
        }
    }

    fn detach(&mut self, key: usize) {
        let parent = self.parent.get(key).copied().flatten();
        match parent.and_then(|p| self.children.get_mut(p)) {
            Some(list) => list.retain(|k| *k != key),
            None => self.roots.retain(|k| *k != key),
        }
    }

    fn kill(&mut self, key: usize) {
        if let Some(alive) = self.alive.get_mut(key) {
            *alive = false;
        }
        for child in self.children.get(key).cloned().unwrap_or_default() {
            self.kill(child);
        }
    }

    fn is_under(&self, mut key: usize, ancestor: usize) -> bool {
        loop {
            if key == ancestor {
                return true;
            }
            match self.parent.get(key).copied().flatten() {
                Some(parent) => key = parent,
                None => return false,
            }
        }
    }
}

#[test]
fn matches_a_reference_model() {
    const CAPACITY: usize = 24;
    let mut memory = Memory::<Slot<usize>, CAPACITY>::new();
    let mut tree = Tree::new(&mut memory);
    let mut model = Model::default();
    // ids[key] is the tree id of model node `key`; the node's value is `key`.
    let mut ids: Vec<NodeId<usize>> = Vec::new();
    let mut rng = Rng::new(77);
    let steps = if cfg!(miri) { 150 } else { 4000 };
    for _ in 0..steps {
        let live: Vec<usize> = (0..ids.len())
            .filter(|key| model.alive.get(*key).copied().unwrap_or(false))
            .collect();
        let some = rng.pick(&live).copied();
        let other = rng.pick(&live).copied();
        let key = ids.len();
        match (rng.u32_below(5), some, other) {
            (0, _, _) | (_, None, _) => match tree.insert_root(key) {
                Ok(id) => {
                    assert!(live.len() < CAPACITY);
                    ids.push(id);
                    model.add(None);
                }
                Err(Full(_)) => assert_eq!(live.len(), CAPACITY),
            },
            (1 | 2, Some(parent), _) => {
                let Some(parent_id) = ids.get(parent).copied() else {
                    continue;
                };
                match tree.insert_child(parent_id, key) {
                    Ok(id) => {
                        ids.push(id);
                        model.add(Some(parent));
                    }
                    Err(error) => {
                        assert_eq!(error, InsertError::Full(key));
                        assert_eq!(live.len(), CAPACITY);
                    }
                }
            }
            (3, Some(victim), _) => {
                let Some(id) = ids.get(victim).copied() else {
                    continue;
                };
                assert_eq!(tree.remove(id), Some(victim));
                model.detach(victim);
                model.kill(victim);
            }
            (_, Some(moved), Some(target)) => {
                let (Some(id), Some(target_id)) =
                    (ids.get(moved).copied(), ids.get(target).copied())
                else {
                    continue;
                };
                if model.is_under(target, moved) {
                    assert_eq!(tree.set_parent(id, Some(target_id)), Err(MoveError::Cycle));
                } else {
                    assert_eq!(tree.set_parent(id, Some(target_id)), Ok(()));
                    model.detach(moved);
                    model.attach(moved, Some(target));
                }
            }
            _ => {}
        }

        // Compare the whole structure.
        let alive = model.alive.iter().filter(|alive| **alive).count();
        assert_eq!(tree.len(), alive);
        let roots: Vec<usize> = tree
            .roots()
            .filter_map(|id| tree.get(id).copied())
            .collect();
        assert_eq!(roots, model.roots);
        for (key, id) in ids.iter().enumerate() {
            let is_alive = model.alive.get(key).copied().unwrap_or(false);
            assert_eq!(tree.contains(*id), is_alive);
            if !is_alive {
                continue;
            }
            let parent = tree.parent(*id).and_then(|p| tree.get(p).copied());
            assert_eq!(parent, model.parent.get(key).copied().flatten());
            let children: Vec<usize> = tree
                .children(*id)
                .filter_map(|c| tree.get(c).copied())
                .collect();
            assert_eq!(Some(&children), model.children.get(key));
            let backwards: Vec<usize> =
                core::iter::successors(tree.last_child(*id), |c| tree.prev_sibling(*c))
                    .filter_map(|c| tree.get(c).copied())
                    .collect();
            assert!(backwards.iter().rev().eq(children.iter()));
        }
    }
}
