#![cfg_attr(feature = "axstd", no_std)]
#![cfg_attr(feature = "axstd", no_main)]
#![feature(asm_const)]
#![feature(riscv_ext_intrinsics)]

extern crate alloc;
#[cfg(feature = "axstd")]
extern crate axstd as std;
#[macro_use]
extern crate axlog;

mod csrs;
mod loader;
mod regs;
mod sbi;
mod task;
mod vcpu;

use crate::regs::GprIndex::{self, A0, A1};
use axhal::mem::{phys_to_virt, virt_to_phys, PhysAddr, VirtAddr, PAGE_SIZE_4K};
use axhal::paging::MappingFlags;
use axmm::AddrSpace;
use csrs::defs::hstatus;
use csrs::{RiscvCsrTrait, CSR};
use loader::load_vm_image;
use riscv::register::{htval, scause, sstatus, stval};
use sbi::SbiMessage;
use std::os::arceos::modules::axalloc::global_allocator;
use tock_registers::LocalRegisterCopy;
use vcpu::_run_guest;
use vcpu::VmCpuRegisters;

const VM_ENTRY: usize = 0x8020_0000;
const GUEST_HART_ID: usize = 0x1234;
const GUEST_DATA_ADDR: usize = 64;
const GUEST_DATA: u64 = 0x6688;
const SV39X4_ROOT_SIZE: usize = 4 * PAGE_SIZE_4K;

#[cfg_attr(feature = "axstd", no_mangle)]
fn main() {
    ax_println!("Hypervisor ...");

    // A new address space for vm.
    // Stay within the low Sv39 guest-physical range covered by the page-table API.
    let mut uspace = AddrSpace::new_empty(0.into(), 1 << 38).unwrap();

    // Load vm binary file into address space.
    if let Err(e) = load_vm_image("/sbin/skernel2", &mut uspace) {
        panic!("Cannot load app! {:?}", e);
    }

    // Setup context to prepare to enter guest mode.
    let mut ctx = VmCpuRegisters::default();
    prepare_guest_context(&mut ctx);

    // Sv39x4 requires a 16 KiB aligned root. The existing page-table API manages
    // the first 4 KiB, which covers the address range used by this guest.
    let root = VirtAddr::from(global_allocator().alloc_pages(4, SV39X4_ROOT_SIZE).unwrap());
    let root_paddr = virt_to_phys(root);
    unsafe { core::ptr::write_bytes(root.as_mut_ptr(), 0, SV39X4_ROOT_SIZE) };
    sync_guest_root(&uspace, root);
    prepare_vm_pgtable(root_paddr);

    loop {
        unsafe { _run_guest(&mut ctx) };
        if vmexit_handler(&mut ctx, &mut uspace, root) {
            break;
        }
    }

    unsafe {
        core::arch::asm!("csrw hgatp, zero");
        core::arch::riscv64::hfence_gvma_all();
    }
    global_allocator().dealloc_pages(root.as_usize(), 4);
}

fn sync_guest_root(aspace: &AddrSpace, root: VirtAddr) {
    let source = phys_to_virt(aspace.page_table_root());
    // Both allocations remain live and the guest is stopped during the copy.
    unsafe { core::ptr::copy_nonoverlapping(source.as_ptr(), root.as_mut_ptr(), PAGE_SIZE_4K) };
}

fn prepare_vm_pgtable(ept_root: PhysAddr) {
    let hgatp = 8usize << 60 | usize::from(ept_root) >> 12;
    unsafe {
        core::arch::asm!(
            "csrw hgatp, {hgatp}",
            hgatp = in(reg) hgatp,
        );
        core::arch::riscv64::hfence_gvma_all();
    }
}

fn vmexit_handler(ctx: &mut VmCpuRegisters, aspace: &mut AddrSpace, root: VirtAddr) -> bool {
    use scause::{Exception, Trap};

    let cause = scause::read().cause();
    let trap_value = stval::read();
    match cause {
        Trap::Exception(Exception::VirtualSupervisorEnvCall) => {
            let msg = SbiMessage::from_regs(ctx.guest_regs.gprs.a_regs()).ok();
            ax_println!("VmExit Reason: VSuperEcall: {:?}", msg);
            assert!(
                matches!(msg, Some(SbiMessage::Reset(_))),
                "expected guest shutdown"
            );
            let data = ctx.guest_regs.gprs.reg(A0);
            let hart = ctx.guest_regs.gprs.reg(A1);
            ax_println!("a0 = {:#x}, a1 = {:#x}", data, hart);
            assert_eq!(data, GUEST_DATA as usize);
            assert_eq!(hart, GUEST_HART_ID);
            ax_println!("Shutdown vm normally!");
            return true;
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            // csrr rd, mhartid encodes CSRRS with x0 as its source.
            const CSRRS_MHARTID: usize = (0xf14 << 20) | (0b010 << 12) | 0x73;
            assert_eq!(
                trap_value & !(0x1f << 7),
                CSRRS_MHARTID,
                "unsupported guest instruction"
            );
            let rd = GprIndex::from_raw(((trap_value >> 7) & 0x1f) as u32).unwrap();
            ctx.guest_regs.gprs.set_reg(rd, GUEST_HART_ID);
            ctx.guest_regs.sepc += 4;
        }
        Trap::Exception(Exception::LoadGuestPageFault) => {
            // htval encodes the guest physical address shifted right by two bits.
            let gpa = (htval::read() << 2) | (trap_value & 3);
            assert_eq!(gpa, GUEST_DATA_ADDR, "unexpected guest load fault");
            let flags = MappingFlags::READ | MappingFlags::USER;
            aspace
                .map_alloc(0.into(), PAGE_SIZE_4K, flags, true)
                .unwrap();
            aspace
                .write(GUEST_DATA_ADDR.into(), &GUEST_DATA.to_le_bytes())
                .unwrap();
            sync_guest_root(aspace, root);
            unsafe { core::arch::riscv64::hfence_gvma_all() };
            // Retry the faulting load with the new mapping.
        }
        _ => panic!(
            "Unhandled trap: {:?}, sepc: {:#x}, stval: {:#x}",
            cause, ctx.guest_regs.sepc, trap_value
        ),
    }
    false
}

fn prepare_guest_context(ctx: &mut VmCpuRegisters) {
    // Set hstatus
    let mut hstatus =
        LocalRegisterCopy::<usize, hstatus::Register>::new(riscv::register::hstatus::read().bits());
    // Set Guest bit in order to return to guest mode.
    hstatus.modify(hstatus::spv::Guest);
    // Set SPVP bit in order to accessing VS-mode memory from HS-mode.
    hstatus.modify(hstatus::spvp::Supervisor);
    CSR.hstatus.write_value(hstatus.get());
    ctx.guest_regs.hstatus = hstatus.get();

    // Set sstatus in guest mode.
    let mut sstatus = sstatus::read();
    sstatus.set_spp(sstatus::SPP::Supervisor);
    ctx.guest_regs.sstatus = sstatus.bits();
    // Return to entry to start vm.
    ctx.guest_regs.sepc = VM_ENTRY;
}
