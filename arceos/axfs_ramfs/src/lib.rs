//! RAM filesystem used by [ArceOS](https://github.com/arceos-org/arceos).
//!
//! The implementation is based on [`axfs_vfs`].

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod fs;
use spin::Mutex;

#[cfg(test)]
mod tests;

pub use self::fs::{DirNode, FileNode, RamFileSystem};
