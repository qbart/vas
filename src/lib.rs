//! vas is a collection of data containers, structures and algorithms for game development focused on speed and memory control.

#![no_std]

mod error;
pub mod memory;
pub mod pool;
pub mod vector;

pub use error::{Full, InsertError};
pub use memory::Memory;
pub use pool::{Handle, Pool};
pub use vector::Vector;
