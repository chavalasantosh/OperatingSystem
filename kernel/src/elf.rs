#![allow(clippy::module_name_repetitions)]

//! Allocation-free ELF64 loader for position-independent user programs.

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElfError {
    Truncated,
    BadMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedType,
    UnsupportedMachine,
    InvalidHeader,
    InvalidSegment,
    ImageTooLarge,
    WriteExecuteSegment,
    EntryOutsideImage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadedImage {
    pub entry_offset: usize,
    pub image_size: usize,
    pub load_segments: usize,
    pub executable_segments: usize,
    pub writable_segments: usize,
    pub write_xor_execute_enforced: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProgramHeader {
    kind: u32,
    flags: u32,
    offset: u64,
    virtual_address: u64,
    file_size: u64,
    memory_size: u64,
}

/// Loads an x86-64 position-independent ELF image into a caller-owned buffer.
///
/// # Errors
///
/// Rejects malformed images, non-x86-64 binaries, writable+executable load
/// segments, and images that do not fit in `destination`.
pub fn load_position_independent(
    elf: &[u8],
    destination: &mut [u8],
) -> Result<LoadedImage, ElfError> {
    let header = elf.get(..ELF_HEADER_SIZE).ok_or(ElfError::Truncated)?;
    if &header[..4] != b"\x7fELF" {
        return Err(ElfError::BadMagic);
    }
    if header[4] != 2 {
        return Err(ElfError::UnsupportedClass);
    }
    if header[5] != 1 {
        return Err(ElfError::UnsupportedEndian);
    }
    if read_u16(header, 16)? != ET_DYN {
        return Err(ElfError::UnsupportedType);
    }
    if read_u16(header, 18)? != EM_X86_64 {
        return Err(ElfError::UnsupportedMachine);
    }

    let entry = read_u64(header, 24)?;
    let program_header_offset =
        usize::try_from(read_u64(header, 32)?).map_err(|_| ElfError::InvalidHeader)?;
    let program_header_size = usize::from(read_u16(header, 54)?);
    let program_header_count = usize::from(read_u16(header, 56)?);
    if program_header_size < PROGRAM_HEADER_SIZE || program_header_count == 0 {
        return Err(ElfError::InvalidHeader);
    }

    let mut minimum_address = u64::MAX;
    let mut maximum_address = 0_u64;
    let mut load_segments = 0_usize;
    let mut executable_segments = 0_usize;
    let mut writable_segments = 0_usize;

    for index in 0..program_header_count {
        let segment = read_program_header(elf, program_header_offset, program_header_size, index)?;
        if segment.kind != PT_LOAD {
            continue;
        }
        validate_segment(segment, elf.len())?;
        if segment.flags & PF_W != 0 && segment.flags & PF_X != 0 {
            return Err(ElfError::WriteExecuteSegment);
        }
        minimum_address = minimum_address.min(segment.virtual_address);
        maximum_address = maximum_address.max(
            segment
                .virtual_address
                .checked_add(segment.memory_size)
                .ok_or(ElfError::InvalidSegment)?,
        );
        load_segments += 1;
        executable_segments += if segment.flags & PF_X != 0 { 1 } else { 0 };
        writable_segments += if segment.flags & PF_W != 0 { 1 } else { 0 };
    }

    if load_segments == 0 || minimum_address == u64::MAX || maximum_address <= minimum_address {
        return Err(ElfError::InvalidHeader);
    }
    let image_size_u64 = maximum_address
        .checked_sub(minimum_address)
        .ok_or(ElfError::InvalidHeader)?;
    let image_size = usize::try_from(image_size_u64).map_err(|_| ElfError::ImageTooLarge)?;
    if image_size > destination.len() {
        return Err(ElfError::ImageTooLarge);
    }

    destination[..image_size].fill(0);
    for index in 0..program_header_count {
        let segment = read_program_header(elf, program_header_offset, program_header_size, index)?;
        if segment.kind != PT_LOAD {
            continue;
        }
        let destination_offset = usize::try_from(
            segment
                .virtual_address
                .checked_sub(minimum_address)
                .ok_or(ElfError::InvalidSegment)?,
        )
        .map_err(|_| ElfError::ImageTooLarge)?;
        let file_size = usize::try_from(segment.file_size).map_err(|_| ElfError::ImageTooLarge)?;
        let source_offset =
            usize::try_from(segment.offset).map_err(|_| ElfError::InvalidSegment)?;
        let source_end = source_offset
            .checked_add(file_size)
            .ok_or(ElfError::InvalidSegment)?;
        let destination_end = destination_offset
            .checked_add(file_size)
            .ok_or(ElfError::ImageTooLarge)?;
        destination
            .get_mut(destination_offset..destination_end)
            .ok_or(ElfError::ImageTooLarge)?
            .copy_from_slice(
                elf.get(source_offset..source_end)
                    .ok_or(ElfError::InvalidSegment)?,
            );
    }

    let entry_offset_u64 = entry
        .checked_sub(minimum_address)
        .ok_or(ElfError::EntryOutsideImage)?;
    let entry_offset =
        usize::try_from(entry_offset_u64).map_err(|_| ElfError::EntryOutsideImage)?;
    if entry_offset >= image_size {
        return Err(ElfError::EntryOutsideImage);
    }

    Ok(LoadedImage {
        entry_offset,
        image_size,
        load_segments,
        executable_segments,
        writable_segments,
        write_xor_execute_enforced: true,
    })
}

fn validate_segment(segment: ProgramHeader, elf_size: usize) -> Result<(), ElfError> {
    if segment.file_size > segment.memory_size {
        return Err(ElfError::InvalidSegment);
    }
    let source_end = segment
        .offset
        .checked_add(segment.file_size)
        .ok_or(ElfError::InvalidSegment)?;
    if source_end > u64::try_from(elf_size).unwrap_or(u64::MAX) {
        return Err(ElfError::InvalidSegment);
    }
    Ok(())
}

fn read_program_header(
    elf: &[u8],
    table_offset: usize,
    stride: usize,
    index: usize,
) -> Result<ProgramHeader, ElfError> {
    let offset = index
        .checked_mul(stride)
        .and_then(|value| table_offset.checked_add(value))
        .ok_or(ElfError::InvalidHeader)?;
    let header = elf
        .get(
            offset
                ..offset
                    .checked_add(PROGRAM_HEADER_SIZE)
                    .ok_or(ElfError::InvalidHeader)?,
        )
        .ok_or(ElfError::Truncated)?;
    Ok(ProgramHeader {
        kind: read_u32(header, 0)?,
        flags: read_u32(header, 4)?,
        offset: read_u64(header, 8)?,
        virtual_address: read_u64(header, 16)?,
        file_size: read_u64(header, 32)?,
        memory_size: read_u64(header, 40)?,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ElfError> {
    let value = bytes
        .get(offset..offset.checked_add(2).ok_or(ElfError::Truncated)?)
        .ok_or(ElfError::Truncated)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ElfError> {
    let value = bytes
        .get(offset..offset.checked_add(4).ok_or(ElfError::Truncated)?)
        .ok_or(ElfError::Truncated)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ElfError> {
    let value = bytes
        .get(offset..offset.checked_add(8).ok_or(ElfError::Truncated)?)
        .ok_or(ElfError::Truncated)?;
    Ok(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::load_position_independent;

    const HELLO: &[u8] = include_bytes!("../../user/programs/bin/hello.elf");

    #[test]
    fn loader_maps_position_independent_elf() {
        let mut image = [0_u8; 16 * 1024];
        let loaded = load_position_independent(HELLO, &mut image).unwrap();
        assert!(loaded.entry_offset < loaded.image_size);
        assert!(loaded.load_segments >= 2);
        assert!(loaded.executable_segments >= 1);
        assert!(loaded.write_xor_execute_enforced);
    }
}
