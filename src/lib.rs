//! vas is a collection of data containers, structures and algorithms for game development focused on speed and memory control.

#![no_std]

pub mod aabb;
pub mod arena;
pub mod binary_tree;
pub mod bitset;
pub mod bvh;
pub mod deque;
mod error;
pub mod graph;
pub mod hash;
pub mod heap;
pub mod kd_tree;
pub mod map;
pub mod memory;
pub mod ordered_map;
pub mod orthtree;
pub mod path;
pub mod pool;
pub mod random;
pub mod spatial_grid;
pub mod static_graph;
pub mod tree;
pub mod vector;

pub use aabb::Aabb;
pub use arena::Arena;
pub use binary_tree::BinaryTree;
pub use bitset::Bitset;
pub use bvh::Bvh;
pub use deque::Deque;
pub use error::{Full, InsertError, OutOfBounds};
pub use graph::Graph;
pub use heap::Heap;
pub use kd_tree::KdTree;
pub use map::Map;
pub use memory::Memory;
pub use ordered_map::OrderedMap;
pub use orthtree::{Octree, Quadtree};
pub use path::PathFinder;
pub use pool::{Handle, Pool};
pub use random::Rng;
pub use spatial_grid::SpatialGrid;
pub use static_graph::StaticGraph;
pub use tree::Tree;
pub use vector::Vector;
