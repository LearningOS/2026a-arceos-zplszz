#![no_std]

use allocator::{AllocError, AllocResult, BaseAllocator, ByteAllocator, PageAllocator};
use core::{alloc::Layout, ptr::NonNull};

/// A two-ended bump allocator over a contiguous memory region.
///
/// Byte allocations grow upward and reset together when the last one is freed.
/// Page allocations grow downward and remain reserved for the allocator's lifetime.
pub struct EarlyAllocator<const SIZE: usize> {
    start: usize,
    end: usize,
    byte_cursor: usize,
    page_cursor: usize,
    live_bytes: usize,
}

impl<const SIZE: usize> Default for EarlyAllocator<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SIZE: usize> EarlyAllocator<SIZE> {
    pub const fn new() -> Self {
        Self {
            start: 0,
            end: 0,
            byte_cursor: 0,
            page_cursor: 0,
            live_bytes: 0,
        }
    }
}

impl<const SIZE: usize> BaseAllocator for EarlyAllocator<SIZE> {
    fn init(&mut self, start: usize, size: usize) {
        assert!(SIZE.is_power_of_two());
        let end = start.checked_add(size).expect("invalid allocator region");
        self.start = start;
        self.end = end;
        self.byte_cursor = start;
        self.page_cursor = end;
        self.live_bytes = 0;
    }

    fn add_memory(&mut self, start: usize, size: usize) -> AllocResult {
        let end = start.checked_add(size).ok_or(AllocError::InvalidParam)?;
        if start < self.end && self.start < end {
            return Err(AllocError::MemoryOverlap);
        }
        if start != self.end || self.page_cursor != self.end {
            return Err(AllocError::InvalidParam);
        }
        self.end = end;
        self.page_cursor = end;
        Ok(())
    }
}

impl<const SIZE: usize> ByteAllocator for EarlyAllocator<SIZE> {
    fn alloc(&mut self, layout: Layout) -> AllocResult<NonNull<u8>> {
        let mask = layout.align() - 1;
        let start = self
            .byte_cursor
            .max(1)
            .checked_add(mask)
            .ok_or(AllocError::NoMemory)?
            & !mask;
        let end = start
            .checked_add(layout.size().max(1))
            .ok_or(AllocError::NoMemory)?;
        if end > self.page_cursor {
            return Err(AllocError::NoMemory);
        }
        let ptr = NonNull::new(start as *mut u8).ok_or(AllocError::NoMemory)?;
        self.byte_cursor = end;
        self.live_bytes += 1;
        Ok(ptr)
    }

    fn dealloc(&mut self, _pos: NonNull<u8>, _layout: Layout) {
        self.live_bytes -= 1;
        if self.live_bytes == 0 {
            self.byte_cursor = self.start;
        }
    }

    fn total_bytes(&self) -> usize {
        self.end - self.start
    }

    fn used_bytes(&self) -> usize {
        self.byte_cursor - self.start
    }

    fn available_bytes(&self) -> usize {
        self.page_cursor - self.byte_cursor
    }
}

impl<const SIZE: usize> PageAllocator for EarlyAllocator<SIZE> {
    const PAGE_SIZE: usize = SIZE;

    fn alloc_pages(&mut self, num_pages: usize, align_pow2: usize) -> AllocResult<usize> {
        if num_pages == 0 || !align_pow2.is_power_of_two() {
            return Err(AllocError::InvalidParam);
        }
        let size = num_pages.checked_mul(SIZE).ok_or(AllocError::NoMemory)?;
        let end = self
            .page_cursor
            .checked_sub(size)
            .ok_or(AllocError::NoMemory)?;
        let start = end & !(align_pow2.max(SIZE) - 1);
        if start < self.byte_cursor {
            return Err(AllocError::NoMemory);
        }
        self.page_cursor = start;
        Ok(start)
    }

    fn dealloc_pages(&mut self, _pos: usize, _num_pages: usize) {
        // Page allocations are permanent in the early allocator.
    }

    fn total_pages(&self) -> usize {
        (self.end / SIZE).saturating_sub(self.start.div_ceil(SIZE))
    }

    fn used_pages(&self) -> usize {
        self.end / SIZE - self.page_cursor / SIZE
    }

    fn available_pages(&self) -> usize {
        (self.page_cursor / SIZE).saturating_sub(self.byte_cursor.div_ceil(SIZE))
    }
}
