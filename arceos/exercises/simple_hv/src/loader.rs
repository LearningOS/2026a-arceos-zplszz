use axhal::{mem::PAGE_SIZE_4K, paging::MappingFlags};
use axmm::AddrSpace;
use std::fs::File;
use std::io::{self, Read};

use crate::VM_ENTRY;

pub fn load_vm_image(fname: &str, aspace: &mut AddrSpace) -> io::Result<()> {
    let mut file = File::open(fname)?;
    let mut buf = [0; PAGE_SIZE_4K];
    let mut len = 0;
    while len < buf.len() {
        let count = file.read(&mut buf[len..])?;
        if count == 0 {
            break;
        }
        len += count;
    }
    if len == 0 || (len == buf.len() && file.read(&mut [0])? != 0) {
        return Err(io::Error::InvalidData);
    }

    let flags = MappingFlags::READ | MappingFlags::EXECUTE | MappingFlags::USER;
    aspace.map_alloc(VM_ENTRY.into(), PAGE_SIZE_4K, flags, true)?;
    aspace.write(VM_ENTRY.into(), &buf[..len])?;
    unsafe { core::arch::asm!("fence.i") };
    Ok(())
}
