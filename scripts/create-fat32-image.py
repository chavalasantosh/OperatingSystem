#!/usr/bin/env python3
"""Create the deterministic read-only FAT32 image used by M6D smoke tests."""

from __future__ import annotations

import argparse
from pathlib import Path

SECTOR_SIZE = 512
TOTAL_SECTORS = 131_072
RESERVED_SECTORS = 32
FAT_COUNT = 2
SECTORS_PER_FAT = 1_009
SECTORS_PER_CLUSTER = 1
FIRST_DATA_SECTOR = RESERVED_SECTORS + FAT_COUNT * SECTORS_PER_FAT
CLUSTER_COUNT = (TOTAL_SECTORS - FIRST_DATA_SECTOR) // SECTORS_PER_CLUSTER
ROOT_CLUSTER = 2
VOLUME_ID = 0x534F_4D41
KNOWN_M6B_PATTERN = b"SANJUOS-M6B-READ-PATTERN"


def set_u16(buffer: bytearray, offset: int, value: int) -> None:
    buffer[offset : offset + 2] = value.to_bytes(2, "little")


def set_u32(buffer: bytearray, offset: int, value: int) -> None:
    buffer[offset : offset + 4] = value.to_bytes(4, "little")


def boot_sector() -> bytes:
    boot = bytearray(SECTOR_SIZE)
    boot[0:3] = b"\xeb\x58\x90"
    boot[3:11] = b"SOMAOS  "
    set_u16(boot, 11, SECTOR_SIZE)
    boot[13] = SECTORS_PER_CLUSTER
    set_u16(boot, 14, RESERVED_SECTORS)
    boot[16] = FAT_COUNT
    set_u16(boot, 17, 0)
    set_u16(boot, 19, 0)
    boot[21] = 0xF8
    set_u16(boot, 22, 0)
    set_u16(boot, 24, 63)
    set_u16(boot, 26, 255)
    set_u32(boot, 28, 0)
    set_u32(boot, 32, TOTAL_SECTORS)
    set_u32(boot, 36, SECTORS_PER_FAT)
    set_u16(boot, 40, 0)
    set_u16(boot, 42, 0)
    set_u32(boot, 44, ROOT_CLUSTER)
    set_u16(boot, 48, 1)
    set_u16(boot, 50, 6)
    boot[64] = 0x80
    boot[66] = 0x29
    set_u32(boot, 67, VOLUME_ID)
    boot[71:82] = b"SOMA OS    "
    boot[82:90] = b"FAT32   "
    boot[510:512] = b"\x55\xaa"
    return bytes(boot)


def fs_info_sector() -> bytes:
    info = bytearray(SECTOR_SIZE)
    set_u32(info, 0, 0x4161_5252)
    set_u32(info, 484, 0x6141_7272)
    set_u32(info, 488, CLUSTER_COUNT - 6)
    set_u32(info, 492, 8)
    set_u32(info, 508, 0xAA55_0000)
    return bytes(info)


def fat_sector() -> bytes:
    fat = bytearray(SECTOR_SIZE)
    entries = {
        0: 0x0FFF_FFF8,
        1: 0xFFFF_FFFF,
        2: 0x0FFF_FFFF,
        3: 0x0FFF_FFFF,
        4: 0x0FFF_FFFF,
        5: 6,
        6: 0x0FFF_FFFF,
        7: 0x0FFF_FFFF,
    }
    for cluster, value in entries.items():
        set_u32(fat, cluster * 4, value)
    return bytes(fat)


def short_checksum(short_name: bytes) -> int:
    checksum = 0
    for byte in short_name:
        checksum = (((checksum & 1) << 7) + (checksum >> 1) + byte) & 0xFF
    return checksum


def short_entry(
    name: bytes, attributes: int, cluster: int, size: int = 0
) -> bytes:
    if len(name) != 11:
        raise ValueError("FAT short names must contain exactly 11 bytes")
    entry = bytearray(32)
    entry[:11] = name
    entry[11] = attributes
    set_u16(entry, 20, cluster >> 16)
    set_u16(entry, 26, cluster & 0xFFFF)
    set_u32(entry, 28, size)
    return bytes(entry)


def lfn_entries(name: str, short_name: bytes) -> list[bytes]:
    units = list(name.encode("utf-16le"))
    utf16 = [
        int.from_bytes(bytes(units[index : index + 2]), "little")
        for index in range(0, len(units), 2)
    ]
    utf16.append(0)
    while len(utf16) % 13:
        utf16.append(0xFFFF)
    count = len(utf16) // 13
    if count == 0 or count > 5:
        raise ValueError("M6D fixture long name exceeds the bounded test contract")
    checksum = short_checksum(short_name)
    offsets = (1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30)
    entries: list[bytes] = []
    for ordinal in range(count, 0, -1):
        entry = bytearray(b"\xff" * 32)
        entry[0] = ordinal | (0x40 if ordinal == count else 0)
        entry[11] = 0x0F
        entry[12] = 0
        entry[13] = checksum
        set_u16(entry, 26, 0)
        chunk = utf16[(ordinal - 1) * 13 : ordinal * 13]
        for offset, unit in zip(offsets, chunk, strict=True):
            set_u16(entry, offset, unit)
        entries.append(bytes(entry))
    return entries


def directory_sector(entries: list[bytes]) -> bytes:
    if len(entries) >= SECTOR_SIZE // 32:
        raise ValueError("fixture directory exceeds one cluster")
    sector = bytearray(SECTOR_SIZE)
    for index, entry in enumerate(entries):
        sector[index * 32 : (index + 1) * 32] = entry
    sector[len(entries) * 32] = 0
    return bytes(sector)


def cluster_sector(cluster: int) -> int:
    if cluster < 2:
        raise ValueError("data clusters start at two")
    return FIRST_DATA_SECTOR + (cluster - 2) * SECTORS_PER_CLUSTER


def write_sector(image, sector: int, data: bytes) -> None:
    if len(data) != SECTOR_SIZE:
        raise ValueError("sector writes must be exactly 512 bytes")
    image.seek(sector * SECTOR_SIZE)
    image.write(data)


def padded_sector(data: bytes, fill: bytes = b"\0") -> bytes:
    if len(fill) != 1 or len(data) > SECTOR_SIZE:
        raise ValueError("invalid sector payload")
    return data + fill * (SECTOR_SIZE - len(data))


def create_image(path: Path) -> None:
    readme = b"Welcome to Soma OS persistent FAT32 storage.\n"
    long_name = "Getting-Started.txt"
    long_alias = b"GETTIN~1TXT"
    long_text = b"Soma OS long filename support is active through the VFS.\n"
    guide_prefix = (
        b"Soma OS M6D multi-cluster guide.\n"
        b"This file proves bounded FAT-chain traversal through cache and VFS.\n"
    )
    guide = (guide_prefix + b"0123456789abcdef" * 64)[:900]

    root_entries = [
        short_entry(b"SOMA OS    ", 0x08, 0),
        short_entry(b"README  TXT", 0x20, 4, len(readme)),
        short_entry(b"DOCS       ", 0x10, 3),
        *lfn_entries(long_name, long_alias),
        short_entry(long_alias, 0x20, 7, len(long_text)),
    ]
    docs_entries = [
        short_entry(b".          ", 0x10, 3),
        short_entry(b"..         ", 0x10, 0),
        short_entry(b"GUIDE   TXT", 0x20, 5, len(guide)),
    ]

    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w+b") as image:
        image.truncate(TOTAL_SECTORS * SECTOR_SIZE)
        boot = boot_sector()
        info = fs_info_sector()
        write_sector(image, 0, boot)
        write_sector(image, 1, info)
        write_sector(image, 6, boot)
        write_sector(image, 7, info)
        write_sector(image, 8, padded_sector(KNOWN_M6B_PATTERN))
        first_fat = fat_sector()
        write_sector(image, RESERVED_SECTORS, first_fat)
        write_sector(image, RESERVED_SECTORS + SECTORS_PER_FAT, first_fat)
        write_sector(image, cluster_sector(2), directory_sector(root_entries))
        write_sector(image, cluster_sector(3), directory_sector(docs_entries))
        write_sector(image, cluster_sector(4), padded_sector(readme))
        write_sector(image, cluster_sector(5), padded_sector(guide[:SECTOR_SIZE]))
        write_sector(image, cluster_sector(6), padded_sector(guide[SECTOR_SIZE:]))
        write_sector(image, cluster_sector(7), padded_sector(long_text))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    create_image(args.output)
    print(
        f"Created deterministic FAT32 image: {args.output} "
        f"({TOTAL_SECTORS} sectors, {CLUSTER_COUNT} clusters)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
