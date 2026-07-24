use core::mem::size_of;
use core::str;

pub const BOOT_INFO_VERSION: u32 = 1;
pub const ARCHITECTURE_TEXT_CAPACITY: usize = 16;
pub const FIRMWARE_TEXT_CAPACITY: usize = 16;
pub const MILESTONE_TEXT_CAPACITY: usize = 128;
pub const COMMAND_LINE_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootInfoError {
    TextTooLong,
    RangeOverflow,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedText<const N: usize> {
    length: u16,
    reserved: u16,
    bytes: [u8; N],
}

impl<const N: usize> FixedText<N> {
    pub fn new(value: &str) -> Result<Self, BootInfoError> {
        let source = value.as_bytes();
        if source.len() > N || source.len() > usize::from(u16::MAX) {
            return Err(BootInfoError::TextTooLong);
        }

        let mut bytes = [0_u8; N];
        bytes[..source.len()].copy_from_slice(source);
        Ok(Self {
            length: u16::try_from(source.len()).map_err(|_| BootInfoError::TextTooLong)?,
            reserved: 0,
            bytes,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        let length = usize::from(self.length).min(N);
        str::from_utf8(&self.bytes[..length]).unwrap_or("<invalid-boot-text>")
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryMapInfo {
    pub buffer_address: u64,
    pub buffer_capacity: u64,
    pub map_size: u64,
    pub map_key: u64,
    pub descriptor_size: u64,
    pub descriptor_version: u32,
    pub reserved: u32,
    pub descriptor_count: u64,
}

impl MemoryMapInfo {
    #[must_use]
    pub const fn is_structurally_valid(self) -> bool {
        self.buffer_address != 0
            && self.map_size <= self.buffer_capacity
            && self.descriptor_size != 0
            && self.map_size.is_multiple_of(self.descriptor_size)
            && self.descriptor_count == self.map_size / self.descriptor_size
    }

    #[must_use]
    pub fn buffer_address_usize(self) -> Option<usize> {
        usize::try_from(self.buffer_address).ok()
    }

    #[must_use]
    pub fn map_size_usize(self) -> Option<usize> {
        usize::try_from(self.map_size).ok()
    }

    #[must_use]
    pub fn descriptor_size_usize(self) -> Option<usize> {
        usize::try_from(self.descriptor_size).ok()
    }

    #[must_use]
    pub fn descriptor_count_usize(self) -> Option<usize> {
        usize::try_from(self.descriptor_count).ok()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalRange {
    pub start: u64,
    pub length: u64,
}

impl PhysicalRange {
    pub const EMPTY: Self = Self {
        start: 0,
        length: 0,
    };

    pub fn from_start_size(start: u64, length: u64) -> Result<Self, BootInfoError> {
        if length == 0 {
            return Ok(Self::EMPTY);
        }
        start
            .checked_add(length)
            .ok_or(BootInfoError::RangeOverflow)?;
        Ok(Self { start, length })
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.length == 0
    }

    #[must_use]
    pub fn end_exclusive(self) -> Option<u64> {
        self.start.checked_add(self.length)
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Unknown = 0,
    Rgb = 1,
    Bgr = 2,
    BitMask = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferInfo {
    pub present: u8,
    pub reserved: [u8; 7],
    pub physical_start: u64,
    pub byte_length: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: PixelFormat,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
}

impl FramebufferInfo {
    pub const ABSENT: Self = Self {
        present: 0,
        reserved: [0; 7],
        physical_start: 0,
        byte_length: 0,
        width: 0,
        height: 0,
        stride: 0,
        pixel_format: PixelFormat::Unknown,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
        reserved_mask: 0,
    };

    #[must_use]
    pub const fn is_present(self) -> bool {
        self.present != 0
    }

    #[must_use]
    pub const fn physical_range(self) -> PhysicalRange {
        if self.present == 0 {
            PhysicalRange::EMPTY
        } else {
            PhysicalRange {
                start: self.physical_start,
                length: self.byte_length,
            }
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionalPhysicalAddress {
    pub present: u8,
    pub reserved: [u8; 7],
    pub address: u64,
}

impl OptionalPhysicalAddress {
    pub const ABSENT: Self = Self {
        present: 0,
        reserved: [0; 7],
        address: 0,
    };

    #[must_use]
    pub const fn is_present(self) -> bool {
        self.present != 0
    }
}

/// Versioned, C-compatible handoff from the UEFI platform layer to the kernel.
///
/// The structure deliberately contains no Rust references, `Option` values, or
/// heap-backed collections. New fields must be appended and guarded by the
/// `version` and `size` header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootInfoV1 {
    pub version: u32,
    pub size: u32,
    architecture: FixedText<ARCHITECTURE_TEXT_CAPACITY>,
    firmware: FixedText<FIRMWARE_TEXT_CAPACITY>,
    milestone: FixedText<MILESTONE_TEXT_CAPACITY>,
    pub memory_map: MemoryMapInfo,
    pub kernel_image: PhysicalRange,
    pub boot_image: PhysicalRange,
    pub boot_info_range: PhysicalRange,
    pub framebuffer: FramebufferInfo,
    pub acpi_rsdp: OptionalPhysicalAddress,
    pub smbios_entry: OptionalPhysicalAddress,
    pub initrd: PhysicalRange,
    command_line: FixedText<COMMAND_LINE_CAPACITY>,
    pub active_page_table_root: u64,
}

impl BootInfoV1 {
    pub fn new(
        architecture: &str,
        firmware: &str,
        milestone: &str,
        memory_map: MemoryMapInfo,
    ) -> Result<Self, BootInfoError> {
        Ok(Self {
            version: BOOT_INFO_VERSION,
            size: u32::try_from(size_of::<Self>()).unwrap_or(u32::MAX),
            architecture: FixedText::new(architecture)?,
            firmware: FixedText::new(firmware)?,
            milestone: FixedText::new(milestone)?,
            memory_map,
            kernel_image: PhysicalRange::EMPTY,
            boot_image: PhysicalRange::EMPTY,
            boot_info_range: PhysicalRange::EMPTY,
            framebuffer: FramebufferInfo::ABSENT,
            acpi_rsdp: OptionalPhysicalAddress::ABSENT,
            smbios_entry: OptionalPhysicalAddress::ABSENT,
            initrd: PhysicalRange::EMPTY,
            command_line: FixedText::new("")?,
            active_page_table_root: 0,
        })
    }

    #[must_use]
    pub fn architecture(&self) -> &str {
        self.architecture.as_str()
    }

    #[must_use]
    pub fn firmware(&self) -> &str {
        self.firmware.as_str()
    }

    #[must_use]
    pub fn milestone(&self) -> &str {
        self.milestone.as_str()
    }

    #[must_use]
    pub fn command_line(&self) -> &str {
        self.command_line.as_str()
    }

    pub fn set_command_line(&mut self, command_line: &str) -> Result<(), BootInfoError> {
        self.command_line = FixedText::new(command_line)?;
        Ok(())
    }

    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.version == BOOT_INFO_VERSION
            && usize::try_from(self.size).is_ok_and(|size| size >= size_of::<Self>())
    }
}

pub type BootInfo = BootInfoV1;

#[cfg(test)]
mod tests {
    use core::mem::{offset_of, size_of};

    use super::{BOOT_INFO_VERSION, BootInfo, MemoryMapInfo, PhysicalRange};

    const fn sample_map() -> MemoryMapInfo {
        MemoryMapInfo {
            buffer_address: 0x10_0000,
            buffer_capacity: 0x20_000,
            map_size: 4_000,
            map_key: 7,
            descriptor_size: 40,
            descriptor_version: 1,
            reserved: 0,
            descriptor_count: 100,
        }
    }

    #[test]
    fn boot_info_v1_is_versioned_and_reference_free_at_the_api_boundary() {
        let info = BootInfo::new("x86_64", "UEFI", "foundation", sample_map()).unwrap();
        assert_eq!(info.version, BOOT_INFO_VERSION);
        assert!(info.is_compatible());
        assert_eq!(info.architecture(), "x86_64");
        assert_eq!(info.firmware(), "UEFI");
        assert_eq!(info.milestone(), "foundation");
    }

    #[test]
    fn boot_info_v1_layout_is_frozen() {
        assert_eq!(size_of::<BootInfo>(), 664);
        assert_eq!(offset_of!(BootInfo, memory_map), 184);
        assert_eq!(offset_of!(BootInfo, active_page_table_root), 656);
    }

    #[test]
    fn physical_range_rejects_overflow() {
        assert!(PhysicalRange::from_start_size(u64::MAX - 1, 4).is_err());
    }
}
