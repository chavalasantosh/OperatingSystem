//! Architecture-independent block-device contracts.

pub const SECTOR_SIZE: usize = 512;
pub const SECTOR_SIZE_U64: u64 = SECTOR_SIZE as u64;

/// Immutable geometry reported by one block device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockGeometry {
    pub sectors: u64,
    pub sector_size: u32,
    pub read_only: bool,
}

impl BlockGeometry {
    #[must_use]
    pub const fn new(sectors: u64, read_only: bool) -> Self {
        Self {
            sectors,
            sector_size: SECTOR_SIZE as u32,
            read_only,
        }
    }

    #[must_use]
    pub const fn byte_capacity(self) -> Option<u64> {
        self.sectors.checked_mul(SECTOR_SIZE_U64)
    }

    #[must_use]
    pub const fn contains_sector(self, sector: u64) -> bool {
        sector < self.sectors
    }
}

/// Stable errors returned through the architecture-independent block API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockError {
    NotReady,
    InvalidBuffer,
    OutOfBounds,
    ReadOnly,
    UnsupportedFeatures,
    UnsupportedRequest,
    Timeout,
    DeviceNeedsReset,
    Transport,
    Io,
}

/// One sector-addressed block device.
///
/// M6B intentionally allows one synchronous request at a time. Asynchronous
/// completion and scatter/gather requests remain later transport extensions.
pub trait BlockDevice {
    fn geometry(&self) -> BlockGeometry;

    /// Reads exactly one 512-byte sector.
    ///
    /// # Errors
    ///
    /// Returns [`BlockError::OutOfBounds`] when `sector` is outside the
    /// reported geometry, plus transport-specific errors for failed requests.
    fn read_sector(
        &mut self,
        sector: u64,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), BlockError>;

    /// Writes exactly one 512-byte sector.
    ///
    /// # Errors
    ///
    /// Returns [`BlockError::ReadOnly`] for read-only devices,
    /// [`BlockError::OutOfBounds`] for an invalid sector, plus
    /// transport-specific errors for failed requests.
    fn write_sector(&mut self, sector: u64, source: &[u8; SECTOR_SIZE]) -> Result<(), BlockError>;
}

/// Validates a sector range without wrapping its end.
///
/// # Errors
///
/// Returns [`BlockError::InvalidBuffer`] for a zero-sector request and
/// [`BlockError::OutOfBounds`] if the range exceeds the device.
pub const fn validate_sector_range(
    geometry: BlockGeometry,
    first_sector: u64,
    sector_count: u64,
) -> Result<(), BlockError> {
    if sector_count == 0 {
        return Err(BlockError::InvalidBuffer);
    }
    let Some(end) = first_sector.checked_add(sector_count) else {
        return Err(BlockError::OutOfBounds);
    };
    if end > geometry.sectors {
        return Err(BlockError::OutOfBounds);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BlockDevice, BlockError, BlockGeometry, SECTOR_SIZE, validate_sector_range};

    struct MemoryBlockDevice {
        geometry: BlockGeometry,
        sector: [u8; SECTOR_SIZE],
    }

    impl BlockDevice for MemoryBlockDevice {
        fn geometry(&self) -> BlockGeometry {
            self.geometry
        }

        fn read_sector(
            &mut self,
            sector: u64,
            destination: &mut [u8; SECTOR_SIZE],
        ) -> Result<(), BlockError> {
            validate_sector_range(self.geometry, sector, 1)?;
            destination.copy_from_slice(&self.sector);
            Ok(())
        }

        fn write_sector(
            &mut self,
            sector: u64,
            source: &[u8; SECTOR_SIZE],
        ) -> Result<(), BlockError> {
            if self.geometry.read_only {
                return Err(BlockError::ReadOnly);
            }
            validate_sector_range(self.geometry, sector, 1)?;
            self.sector.copy_from_slice(source);
            Ok(())
        }
    }

    #[test]
    fn geometry_capacity_is_sector_based_and_checked() {
        let geometry = BlockGeometry::new(16_384, false);
        assert_eq!(geometry.byte_capacity(), Some(8 * 1024 * 1024));
        assert_eq!(BlockGeometry::new(u64::MAX, false).byte_capacity(), None);
    }

    #[test]
    fn range_validation_rejects_zero_overflow_and_end_boundary() {
        let geometry = BlockGeometry::new(8, false);
        assert_eq!(
            validate_sector_range(geometry, 0, 0),
            Err(BlockError::InvalidBuffer)
        );
        assert_eq!(validate_sector_range(geometry, 7, 1), Ok(()));
        assert_eq!(
            validate_sector_range(geometry, 8, 1),
            Err(BlockError::OutOfBounds)
        );
        assert_eq!(
            validate_sector_range(geometry, u64::MAX, 2),
            Err(BlockError::OutOfBounds)
        );
    }

    #[test]
    fn block_contract_round_trips_one_sector_and_enforces_bounds() {
        let mut device = MemoryBlockDevice {
            geometry: BlockGeometry::new(1, false),
            sector: [0; SECTOR_SIZE],
        };
        let source = [0x5a; SECTOR_SIZE];
        let mut destination = [0; SECTOR_SIZE];
        device.write_sector(0, &source).unwrap();
        device.read_sector(0, &mut destination).unwrap();
        assert_eq!(destination, source);
        assert_eq!(
            device.read_sector(1, &mut destination),
            Err(BlockError::OutOfBounds)
        );
    }

    #[test]
    fn block_contract_rejects_writes_to_read_only_media() {
        let mut device = MemoryBlockDevice {
            geometry: BlockGeometry::new(1, true),
            sector: [0; SECTOR_SIZE],
        };
        assert_eq!(
            device.write_sector(0, &[0x5a; SECTOR_SIZE]),
            Err(BlockError::ReadOnly)
        );
    }
}
