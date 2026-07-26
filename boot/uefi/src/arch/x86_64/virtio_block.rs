//! Polling virtio-blk transport over modern PCI capabilities.

use core::cmp;
use core::hint::spin_loop;
use core::ptr;
use core::sync::atomic::{Ordering, fence};

use sanju_kernel::block::{
    BlockDevice, BlockError, BlockGeometry, SECTOR_SIZE, validate_sector_range,
};
use sanju_kernel::memory::{FrameAllocator, PAGE_SIZE, PhysicalFrame};
use sanju_kernel::paging::VirtualMemoryLayout;
use sanju_kernel::pci::{PciAddress, PciBar, PciInventory, StorageControllerKind, decode_bar};

use super::pci::PciConfigSession;

const PCI_COMMAND_OFFSET: u8 = 0x04;
const PCI_STATUS_OFFSET: u8 = 0x06;
const PCI_BAR_ZERO_OFFSET: u8 = 0x10;
const PCI_CAPABILITY_POINTER_OFFSET: u8 = 0x34;
const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;
const PCI_STATUS_CAPABILITY_LIST: u16 = 1 << 4;
const PCI_CAPABILITY_VENDOR_SPECIFIC: u8 = 0x09;
const MAX_CAPABILITIES: usize = 48;

const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;

const COMMON_DEVICE_FEATURE_SELECT: u32 = 0;
const COMMON_DEVICE_FEATURE: u32 = 4;
const COMMON_DRIVER_FEATURE_SELECT: u32 = 8;
const COMMON_DRIVER_FEATURE: u32 = 12;
const COMMON_DEVICE_STATUS: u32 = 20;
const COMMON_CONFIG_GENERATION: u32 = 21;
const COMMON_QUEUE_SELECT: u32 = 22;
const COMMON_QUEUE_SIZE: u32 = 24;
const COMMON_QUEUE_ENABLE: u32 = 28;
const COMMON_QUEUE_NOTIFY_OFFSET: u32 = 30;
const COMMON_QUEUE_DESC_LOW: u32 = 32;
const COMMON_QUEUE_DESC_HIGH: u32 = 36;
const COMMON_QUEUE_DRIVER_LOW: u32 = 40;
const COMMON_QUEUE_DRIVER_HIGH: u32 = 44;
const COMMON_QUEUE_DEVICE_LOW: u32 = 48;
const COMMON_QUEUE_DEVICE_HIGH: u32 = 52;
const MIN_COMMON_CONFIG_BYTES: u32 = 56;

const DEVICE_STATUS_ACKNOWLEDGE: u8 = 1;
const DEVICE_STATUS_DRIVER: u8 = 2;
const DEVICE_STATUS_DRIVER_OK: u8 = 4;
const DEVICE_STATUS_FEATURES_OK: u8 = 8;
const DEVICE_STATUS_DEVICE_NEEDS_RESET: u8 = 64;
const DEVICE_STATUS_FAILED: u8 = 128;

const VIRTIO_BLK_F_RO: u32 = 1 << 5;
const VIRTIO_F_VERSION_1_HIGH: u32 = 1;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_GET_ID: u32 = 8;
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;
const VIRTQ_AVAIL_F_NO_INTERRUPT: u16 = 1;
const DRIVER_QUEUE_SIZE: u16 = 8;
const MIN_REQUEST_DESCRIPTORS: u16 = 3;
const POLL_LIMIT: usize = 10_000_000;

const DESCRIPTOR_AREA_OFFSET: usize = 0x000;
const AVAILABLE_AREA_OFFSET: usize = 0x080;
const USED_AREA_OFFSET: usize = 0x098;
const REQUEST_HEADER_OFFSET: usize = 0x100;
const DATA_BUFFER_OFFSET: usize = 0x200;
const STATUS_BYTE_OFFSET: usize = 0x400;

const KNOWN_READ_SECTOR: u64 = 8;
const DISPOSABLE_WRITE_SECTOR: u64 = 16;
const EXPECTED_DEVICE_ID: &[u8] = b"SANJU-M6B";
const EXPECTED_READ_PATTERN: &[u8] = b"SANJUOS-M6B-READ-PATTERN";

/// Hardware evidence returned by the M6B transport and request probes.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtioBlockProbeReport {
    pub modern_pci_capabilities_active: bool,
    pub pci_bars_parsed: usize,
    pub pci_bus_master_active: bool,
    pub feature_negotiation_active: bool,
    pub dma_queue_active: bool,
    pub queue_size: u16,
    pub capacity_sectors: u64,
    pub dedicated_device_identity_verified: bool,
    pub known_sector_read_passed: bool,
    pub disposable_sector_write_readback_passed: bool,
    pub disposable_sector_restored: bool,
    pub bounds_check_passed: bool,
    pub timeout_protection_active: bool,
}

/// Failures that prevent the M6B virtio block device from becoming live.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioBlockError {
    TargetUnavailable,
    AmbiguousTarget,
    CapabilityListUnavailable,
    MalformedCapabilityList,
    MissingCommonConfiguration,
    MissingNotificationConfiguration,
    MissingDeviceConfiguration,
    MissingPciConfigurationWindow,
    InvalidCapability,
    InvalidBar,
    PciCommandRejected,
    ResetTimeout,
    UnsupportedFeatures,
    FeatureNegotiationRejected,
    QueueUnavailable,
    QueueTooSmall,
    DmaUnavailable,
    DirectMapUnavailable,
    QueueEnableRejected,
    DriverRejected,
    InvalidCapacity,
    Block(BlockError),
}

impl From<BlockError> for VirtioBlockError {
    fn from(value: BlockError) -> Self {
        Self::Block(value)
    }
}

#[derive(Clone, Copy)]
struct VirtioRegion {
    bar: u8,
    offset: u32,
    length: u32,
}

#[derive(Clone, Copy)]
struct VirtioPciTransport {
    address: PciAddress,
    common: VirtioRegion,
    notify: VirtioRegion,
    device: VirtioRegion,
    pci_config_capability: u8,
    notify_multiplier: u32,
    parsed_bars: usize,
}

#[derive(Clone, Copy)]
struct PciConfigWindow {
    address: PciAddress,
    capability: u8,
}

impl PciConfigWindow {
    unsafe fn read_u8(
        self,
        region: VirtioRegion,
        register_offset: u32,
    ) -> Result<u8, VirtioBlockError> {
        let absolute = validate_region_access(region, register_offset, 1)?;
        // SAFETY: The bootstrap CPU is the only PCI configuration client.
        let session = unsafe { PciConfigSession::acquire() };
        // SAFETY: The parsed PCI configuration capability is bounded to the
        // standard 256-byte configuration space.
        unsafe {
            self.select(&session, region.bar, absolute, 1)?;
            Ok(session.read_u8(self.address, self.data_offset()?))
        }
    }

    unsafe fn read_u16(
        self,
        region: VirtioRegion,
        register_offset: u32,
    ) -> Result<u16, VirtioBlockError> {
        let absolute = validate_region_access(region, register_offset, 2)?;
        // SAFETY: The bootstrap CPU is the only PCI configuration client.
        let session = unsafe { PciConfigSession::acquire() };
        // SAFETY: Region validation enforces the required width and alignment.
        unsafe {
            self.select(&session, region.bar, absolute, 2)?;
            Ok(session.read_u16(self.address, self.data_offset()?))
        }
    }

    unsafe fn read_u32(
        self,
        region: VirtioRegion,
        register_offset: u32,
    ) -> Result<u32, VirtioBlockError> {
        let absolute = validate_region_access(region, register_offset, 4)?;
        // SAFETY: The bootstrap CPU is the only PCI configuration client.
        let session = unsafe { PciConfigSession::acquire() };
        // SAFETY: Region validation enforces the required width and alignment.
        unsafe {
            self.select(&session, region.bar, absolute, 4)?;
            Ok(session.read_u32(self.address, self.data_offset()?))
        }
    }

    unsafe fn write_u8(
        self,
        region: VirtioRegion,
        register_offset: u32,
        value: u8,
    ) -> Result<(), VirtioBlockError> {
        let absolute = validate_region_access(region, register_offset, 1)?;
        // SAFETY: The bootstrap CPU is the only PCI configuration client.
        let session = unsafe { PciConfigSession::acquire() };
        // SAFETY: Region validation bounds the selected device register.
        unsafe {
            self.select(&session, region.bar, absolute, 1)?;
            session.write_u8(self.address, self.data_offset()?, value);
        }
        Ok(())
    }

    unsafe fn write_u16(
        self,
        region: VirtioRegion,
        register_offset: u32,
        value: u16,
    ) -> Result<(), VirtioBlockError> {
        let absolute = validate_region_access(region, register_offset, 2)?;
        // SAFETY: The bootstrap CPU is the only PCI configuration client.
        let session = unsafe { PciConfigSession::acquire() };
        // SAFETY: Region validation enforces the required width and alignment.
        unsafe {
            self.select(&session, region.bar, absolute, 2)?;
            session.write_u16(self.address, self.data_offset()?, value);
        }
        Ok(())
    }

    unsafe fn write_u32(
        self,
        region: VirtioRegion,
        register_offset: u32,
        value: u32,
    ) -> Result<(), VirtioBlockError> {
        let absolute = validate_region_access(region, register_offset, 4)?;
        // SAFETY: The bootstrap CPU is the only PCI configuration client.
        let session = unsafe { PciConfigSession::acquire() };
        // SAFETY: Region validation enforces the required width and alignment.
        unsafe {
            self.select(&session, region.bar, absolute, 4)?;
            session.write_u32(self.address, self.data_offset()?, value);
        }
        Ok(())
    }

    unsafe fn select(
        self,
        session: &PciConfigSession,
        bar: u8,
        absolute_offset: u32,
        width: u32,
    ) -> Result<(), VirtioBlockError> {
        let bar_offset = self
            .capability
            .checked_add(4)
            .ok_or(VirtioBlockError::InvalidCapability)?;
        let region_offset = self
            .capability
            .checked_add(8)
            .ok_or(VirtioBlockError::InvalidCapability)?;
        let length_offset = self
            .capability
            .checked_add(12)
            .ok_or(VirtioBlockError::InvalidCapability)?;
        // SAFETY: The caller owns this configuration session and each field is
        // writable for VIRTIO_PCI_CAP_PCI_CFG.
        unsafe {
            session.write_u8(self.address, bar_offset, bar);
            session.write_u32(self.address, region_offset, absolute_offset);
            session.write_u32(self.address, length_offset, width);
        }
        Ok(())
    }

    fn data_offset(self) -> Result<u8, VirtioBlockError> {
        self.capability
            .checked_add(16)
            .ok_or(VirtioBlockError::InvalidCapability)
    }
}

struct DmaQueue {
    physical_base: u64,
    virtual_base: *mut u8,
    queue_size: u16,
    available_index: u16,
    last_used_index: u16,
}

impl DmaQueue {
    unsafe fn new(frame: PhysicalFrame, queue_size: u16) -> Result<Self, VirtioBlockError> {
        let virtual_address = VirtualMemoryLayout::sanjuos()
            .direct_map_address(frame.start_address())
            .ok_or(VirtioBlockError::DirectMapUnavailable)?;
        let virtual_address =
            usize::try_from(virtual_address).map_err(|_| VirtioBlockError::DirectMapUnavailable)?;
        let virtual_base = virtual_address as *mut u8;
        // SAFETY: The allocator exclusively assigned this complete frame to
        // the driver and the direct map aliases the same physical bytes.
        unsafe {
            ptr::write_bytes(virtual_base, 0, usize::try_from(PAGE_SIZE).unwrap_or(4096));
            write_u16(
                virtual_base,
                AVAILABLE_AREA_OFFSET,
                VIRTQ_AVAIL_F_NO_INTERRUPT,
            );
        }
        Ok(Self {
            physical_base: frame.start_address(),
            virtual_base,
            queue_size,
            available_index: 0,
            last_used_index: 0,
        })
    }

    fn descriptor_physical(&self) -> u64 {
        self.physical_base + u64::try_from(DESCRIPTOR_AREA_OFFSET).unwrap_or(0)
    }

    fn available_physical(&self) -> u64 {
        self.physical_base + u64::try_from(AVAILABLE_AREA_OFFSET).unwrap_or(0)
    }

    fn used_physical(&self) -> u64 {
        self.physical_base + u64::try_from(USED_AREA_OFFSET).unwrap_or(0)
    }

    unsafe fn prepare_request(
        &mut self,
        request_type: u32,
        sector: u64,
        data: &[u8; SECTOR_SIZE],
        data_length: usize,
        device_writes_data: bool,
    ) -> Result<u16, BlockError> {
        if data_length == 0 || data_length > SECTOR_SIZE {
            return Err(BlockError::InvalidBuffer);
        }
        // SAFETY: All fixed offsets were selected to remain within the
        // allocator-owned DMA frame and meet split-ring alignment.
        unsafe {
            write_u32(
                self.virtual_base,
                REQUEST_HEADER_OFFSET,
                request_type.to_le(),
            );
            write_u32(self.virtual_base, REQUEST_HEADER_OFFSET + 4, 0);
            write_u64(self.virtual_base, REQUEST_HEADER_OFFSET + 8, sector.to_le());
            for (index, byte) in data.iter().take(data_length).enumerate() {
                let value = if device_writes_data { 0 } else { *byte };
                self.virtual_base
                    .add(DATA_BUFFER_OFFSET + index)
                    .write_volatile(value);
            }
            self.virtual_base
                .add(STATUS_BYTE_OFFSET)
                .write_volatile(u8::MAX);

            self.write_descriptor(
                0,
                self.physical_base + u64::try_from(REQUEST_HEADER_OFFSET).unwrap_or(0),
                16,
                VIRTQ_DESC_F_NEXT,
                1,
            );
            self.write_descriptor(
                1,
                self.physical_base + u64::try_from(DATA_BUFFER_OFFSET).unwrap_or(0),
                u32::try_from(data_length).map_err(|_| BlockError::InvalidBuffer)?,
                VIRTQ_DESC_F_NEXT
                    | if device_writes_data {
                        VIRTQ_DESC_F_WRITE
                    } else {
                        0
                    },
                2,
            );
            self.write_descriptor(
                2,
                self.physical_base + u64::try_from(STATUS_BYTE_OFFSET).unwrap_or(0),
                1,
                VIRTQ_DESC_F_WRITE,
                0,
            );

            let slot = self.available_index % self.queue_size;
            write_u16(
                self.virtual_base,
                AVAILABLE_AREA_OFFSET + 4 + usize::from(slot) * 2,
                0,
            );
            fence(Ordering::SeqCst);
            self.available_index = self.available_index.wrapping_add(1);
            write_u16(
                self.virtual_base,
                AVAILABLE_AREA_OFFSET + 2,
                self.available_index,
            );
            fence(Ordering::SeqCst);
        }
        Ok(self.last_used_index.wrapping_add(1))
    }

    unsafe fn poll_completion(
        &mut self,
        expected_used_index: u16,
        status_reader: impl Fn() -> Result<u8, BlockError>,
    ) -> Result<(), BlockError> {
        for iteration in 0..POLL_LIMIT {
            // SAFETY: The used index is device-owned coherent DMA memory.
            let used_index = unsafe { read_u16(self.virtual_base, USED_AREA_OFFSET + 2) };
            if used_index == expected_used_index {
                fence(Ordering::SeqCst);
                let slot = self.last_used_index % self.queue_size;
                // SAFETY: The device completed this used-ring entry before
                // advancing the index observed above.
                let descriptor_id = unsafe {
                    read_u32(
                        self.virtual_base,
                        USED_AREA_OFFSET + 4 + usize::from(slot) * 8,
                    )
                };
                if descriptor_id != 0 {
                    return Err(BlockError::Transport);
                }
                self.last_used_index = used_index;
                return Ok(());
            }
            if used_index != self.last_used_index {
                return Err(BlockError::Transport);
            }
            if iteration.is_multiple_of(4096)
                && status_reader()? & DEVICE_STATUS_DEVICE_NEEDS_RESET != 0
            {
                return Err(BlockError::DeviceNeedsReset);
            }
            spin_loop();
        }
        Err(BlockError::Timeout)
    }

    unsafe fn read_status(&self) -> u8 {
        // SAFETY: The status byte is inside the allocator-owned DMA frame.
        unsafe { self.virtual_base.add(STATUS_BYTE_OFFSET).read_volatile() }
    }

    unsafe fn copy_device_data(&self, destination: &mut [u8], data_length: usize) {
        for (index, byte) in destination.iter_mut().take(data_length).enumerate() {
            // SAFETY: `data_length` was bounded to the fixed DMA data buffer.
            *byte = unsafe {
                self.virtual_base
                    .add(DATA_BUFFER_OFFSET + index)
                    .read_volatile()
            };
        }
    }

    unsafe fn write_descriptor(
        &self,
        index: usize,
        address: u64,
        length: u32,
        flags: u16,
        next: u16,
    ) {
        let descriptor = DESCRIPTOR_AREA_OFFSET + index * 16;
        // SAFETY: The queue size is at most the eight descriptor slots
        // reserved in this DMA frame.
        unsafe {
            write_u64(self.virtual_base, descriptor, address.to_le());
            write_u32(self.virtual_base, descriptor + 8, length.to_le());
            write_u16(self.virtual_base, descriptor + 12, flags.to_le());
            write_u16(self.virtual_base, descriptor + 14, next.to_le());
        }
    }
}

/// One live, synchronous virtio block device.
pub struct VirtioBlockDevice {
    window: PciConfigWindow,
    common: VirtioRegion,
    notify: VirtioRegion,
    geometry: BlockGeometry,
    queue: DmaQueue,
}

impl VirtioBlockDevice {
    fn submit(
        &mut self,
        request_type: u32,
        sector: u64,
        buffer: &mut [u8; SECTOR_SIZE],
        data_length: usize,
        device_writes_data: bool,
    ) -> Result<(), BlockError> {
        // SAFETY: This device permits exactly one outstanding request and the
        // DMA queue remains allocated for its entire lifetime.
        let expected = unsafe {
            self.queue.prepare_request(
                request_type,
                sector,
                buffer,
                data_length,
                device_writes_data,
            )?
        };
        // SAFETY: The notify region and queue index were validated during
        // initialization, and DRIVER_OK is set before any notification.
        unsafe {
            self.window
                .write_u16(self.notify, 0, 0)
                .map_err(|_| BlockError::Transport)?;
            self.queue.poll_completion(expected, || {
                self.window
                    .read_u8(self.common, COMMON_DEVICE_STATUS)
                    .map_err(|_| BlockError::Transport)
            })?;
        }
        fence(Ordering::SeqCst);
        // SAFETY: Completion transfers ownership of device-writable buffers
        // back to the driver.
        let status = unsafe { self.queue.read_status() };
        match status {
            VIRTIO_BLK_S_OK => {}
            VIRTIO_BLK_S_IOERR => return Err(BlockError::Io),
            VIRTIO_BLK_S_UNSUPP => return Err(BlockError::UnsupportedRequest),
            _ => return Err(BlockError::Transport),
        }
        if device_writes_data {
            // SAFETY: The status byte completed after the data descriptors.
            unsafe {
                self.queue
                    .copy_device_data(&mut buffer[..data_length], data_length);
            }
        }
        Ok(())
    }

    fn read_device_id(&mut self) -> Result<[u8; 20], BlockError> {
        let mut sector_buffer = [0_u8; SECTOR_SIZE];
        self.submit(VIRTIO_BLK_T_GET_ID, 0, &mut sector_buffer, 20, true)?;
        let mut device_id = [0_u8; 20];
        device_id.copy_from_slice(&sector_buffer[..20]);
        Ok(device_id)
    }

    /// Executes the bounded M6B hardware acceptance probes.
    ///
    /// # Errors
    ///
    /// Returns a transport or block error if device identity, known-sector
    /// read, disposable write/readback, or restoration cannot be completed.
    fn run_acceptance_probe(
        &mut self,
        transport: VirtioPciTransport,
    ) -> Result<VirtioBlockProbeReport, VirtioBlockError> {
        let device_id = self.read_device_id()?;
        let identity_verified = device_id.starts_with(EXPECTED_DEVICE_ID);
        if !identity_verified {
            return Err(VirtioBlockError::Block(BlockError::NotReady));
        }

        let mut known_sector = [0_u8; SECTOR_SIZE];
        self.read_sector(KNOWN_READ_SECTOR, &mut known_sector)?;
        let known_sector_read_passed = known_sector.starts_with(EXPECTED_READ_PATTERN);
        if !known_sector_read_passed {
            return Err(VirtioBlockError::Block(BlockError::Io));
        }

        let mut original = [0_u8; SECTOR_SIZE];
        self.read_sector(DISPOSABLE_WRITE_SECTOR, &mut original)?;
        let mut probe = [0_u8; SECTOR_SIZE];
        for (index, byte) in probe.iter_mut().enumerate() {
            *byte = u8::try_from(index & 0xff).unwrap_or(0).wrapping_mul(37) ^ 0xa5;
        }
        probe[..EXPECTED_DEVICE_ID.len()].copy_from_slice(EXPECTED_DEVICE_ID);

        self.write_sector(DISPOSABLE_WRITE_SECTOR, &probe)?;
        let mut readback = [0_u8; SECTOR_SIZE];
        let readback_result = self.read_sector(DISPOSABLE_WRITE_SECTOR, &mut readback);
        let write_readback_passed = readback_result.is_ok() && readback == probe;

        let restore_result = self.write_sector(DISPOSABLE_WRITE_SECTOR, &original);
        let mut restored = [0_u8; SECTOR_SIZE];
        let restored_result = self.read_sector(DISPOSABLE_WRITE_SECTOR, &mut restored);
        let disposable_sector_restored =
            restore_result.is_ok() && restored_result.is_ok() && restored == original;
        if !write_readback_passed || !disposable_sector_restored {
            return Err(VirtioBlockError::Block(BlockError::Io));
        }

        let mut bounds_probe = [0_u8; SECTOR_SIZE];
        let bounds_check_passed = self.read_sector(self.geometry.sectors, &mut bounds_probe)
            == Err(BlockError::OutOfBounds);
        if !bounds_check_passed {
            return Err(VirtioBlockError::Block(BlockError::OutOfBounds));
        }

        Ok(VirtioBlockProbeReport {
            modern_pci_capabilities_active: true,
            pci_bars_parsed: transport.parsed_bars,
            pci_bus_master_active: true,
            feature_negotiation_active: true,
            dma_queue_active: true,
            queue_size: self.queue.queue_size,
            capacity_sectors: self.geometry.sectors,
            dedicated_device_identity_verified: identity_verified,
            known_sector_read_passed,
            disposable_sector_write_readback_passed: write_readback_passed,
            disposable_sector_restored,
            bounds_check_passed,
            timeout_protection_active: POLL_LIMIT > 0,
        })
    }
}

impl BlockDevice for VirtioBlockDevice {
    fn geometry(&self) -> BlockGeometry {
        self.geometry
    }

    fn read_sector(
        &mut self,
        sector: u64,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), BlockError> {
        validate_sector_range(self.geometry, sector, 1)?;
        self.submit(VIRTIO_BLK_T_IN, sector, destination, SECTOR_SIZE, true)
    }

    fn write_sector(&mut self, sector: u64, source: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        if self.geometry.read_only {
            return Err(BlockError::ReadOnly);
        }
        validate_sector_range(self.geometry, sector, 1)?;
        let mut request_buffer = *source;
        self.submit(
            VIRTIO_BLK_T_OUT,
            sector,
            &mut request_buffer,
            SECTOR_SIZE,
            false,
        )
    }
}

/// Initializes the first and only discovered virtio block function.
///
/// # Safety
///
/// The caller must own PCI configuration mechanism #1, the active physical
/// direct map, and the supplied frame allocator. No IOMMU translation may be
/// active between the device and guest physical memory.
unsafe fn initialize_virtio_block(
    inventory: &PciInventory,
    frame_allocator: &mut FrameAllocator<'_>,
) -> Result<(VirtioBlockDevice, VirtioPciTransport), VirtioBlockError> {
    // SAFETY: The caller owns PCI configuration access for this bootstrap CPU.
    let transport = unsafe { parse_transport(inventory)? };
    // SAFETY: The parsed function is the dedicated virtio block target.
    unsafe {
        enable_bus_master(transport.address)?;
    }
    let window = PciConfigWindow {
        address: transport.address,
        capability: transport.pci_config_capability,
    };

    // SAFETY: All common-register accesses use the validated PCI config window.
    unsafe {
        window.write_u8(transport.common, COMMON_DEVICE_STATUS, 0)?;
    }
    let mut reset_complete = false;
    for _ in 0..POLL_LIMIT {
        // SAFETY: The common status field is one aligned byte.
        if unsafe { window.read_u8(transport.common, COMMON_DEVICE_STATUS)? } == 0 {
            reset_complete = true;
            break;
        }
        spin_loop();
    }
    if !reset_complete {
        return Err(VirtioBlockError::ResetTimeout);
    }

    // SAFETY: Status bits are added in the order required by the specification.
    unsafe {
        window.write_u8(
            transport.common,
            COMMON_DEVICE_STATUS,
            DEVICE_STATUS_ACKNOWLEDGE | DEVICE_STATUS_DRIVER,
        )?;
        window.write_u32(transport.common, COMMON_DEVICE_FEATURE_SELECT, 0)?;
    }
    // SAFETY: Feature selection above chooses bits 0 through 31.
    let device_features_low = unsafe { window.read_u32(transport.common, COMMON_DEVICE_FEATURE)? };
    // SAFETY: Select and read feature bits 32 through 63.
    unsafe {
        window.write_u32(transport.common, COMMON_DEVICE_FEATURE_SELECT, 1)?;
    }
    // SAFETY: Feature selection above chooses bits 32 through 63.
    let device_features_high = unsafe { window.read_u32(transport.common, COMMON_DEVICE_FEATURE)? };
    if device_features_high & VIRTIO_F_VERSION_1_HIGH == 0
        || device_features_low & VIRTIO_BLK_F_RO != 0
    {
        // SAFETY: Mark this recognized device failed before giving up.
        let _ = unsafe {
            window.write_u8(
                transport.common,
                COMMON_DEVICE_STATUS,
                DEVICE_STATUS_ACKNOWLEDGE | DEVICE_STATUS_DRIVER | DEVICE_STATUS_FAILED,
            )
        };
        return Err(VirtioBlockError::UnsupportedFeatures);
    }

    // Negotiate only VIRTIO_F_VERSION_1. Split rings, polling, one queue, and
    // 512-byte sectors require no optional feature bits.
    // SAFETY: Driver feature selectors and values are aligned 32-bit fields.
    unsafe {
        window.write_u32(transport.common, COMMON_DRIVER_FEATURE_SELECT, 0)?;
        window.write_u32(transport.common, COMMON_DRIVER_FEATURE, 0)?;
        window.write_u32(transport.common, COMMON_DRIVER_FEATURE_SELECT, 1)?;
        window.write_u32(
            transport.common,
            COMMON_DRIVER_FEATURE,
            VIRTIO_F_VERSION_1_HIGH,
        )?;
        window.write_u8(
            transport.common,
            COMMON_DEVICE_STATUS,
            DEVICE_STATUS_ACKNOWLEDGE | DEVICE_STATUS_DRIVER | DEVICE_STATUS_FEATURES_OK,
        )?;
    }
    // SAFETY: Re-read status to verify the device accepted the feature subset.
    let features_status = unsafe { window.read_u8(transport.common, COMMON_DEVICE_STATUS)? };
    if features_status & DEVICE_STATUS_FEATURES_OK == 0 {
        return Err(VirtioBlockError::FeatureNegotiationRejected);
    }

    // SAFETY: The validated device configuration contains the mandatory
    // capacity field, and the common configuration provides its generation.
    let capacity = unsafe { read_capacity(window, transport)? };
    if capacity <= DISPOSABLE_WRITE_SECTOR {
        return Err(VirtioBlockError::InvalidCapacity);
    }

    // SAFETY: Queue zero is the virtio-blk request queue.
    unsafe {
        window.write_u16(transport.common, COMMON_QUEUE_SELECT, 0)?;
    }
    // SAFETY: Queue selection above chooses request queue zero.
    let maximum_queue_size = unsafe { window.read_u16(transport.common, COMMON_QUEUE_SIZE)? };
    if maximum_queue_size == 0 {
        return Err(VirtioBlockError::QueueUnavailable);
    }
    let queue_size = cmp::min(maximum_queue_size, DRIVER_QUEUE_SIZE);
    if queue_size < MIN_REQUEST_DESCRIPTORS || !queue_size.is_power_of_two() {
        return Err(VirtioBlockError::QueueTooSmall);
    }
    // SAFETY: Queue-enable is an aligned field in the validated common region.
    let queue_enable = unsafe { window.read_u16(transport.common, COMMON_QUEUE_ENABLE)? };
    if queue_enable != 0 {
        return Err(VirtioBlockError::QueueEnableRejected);
    }
    // SAFETY: Queue-notify offset is an aligned field in the validated common
    // region for the selected queue.
    let queue_notify_offset =
        unsafe { window.read_u16(transport.common, COMMON_QUEUE_NOTIFY_OFFSET)? };
    let notify_delta = u32::from(queue_notify_offset)
        .checked_mul(transport.notify_multiplier)
        .ok_or(VirtioBlockError::InvalidCapability)?;
    validate_region_access(transport.notify, notify_delta, 2)?;
    let notify = VirtioRegion {
        bar: transport.notify.bar,
        offset: transport
            .notify
            .offset
            .checked_add(notify_delta)
            .ok_or(VirtioBlockError::InvalidCapability)?,
        length: 2,
    };

    let dma_frame = frame_allocator
        .allocate_contiguous(1, 1)
        .ok_or(VirtioBlockError::DmaUnavailable)?
        .start;
    // SAFETY: The allocated frame is exclusively owned and direct-mapped.
    let queue = unsafe { DmaQueue::new(dma_frame, queue_size)? };

    // SAFETY: Queue fields are programmed before queue_enable as required.
    unsafe {
        window.write_u16(transport.common, COMMON_QUEUE_SIZE, queue_size)?;
        write_common_u64(
            window,
            transport.common,
            COMMON_QUEUE_DESC_LOW,
            COMMON_QUEUE_DESC_HIGH,
            queue.descriptor_physical(),
        )?;
        write_common_u64(
            window,
            transport.common,
            COMMON_QUEUE_DRIVER_LOW,
            COMMON_QUEUE_DRIVER_HIGH,
            queue.available_physical(),
        )?;
        write_common_u64(
            window,
            transport.common,
            COMMON_QUEUE_DEVICE_LOW,
            COMMON_QUEUE_DEVICE_HIGH,
            queue.used_physical(),
        )?;
        window.write_u16(transport.common, COMMON_QUEUE_ENABLE, 1)?;
    }
    // SAFETY: Read back the aligned queue-enable field just programmed.
    if unsafe { window.read_u16(transport.common, COMMON_QUEUE_ENABLE)? } != 1 {
        return Err(VirtioBlockError::QueueEnableRejected);
    }

    let live_status = DEVICE_STATUS_ACKNOWLEDGE
        | DEVICE_STATUS_DRIVER
        | DEVICE_STATUS_FEATURES_OK
        | DEVICE_STATUS_DRIVER_OK;
    // SAFETY: Queue configuration is complete before DRIVER_OK.
    unsafe {
        window.write_u8(transport.common, COMMON_DEVICE_STATUS, live_status)?;
    }
    // SAFETY: Read back the device-status byte just programmed.
    if unsafe { window.read_u8(transport.common, COMMON_DEVICE_STATUS)? } & live_status
        != live_status
    {
        return Err(VirtioBlockError::DriverRejected);
    }

    Ok((
        VirtioBlockDevice {
            window,
            common: transport.common,
            notify,
            geometry: BlockGeometry::new(capacity, false),
            queue,
        },
        transport,
    ))
}

/// Initializes the dedicated block target and executes the complete M6B gate.
///
/// # Safety
///
/// The caller must satisfy [`initialize_virtio_block`]'s PCI, direct-map,
/// allocator, and no-IOMMU ownership requirements.
pub unsafe fn initialize_and_probe(
    inventory: &PciInventory,
    frame_allocator: &mut FrameAllocator<'_>,
) -> Result<(VirtioBlockDevice, VirtioBlockProbeReport), VirtioBlockError> {
    // SAFETY: The caller forwards the complete hardware ownership contract.
    let (mut device, transport) = unsafe { initialize_virtio_block(inventory, frame_allocator)? };
    let report = device.run_acceptance_probe(transport)?;
    Ok((device, report))
}

unsafe fn parse_transport(
    inventory: &PciInventory,
) -> Result<VirtioPciTransport, VirtioBlockError> {
    let mut targets = inventory
        .devices()
        .iter()
        .filter(|device| device.storage_kind() == Some(StorageControllerKind::VirtioBlock));
    let target = *targets.next().ok_or(VirtioBlockError::TargetUnavailable)?;
    if targets.next().is_some() {
        return Err(VirtioBlockError::AmbiguousTarget);
    }

    // SAFETY: The caller owns PCI configuration mechanism #1.
    let session = unsafe { PciConfigSession::acquire() };
    // SAFETY: Standard type-zero configuration header access.
    let status = unsafe { session.read_u16(target.address, PCI_STATUS_OFFSET) };
    if status & PCI_STATUS_CAPABILITY_LIST == 0 {
        return Err(VirtioBlockError::CapabilityListUnavailable);
    }

    // Parse every assigned BAR once without destructive sizing writes.
    let mut bars: [Option<PciBar>; 6] = [None; 6];
    let mut bar_index = 0_u8;
    let mut parsed_bars = 0_usize;
    while bar_index < 6 {
        let offset = PCI_BAR_ZERO_OFFSET + bar_index * 4;
        // SAFETY: BAR offsets are aligned standard-header dwords.
        let low = unsafe { session.read_u32(target.address, offset) };
        if low == 0 {
            bar_index += 1;
            continue;
        }
        let is_memory_64 = low & 1 == 0 && ((low >> 1) & 0x03) == 2;
        let upper = if is_memory_64 && bar_index < 5 {
            // SAFETY: The upper half immediately follows a 64-bit BAR.
            Some(unsafe { session.read_u32(target.address, offset + 4) })
        } else {
            None
        };
        let (bar, consumes_upper) =
            decode_bar(bar_index, low, upper).map_err(|_| VirtioBlockError::InvalidBar)?;
        bars[usize::from(bar_index)] = Some(bar);
        parsed_bars = parsed_bars.saturating_add(1);
        bar_index += if consumes_upper { 2 } else { 1 };
    }

    // SAFETY: Capability pointer is a standard one-byte header field.
    let mut capability =
        unsafe { session.read_u8(target.address, PCI_CAPABILITY_POINTER_OFFSET) } & 0xfc;
    let mut visited = [false; 256];
    let mut common = None;
    let mut notify = None;
    let mut device = None;
    let mut pci_config_capability = None;
    let mut notify_multiplier = 0_u32;
    let mut capability_count = 0_usize;

    while capability != 0 {
        if !(0x40..=0xec).contains(&capability)
            || !capability.is_multiple_of(4)
            || visited[usize::from(capability)]
            || capability_count == MAX_CAPABILITIES
        {
            return Err(VirtioBlockError::MalformedCapabilityList);
        }
        visited[usize::from(capability)] = true;
        capability_count += 1;
        // SAFETY: Capability traversal bounds this header within 256 bytes.
        let capability_id = unsafe { session.read_u8(target.address, capability) };
        // SAFETY: The next pointer immediately follows the capability ID.
        let next = unsafe { session.read_u8(target.address, capability + 1) } & 0xfc;

        if capability_id == PCI_CAPABILITY_VENDOR_SPECIFIC {
            // SAFETY: Vendor capability prefix bytes are inside the bounded
            // standard configuration-space capability.
            let capability_length = unsafe { session.read_u8(target.address, capability + 2) };
            // SAFETY: Configuration type immediately follows the bounded
            // capability length byte.
            let configuration_type = unsafe { session.read_u8(target.address, capability + 3) };
            if capability_length < 16 {
                return Err(VirtioBlockError::InvalidCapability);
            }
            if configuration_type == VIRTIO_PCI_CAP_PCI_CFG {
                if capability_length < 20 || pci_config_capability.is_some() {
                    return Err(VirtioBlockError::InvalidCapability);
                }
                pci_config_capability = Some(capability);
            } else if matches!(
                configuration_type,
                VIRTIO_PCI_CAP_COMMON_CFG | VIRTIO_PCI_CAP_NOTIFY_CFG | VIRTIO_PCI_CAP_DEVICE_CFG
            ) {
                // SAFETY: BAR index is within the validated 16-byte prefix.
                let bar = unsafe { session.read_u8(target.address, capability + 4) };
                if bar >= 6 || bars[usize::from(bar)].is_none() {
                    return Err(VirtioBlockError::InvalidBar);
                }
                // SAFETY: Region offset is the aligned field at bytes 8..12.
                let offset = unsafe { session.read_u32(target.address, capability + 8) };
                // SAFETY: Region length is the aligned field at bytes 12..16.
                let length = unsafe { session.read_u32(target.address, capability + 12) };
                if length == 0 || offset.checked_add(length).is_none() {
                    return Err(VirtioBlockError::InvalidCapability);
                }
                let region = VirtioRegion {
                    bar,
                    offset,
                    length,
                };
                match configuration_type {
                    VIRTIO_PCI_CAP_COMMON_CFG => {
                        if common.replace(region).is_some() {
                            return Err(VirtioBlockError::InvalidCapability);
                        }
                    }
                    VIRTIO_PCI_CAP_NOTIFY_CFG => {
                        if notify.is_some() {
                            return Err(VirtioBlockError::InvalidCapability);
                        }
                        if capability_length < 20 {
                            return Err(VirtioBlockError::InvalidCapability);
                        }
                        // SAFETY: A 20-byte notify capability contains the
                        // aligned multiplier at bytes 16..20.
                        notify_multiplier =
                            unsafe { session.read_u32(target.address, capability + 16) };
                        notify = Some(region);
                    }
                    VIRTIO_PCI_CAP_DEVICE_CFG if device.replace(region).is_some() => {
                        return Err(VirtioBlockError::InvalidCapability);
                    }
                    _ => {}
                }
            }
        }
        capability = next;
    }

    let common = common.ok_or(VirtioBlockError::MissingCommonConfiguration)?;
    let notify = notify.ok_or(VirtioBlockError::MissingNotificationConfiguration)?;
    let device = device.ok_or(VirtioBlockError::MissingDeviceConfiguration)?;
    if common.length < MIN_COMMON_CONFIG_BYTES || notify.length < 2 || device.length < 8 {
        return Err(VirtioBlockError::InvalidCapability);
    }
    Ok(VirtioPciTransport {
        address: target.address,
        common,
        notify,
        device,
        pci_config_capability: pci_config_capability
            .ok_or(VirtioBlockError::MissingPciConfigurationWindow)?,
        notify_multiplier,
        parsed_bars,
    })
}

unsafe fn enable_bus_master(address: PciAddress) -> Result<(), VirtioBlockError> {
    // SAFETY: The caller owns PCI configuration mechanism #1.
    let session = unsafe { PciConfigSession::acquire() };
    // SAFETY: PCI command is an aligned standard-header word.
    let command = unsafe { session.read_u16(address, PCI_COMMAND_OFFSET) };
    let required = PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER;
    // SAFETY: Preserve firmware-selected command bits and enable only memory
    // decoding plus bus mastering.
    unsafe {
        session.write_u16(address, PCI_COMMAND_OFFSET, command | required);
    }
    // SAFETY: Read back the same command word to prove acceptance.
    if unsafe { session.read_u16(address, PCI_COMMAND_OFFSET) } & required != required {
        return Err(VirtioBlockError::PciCommandRejected);
    }
    Ok(())
}

unsafe fn read_capacity(
    window: PciConfigWindow,
    transport: VirtioPciTransport,
) -> Result<u64, VirtioBlockError> {
    for _ in 0..256 {
        // SAFETY: Generation is a one-byte common configuration field.
        let before = unsafe { window.read_u8(transport.common, COMMON_CONFIG_GENERATION)? };
        // SAFETY: Capacity is always present as two aligned 32-bit halves.
        let low = unsafe { window.read_u32(transport.device, 0)? };
        // SAFETY: This is the aligned upper half of the mandatory capacity.
        let high = unsafe { window.read_u32(transport.device, 4)? };
        // SAFETY: Re-read generation to verify an atomic configuration view.
        let after = unsafe { window.read_u8(transport.common, COMMON_CONFIG_GENERATION)? };
        if before == after {
            return Ok((u64::from(high) << 32) | u64::from(low));
        }
    }
    Err(VirtioBlockError::InvalidCapacity)
}

unsafe fn write_common_u64(
    window: PciConfigWindow,
    common: VirtioRegion,
    low_offset: u32,
    high_offset: u32,
    value: u64,
) -> Result<(), VirtioBlockError> {
    // SAFETY: Virtio PCI permits independent aligned access to both 32-bit
    // halves of a 64-bit common-configuration field.
    unsafe {
        window.write_u32(common, low_offset, value as u32)?;
        window.write_u32(common, high_offset, (value >> 32) as u32)?;
    }
    Ok(())
}

fn validate_region_access(
    region: VirtioRegion,
    register_offset: u32,
    width: u32,
) -> Result<u32, VirtioBlockError> {
    if !matches!(width, 1 | 2 | 4) || !register_offset.is_multiple_of(width) {
        return Err(VirtioBlockError::InvalidCapability);
    }
    let end = register_offset
        .checked_add(width)
        .ok_or(VirtioBlockError::InvalidCapability)?;
    if end > region.length {
        return Err(VirtioBlockError::InvalidCapability);
    }
    let absolute = region
        .offset
        .checked_add(register_offset)
        .ok_or(VirtioBlockError::InvalidCapability)?;
    if !absolute.is_multiple_of(width) {
        return Err(VirtioBlockError::InvalidCapability);
    }
    Ok(absolute)
}

unsafe fn write_u16(base: *mut u8, offset: usize, value: u16) {
    // SAFETY: Callers provide an aligned, in-frame offset.
    unsafe {
        base.add(offset).cast::<u16>().write_volatile(value);
    }
}

unsafe fn write_u32(base: *mut u8, offset: usize, value: u32) {
    // SAFETY: Callers provide an aligned, in-frame offset.
    unsafe {
        base.add(offset).cast::<u32>().write_volatile(value);
    }
}

unsafe fn write_u64(base: *mut u8, offset: usize, value: u64) {
    // SAFETY: Callers provide an aligned, in-frame offset.
    unsafe {
        base.add(offset).cast::<u64>().write_volatile(value);
    }
}

unsafe fn read_u16(base: *mut u8, offset: usize) -> u16 {
    // SAFETY: Callers provide an aligned, in-frame offset.
    unsafe { base.add(offset).cast::<u16>().read_volatile() }
}

unsafe fn read_u32(base: *mut u8, offset: usize) -> u32 {
    // SAFETY: Callers provide an aligned, in-frame offset.
    unsafe { base.add(offset).cast::<u32>().read_volatile() }
}
