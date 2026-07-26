//! x86 PCI configuration-mechanism #1 discovery.

use core::arch::asm;

use sanju_kernel::pci::{PciAddress, PciDevice, PciInventory};

const CONFIG_ADDRESS_PORT: u16 = 0x0cf8;
const CONFIG_DATA_PORT: u16 = 0x0cfc;
const CONFIG_ENABLE: u32 = 1 << 31;
const MAX_QUEUED_BUSES: usize = 32;
const INTERRUPT_FLAG: u64 = 1 << 9;

/// Hardware evidence returned by the allocation-free PCI scanner.
#[derive(Clone, Copy)]
pub struct PciDiscoveryReport {
    pub configuration_mechanism_one_active: bool,
    pub inventory_complete: bool,
    pub buses_scanned: usize,
    pub inventory: PciInventory,
}

impl PciDiscoveryReport {
    const fn unavailable() -> Self {
        Self {
            configuration_mechanism_one_active: false,
            inventory_complete: false,
            buses_scanned: 0,
            inventory: PciInventory::new(),
        }
    }
}

/// Enumerates PCI functions reachable from bus zero and discovered bridges.
///
/// # Safety
///
/// The caller must own legacy PCI configuration ports `0xCF8` and `0xCFC`.
/// No other CPU or driver may access configuration mechanism #1 concurrently.
#[must_use]
pub unsafe fn discover_pci() -> PciDiscoveryReport {
    // SAFETY: The caller owns the PCI configuration ports. Interrupts are
    // restored to their incoming state after the complete transaction batch.
    let interrupts_were_enabled = unsafe { disable_interrupts() };
    // SAFETY: Configuration-port ownership is established by the caller.
    let mechanism_active = unsafe { configuration_mechanism_one_available() };
    if !mechanism_active {
        // SAFETY: Restore the caller's interrupt state before returning.
        unsafe {
            restore_interrupts(interrupts_were_enabled);
        }
        return PciDiscoveryReport::unavailable();
    }

    let mut inventory = PciInventory::new();
    let mut bus_queue = [0_u8; MAX_QUEUED_BUSES];
    let mut visited = [false; 256];
    let mut queue_head = 0_usize;
    let mut queue_tail = 1_usize;
    let mut buses_scanned = 0_usize;
    let mut inventory_complete = true;
    bus_queue[0] = 0;
    visited[0] = true;

    while queue_head < queue_tail {
        let bus = bus_queue[queue_head];
        queue_head += 1;
        buses_scanned = buses_scanned.saturating_add(1);

        for device_number in 0..32_u8 {
            let function_zero = PciAddress {
                bus,
                device: device_number,
                function: 0,
            };
            // SAFETY: Interrupts remain disabled and the configuration ports
            // are exclusively owned for this discovery pass.
            let Some(device) = (unsafe { read_device(function_zero) }) else {
                continue;
            };
            let function_count = if device.is_multifunction() { 8 } else { 1 };
            if !record_device_and_bridge(
                device,
                &mut inventory,
                &mut bus_queue,
                &mut visited,
                &mut queue_tail,
            ) {
                inventory_complete = false;
            }

            for function in 1..function_count {
                let address = PciAddress {
                    bus,
                    device: device_number,
                    function,
                };
                // SAFETY: Same exclusive configuration-port ownership.
                let Some(device) = (unsafe { read_device(address) }) else {
                    continue;
                };
                if !record_device_and_bridge(
                    device,
                    &mut inventory,
                    &mut bus_queue,
                    &mut visited,
                    &mut queue_tail,
                ) {
                    inventory_complete = false;
                }
            }
        }
    }

    // SAFETY: All configuration transactions are complete.
    unsafe {
        restore_interrupts(interrupts_were_enabled);
    }
    PciDiscoveryReport {
        configuration_mechanism_one_active: true,
        inventory_complete,
        buses_scanned,
        inventory,
    }
}

fn record_device_and_bridge(
    device: PciDevice,
    inventory: &mut PciInventory,
    bus_queue: &mut [u8; MAX_QUEUED_BUSES],
    visited: &mut [bool; 256],
    queue_tail: &mut usize,
) -> bool {
    let complete = inventory.record(device).is_ok();
    if !device.is_pci_bridge() {
        return complete;
    }

    // SAFETY: The caller disables interrupts around the complete discovery
    // pass and owns the PCI configuration ports.
    let buses = unsafe { read_config_u32(device.address, 0x18) };
    let secondary_bus = u8::try_from((buses >> 8) & 0xff).unwrap_or(0);
    if secondary_bus == 0 || visited[usize::from(secondary_bus)] {
        return complete;
    }
    visited[usize::from(secondary_bus)] = true;
    if *queue_tail == bus_queue.len() {
        return false;
    }
    bus_queue[*queue_tail] = secondary_bus;
    *queue_tail += 1;
    complete
}

unsafe fn read_device(address: PciAddress) -> Option<PciDevice> {
    // SAFETY: The caller serializes configuration-port access.
    let identity = unsafe { read_config_u32(address, 0x00) };
    let vendor_id = u16::try_from(identity & 0xffff).ok()?;
    if vendor_id == u16::MAX {
        return None;
    }
    // SAFETY: Same serialized configuration transaction.
    let class = unsafe { read_config_u32(address, 0x08) };
    // SAFETY: Same serialized configuration transaction.
    let header = unsafe { read_config_u32(address, 0x0c) };
    Some(PciDevice {
        address,
        vendor_id,
        device_id: u16::try_from(identity >> 16).ok()?,
        class_code: u8::try_from(class >> 24).ok()?,
        subclass: u8::try_from((class >> 16) & 0xff).ok()?,
        programming_interface: u8::try_from((class >> 8) & 0xff).ok()?,
        revision: u8::try_from(class & 0xff).ok()?,
        header_type: u8::try_from((header >> 16) & 0xff).ok()?,
    })
}

unsafe fn configuration_mechanism_one_available() -> bool {
    // SAFETY: The discovery caller owns the address port.
    let previous = unsafe { inl(CONFIG_ADDRESS_PORT) };
    let probe = CONFIG_ENABLE;
    // SAFETY: The original address value is restored before returning.
    unsafe {
        outl(CONFIG_ADDRESS_PORT, probe);
    }
    // SAFETY: Reading back CF8 is the mechanism #1 presence probe.
    let available = unsafe { inl(CONFIG_ADDRESS_PORT) } == probe;
    // SAFETY: Restore the configuration selector observed on entry.
    unsafe {
        outl(CONFIG_ADDRESS_PORT, previous);
    }
    available
}

unsafe fn read_config_u32(address: PciAddress, offset: u8) -> u32 {
    let selector = CONFIG_ENABLE
        | (u32::from(address.bus) << 16)
        | (u32::from(address.device) << 11)
        | (u32::from(address.function) << 8)
        | u32::from(offset & 0xfc);
    // SAFETY: The caller serializes the selector/data port pair.
    unsafe {
        outl(CONFIG_ADDRESS_PORT, selector);
        inl(CONFIG_DATA_PORT)
    }
}

unsafe fn disable_interrupts() -> bool {
    let flags: u64;
    // SAFETY: Reading RFLAGS and clearing IF is permitted in Ring 0.
    unsafe {
        asm!(
            "pushfq",
            "pop {flags}",
            "cli",
            flags = out(reg) flags
        );
    }
    flags & INTERRUPT_FLAG != 0
}

unsafe fn restore_interrupts(were_enabled: bool) {
    if were_enabled {
        // SAFETY: This restores the IF state captured by `disable_interrupts`.
        unsafe {
            asm!("sti", options(nomem, nostack));
        }
    }
}

unsafe fn outl(port: u16, value: u32) {
    // SAFETY: The caller owns the selected x86 I/O port.
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: The caller owns the selected x86 I/O port.
    unsafe {
        asm!(
            "in eax, dx",
            in("dx") port,
            out("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
