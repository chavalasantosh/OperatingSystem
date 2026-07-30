#![allow(clippy::module_name_repetitions)]

//! Fixed-capacity, allocation-free block cache.
//!
//! M6C deliberately uses a read-through cache with a hard read-only policy.
//! Persistent dirty data, flushing, and writeback are not enabled until a
//! later recovery and power-loss safety gate.

use crate::block::{BlockDevice, BlockError, BlockGeometry, SECTOR_SIZE, validate_sector_range};

/// Number of sectors retained by the M6C cache.
pub const DEFAULT_CACHE_ENTRIES: usize = 16;

/// Persistent-write policy enforced by the cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirtyStatePolicy {
    /// Reject every write before it can reach the device or create dirty data.
    RejectWrites,
}

/// Failures returned through the cache boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheError {
    ZeroCapacity,
    ReadOnlyPolicy,
    NoEvictableEntry,
    Block(BlockError),
}

impl From<BlockError> for CacheError {
    fn from(value: BlockError) -> Self {
        Self::Block(value)
    }
}

/// Observable cache counters used by diagnostics and acceptance tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub device_reads: u64,
    pub rejected_writes: u64,
    pub dirty_entries: usize,
}

#[derive(Clone, Copy)]
struct CacheEntry {
    valid: bool,
    dirty: bool,
    sector: u64,
    last_used: u64,
    data: [u8; SECTOR_SIZE],
}

impl CacheEntry {
    const fn empty() -> Self {
        Self {
            valid: false,
            dirty: false,
            sector: 0,
            last_used: 0,
            data: [0; SECTOR_SIZE],
        }
    }
}

/// One fixed-capacity read-through cache over a sector-addressed device.
pub struct BlockCache<D, const ENTRIES: usize> {
    device: D,
    entries: [CacheEntry; ENTRIES],
    policy: DirtyStatePolicy,
    clock: u64,
    stats: CacheStats,
}

impl<D: BlockDevice, const ENTRIES: usize> BlockCache<D, ENTRIES> {
    /// Creates an empty cache.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::ZeroCapacity`] when `ENTRIES` is zero.
    pub fn new(device: D, policy: DirtyStatePolicy) -> Result<Self, CacheError> {
        if ENTRIES == 0 {
            return Err(CacheError::ZeroCapacity);
        }
        Ok(Self {
            device,
            entries: [CacheEntry::empty(); ENTRIES],
            policy,
            clock: 0,
            stats: CacheStats::default(),
        })
    }

    /// Returns the geometry of the underlying device.
    #[must_use]
    pub fn geometry(&self) -> BlockGeometry {
        self.device.geometry()
    }

    /// Returns the compile-time entry capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        ENTRIES
    }

    /// Returns the active dirty-state policy.
    #[must_use]
    pub const fn policy(&self) -> DirtyStatePolicy {
        self.policy
    }

    /// Reads exactly one sector, serving a cached copy when present.
    ///
    /// A failed device read leaves the existing cache contents unchanged.
    ///
    /// # Errors
    ///
    /// Returns a block error for an invalid or failed device read, or
    /// [`CacheError::NoEvictableEntry`] if every entry were dirty.
    pub fn read_sector(
        &mut self,
        sector: u64,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), CacheError> {
        validate_sector_range(self.device.geometry(), sector, 1)?;
        let timestamp = self.next_timestamp();

        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.valid && entry.sector == sector)
        {
            let entry = &mut self.entries[index];
            entry.last_used = timestamp;
            destination.copy_from_slice(&entry.data);
            self.stats.hits = self.stats.hits.saturating_add(1);
            return Ok(());
        }

        self.stats.misses = self.stats.misses.saturating_add(1);
        let replacement = self.replacement_index()?;
        let mut fetched = [0_u8; SECTOR_SIZE];
        self.device.read_sector(sector, &mut fetched)?;
        self.stats.device_reads = self.stats.device_reads.saturating_add(1);

        if self.entries[replacement].valid {
            self.stats.evictions = self.stats.evictions.saturating_add(1);
        }
        self.entries[replacement] = CacheEntry {
            valid: true,
            dirty: false,
            sector,
            last_used: timestamp,
            data: fetched,
        };
        destination.copy_from_slice(&fetched);
        Ok(())
    }

    /// Rejects a sector write under the M6C read-only policy.
    ///
    /// The request is range-checked first, but is never forwarded to the
    /// device and never creates a dirty cache entry.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::ReadOnlyPolicy`] for an in-range request.
    pub fn write_sector(
        &mut self,
        sector: u64,
        _source: &[u8; SECTOR_SIZE],
    ) -> Result<(), CacheError> {
        validate_sector_range(self.device.geometry(), sector, 1)?;
        self.stats.rejected_writes = self.stats.rejected_writes.saturating_add(1);
        Err(CacheError::ReadOnlyPolicy)
    }

    /// Invalidates all clean entries.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::NoEvictableEntry`] if a future policy leaves any
    /// dirty entry behind. M6C cannot normally reach that state.
    pub fn invalidate_all(&mut self) -> Result<(), CacheError> {
        if self.dirty_entries() != 0 {
            return Err(CacheError::NoEvictableEntry);
        }
        for entry in &mut self.entries {
            *entry = CacheEntry::empty();
        }
        Ok(())
    }

    /// Returns current diagnostic counters.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            dirty_entries: self.dirty_entries(),
            ..self.stats
        }
    }

    /// Returns ownership of the wrapped block device.
    #[must_use]
    pub fn into_inner(self) -> D {
        self.device
    }

    fn next_timestamp(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn replacement_index(&self) -> Result<usize, CacheError> {
        if let Some(index) = self.entries.iter().position(|entry| !entry.valid) {
            return Ok(index);
        }

        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.dirty)
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(index, _)| index)
            .ok_or(CacheError::NoEvictableEntry)
    }

    fn dirty_entries(&self) -> usize {
        self.entries.iter().filter(|entry| entry.dirty).count()
    }
}

impl<D: BlockDevice, const ENTRIES: usize> BlockDevice for BlockCache<D, ENTRIES> {
    fn geometry(&self) -> BlockGeometry {
        BlockGeometry {
            read_only: self.policy == DirtyStatePolicy::RejectWrites,
            ..self.device.geometry()
        }
    }

    fn read_sector(
        &mut self,
        sector: u64,
        destination: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), BlockError> {
        BlockCache::read_sector(self, sector, destination).map_err(cache_block_error)
    }

    fn write_sector(&mut self, sector: u64, source: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        BlockCache::write_sector(self, sector, source).map_err(cache_block_error)
    }
}

const fn cache_block_error(error: CacheError) -> BlockError {
    match error {
        CacheError::Block(error) => error,
        CacheError::ReadOnlyPolicy => BlockError::ReadOnly,
        CacheError::ZeroCapacity | CacheError::NoEvictableEntry => BlockError::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockCache, CacheError, DirtyStatePolicy};
    use crate::block::{BlockDevice, BlockError, BlockGeometry, SECTOR_SIZE};

    struct CountingDevice {
        sectors: [[u8; SECTOR_SIZE]; 4],
        reads: u64,
        writes: u64,
        failing_read: Option<u64>,
    }

    impl CountingDevice {
        fn new() -> Self {
            let mut sectors = [[0_u8; SECTOR_SIZE]; 4];
            for (sector, data) in sectors.iter_mut().enumerate() {
                data.fill(u8::try_from(sector).unwrap());
            }
            Self {
                sectors,
                reads: 0,
                writes: 0,
                failing_read: None,
            }
        }

        fn with_failing_read(sector: u64) -> Self {
            Self {
                failing_read: Some(sector),
                ..Self::new()
            }
        }
    }

    impl BlockDevice for CountingDevice {
        fn geometry(&self) -> BlockGeometry {
            BlockGeometry::new(self.sectors.len() as u64, false)
        }

        fn read_sector(
            &mut self,
            sector: u64,
            destination: &mut [u8; SECTOR_SIZE],
        ) -> Result<(), BlockError> {
            self.reads += 1;
            if self.failing_read == Some(sector) {
                return Err(BlockError::Io);
            }
            let index = usize::try_from(sector).map_err(|_| BlockError::OutOfBounds)?;
            let source = self.sectors.get(index).ok_or(BlockError::OutOfBounds)?;
            destination.copy_from_slice(source);
            Ok(())
        }

        fn write_sector(
            &mut self,
            sector: u64,
            source: &[u8; SECTOR_SIZE],
        ) -> Result<(), BlockError> {
            let index = usize::try_from(sector).map_err(|_| BlockError::OutOfBounds)?;
            let destination = self.sectors.get_mut(index).ok_or(BlockError::OutOfBounds)?;
            destination.copy_from_slice(source);
            self.writes += 1;
            Ok(())
        }
    }

    #[test]
    fn repeat_read_hits_cache_without_second_device_request() {
        let device = CountingDevice::new();
        let mut cache = BlockCache::<_, 2>::new(device, DirtyStatePolicy::RejectWrites).unwrap();
        let mut first = [0_u8; SECTOR_SIZE];
        let mut second = [0_u8; SECTOR_SIZE];

        cache.read_sector(2, &mut first).unwrap();
        cache.read_sector(2, &mut second).unwrap();

        assert_eq!(first, second);
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().device_reads, 1);
        assert_eq!(cache.into_inner().reads, 1);
    }

    #[test]
    fn least_recently_used_clean_entry_is_evicted() {
        let device = CountingDevice::new();
        let mut cache = BlockCache::<_, 2>::new(device, DirtyStatePolicy::RejectWrites).unwrap();
        let mut buffer = [0_u8; SECTOR_SIZE];

        cache.read_sector(0, &mut buffer).unwrap();
        cache.read_sector(1, &mut buffer).unwrap();
        cache.read_sector(0, &mut buffer).unwrap();
        cache.read_sector(2, &mut buffer).unwrap();
        cache.read_sector(1, &mut buffer).unwrap();

        assert_eq!(cache.stats().evictions, 2);
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 4);
    }

    #[test]
    fn read_only_policy_rejects_write_before_device() {
        let device = CountingDevice::new();
        let mut cache = BlockCache::<_, 2>::new(device, DirtyStatePolicy::RejectWrites).unwrap();

        assert_eq!(
            cache.write_sector(0, &[0xa5; SECTOR_SIZE]),
            Err(CacheError::ReadOnlyPolicy)
        );
        assert_eq!(cache.stats().rejected_writes, 1);
        assert_eq!(cache.stats().dirty_entries, 0);
        assert_eq!(cache.into_inner().writes, 0);
    }

    #[test]
    fn failed_device_read_preserves_existing_cached_entry() {
        let device = CountingDevice::with_failing_read(1);
        let mut cache = BlockCache::<_, 1>::new(device, DirtyStatePolicy::RejectWrites).unwrap();
        let mut original = [0_u8; SECTOR_SIZE];
        let mut destination = [0_u8; SECTOR_SIZE];

        cache.read_sector(0, &mut original).unwrap();
        assert_eq!(
            cache.read_sector(1, &mut destination),
            Err(CacheError::Block(BlockError::Io))
        );
        cache.read_sector(0, &mut destination).unwrap();

        assert_eq!(destination, original);
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 2);
        assert_eq!(cache.into_inner().reads, 2);
    }

    #[test]
    fn zero_capacity_and_out_of_bounds_are_rejected() {
        assert!(matches!(
            BlockCache::<_, 0>::new(CountingDevice::new(), DirtyStatePolicy::RejectWrites),
            Err(CacheError::ZeroCapacity)
        ));

        let mut cache =
            BlockCache::<_, 1>::new(CountingDevice::new(), DirtyStatePolicy::RejectWrites).unwrap();
        assert_eq!(
            cache.read_sector(4, &mut [0_u8; SECTOR_SIZE]),
            Err(CacheError::Block(BlockError::OutOfBounds))
        );
    }
}
