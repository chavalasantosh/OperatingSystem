//! Architecture-independent PCI inventory and storage-controller matching.

pub const MAX_PCI_DEVICES: usize = 64;

/// Address-space type encoded by one PCI base-address register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PciBarKind {
    Io,
    Memory32,
    Memory64,
}

/// Decoded, firmware-assigned PCI base-address register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PciBar {
    pub index: u8,
    pub kind: PciBarKind,
    pub base_address: u64,
    pub prefetchable: bool,
}

/// Invalid or unsupported BAR encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PciBarError {
    InvalidIndex,
    MissingUpperHalf,
    ReservedMemoryType,
    Unassigned,
}

/// Decodes one firmware-assigned BAR without modifying PCI configuration.
///
/// The returned boolean is true when a 64-bit BAR consumes `index + 1`.
///
/// # Errors
///
/// Returns [`PciBarError`] for reserved, truncated, or unassigned encodings.
pub fn decode_bar(index: u8, low: u32, upper: Option<u32>) -> Result<(PciBar, bool), PciBarError> {
    if index >= 6 {
        return Err(PciBarError::InvalidIndex);
    }
    if low & 1 != 0 {
        let base_address = u64::from(low & !0x03);
        if base_address == 0 {
            return Err(PciBarError::Unassigned);
        }
        return Ok((
            PciBar {
                index,
                kind: PciBarKind::Io,
                base_address,
                prefetchable: false,
            },
            false,
        ));
    }

    let prefetchable = low & 0x08 != 0;
    match (low >> 1) & 0x03 {
        0 => {
            let base_address = u64::from(low & !0x0f);
            if base_address == 0 {
                return Err(PciBarError::Unassigned);
            }
            Ok((
                PciBar {
                    index,
                    kind: PciBarKind::Memory32,
                    base_address,
                    prefetchable,
                },
                false,
            ))
        }
        2 => {
            if index == 5 {
                return Err(PciBarError::MissingUpperHalf);
            }
            let Some(upper) = upper else {
                return Err(PciBarError::MissingUpperHalf);
            };
            let base_address = (u64::from(upper) << 32) | u64::from(low & !0x0f);
            if base_address == 0 {
                return Err(PciBarError::Unassigned);
            }
            Ok((
                PciBar {
                    index,
                    kind: PciBarKind::Memory64,
                    base_address,
                    prefetchable,
                },
                true,
            ))
        }
        _ => Err(PciBarError::ReservedMemoryType),
    }
}

/// One PCI bus/device/function address.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    #[must_use]
    pub const fn new(bus: u8, device: u8, function: u8) -> Option<Self> {
        if device < 32 && function < 8 {
            Some(Self {
                bus,
                device,
                function,
            })
        } else {
            None
        }
    }
}

/// Storage-controller family selected from PCI identity and class fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageControllerKind {
    VirtioBlock,
    Ahci,
    Nvme,
    Ide,
    OtherMassStorage,
}

/// Immutable PCI function descriptor retained after enumeration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub programming_interface: u8,
    pub revision: u8,
    pub header_type: u8,
}

impl PciDevice {
    const fn empty() -> Self {
        Self {
            address: PciAddress {
                bus: 0,
                device: 0,
                function: 0,
            },
            vendor_id: u16::MAX,
            device_id: u16::MAX,
            class_code: 0,
            subclass: 0,
            programming_interface: 0,
            revision: 0,
            header_type: 0,
        }
    }

    #[must_use]
    pub const fn is_multifunction(self) -> bool {
        self.header_type & 0x80 != 0
    }

    #[must_use]
    pub const fn is_pci_bridge(self) -> bool {
        self.class_code == 0x06 && self.subclass == 0x04
    }

    #[must_use]
    pub const fn storage_kind(self) -> Option<StorageControllerKind> {
        if self.vendor_id == 0x1af4 && matches!(self.device_id, 0x1001 | 0x1042) {
            return Some(StorageControllerKind::VirtioBlock);
        }
        if self.class_code != 0x01 {
            return None;
        }
        Some(match (self.subclass, self.programming_interface) {
            (0x01, _) => StorageControllerKind::Ide,
            (0x06, 0x01) => StorageControllerKind::Ahci,
            (0x08, 0x02) => StorageControllerKind::Nvme,
            _ => StorageControllerKind::OtherMassStorage,
        })
    }
}

/// Failure to retain a discovered PCI function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PciInventoryError {
    Full,
    Duplicate,
}

/// Fixed-capacity PCI inventory used before general kernel allocation.
#[derive(Clone, Copy)]
pub struct PciInventory {
    devices: [PciDevice; MAX_PCI_DEVICES],
    count: usize,
}

impl PciInventory {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            devices: [PciDevice::empty(); MAX_PCI_DEVICES],
            count: 0,
        }
    }

    /// Retains one discovered function.
    ///
    /// # Errors
    ///
    /// Returns [`PciInventoryError::Full`] when the fixed inventory is full or
    /// [`PciInventoryError::Duplicate`] for a repeated BDF address.
    pub fn record(&mut self, device: PciDevice) -> Result<(), PciInventoryError> {
        if self
            .devices()
            .iter()
            .any(|existing| existing.address == device.address)
        {
            return Err(PciInventoryError::Duplicate);
        }
        if self.count == self.devices.len() {
            return Err(PciInventoryError::Full);
        }
        self.devices[self.count] = device;
        self.count += 1;
        Ok(())
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[must_use]
    pub fn devices(&self) -> &[PciDevice] {
        &self.devices[..self.count]
    }

    #[must_use]
    pub fn storage_controller_count(&self) -> usize {
        self.devices()
            .iter()
            .filter(|device| device.storage_kind().is_some())
            .count()
    }

    #[must_use]
    pub fn storage_kind_count(&self, kind: StorageControllerKind) -> usize {
        self.devices()
            .iter()
            .filter(|device| device.storage_kind() == Some(kind))
            .count()
    }
}

impl Default for PciInventory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PCI_DEVICES, PciAddress, PciBar, PciBarError, PciBarKind, PciDevice, PciInventory,
        PciInventoryError, StorageControllerKind, decode_bar,
    };

    fn device(
        index: u8,
        vendor_id: u16,
        device_id: u16,
        class_code: u8,
        subclass: u8,
        programming_interface: u8,
    ) -> PciDevice {
        PciDevice {
            address: PciAddress::new(0, index, 0).unwrap(),
            vendor_id,
            device_id,
            class_code,
            subclass,
            programming_interface,
            revision: 1,
            header_type: 0,
        }
    }

    #[test]
    fn classifies_supported_storage_targets() {
        let virtio = device(1, 0x1af4, 0x1042, 0x01, 0x00, 0x00);
        let ahci = device(2, 0x8086, 0x2922, 0x01, 0x06, 0x01);
        let nvme = device(3, 0x8086, 0xf1a5, 0x01, 0x08, 0x02);
        assert_eq!(
            virtio.storage_kind(),
            Some(StorageControllerKind::VirtioBlock)
        );
        assert_eq!(ahci.storage_kind(), Some(StorageControllerKind::Ahci));
        assert_eq!(nvme.storage_kind(), Some(StorageControllerKind::Nvme));
    }

    #[test]
    fn pci_addresses_enforce_device_and_function_widths() {
        assert!(PciAddress::new(255, 31, 7).is_some());
        assert!(PciAddress::new(0, 32, 0).is_none());
        assert!(PciAddress::new(0, 0, 8).is_none());
    }

    #[test]
    fn decodes_memory_and_io_bars_without_configuration_writes() {
        assert_eq!(
            decode_bar(0, 0x0000_c001, None),
            Ok((
                PciBar {
                    index: 0,
                    kind: PciBarKind::Io,
                    base_address: 0xc000,
                    prefetchable: false,
                },
                false,
            ))
        );
        assert_eq!(
            decode_bar(2, 0x9000_000c, Some(1)),
            Ok((
                PciBar {
                    index: 2,
                    kind: PciBarKind::Memory64,
                    base_address: 0x0000_0001_9000_0000,
                    prefetchable: true,
                },
                true,
            ))
        );
        assert_eq!(
            decode_bar(5, 0x0000_0004, None),
            Err(PciBarError::MissingUpperHalf)
        );
        assert_eq!(decode_bar(0, 0, None), Err(PciBarError::Unassigned));
    }

    #[test]
    fn inventory_rejects_duplicates_and_overflow() {
        let mut inventory = PciInventory::new();
        let first = device(0, 0x1234, 0x0001, 0x06, 0x00, 0x00);
        inventory.record(first).unwrap();
        assert_eq!(inventory.record(first), Err(PciInventoryError::Duplicate));
        for index in 1..u8::try_from(MAX_PCI_DEVICES).unwrap() {
            inventory
                .record(PciDevice {
                    address: PciAddress {
                        bus: index / 32,
                        device: index % 32,
                        function: 0,
                    },
                    ..first
                })
                .unwrap();
        }
        assert_eq!(
            inventory.record(PciDevice {
                address: PciAddress::new(2, 0, 0).unwrap(),
                ..first
            }),
            Err(PciInventoryError::Full)
        );
    }
}
