cfg_if::cfg_if! {
    if #[cfg(feature = "myfs")] {
        pub mod myfs;
    } else if #[cfg(feature = "fatfs")] {
        pub mod fatfs;
    }
}

#[cfg(feature = "devfs")]
pub use axfs_devfs as devfs;

#[cfg(any(feature = "ramfs", feature = "procfs", feature = "sysfs"))]
use axsync::Mutex;

// Include the course RAMFS implementation so the mounted filesystem uses it.
#[cfg(any(feature = "ramfs", feature = "procfs", feature = "sysfs"))]
#[path = "../../../../axfs_ramfs/src/fs.rs"]
pub mod ramfs;
