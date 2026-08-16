#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Build an ordinal-only Inventor envelope census and framing oracle.

The reader in this file is intentionally independent of the Rust codec.  It
walks CFB allocation and directory structures itself, then parses only the
RSe structures needed to establish an envelope, frame records, and identify a
typed kernel carrier.  It accepts paths only as input selectors.  Output does
not contain input paths or directory names, so a result can be retained as
aggregate evidence without exposing the source collection.

The optional CLI comparison mode invokes a caller-supplied ``cadmpeg`` binary
on an input and on each independently extracted carrier.  Extracted carriers
are held in a temporary directory and are deleted before the command exits.
No comparison result is written unless the caller redirects the JSON output.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from typing import Iterable, Iterator, Sequence
import zlib


MAGIC = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"
FREE = 0xFFFF_FFFF
EOC = 0xFFFF_FFFE
FAT_SECTOR = 0xFFFF_FFFD
DIFAT_SECTOR = 0xFFFF_FFFC
KERNEL_RECORD_TYPE_ID = bytes.fromhex("5c5945f6d5113313100060a6bba647b5")
V3_MAX_FILE_SIZE = 0x8000_0000
RANGE_LOCK_START = 0x7FFF_FF00
RANGE_LOCK_END = 0x8000_0000


class EvidenceError(Exception):
    """A structural error in one input document."""


def u16(data: bytes, offset: int) -> int:
    if offset < 0 or offset + 2 > len(data):
        raise EvidenceError("truncated u16")
    return int.from_bytes(data[offset : offset + 2], "little")


def u32(data: bytes, offset: int) -> int:
    if offset < 0 or offset + 4 > len(data):
        raise EvidenceError("truncated u32")
    return int.from_bytes(data[offset : offset + 4], "little")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def checked_end(start: int, length: int, limit: int, what: str) -> int:
    if start < 0 or length < 0 or start > limit or length > limit - start:
        raise EvidenceError(f"{what} range is outside its parent")
    return start + length


def exact_zlib(data: bytes) -> bytes:
    decoder = zlib.decompressobj()
    try:
        expanded = decoder.decompress(data)
        expanded += decoder.flush()
    except zlib.error as error:
        raise EvidenceError(f"zlib member is invalid: {error}") from error
    if not decoder.eof:
        raise EvidenceError("zlib member is truncated")
    if decoder.unused_data or decoder.unconsumed_tail:
        raise EvidenceError("zlib member has trailing or unconsumed bytes")
    return expanded


@dataclass(frozen=True)
class Span:
    """A physical file range occupied by logical stream bytes."""

    offset: int
    length: int

    def as_json(self) -> dict[str, int]:
        return {"offset": self.offset, "length": self.length}


@dataclass
class Stream:
    path: str
    size: int
    data: bytes
    spans: list[Span]
    allocation: str

    def range_json(self, offset: int = 0, length: int | None = None) -> dict:
        if length is None:
            length = len(self.data) - offset
        checked_end(offset, length, len(self.data), f"stream {self.path}")
        return {
            "stream_offset": offset,
            "length": length,
            "sha256": digest(self.data[offset : offset + length]),
        }


@dataclass
class DirectoryEntry:
    index: int
    name: str
    object_type: int
    color: int
    left: int
    right: int
    child: int
    start_sector: int
    size: int
    path: str = ""
    parent: int | None = None


class CompoundReader:
    """Independent CFB v3/v4 reader with allocation ownership tracking."""

    def __init__(self, source: bytes, max_stream_bytes: int) -> None:
        self.source = source
        self.max_stream_bytes = max_stream_bytes
        self.major = 0
        self.sector_size = 0
        self.header_size = 512
        self.mini_sector_size = 0
        self.mini_cutoff = 0
        self.sector_count = 0
        self.fat: list[int] = []
        self.full_fat: list[int] = []
        self.directory: list[DirectoryEntry] = []
        self.streams: dict[str, Stream] = {}
        self.roles: list[str | None] = []
        self.mini_roles: list[int | None] = []
        self.root_mini_data = b""
        self.root_mini_spans: list[Span] = []
        self.mini_fat: list[int] = []

    def parse(self) -> "CompoundReader":
        self._parse_header()
        difat, difat_sector_ids = self._read_difat()
        self._load_fat(difat)
        if any(self.fat[sector] != DIFAT_SECTOR for sector in difat_sector_ids):
            raise EvidenceError("CFB DIFAT sector has the wrong role marker")
        if self.major == 4 and len(self.source) > RANGE_LOCK_END:
            range_lock_sector = RANGE_LOCK_START // self.sector_size - 1
            if range_lock_sector >= self.sector_count or self.fat[range_lock_sector] != EOC:
                raise EvidenceError("CFB range lock sector is not an end-of-chain sector")
            self._claim([range_lock_sector], "range lock")
        self._claim(difat, "fat")
        self._claim(difat_sector_ids, "difat")

        directory_chain = self._follow_chain(
            u32(self.source, 48),
            expected=None if self.major == 3 else u32(self.source, 40),
            what="directory",
        )
        self._claim(directory_chain, "directory")
        directory_bytes = self._sector_bytes(directory_chain)
        self._parse_directory(directory_bytes)

        mini_chain = self._follow_chain(
            u32(self.source, 60),
            expected=u32(self.source, 64),
            what="mini FAT",
        )
        self._claim(mini_chain, "mini_fat")
        mini_bytes = self._sector_bytes(mini_chain)
        self.mini_fat = [
            u32(mini_bytes, offset) for offset in range(0, len(mini_bytes) - 3, 4)
        ]

        root = self.directory[0]
        if root.object_type != 5:
            raise EvidenceError("directory entry zero is not the root storage")
        self.root_mini_data, self.root_mini_spans = self._read_regular_stream(
            root.start_sector,
            root.size,
            "root mini stream",
        )
        self.mini_roles = [None] * (
            (len(self.root_mini_data) + self.mini_sector_size - 1) // self.mini_sector_size
        )

        live = [entry for entry in self.directory if entry.object_type in (1, 2, 5)]
        for entry in live:
            if entry.object_type != 2:
                continue
            if entry.size > self.max_stream_bytes:
                raise EvidenceError("stream exceeds the configured evidence limit")
            stream = self._read_stream(entry)
            self.streams[entry.path] = stream

        self._finish_allocation_checks()
        return self

    def _parse_header(self) -> None:
        if len(self.source) < 512 or self.source[:8] != MAGIC:
            raise EvidenceError("not a CFB file")
        self.major = u16(self.source, 26)
        if self.major not in (3, 4):
            raise EvidenceError(f"unsupported CFB major version {self.major}")
        if self.source[8:24] != bytes(16) or u16(self.source, 24) != 0x003E:
            raise EvidenceError("invalid CFB header identity")
        if u16(self.source, 28) != 0xFFFE:
            raise EvidenceError("CFB byte order is not little endian")
        sector_shift = u16(self.source, 30)
        mini_shift = u16(self.source, 32)
        if sector_shift not in (9, 12) or mini_shift != 6:
            raise EvidenceError("unsupported CFB sector layout")
        self.sector_size = 1 << sector_shift
        self.header_size = self.sector_size if self.major == 4 else 512
        self.mini_sector_size = 1 << mini_shift
        self.mini_cutoff = u32(self.source, 56)
        directory_sector_count = u32(self.source, 40)
        if (self.major == 3 and directory_sector_count != 0) or (
            self.major == 4 and directory_sector_count == 0
        ):
            raise EvidenceError("CFB directory sector count is invalid for its version")
        if self.source[34:40] != bytes(6) or self.mini_cutoff != 4096:
            raise EvidenceError("invalid CFB mini-stream cutoff")
        if self.major == 4 and self.source[512 : 1 << sector_shift] != bytes(
            (1 << sector_shift) - 512
        ):
            raise EvidenceError("CFB v4 header padding is not zero")
        if self.major == 3 and len(self.source) > V3_MAX_FILE_SIZE:
            raise EvidenceError("CFB v3 file exceeds the 2 GiB size ceiling")
        if len(self.source) < self.header_size or (len(self.source) - self.header_size) % self.sector_size:
            raise EvidenceError("CFB file has a partial sector")
        self.sector_count = (len(self.source) - self.header_size) // self.sector_size
        if self.sector_count < 2:
            raise EvidenceError("CFB file has fewer than the minimum sector count")
        fat_count = u32(self.source, 44)
        difat_count = u32(self.source, 72)
        if fat_count == 0 or fat_count > self.sector_count or difat_count > self.sector_count:
            raise EvidenceError("CFB allocation-table count is invalid")
        self.roles = [None] * self.sector_count

    def _sector(self, sector: int) -> bytes:
        if sector < 0 or sector >= self.sector_count:
            raise EvidenceError(f"CFB sector {sector} is outside the file")
        start = self.header_size + sector * self.sector_size
        return self.source[start : start + self.sector_size]

    def _read_difat(self) -> tuple[list[int], list[int]]:
        fat_count = u32(self.source, 44)
        first_difat = u32(self.source, 68)
        difat_count = u32(self.source, 72)
        if fat_count > self.sector_count:
            raise EvidenceError("CFB FAT sector count is too large")
        if (difat_count == 0 and first_difat != EOC) or (
            difat_count != 0 and first_difat in (EOC, FREE)
        ):
            raise EvidenceError("CFB DIFAT chain length does not match the header")
        values: list[int] = []
        header_free_seen = False
        for offset in range(76, 512, 4):
            value = u32(self.source, offset)
            if value == FREE:
                header_free_seen = True
            else:
                if header_free_seen:
                    raise EvidenceError("non-free CFB header DIFAT entry follows a free entry")
                values.append(value)
        difat_sectors: list[int] = []
        current = first_difat
        seen: set[int] = set()
        entries_per_difat = self.sector_size // 4 - 1
        for _ in range(difat_count):
            if current in seen or current >= self.sector_count:
                raise EvidenceError("CFB DIFAT chain is cyclic or out of range")
            seen.add(current)
            difat_sectors.append(current)
            sector = self._sector(current)
            free_seen = False
            for offset in range(0, entries_per_difat * 4, 4):
                value = u32(sector, offset)
                if value == FREE:
                    free_seen = True
                else:
                    if free_seen:
                        raise EvidenceError("non-free CFB DIFAT entry follows a free entry")
                    values.append(value)
            current = u32(sector, entries_per_difat * 4)
        if difat_count and current != EOC:
            raise EvidenceError("CFB DIFAT chain has an unexpected successor")
        if len(values) < fat_count:
            raise EvidenceError("CFB DIFAT does not name every FAT sector")
        values = values[:fat_count]
        if len(set(values)) != len(values):
            raise EvidenceError("CFB DIFAT names a FAT sector more than once")
        if set(values) & set(difat_sectors):
            raise EvidenceError("CFB sector has both FAT and DIFAT roles")
        for value in values:
            if value >= self.sector_count:
                raise EvidenceError("CFB DIFAT names an out-of-range FAT sector")
        return values, difat_sectors

    def _load_fat(self, fat_sector_ids: Sequence[int]) -> None:
        fat_bytes = b"".join(self._sector(sector) for sector in fat_sector_ids)
        self.full_fat = [
            u32(fat_bytes, offset) for offset in range(0, len(fat_bytes) - 3, 4)
        ]
        if len(self.full_fat) < self.sector_count:
            raise EvidenceError("CFB FAT does not cover every file sector")
        if any(value != FREE for value in self.full_fat[self.sector_count :]):
            raise EvidenceError("CFB FAT entries past end-of-file are not free")
        if any(self.full_fat[sector] != FAT_SECTOR for sector in fat_sector_ids):
            raise EvidenceError("CFB FAT sector has the wrong role marker")
        self.fat = self.full_fat[: self.sector_count]

    def _claim(self, sectors: Iterable[int], role: str) -> None:
        for sector in sectors:
            if sector < 0 or sector >= self.sector_count:
                raise EvidenceError(f"{role} sector is out of range")
            owner = self.roles[sector]
            if owner is not None:
                raise EvidenceError(
                    f"CFB sector {sector} is allocated as both {owner} and {role}"
                )
            self.roles[sector] = role

    def _follow_chain(self, start: int, expected: int | None, what: str) -> list[int]:
        if start in (EOC, FREE):
            if expected not in (None, 0):
                raise EvidenceError(f"{what} chain is empty but has a declared count")
            return []
        result: list[int] = []
        seen: set[int] = set()
        current = start
        while True:
            if current in seen or current >= self.sector_count:
                raise EvidenceError(f"{what} chain is cyclic or out of range")
            seen.add(current)
            result.append(current)
            if len(result) > self.sector_count:
                raise EvidenceError(f"{what} chain is too long")
            successor = self.fat[current]
            if successor == EOC:
                break
            if successor in (FREE, FAT_SECTOR, DIFAT_SECTOR) or successor >= self.sector_count:
                raise EvidenceError(f"{what} chain points to an invalid successor")
            current = successor
        if expected is not None and len(result) != expected:
            raise EvidenceError(
                f"{what} chain has {len(result)} sectors, expected {expected}"
            )
        return result

    def _sector_bytes(self, sectors: Sequence[int]) -> bytes:
        return b"".join(self._sector(sector) for sector in sectors)

    def _parse_directory(self, data: bytes) -> None:
        if len(data) < 128 or len(data) % 128:
            raise EvidenceError("CFB directory stream has an invalid length")
        entries: list[DirectoryEntry] = []
        for index, entry in enumerate(
            data[offset : offset + 128] for offset in range(0, len(data), 128)
        ):
            object_type = entry[66]
            if object_type == 0:
                if (
                    any(entry[:68])
                    or any(u32(entry, offset) != FREE for offset in (68, 72, 76))
                    or any(entry[80:])
                ):
                    raise EvidenceError("CFB unallocated directory entry is not zeroed")
                entries.append(
                    DirectoryEntry(index, "", 0, 0, FREE, FREE, FREE, EOC, 0)
                )
                continue
            name_len = u16(entry, 64)
            if name_len < 2 or name_len > 64 or name_len % 2:
                raise EvidenceError("CFB live directory entry has an invalid name length")
            if entry[name_len - 2 : name_len] != b"\x00\x00":
                raise EvidenceError("CFB directory name is not NUL terminated")
            try:
                name = entry[: name_len - 2].decode("utf-16-le")
            except UnicodeDecodeError as error:
                raise EvidenceError("CFB directory name is not valid UTF-16") from error
            if any(character in name for character in "/\\:!"):
                raise EvidenceError("CFB directory name contains a forbidden character")
            if object_type not in (1, 2, 5):
                raise EvidenceError("CFB directory entry has an invalid object type")
            color = entry[67]
            if color not in (0, 1):
                raise EvidenceError("CFB directory entry has an invalid color")
            entries.append(
                DirectoryEntry(
                    index,
                    name,
                    object_type,
                    color,
                    u32(entry, 68),
                    u32(entry, 72),
                    u32(entry, 76),
                    u32(entry, 116),
                    (u32(entry, 124) << 32 | u32(entry, 120))
                    if self.major == 4
                    else u32(entry, 120),
                )
            )
        if not entries or entries[0].object_type != 5:
            raise EvidenceError("CFB directory has no root entry")
        if (
            entries[0].name != "Root Entry"
            or entries[0].left != FREE
            or entries[0].right != FREE
            or any(entry.object_type == 5 for entry in entries[1:])
        ):
            raise EvidenceError("CFB root directory entry is invalid")
        self.directory = entries
        self._walk_storage_tree(0, "")
        for entry in self.directory:
            if entry.index != 0 and entry.object_type in (1, 2, 5) and not entry.path:
                raise EvidenceError("CFB live directory entry is unreachable")

    @staticmethod
    def _name_key(name: str) -> tuple[int, tuple[int, ...]]:
        units = [
            int.from_bytes(name.encode("utf-16-le")[offset : offset + 2], "little")
            for offset in range(0, len(name.encode("utf-16-le")), 2)
        ]
        return (len(units), tuple(CompoundReader._uppercase_unit(unit) for unit in units))

    @staticmethod
    def _uppercase_unit(unit: int) -> int:
        if 0xD800 <= unit <= 0xDFFF:
            return unit
        upper = chr(unit).upper().encode("utf-16-le")
        if len(upper) == 2:
            return int.from_bytes(upper, "little")
        return unit

    def _walk_storage_tree(self, storage_index: int, storage_path: str) -> None:
        storage = self.directory[storage_index]
        visited: set[int] = set()
        active_storages: list[tuple[int, str]] = [(storage.child, storage_path)]
        while active_storages:
            child_root, parent_path = active_storages.pop()
            if child_root == FREE:
                continue
            values: list[tuple[int, tuple[int, tuple[int, ...]]]] = []
            stack: list[tuple[int, int]] = [(child_root, 0)]
            while stack:
                node, state = stack.pop()
                if node == FREE:
                    continue
                if node >= len(self.directory):
                    raise EvidenceError("CFB sibling tree points outside the directory")
                if state == 0:
                    if node in visited:
                        raise EvidenceError("CFB sibling tree reuses a directory entry")
                    visited.add(node)
                    stack.append((self.directory[node].right, 0))
                    stack.append((node, 1))
                    stack.append((self.directory[node].left, 0))
                else:
                    entry = self.directory[node]
                    key = self._name_key(entry.name)
                    values.append((node, key))
                    if entry.color == 0:
                        for child in (entry.left, entry.right):
                            if child != FREE and self.directory[child].color == 0:
                                raise EvidenceError("CFB red directory entry has a red child")
            if self.directory[child_root].color != 1:
                raise EvidenceError("CFB sibling tree root is not black")
            if any(values[index - 1][1] >= values[index][1] for index in range(1, len(values))):
                raise EvidenceError("CFB sibling tree is not strictly ordered")
            for node, _key in values:
                entry = self.directory[node]
                if entry.object_type not in (1, 2):
                    raise EvidenceError("CFB sibling tree contains a non-live entry")
                path = entry.name if not parent_path else f"{parent_path}/{entry.name}"
                entry.path = path
                entry.parent = storage_index
                if entry.object_type == 1:
                    active_storages.append((entry.child, path))

    def _read_regular_stream(
        self, start: int, size: int, role: str
    ) -> tuple[bytes, list[Span]]:
        if size == 0:
            if start not in (EOC, FREE):
                raise EvidenceError(f"empty {role} has a non-empty start sector")
            return b"", []
        if size > self.max_stream_bytes:
            raise EvidenceError(f"{role} exceeds the configured evidence limit")
        expected = (size + self.sector_size - 1) // self.sector_size
        chain = self._follow_chain(start, expected, role)
        self._claim(chain, role)
        data = bytearray()
        spans: list[Span] = []
        remaining = size
        for sector in chain:
            length = min(remaining, self.sector_size)
            data.extend(self._sector(sector)[:length])
            spans.append(Span(self.header_size + sector * self.sector_size, length))
            remaining -= length
        if remaining:
            raise EvidenceError(f"{role} did not provide its declared size")
        return bytes(data), spans

    def _read_mini_stream(self, start: int, size: int, role: str) -> tuple[bytes, list[Span]]:
        if size == 0:
            if start not in (EOC, FREE):
                raise EvidenceError(f"empty {role} has a non-empty mini-sector")
            return b"", []
        expected = (size + self.mini_sector_size - 1) // self.mini_sector_size
        if not self.mini_fat:
            raise EvidenceError(f"{role} uses a missing mini FAT")
        chain: list[int] = []
        seen: set[int] = set()
        current = start
        while True:
            if current in seen or current >= len(self.mini_fat):
                raise EvidenceError(f"{role} mini-sector chain is cyclic or out of range")
            seen.add(current)
            chain.append(current)
            if len(chain) > len(self.mini_fat):
                raise EvidenceError(f"{role} mini-sector chain is too long")
            successor = self.mini_fat[current]
            if successor == EOC:
                break
            if successor in (FREE, FAT_SECTOR, DIFAT_SECTOR) or successor >= len(self.mini_fat):
                raise EvidenceError(f"{role} mini-sector chain has an invalid successor")
            current = successor
        if len(chain) != expected:
            raise EvidenceError(
                f"{role} mini-sector chain has {len(chain)} sectors, expected {expected}"
            )
        data = bytearray()
        spans: list[Span] = []
        remaining = size
        for mini_sector in chain:
            if mini_sector >= len(self.mini_roles):
                raise EvidenceError(f"{role} mini-sector is outside the root mini stream")
            if self.mini_roles[mini_sector] is not None:
                raise EvidenceError(f"mini-sector {mini_sector} is allocated twice")
            self.mini_roles[mini_sector] = mini_sector
            length = min(remaining, self.mini_sector_size)
            begin = mini_sector * self.mini_sector_size
            data.extend(self.root_mini_data[begin : begin + length])
            remaining -= length
            for root_span in self.root_mini_spans:
                root_begin = root_span.offset - self.header_size
                root_end = root_begin + root_span.length
                logical_begin = begin
                logical_end = begin + length
                overlap_begin = max(root_begin, logical_begin)
                overlap_end = min(root_end, logical_end)
                if overlap_begin < overlap_end:
                    spans.append(
                        Span(
                            root_span.offset + overlap_begin - root_begin,
                            overlap_end - overlap_begin,
                        )
                    )
        if remaining:
            raise EvidenceError(f"{role} did not provide its declared size")
        return bytes(data), spans

    def _read_stream(self, entry: DirectoryEntry) -> Stream:
        if entry.size >= self.mini_cutoff:
            data, spans = self._read_regular_stream(
                entry.start_sector, entry.size, f"stream {entry.path}"
            )
            allocation = "regular"
        else:
            data, spans = self._read_mini_stream(
                entry.start_sector, entry.size, f"stream {entry.path}"
            )
            allocation = "mini"
        return Stream(entry.path, entry.size, data, spans, allocation)

    def _finish_allocation_checks(self) -> None:
        for mini_sector, marker in enumerate(self.mini_fat):
            if (
                mini_sector >= len(self.mini_roles)
                or self.mini_roles[mini_sector] is None
            ) and marker != FREE:
                raise EvidenceError(
                    f"CFB mini-sector {mini_sector} is allocated in mini FAT but unreachable"
                )
        for sector, role in enumerate(self.roles):
            if role is None:
                if self.fat[sector] != FREE:
                    raise EvidenceError(
                        f"CFB sector {sector} is allocated in FAT but unreachable"
                    )

    def stream(self, path: str) -> Stream | None:
        return self.streams.get(path)


@dataclass
class RegistrySegment:
    display_name: str
    type_name: str
    segment_id: bytes
    version_major: int


@dataclass
class MetaInfo:
    stream: Stream
    version: int
    display_name: str
    segment_id: bytes
    body: bytes
    blocks: list[tuple[bool, int]]
    type_ids: list[bytes]
    compressed_offset: int


@dataclass
class RecordInfo:
    ordinal: int
    selector: int
    type_id: bytes
    frame_start: int
    frame_end: int
    payload_offset: int
    payload_length: int
    trailer_start: int
    trailer_end: int
    payload: bytes


@dataclass
class BulkInfo:
    stream: Stream
    form: int
    expanded: bytes
    compressed_offset: int
    records: list[RecordInfo]
    stream_trailer_start: int


def utf8_field(data: bytes, cursor: list[int], what: str, maximum: int = 1 << 20) -> str:
    length = u32(data, cursor[0])
    cursor[0] += 4
    if length > maximum:
        raise EvidenceError(f"{what} is too large")
    end = checked_end(cursor[0], length, len(data), what)
    try:
        value = data[cursor[0] : end].decode("utf-8")
    except UnicodeDecodeError as error:
        raise EvidenceError(f"{what} is not UTF-8") from error
    cursor[0] = end
    return value


def utf16_field(data: bytes, cursor: list[int], what: str, maximum: int = 1 << 20) -> str:
    count = u32(data, cursor[0])
    cursor[0] += 4
    if count > maximum:
        raise EvidenceError(f"{what} is too large")
    length = count * 2
    end = checked_end(cursor[0], length, len(data), what)
    try:
        value = data[cursor[0] : end].decode("utf-16-le")
    except UnicodeDecodeError as error:
        raise EvidenceError(f"{what} is not UTF-16") from error
    cursor[0] = end
    return value


def finish_cursor(data: bytes, cursor: list[int], what: str) -> None:
    if cursor[0] != len(data):
        raise EvidenceError(f"{what} has {len(data) - cursor[0]} trailing bytes")


def parse_database(stream: Stream) -> tuple[int, tuple[int, int, int]]:
    data = stream.data
    cursor = [0]
    if len(data) < 20:
        raise EvidenceError("RSe database is truncated")
    cursor[0] += 16
    schema = u32(data, cursor[0])
    cursor[0] += 4
    def version() -> tuple[int, int, int]:
        if cursor[0] + 8 > len(data):
            raise EvidenceError("RSe database version is truncated")
        value = (data[cursor[0]], data[cursor[0] + 1], data[cursor[0] + 2])
        cursor[0] += 8
        return value
    _created = version()
    if cursor[0] + 8 > len(data):
        raise EvidenceError("RSe database creation time is truncated")
    cursor[0] += 8
    saved = version()
    if cursor[0] + 8 > len(data):
        raise EvidenceError("RSe database save time is truncated")
    cursor[0] += 8
    utf16_field(data, cursor, "RSe database note", 65_536)
    finish_cursor(data, cursor, "RSe database")
    return schema, saved


def parse_registry(stream: Stream) -> list[RegistrySegment]:
    data = stream.data
    cursor = [0]
    count = u32(data, cursor[0])
    cursor[0] += 4
    if count > 65_536:
        raise EvidenceError("RSe registry has too many entries")
    result: list[RegistrySegment] = []
    for _ in range(count):
        display_name = utf16_field(data, cursor, "RSe segment display name", 4_096)
        begin = cursor[0]
        segment_id = data[begin : begin + 16]
        cursor[0] = checked_end(begin, 16, len(data), "RSe segment id")
        cursor[0] = checked_end(cursor[0], 16, len(data), "RSe segment revision id")
        cursor[0] += 4
        object_count = u32(data, cursor[0])
        cursor[0] += 4
        if object_count > 1_000_000:
            raise EvidenceError("RSe registry object count is too large")
        cursor[0] = checked_end(cursor[0], 20, len(data), "RSe segment state")
        cursor[0] += 4
        type_name = utf16_field(data, cursor, "RSe segment type name", 4_096)
        cursor[0] = checked_end(cursor[0], 8, len(data), "RSe segment type state")
        if cursor[0] + 8 > len(data):
            raise EvidenceError("RSe segment version is truncated")
        version_major = data[cursor[0] + 2]
        cursor[0] += 8
        cursor[0] += 4
        node_count: int | None = None
        for _ in range(object_count):
            cursor[0] = checked_end(cursor[0], 16 + 9 + 16 + 4, len(data), "RSe segment object")
            node_count = u32(data, cursor[0])
            cursor[0] += 4
        nodes = 0 if node_count is None else node_count - 1
        if node_count == 0:
            raise EvidenceError("RSe segment node count is zero")
        if nodes > 1_000_000:
            raise EvidenceError("RSe segment node count is too large")
        cursor[0] = checked_end(cursor[0], nodes * 22, len(data), "RSe segment nodes")
        result.append(RegistrySegment(display_name, type_name, segment_id, version_major))
    cursor[0] = checked_end(cursor[0], 4, len(data), "RSe registry state")
    for _list_name in ("primary", "secondary"):
        count = u32(data, cursor[0])
        cursor[0] += 4
        if count > 1_000_000:
            raise EvidenceError("RSe registry identifier list is too large")
        cursor[0] = checked_end(cursor[0], count * 16, len(data), "RSe registry identifiers")
    finish_cursor(data, cursor, "RSe registry")
    return result


def parse_meta(stream: Stream) -> MetaInfo:
    data = stream.data
    cursor = [0]
    marker = utf8_field(data, cursor, "RSe metadata marker")
    version = u16(data, cursor[0])
    cursor[0] += 2
    if marker != "RSe Meta Stream Version 8" or version != 8:
        raise EvidenceError("RSe metadata is not Meta Stream version 8")
    header_len = checked_end(cursor[0], 16, len(data), "RSe metadata header")
    cursor[0] = header_len
    display_name = utf16_field(data, cursor, "RSe metadata display name", 4_096)
    segment_id = data[cursor[0] : cursor[0] + 16]
    cursor[0] = checked_end(cursor[0], 16, len(data), "RSe metadata segment id")
    cursor[0] = checked_end(cursor[0], 12, len(data), "RSe metadata state")
    utf8_field(data, cursor, "RSe metadata creation time", 256)
    utf8_field(data, cursor, "RSe metadata modification time", 256)
    cursor[0] = checked_end(cursor[0], 1, len(data), "RSe metadata body form")
    compressed_offset = cursor[0]
    if compressed_offset == len(data):
        raise EvidenceError("RSe metadata has no compressed body")
    body = exact_zlib(data[compressed_offset:])
    blocks, type_ids = parse_meta_body(body)
    return MetaInfo(stream, version, display_name, segment_id, body, blocks, type_ids, compressed_offset)


def counted_section(
    body: bytes, offset: int, item_size: int, what: str
) -> tuple[int, int, int, int]:
    count = u32(body, offset)
    if count > 1_000_000:
        raise EvidenceError(f"{what} count is too large")
    payload_start = offset + 4
    payload_length = count * item_size
    payload_end = checked_end(payload_start, payload_length, len(body), what)
    span = u32(body, payload_end)
    if span != 4 + payload_length:
        raise EvidenceError(f"{what} has an invalid span")
    return count, payload_start, payload_end, payload_end + 4


def parse_meta_body(body: bytes) -> tuple[list[tuple[bool, int]], list[bytes]]:
    if len(body) < 14 + 16:
        raise EvidenceError("RSe metadata body is truncated")
    offset = 14
    block_count, payload_start, payload_end, next_offset = counted_section(
        body, offset, 4, "RSe block table"
    )
    blocks = []
    for ordinal in range(block_count):
        encoded = u32(body, payload_start + ordinal * 4)
        blocks.append((bool(encoded & 0x8000_0000), encoded & 0x7FFF_FFFF))
    offset = next_offset
    _count, _start, _end, offset = counted_section(body, offset, 10, "RSe section 2")
    _count, _start, _end, offset = counted_section(body, offset, 28, "RSe section 3")
    type_count, type_start, section_4_footer, _next_offset = counted_section(
        body, offset, 28, "RSe type table"
    )
    if type_count > 256:
        raise EvidenceError("RSe type table has too many entries")
    type_ids = [
        body[type_start + ordinal * 28 : type_start + ordinal * 28 + 16]
        for ordinal in range(type_count)
    ]
    terminal_start = len(body) - 16
    end = terminal_start
    payload_length = 0x48
    for number in range(11, 4, -1):
        header = end - payload_length - 8
        if header < section_4_footer:
            raise EvidenceError("RSe reverse metadata chain underflows")
        previous_span = u32(body, header)
        discriminator = u32(body, header + 4)
        payload = body[header + 8 : end]
        if previous_span < 4:
            raise EvidenceError("RSe reverse metadata chain has an invalid back span")
        if number == 7:
            if discriminator and len(payload) // discriminator < 0x4C:
                expected = discriminator * 32
                if len(payload) != expected:
                    raise EvidenceError("RSe section 7 has an invalid payload")
        elif number == 8:
            expected = discriminator * 20
            if len(payload) != expected:
                raise EvidenceError("RSe section 8 has an invalid payload")
        elif number == 9:
            expected = discriminator * 19
            if len(payload) != expected:
                raise EvidenceError("RSe section 9 has an invalid payload")
        elif number == 10:
            expected = discriminator * 8
            if len(payload) != expected:
                raise EvidenceError("RSe section 10 has an invalid payload")
        elif number == 11:
            expected = discriminator * 4
            if len(payload) != expected:
                raise EvidenceError("RSe section 11 has an invalid payload")
        end = header
        payload_length = previous_span - 4
    if end != section_4_footer:
        raise EvidenceError("RSe reverse metadata chain does not join section 4")
    return blocks, type_ids


def parse_extended_trailer(data: bytes, cursor: list[int]) -> None:
    present = data[cursor[0]] if cursor[0] < len(data) else 2
    cursor[0] += 1
    if present > 1:
        raise EvidenceError("RSe record trailer presence is invalid")
    if not present:
        return
    count = u32(data, cursor[0])
    cursor[0] += 4
    if count & 0x8000_0000:
        return
    if count > 65_536:
        raise EvidenceError("RSe record trailer property count is too large")
    for _ in range(count):
        length = u32(data, cursor[0])
        cursor[0] += 4
        if length > 65_536:
            raise EvidenceError("RSe record trailer property name is too large")
        cursor[0] = checked_end(cursor[0], length, len(data), "RSe record trailer property name")
        property_type = u32(data, cursor[0])
        cursor[0] += 4
        sizes = {1: 3, 3: 4, 7: 4, 8: 6, 10: 6, 11: 10}
        if property_type in sizes:
            cursor[0] = checked_end(cursor[0], sizes[property_type], len(data), "RSe record trailer property")
        elif property_type == 14:
            cursor[0] = checked_end(cursor[0], 2, len(data), "RSe record trailer array type")
            length = u32(data, cursor[0])
            cursor[0] += 4
            cursor[0] = checked_end(cursor[0], length, len(data), "RSe record trailer byte array")
        else:
            raise EvidenceError(f"RSe record trailer property type {property_type} is unsupported")
    list_type = u16(data, cursor[0])
    list_marker = u16(data, cursor[0] + 2)
    cursor[0] += 4
    if (list_type, list_marker) != (6, 0x3000):
        raise EvidenceError("RSe record trailer list marker is invalid")
    reference_count = u32(data, cursor[0])
    cursor[0] += 4
    if reference_count > 65_536:
        raise EvidenceError("RSe record trailer reference count is too large")
    if reference_count:
        cursor[0] = checked_end(cursor[0], 8, len(data), "RSe record trailer reference header")
        for _ in range(reference_count):
            length = u32(data, cursor[0])
            cursor[0] += 4
            if length > 65_536:
                raise EvidenceError("RSe record trailer reference name is too large")
            cursor[0] = checked_end(cursor[0], length, len(data), "RSe record trailer reference name")
            cursor[0] = checked_end(cursor[0], 4, len(data), "RSe record trailer reference")


def frame_records(
    stream: Stream,
    expanded: bytes,
    meta: MetaInfo,
    version_major: int,
) -> tuple[list[RecordInfo], int]:
    cursor = [0]
    records: list[RecordInfo] = []
    for ordinal, (stored, payload_length) in enumerate(meta.blocks):
        if not stored:
            continue
        frame_start = cursor[0]
        selector = u32(expanded, cursor[0])
        cursor[0] += 4
        type_index = selector & 0xFF
        if type_index >= len(meta.type_ids):
            raise EvidenceError("RSe record selects an absent type descriptor")
        payload_offset = cursor[0]
        payload_end = checked_end(payload_offset, payload_length, len(expanded), "RSe record payload")
        payload = expanded[payload_offset:payload_end]
        cursor[0] = payload_end
        trailing_length = u32(expanded, cursor[0])
        cursor[0] += 4
        if trailing_length not in (0, payload_length):
            raise EvidenceError("RSe record trailing length disagrees with its block")
        trailer_start = cursor[0]
        if version_major > 18:
            parse_extended_trailer(expanded, cursor)
        records.append(
            RecordInfo(
                ordinal,
                selector,
                meta.type_ids[type_index],
                frame_start,
                cursor[0],
                payload_offset,
                payload_length,
                trailer_start,
                cursor[0],
                payload,
            )
        )
    stream_trailer_start = cursor[0]
    marker = u32(expanded, cursor[0])
    cursor[0] += 4
    if marker != FREE:
        raise EvidenceError("RSe bulk stream has an invalid trailer marker")
    return records, stream_trailer_start


def classify_kind(display_name: str, type_name: str) -> str:
    value = type_name or display_name
    known = {
        "PmBRepSegment": "pm_brep",
        "PmBrepSegmentType": "pm_brep",
        "PmDcSegmentType": "pm_dc",
        "PmGRxSegmentType": "pm_graphics",
        "PmAppSegmentType": "pm_app",
        "PmBRxSegmentType": "pm_browser",
        "PmResultSegmentType": "pm_result",
        "PmDCSegment": "pm_dc",
        "PmGraphicsSegment": "pm_graphics",
        "PmAppSegment": "pm_app",
        "PmBrowserSegment": "pm_browser",
        "PmResultSegment": "pm_result",
        "FBAttributeSegment": "fb_attribute",
        "AmDCSegment": "am_dc",
        "AmBREPSegment": "am_brep",
        "AmGRxSegmentType": "am_graphics",
        "AmAppSegmentType": "am_app",
        "AmBRxSegmentType": "am_browser",
        "AmRxSegmentType": "am_rx",
        "NBNotebookSegment": "notebook",
        "NotebookSegmentType": "notebook",
        "FWxDesignViewType": "design_view",
        "FWxDesignViewManagerType": "design_view",
    }
    return known.get(value, value or "unknown")


@dataclass
class DocumentEvidence:
    ordinal: int
    cfb: CompoundReader
    root_rse: str
    databases: list[dict] = field(default_factory=list)
    registry: list[RegistrySegment] = field(default_factory=list)
    metas: dict[str, MetaInfo] = field(default_factory=dict)
    bulks: dict[str, BulkInfo] = field(default_factory=dict)
    segments: list[dict] = field(default_factory=list)
    carriers: list[dict] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    active_carrier_index: int | None = None
    active_carrier_state: str = "not_applicable"
    active_carrier_detail: str | None = None

    def parse(self) -> "DocumentEvidence":
        for path, stream in sorted(self.cfb.streams.items()):
            parts = path.split("/")
            if (
                len(parts) == 3
                and parts[0].casefold() == self.root_rse.casefold()
                and parts[2].casefold() == "rsedb"
            ):
                try:
                    schema, saved = parse_database(stream)
                except EvidenceError as error:
                    self.errors.append(str(error))
                    continue
                self.databases.append(
                    {
                        "band": parts[1],
                        "schema": schema,
                        "saved_version": {
                            "revision": saved[0],
                            "minor": saved[1],
                            "major": saved[2],
                        },
                    }
                )
        registry_stream = next(
            (
                stream
                for path, stream in self.cfb.streams.items()
                if path.casefold() == f"{self.root_rse}/rseseginfo".casefold()
            ),
            None,
        )
        if registry_stream is not None:
            self.registry = parse_registry(registry_stream)
        metadata: dict[str, Stream] = {}
        bulk: dict[str, Stream] = {}
        for path, stream in self.cfb.streams.items():
            parts = path.split("/")
            if (
                len(parts) != 2
                or parts[0].casefold() != self.root_rse.casefold()
                or not parts[1]
            ):
                continue
            if parts[1][0] in "MB" and len(parts[1]) > 1:
                target = metadata if parts[1][0] == "M" else bulk
                token = parts[1][1:]
                if token in target:
                    raise EvidenceError("RSe segment stream token is duplicated")
                target[token] = stream
        for token in sorted(set(metadata) | set(bulk)):
            if token not in metadata or token not in bulk:
                self.segments.append(
                    {"token": token, "state": "unpaired", "missing": "bulk" if token in metadata else "metadata"}
                )
                continue
            meta = parse_meta(metadata[token])
            expanded = exact_zlib(bulk[token].data[18:])
            form = u16(bulk[token].data, 16)
            registry_matches = [
                entry for entry in self.registry if entry.segment_id == meta.segment_id
            ]
            if len(registry_matches) != 1:
                raise EvidenceError(
                    "RSe metadata segment id does not select exactly one registry entry"
                )
            registry = registry_matches[0]
            if registry.display_name != meta.display_name:
                raise EvidenceError("RSe registry and metadata display names differ")
            version_major = registry.version_major
            records, trailer_start = frame_records(bulk[token], expanded, meta, version_major)
            bulk_info = BulkInfo(bulk[token], form, expanded, 18, records, trailer_start)
            self.metas[token] = meta
            self.bulks[token] = bulk_info
            segment_kind = classify_kind(
                meta.display_name, registry.type_name if registry is not None else ""
            )
            segment = {
                "token": token,
                "state": "framed",
                "kind": segment_kind,
                "display_name": meta.display_name,
                "segment_id_sha256": digest(meta.segment_id),
                "meta": {
                    "stream_size": meta.stream.size,
                    "version": meta.version,
                    "compressed": meta.stream.range_json(meta.compressed_offset),
                    "expanded": {"length": len(meta.body), "sha256": digest(meta.body)},
                    "physical_spans": [span.as_json() for span in meta.stream.spans],
                    "block_count": len(meta.blocks),
                    "stored_block_count": sum(1 for stored, _length in meta.blocks if stored),
                },
                "bulk": {
                    "stream_size": bulk[token].size,
                    "form": form,
                    "compressed": bulk[token].range_json(18),
                    "expanded": {"length": len(expanded), "sha256": digest(expanded)},
                    "physical_spans": [span.as_json() for span in bulk[token].spans],
                    "record_count": len(records),
                    "stream_trailer": {
                        "offset": trailer_start,
                        "length": len(expanded) - trailer_start,
                        "sha256": digest(expanded[trailer_start:]),
                    },
                },
                "records": [],
            }
            for record in records:
                record_json = {
                    "ordinal": record.ordinal,
                    "selector": record.selector,
                    "type_id": record.type_id.hex(),
                    "frame": {
                        "offset": record.frame_start,
                        "length": record.frame_end - record.frame_start,
                        "sha256": digest(expanded[record.frame_start : record.frame_end]),
                    },
                    "payload": {
                        "offset": record.payload_offset,
                        "length": record.payload_length,
                        "sha256": digest(record.payload),
                    },
                    "trailer": {
                        "offset": record.trailer_start,
                        "length": record.trailer_end - record.trailer_start,
                        "sha256": digest(expanded[record.trailer_start : record.trailer_end]),
                    },
                }
                segment["records"].append(record_json)
                if record.type_id == KERNEL_RECORD_TYPE_ID:
                    self.carriers.append(self._carrier_json(token, segment_kind, version_major, record))
            self.segments.append(segment)
        self._select_active_carrier()
        return self

    def _select_active_carrier(self) -> None:
        document_kind = self.envelope()["document_kind"]
        if document_kind != "part":
            self.active_carrier_state = "not_applicable"
            return
        brep_segments = [segment for segment in self.segments if segment.get("kind") == "pm_brep"]
        if len(brep_segments) != 1:
            self.active_carrier_state = "unavailable"
            self.active_carrier_detail = (
                f"part document has {len(brep_segments)} PmBRep segments; expected one"
            )
            return
        candidates = [
            index
            for index, carrier in enumerate(self.carriers)
            if carrier.get("segment_kind") == "pm_brep"
        ]
        if len(candidates) != 1:
            self.active_carrier_state = "unavailable"
            self.active_carrier_detail = (
                f"PmBRep contains {len(candidates)} typed kernel-carrier records; expected one"
            )
            return
        index = candidates[0]
        carrier = self.carriers[index]
        if carrier.get("state") != "framed":
            self.active_carrier_state = "unavailable"
            self.active_carrier_detail = carrier.get("detail")
            return
        carrier["active"] = True
        self.active_carrier_index = index
        self.active_carrier_state = "selected"

    def _carrier_json(
        self, token: str, segment_kind: str, version_major: int, record: RecordInfo
    ) -> dict:
        payload = record.payload
        footer_len = 17 if 15 <= version_major <= 22 else 18 if version_major >= 23 else None
        state = "framed"
        detail: str | None = None
        family: str | None = None
        carrier = b""
        if footer_len is None or len(payload) < 14 + footer_len:
            state = "unavailable"
            detail = "kernel-carrier envelope version or length is unsupported"
        else:
            carrier_end = len(payload) - footer_len
            candidate = payload[14:carrier_end]
            if candidate.startswith(b"ASM BinaryFile"):
                family = "asm"
            elif candidate.startswith(b"ACIS BinaryFile"):
                family = "acis"
            else:
                state = "unavailable"
                detail = "typed carrier has no kernel signature at its declared payload start"
            if state == "framed":
                cursor = carrier_end
                _selected_key = u32(payload, cursor)
                cursor += 4
                enabled = payload[cursor]
                cursor += 1
                _delta_state = u32(payload, cursor)
                cursor += 4
                if version_major >= 23:
                    if payload[cursor] != 0:
                        state = "unavailable"
                        detail = "typed carrier versioned padding is nonzero"
                    cursor += 1
                _history_reference = u32(payload, cursor)
                cursor += 4
                terminator = u32(payload, cursor)
                cursor += 4
                if enabled not in (0, 1):
                    state = "unavailable"
                    detail = "typed carrier enabled flag is invalid"
                elif terminator != FREE or cursor != len(payload):
                    state = "unavailable"
                    detail = "typed carrier footer is not exactly exhausted"
                else:
                    carrier = candidate
        result = {
            "segment_token": token,
            "segment_kind": segment_kind,
            "record_ordinal": record.ordinal,
            "record_payload_offset": record.payload_offset,
            "record_payload_length": len(payload),
            "state": state,
            "family": family,
            "active": False,
            "carrier": {
                "offset": record.payload_offset + 14,
                "length": len(carrier),
                "sha256": digest(carrier),
            },
            "carrier_bytes": carrier,
            "record_sha256": digest(payload),
            "detail": detail,
        }
        return result

    def envelope(self) -> dict:
        schemas = sorted({database["schema"] for database in self.databases})
        meta_versions = sorted({meta.version for meta in self.metas.values()})
        bulk_framing = sorted({"prefix16_form16_exact_zlib" for _bulk in self.bulks.values()})
        kinds = sorted({segment.get("kind", "unknown") for segment in self.segments})
        document_kind = (
            "part"
            if any(kind.startswith("pm_") for kind in kinds)
            and not any(kind.startswith("am_") for kind in kinds)
            else "assembly"
            if any(kind.startswith("am_") for kind in kinds)
            and not any(kind.startswith("pm_") for kind in kinds)
            else "unknown"
        )
        kernel_bands = []
        for carrier in self.carriers:
            carrier_band = None
            raw = carrier.get("carrier_bytes", b"")
            if raw.startswith(b"ASM BinaryFile") and len(raw) >= 19:
                carrier_band = int.from_bytes(raw[15:19], "little")
            elif raw.startswith(b"ACIS BinaryFile") and len(raw) >= 19:
                carrier_band = int.from_bytes(raw[15:19], "little")
            kernel_bands.append(
                {
                    "family": carrier.get("family"),
                    "save_format": carrier_band,
                    "state": carrier.get("state"),
                }
            )
        return {
            "document_kind": document_kind,
            "cfb_major": self.cfb.major,
            "sector_size": self.cfb.sector_size,
            "rse_schema": schemas,
            "meta_stream_versions": meta_versions,
            "bulk_framing": bulk_framing,
            "segment_kinds": kinds,
            "kernel_bands": kernel_bands,
        }

    def as_json(self) -> dict:
        def strip_bytes(value):
            if isinstance(value, bytes):
                return None
            if isinstance(value, dict):
                return {key: strip_bytes(item) for key, item in value.items() if key != "carrier_bytes"}
            if isinstance(value, list):
                return [strip_bytes(item) for item in value]
            return value

        return {
            "ordinal": self.ordinal,
            "status": "ok" if not self.errors else "partial",
            "envelope": self.envelope(),
            "storage_bands": sorted({database["band"] for database in self.databases}),
            "databases": self.databases,
            "segments": strip_bytes(self.segments),
            "carriers": strip_bytes(self.carriers),
            "active_carrier": {
                "state": self.active_carrier_state,
                "index": self.active_carrier_index,
                "detail": self.active_carrier_detail,
            },
            "errors": self.errors,
        }


def locate_document(source: bytes, ordinal: int, max_stream_bytes: int) -> DocumentEvidence:
    cfb = CompoundReader(source, max_stream_bytes).parse()
    root = "RSeStorage"
    if not any(
        entry.path.casefold() == root.casefold()
        for entry in cfb.directory
        if entry.object_type == 1
    ):
        raise EvidenceError("no RSeStorage storage")
    return DocumentEvidence(ordinal, cfb, root).parse()


def iter_files(root: Path) -> Iterator[Path]:
    if root.is_file():
        yield root
        return
    for directory, _dirnames, filenames in os.walk(root):
        for name in sorted(filenames):
            path = Path(directory) / name
            if path.is_file():
                yield path


def normalize_strings(value, replacements: Sequence[tuple[str, str]]):
    if isinstance(value, str):
        for old, new in replacements:
            value = value.replace(old, new)
        return value
    if isinstance(value, list):
        return [normalize_strings(item, replacements) for item in value]
    if isinstance(value, dict):
        return {key: normalize_strings(item, replacements) for key, item in value.items()}
    return value


def load_json_command(
    command: Sequence[str], timeout: float
) -> tuple[int, object | None]:
    status, stdout = run_capture(command, timeout)
    try:
        return status, json.loads(stdout)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return status, None


def run_capture(command: Sequence[str], timeout: float) -> tuple[int, str]:
    try:
        result = subprocess.run(
            list(command),
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=timeout,
            check=False,
            text=True,
        )
    except subprocess.TimeoutExpired:
        return 124, ""
    except OSError:
        return 127, ""
    return result.returncode, result.stdout


def cli_sweep(
    source_path: Path, ordinal: int, cadmpeg: Path, timeout: float
) -> dict:
    """Run the local inspect/dump/check matrix for one document."""

    runs = []
    with tempfile.TemporaryDirectory(prefix="inventor-sweep-") as directory:
        temporary = Path(directory)
        for profile in ("desktop", "service"):
            for strict in (False, True):
                strict_flag = ("--no-salvage",) if strict else ()
                prefix = f"{profile}-{'strict' if strict else 'salvage'}"
                inspect_command = [
                    str(cadmpeg),
                    "inspect",
                    "--input-format",
                    "inventor",
                    "--limits",
                    profile,
                    "--json",
                    str(source_path),
                ]
                inspect_status, inspect_stdout = run_capture(inspect_command, timeout)
                decode_outputs = []
                validate_outputs = []
                for repeat in (0, 1):
                    decode_path = temporary / f"{prefix}-decode-{repeat}.json"
                    decode_command = [
                        str(cadmpeg),
                        "dump",
                        "--input-format",
                        "inventor",
                        "--limits",
                        profile,
                        *strict_flag,
                        "--output",
                        str(decode_path),
                        "--force",
                        str(source_path),
                    ]
                    decode_status, _decode_stdout = run_capture(decode_command, timeout)
                    decode_outputs.append(
                        {
                            "status": decode_status,
                            "sha256": digest(decode_path.read_bytes()) if decode_path.exists() else None,
                        }
                    )
                    report_path = temporary / f"{prefix}-validate-{repeat}.json"
                    validate_command = [
                        str(cadmpeg),
                        "check",
                        "--input-format",
                        "inventor",
                        "--limits",
                        profile,
                        *strict_flag,
                        "--json",
                        "--report",
                        str(report_path),
                        "--force",
                        str(source_path),
                    ]
                    validate_status, validate_stdout = run_capture(validate_command, timeout)
                    validate_outputs.append(
                        {
                            "status": validate_status,
                            "sha256": digest(validate_stdout.encode()),
                        }
                    )
                runs.append(
                    {
                        "profile": profile,
                        "mode": "strict" if strict else "salvage",
                        "inspect": {
                            "status": inspect_status,
                            "sha256": digest(inspect_stdout.encode()),
                        },
                        "decode": decode_outputs,
                        "validate": validate_outputs,
                        "decode_deterministic": decode_outputs[0] == decode_outputs[1],
                        "validate_deterministic": validate_outputs[0] == validate_outputs[1],
                    }
                )
    return {"source_ordinal": ordinal, "runs": runs}


def compare_carriers(
    source_path: Path,
    evidence: DocumentEvidence,
    cadmpeg: Path,
    timeout: float,
) -> dict:
    geometry_keys = (
        "bodies",
        "regions",
        "shells",
        "faces",
        "loops",
        "coedges",
        "edges",
        "vertices",
        "points",
        "surfaces",
        "curves",
        "subds",
        "pcurves",
        "procedural_surfaces",
        "procedural_curves",
        "attributes",
    )
    comparisons = []
    active = [
        (index, carrier)
        for index, carrier in enumerate(evidence.carriers)
        if carrier.get("active")
    ]
    if not active:
        return {
            "source_ordinal": evidence.ordinal,
            "comparisons": [
                {
                    "state": evidence.active_carrier_state,
                    "detail": evidence.active_carrier_detail,
                }
            ],
        }
    with tempfile.TemporaryDirectory(prefix="inventor-evidence-") as directory:
        temporary = Path(directory)
        for carrier_index, carrier in active:
            carrier_bytes = carrier.get("carrier_bytes", b"")
            if carrier.get("state") != "framed" or not carrier_bytes:
                comparisons.append(
                    {"carrier_index": carrier_index, "state": "skipped", "detail": carrier.get("detail")}
                )
                continue
            carrier_path = temporary / f"carrier-{carrier_index}.sab"
            wrapper_cadir = temporary / f"wrapper-{carrier_index}.json"
            direct_cadir = temporary / f"direct-{carrier_index}.json"
            wrapper_report = temporary / f"wrapper-{carrier_index}-validation.json"
            direct_report = temporary / f"direct-{carrier_index}-validation.json"
            carrier_path.write_bytes(carrier_bytes)
            wrapper_status, _wrapper_json = load_json_command(
                [
                    str(cadmpeg),
                    "dump",
                    "--input-format",
                    "inventor",
                    "--output",
                    str(wrapper_cadir),
                    "--force",
                    str(source_path),
                ],
                timeout,
            )
            direct_status, _direct_json = load_json_command(
                [
                    str(cadmpeg),
                    "dump",
                    "--input-format",
                    "sat",
                    "--output",
                    str(direct_cadir),
                    "--force",
                    str(carrier_path),
                ],
                timeout,
            )
            wrapper_validation_status, wrapper_validation = load_json_command(
                [
                    str(cadmpeg),
                    "check",
                    "--input-format",
                    "inventor",
                    "--json",
                    "--report",
                    str(wrapper_report),
                    "--force",
                    str(source_path),
                ],
                timeout,
            )
            direct_validation_status, direct_validation = load_json_command(
                [
                    str(cadmpeg),
                    "check",
                    "--input-format",
                    "sat",
                    "--json",
                    "--report",
                    str(direct_report),
                    "--force",
                    str(carrier_path),
                ],
                timeout,
            )

            semantic_equal = None
            if wrapper_cadir.exists() and direct_cadir.exists():
                try:
                    wrapper_model = json.loads(wrapper_cadir.read_text())
                    direct_model = json.loads(direct_cadir.read_text())
                    wrapper_geometry = normalize_geometry(
                        {
                            key: wrapper_model.get("model", {}).get(key, [])
                            for key in geometry_keys
                        }
                    )
                    direct_geometry = normalize_geometry(
                        {
                            key: direct_model.get("model", {}).get(key, [])
                            for key in geometry_keys
                        }
                    )
                    semantic_equal = normalize_strings(
                        wrapper_geometry,
                        (("inventor:", "kernel:"), ("sat:", "kernel:")),
                    ) == normalize_strings(
                        direct_geometry,
                        (("inventor:", "kernel:"), ("sat:", "kernel:")),
                    )
                except (OSError, json.JSONDecodeError):
                    semantic_equal = False

            def findings(value):
                if not isinstance(value, dict):
                    return None
                report = value.get("check_report")
                if not isinstance(report, dict):
                    return None
                return normalize_strings(
                    report.get("findings", []),
                    (("inventor:", "kernel:"), ("sat:", "kernel:")),
                )

            wrapper_findings = findings(wrapper_validation)
            direct_findings = findings(direct_validation)
            validation_equal = (
                wrapper_findings is not None
                and direct_findings is not None
                and wrapper_findings == direct_findings
            )
            comparisons.append(
                {
                    "carrier_index": carrier_index,
                    "state": "compared",
                    "oracle_sha256": carrier["carrier"]["sha256"],
                    "semantic_scope": "shared_geometry_arenas",
                    "wrapper_decode_status": wrapper_status,
                    "direct_decode_status": direct_status,
                    "wrapper_validation_status": wrapper_validation_status,
                    "direct_validation_status": direct_validation_status,
                    "semantic_model_equal": semantic_equal,
                    "validation_findings_equal": validation_equal,
                }
            )
    return {"source_ordinal": evidence.ordinal, "comparisons": comparisons}


def normalize_geometry(value):
    if not isinstance(value, dict):
        return value
    normalized = dict(value)
    faces = normalized.get("faces")
    if isinstance(faces, list):
        normalized["faces"] = [
            {key: item for key, item in face.items() if key != "color"}
            if isinstance(face, dict)
            else face
            for face in faces
        ]
    return normalized


def process(args: argparse.Namespace) -> dict:
    paths = list(iter_files(Path(args.root)))
    documents = []
    failures = []
    parity = []
    sweeps = []
    ordinal = 0
    for path in paths:
        try:
            source = path.read_bytes()
        except OSError:
            continue
        try:
            evidence = locate_document(source, ordinal, args.max_stream_bytes)
        except EvidenceError as error:
            ordinal += 1
            if str(error) != "not a CFB file":
                failures.append({"ordinal": ordinal - 1, "status": "rejected", "reason": str(error)})
            continue
        ordinal += 1
        if args.compare:
            try:
                parity.append(compare_carriers(path, evidence, Path(args.cadmpeg), args.timeout))
            except (OSError, subprocess.SubprocessError):
                parity.append(
                    {
                        "source_ordinal": evidence.ordinal,
                        "state": "error",
                        "detail": "temporary comparison operation failed",
                    }
                )
        if args.sweep:
            sweeps.append(cli_sweep(path, evidence.ordinal, Path(args.cadmpeg), args.timeout))
        documents.append(evidence.as_json())
    envelope_counts: dict[str, int] = {}
    for document in documents:
        key = json.dumps(document["envelope"], sort_keys=True, separators=(",", ":"))
        envelope_counts[key] = envelope_counts.get(key, 0) + 1
    result = {
        "schema_version": 1,
        "tool": "inventor-evidence",
        "input_count": len(paths),
        "cfb_or_rse_rejection_count": len(failures),
        "document_count": len(documents),
        "envelopes": [
            {"count": count, "envelope": json.loads(key)}
            for key, count in sorted(envelope_counts.items())
        ],
        "documents": documents,
        "rejections": failures,
    }
    if args.compare:
        result["carrier_parity"] = parity
    if args.sweep:
        result["cli_sweep"] = sweeps
    return result


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="file or directory to scan")
    parser.add_argument(
        "--max-stream-bytes",
        type=int,
        default=512 * 1024 * 1024,
        help="maximum logical stream size admitted by the local reader",
    )
    parser.add_argument(
        "--compare",
        action="store_true",
        help="run wrapper/direct-carrier semantic and validation comparisons",
    )
    parser.add_argument(
        "--sweep",
        action="store_true",
        help="run inspect/dump/check in desktop/service and salvage/strict modes",
    )
    parser.add_argument(
        "--cadmpeg",
        default="target/debug/cadmpeg",
        help="cadmpeg executable used by --compare",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=60.0,
        help="per-command timeout in seconds for --compare",
    )
    parser.add_argument("--output", help="write JSON to this path instead of stdout")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if args.max_stream_bytes <= 0 or args.timeout <= 0:
        print("limits must be positive", file=sys.stderr)
        return 2
    try:
        result = process(args)
        encoded = json.dumps(result, indent=2, sort_keys=True) + "\n"
        if args.output:
            output = Path(args.output)
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(encoded)
        else:
            sys.stdout.write(encoded)
        return 0
    except (OSError, EvidenceError, ValueError) as error:
        print(f"inventor evidence failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
