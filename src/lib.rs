//! vas is a collection of data containers, structures and algorithms for game development focused on speed and memory control.

#![no_std]

pub mod bitset;
pub mod deque;
mod error;
pub mod hash;
pub mod heap;
pub mod map;
pub mod memory;
pub mod path;
pub mod pool;
pub mod random;
pub mod tree;
pub mod vector;

pub use bitset::Bitset;
pub use deque::Deque;
pub use error::{Full, InsertError, OutOfBounds};
pub use heap::Heap;
pub use map::Map;
pub use memory::Memory;
pub use path::PathFinder;
pub use pool::{Handle, Pool};
pub use random::Rng;
pub use tree::Tree;
pub use vector::Vector;
