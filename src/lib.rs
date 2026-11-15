//! vas is a collection of data containers, structures and algorithms for game development focused on speed and memory control.

#![no_std]

mod error;
pub mod memory;
pub mod vector;

pub use error::Full;
pub use memory::Memory;
pub use vector::Vector;
