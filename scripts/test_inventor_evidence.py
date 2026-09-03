#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Synthetic regression tests for ``inventor-evidence.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path
import struct
import unittest
import zlib


SCRIPT = Path(__file__).with_name("inventor-evidence.py")
SPEC = importlib.util.spec_from_file_location("inventor_evidence", SCRIPT)
assert SPEC and SPEC.loader
evidence = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = evidence
SPEC.loader.exec_module(evidence)


def put16(data: bytearray, offset: int, value: int) -> None:
    data[offset : offset + 2] = value.to_bytes(2, "little")


def put32(data: bytearray, offset: int, value: int) -> None:
    data[offset : offset + 4] = value.to_bytes(4, "little")


def push32(data: bytearray, value: int) -> None:
    data.extend(value.to_bytes(4, "little"))


def counted_utf8(data: bytearray, value: bytes) -> None:
    push32(data, len(value))
    data.extend(value)


def counted_utf16(data: bytearray, value: str) -> None:
    encoded = value.encode("utf-16-le")
    push32(data, len(encoded) // 2)
    data.extend(encoded)


def version(data: bytearray, major: int) -> None:
    data.extend(bytes((1, 2, major, 4, 5, 6, 7, 8)))


def counted_section(data: bytearray, values: list[bytes], item_size: int) -> None:
    push32(data, len(values))
    for value in values:
        if len(value) != item_size:
            raise AssertionError("field builder item has the wrong size")
        data.extend(value)
    push32(data, 4 + len(values) * item_size)


def metadata_body(payload_length: int, short_section7: bool = False) -> bytes:
    data = bytearray()
    for value in (3, 0, 2, 1, 0, 4, 0):
        put16(data, len(data), value)
    counted_section(
        data,
        [(0x8000_0000 | payload_length).to_bytes(4, "little")],
        4,
    )
    counted_section(data, [], 10)
    counted_section(data, [], 28)
    type_id = evidence.KERNEL_RECORD_TYPE_ID
    counted_section(data, [type_id + (1).to_bytes(2, "little") + (2).to_bytes(4, "little") + (3).to_bytes(2, "little") + (4).to_bytes(4, "little")], 28)
    payloads = (0, 0, 32 if short_section7 else 0, 0, 0, 0, 0x48)
    discriminators = (evidence.FREE, 0, 1 if short_section7 else 0, 0, 0, 0, 18)
    push32(data, discriminators[0])
    for index in range(1, len(payloads)):
        push32(data, payloads[index - 1] + 4)
        push32(data, discriminators[index])
        data.extend(bytes(payloads[index]))
    data.extend(bytes(16))
    return bytes(data)


def carrier() -> bytes:
    kernel = bytearray(b"ASM BinaryFile4")
    kernel.extend(struct.pack("<I", 700))
    kernel.extend(bytes(12))
    for value in (b"Inventor", b"synthetic ASM", b"2000-01-01"):
        kernel.extend((0x07, len(value)))
        kernel.extend(value)
    for value in (1.0, 1.0e-6, 1.0e-10):
        kernel.append(0x06)
        kernel.extend(struct.pack("<d", value))
    payload = bytearray()
    payload.extend((1).to_bytes(4, "little"))
    payload.extend((2).to_bytes(2, "little"))
    payload.extend((3).to_bytes(4, "little"))
    payload.extend((4).to_bytes(4, "little"))
    payload.extend(kernel)
    payload.extend((5).to_bytes(4, "little"))
    payload.append(1)
    payload.extend((-1).to_bytes(4, "little", signed=True))
    payload.extend((6).to_bytes(4, "little"))
    payload.extend(evidence.FREE.to_bytes(4, "little"))
    return bytes(payload)


def database() -> bytes:
    data = bytearray(bytes(16))
    push32(data, 31)
    version(data, 24)
    data.extend((17).to_bytes(8, "little"))
    version(data, 25)
    data.extend((18).to_bytes(8, "little"))
    counted_utf16(data, "synthetic primary document")
    return bytes(data)


def registry() -> bytes:
    data = bytearray()
    push32(data, 1)
    counted_utf16(data, "PmBRepSegment")
    data.extend(bytes.fromhex("5a" * 16))
    data.extend(bytes.fromhex("20" * 16))
    push32(data, 3)
    push32(data, 1)
    for value in range(4, 9):
        push32(data, value)
    push32(data, 9)
    counted_utf16(data, "PmBrepSegmentType")
    push32(data, 10)
    push32(data, 11)
    version(data, 18)
    push32(data, 12)
    data.extend(bytes.fromhex("20" * 16))
    data.extend(bytes.fromhex("30" * 9))
    data.extend(bytes.fromhex("5a" * 16))
    push32(data, 13)
    push32(data, 2)
    push32(data, 14)
    data.extend((-1).to_bytes(2, "little", signed=True))
    data.extend((2).to_bytes(2, "little", signed=True))
    for value in range(15, 21):
        data.extend(value.to_bytes(2, "little"))
    data.extend((21).to_bytes(2, "little"))
    data.extend((22).to_bytes(2, "little"))
    data.extend((23).to_bytes(2, "little"))
    push32(data, 1)
    data.extend(bytes.fromhex("61" * 16))
    push32(data, 0)
    return bytes(data)


def metadata(payload_length: int) -> bytes:
    data = bytearray()
    counted_utf8(data, b"RSe Meta Stream Version 8")
    data.extend((8).to_bytes(2, "little"))
    for value in (1, 2, 3, 4, 5, 6, 7, 8):
        data.extend(value.to_bytes(2, "little"))
    counted_utf16(data, "PmBRepSegment")
    data.extend(bytes.fromhex("5a" * 16))
    for value in (5, 6, 7):
        push32(data, value)
    counted_utf8(data, b"created")
    counted_utf8(data, b"modified")
    data.append(1)
    data.extend(zlib.compress(metadata_body(payload_length)))
    return bytes(data)


def bulk(payload: bytes, stream_trailer: bytes = b"") -> bytes:
    expanded = bytearray()
    push32(expanded, 0)
    expanded.extend(payload)
    push32(expanded, len(payload))
    push32(expanded, evidence.FREE)
    expanded.extend(stream_trailer)
    return bytes(bytes(16) + (0x0104).to_bytes(2, "little") + zlib.compress(expanded))


def directory_entry(
    data: bytearray,
    index: int,
    name: str,
    object_type: int,
    left: int,
    right: int,
    child: int,
    start: int,
    size: int,
) -> None:
    entry = memoryview(data)[index * 128 : (index + 1) * 128]
    encoded = name.encode("utf-16-le")
    entry[: len(encoded)] = encoded
    entry[64:66] = ((len(encoded) + 2).to_bytes(2, "little"))
    entry[66] = object_type
    entry[67] = 1
    entry[68:72] = left.to_bytes(4, "little")
    entry[72:76] = right.to_bytes(4, "little")
    entry[76:80] = child.to_bytes(4, "little")
    entry[116:120] = start.to_bytes(4, "little")
    entry[120:128] = size.to_bytes(8, "little")


def synthetic_primary() -> bytes:
    streams = {
        "registry": registry(),
        "database": database(),
    }
    carrier_value = carrier()
    streams["meta"] = metadata(len(carrier_value))
    streams["bulk"] = bulk(carrier_value, b"\x07\x08")
    mini_stream = bytearray()
    mini_fat = [evidence.FREE] * 128
    allocation: dict[str, tuple[int, int]] = {}
    for name, value in streams.items():
        start = len(mini_stream) // 64
        count = (len(value) + 63) // 64
        for ordinal in range(count):
            chunk = value[ordinal * 64 : (ordinal + 1) * 64]
            mini_stream.extend(chunk)
            mini_stream.extend(bytes(64 - len(chunk)))
            mini_fat[start + ordinal] = evidence.EOC if ordinal + 1 == count else start + ordinal + 1
        allocation[name] = (start, len(value))
    root_mini_sectors = (len(mini_stream) + 511) // 512
    mini_stream.extend(bytes(root_mini_sectors * 512 - len(mini_stream)))
    directory_sectors = 2
    root_mini_start = directory_sectors
    mini_fat_sector = root_mini_start + root_mini_sectors
    fat_sector = mini_fat_sector + 1
    sector_count = fat_sector + 1
    file = bytearray((sector_count + 1) * 512)
    file[:8] = evidence.MAGIC
    put16(file, 24, 0x003E)
    put16(file, 26, 3)
    put16(file, 28, 0xFFFE)
    put16(file, 30, 9)
    put16(file, 32, 6)
    put32(file, 40, 0)
    put32(file, 44, 1)
    put32(file, 48, 0)
    put32(file, 56, 4096)
    put32(file, 60, mini_fat_sector)
    put32(file, 64, 1)
    put32(file, 68, evidence.EOC)
    put32(file, 72, 0)
    for offset in range(76, 512, 4):
        put32(file, offset, evidence.FREE)
    put32(file, 76, fat_sector)

    directory = bytearray(directory_sectors * 512)
    for offset in range(0, len(directory), 128):
        directory[offset + 68 : offset + 80] = bytes([0xFF]) * 12
    directory_entry(directory, 0, "Root Entry", 5, evidence.FREE, evidence.FREE, 1, root_mini_start, len(mini_stream))
    directory_entry(directory, 1, "RSeStorage", 1, evidence.FREE, evidence.FREE, 2, evidence.EOC, 0)
    directory_entry(directory, 2, "V1", 1, evidence.FREE, 5, 4, evidence.EOC, 0)
    directory_entry(directory, 3, "RSeSegInfo", 2, evidence.FREE, evidence.FREE, evidence.FREE, allocation["registry"][0], allocation["registry"][1])
    directory_entry(directory, 4, "RSeDb", 2, evidence.FREE, evidence.FREE, evidence.FREE, allocation["database"][0], allocation["database"][1])
    directory_entry(directory, 5, "Bseg", 2, evidence.FREE, 6, evidence.FREE, allocation["bulk"][0], allocation["bulk"][1])
    directory_entry(directory, 6, "Mseg", 2, evidence.FREE, 3, evidence.FREE, allocation["meta"][0], allocation["meta"][1])
    file[512 : 512 + len(directory)] = directory

    fat = [evidence.FREE] * 128
    for sector in range(directory_sectors):
        fat[sector] = evidence.EOC if sector + 1 == directory_sectors else sector + 1
    for sector in range(root_mini_sectors):
        physical = root_mini_start + sector
        fat[physical] = evidence.EOC if sector + 1 == root_mini_sectors else physical + 1
    fat[mini_fat_sector] = evidence.EOC
    fat[fat_sector] = evidence.FAT_SECTOR
    mini_offset = (root_mini_start + 1) * 512
    file[mini_offset : mini_offset + len(mini_stream)] = mini_stream
    for index, value in enumerate(mini_fat):
        put32(file, (mini_fat_sector + 1) * 512 + index * 4, value)
    for index, value in enumerate(fat):
        put32(file, (fat_sector + 1) * 512 + index * 4, value)
    return bytes(file)


def synthetic_v4_regular() -> bytes:
    sector_size = 4096
    header_size = sector_size
    directory_sector = 0
    stream_sector = 1
    fat_sector = 2
    file = bytearray(header_size + (fat_sector + 1) * sector_size)
    file[:8] = evidence.MAGIC
    put16(file, 24, 0x003E)
    put16(file, 26, 4)
    put16(file, 28, 0xFFFE)
    put16(file, 30, 12)
    put16(file, 32, 6)
    put32(file, 40, 1)
    put32(file, 44, 1)
    put32(file, 48, directory_sector)
    put32(file, 56, 4096)
    put32(file, 60, evidence.EOC)
    put32(file, 64, 0)
    put32(file, 68, evidence.EOC)
    put32(file, 72, 0)
    for offset in range(76, 512, 4):
        put32(file, offset, evidence.FREE)
    put32(file, 76, fat_sector)
    directory = bytearray(sector_size)
    for offset in range(0, len(directory), 128):
        directory[offset + 68 : offset + 80] = bytes([0xFF]) * 12
    directory_entry(directory, 0, "Root Entry", 5, evidence.FREE, evidence.FREE, 1, evidence.EOC, 0)
    directory_entry(directory, 1, "Wide", 2, evidence.FREE, evidence.FREE, evidence.FREE, stream_sector, sector_size)
    file[header_size : header_size + sector_size] = directory
    file[
        header_size + stream_sector * sector_size : header_size + (stream_sector + 1) * sector_size
    ] = bytes([0x6D]) * sector_size
    fat = [evidence.FREE] * (sector_size // 4)
    fat[directory_sector] = evidence.EOC
    fat[stream_sector] = evidence.EOC
    fat[fat_sector] = evidence.FAT_SECTOR
    for index, value in enumerate(fat):
        put32(file, header_size + fat_sector * sector_size + index * 4, value)
    return bytes(file)


class EvidenceTest(unittest.TestCase):
    def test_active_carrier_selection_is_scoped_to_pm_brep(self) -> None:
        reader = evidence.CompoundReader(b"", 1 << 20)
        reader.major = 3
        reader.sector_size = 512
        document = evidence.DocumentEvidence(
            0,
            reader,
            "RSeStorage",
            segments=[{"kind": "pm_dc"}, {"kind": "pm_brep"}],
            carriers=[
                {"segment_kind": "pm_dc", "state": "framed", "active": False},
                {"segment_kind": "pm_brep", "state": "framed", "active": False},
            ],
        )

        document._select_active_carrier()

        self.assertEqual(document.active_carrier_state, "selected")
        self.assertEqual(document.active_carrier_index, 1)
        self.assertFalse(document.carriers[0]["active"])
        self.assertTrue(document.carriers[1]["active"])

    def test_geometry_comparison_ignores_face_color_only(self) -> None:
        geometry = {
            "faces": [{"id": "face", "color": {"r": 1.0}}],
            "attributes": [{"id": "attribute", "color": {"r": 0.5}}],
        }

        normalized = evidence.normalize_geometry(geometry)

        self.assertEqual(normalized["faces"], [{"id": "face"}])
        self.assertEqual(normalized["attributes"], geometry["attributes"])

    def test_metadata_section7_short_form_is_framed(self) -> None:
        blocks, type_ids = evidence.parse_meta_body(metadata_body(1, short_section7=True))
        self.assertEqual(blocks, [(True, 1)])
        self.assertEqual(type_ids, [evidence.KERNEL_RECORD_TYPE_ID])

    def test_v4_uses_the_full_directory_stream_size(self) -> None:
        reader = evidence.CompoundReader(synthetic_v4_regular(), 1 << 20).parse()
        stream = reader.stream("Wide")
        self.assertIsNotNone(stream)
        assert stream is not None
        self.assertEqual(stream.allocation, "regular")
        self.assertEqual(stream.size, 4096)
        self.assertEqual(stream.data, bytes([0x6D]) * 4096)

    def test_primary_rse_envelope_records_exact_ranges(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "synthetic.ipt"
            path.write_bytes(synthetic_primary())
            evidence_result = evidence.locate_document(path.read_bytes(), 0, 1 << 20)
        self.assertEqual(evidence_result.envelope()["cfb_major"], 3)
        self.assertEqual(evidence_result.envelope()["document_kind"], "part")
        self.assertEqual(evidence_result.envelope()["rse_schema"], [31])
        self.assertEqual(evidence_result.envelope()["meta_stream_versions"], [8])
        self.assertEqual(len(evidence_result.carriers), 1)
        carrier = evidence_result.carriers[0]
        self.assertEqual(carrier["family"], "asm")
        self.assertEqual(carrier["state"], "framed")
        self.assertTrue(carrier["active"])
        self.assertEqual(evidence_result.as_json()["active_carrier"]["state"], "selected")
        self.assertEqual(carrier["carrier"]["length"], len(carrier["carrier_bytes"]))
        self.assertEqual(len(evidence_result.bulks["seg"].records), 1)
        self.assertEqual(
            evidence_result.bulks["seg"].expanded[
                evidence_result.bulks["seg"].stream_trailer_start :
            ],
            bytes.fromhex("ffffffff0708"),
        )

    def test_primary_carrier_parity_when_cli_is_built(self) -> None:
        cadmpeg = Path("target/debug/cadmpeg")
        if not cadmpeg.is_file():
            self.skipTest("the cadmpeg binary is not built")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "synthetic.ipt"
            path.write_bytes(synthetic_primary())
            document = evidence.locate_document(path.read_bytes(), 0, 1 << 20)
            result = evidence.compare_carriers(path, document, cadmpeg, 30)
        comparison = result["comparisons"][0]
        self.assertEqual(comparison["state"], "compared")
        self.assertEqual(comparison["wrapper_decode_status"], 0)
        self.assertEqual(comparison["direct_decode_status"], 0)
        self.assertEqual(comparison["wrapper_validation_status"], 0)
        self.assertEqual(comparison["direct_validation_status"], 0)
        self.assertTrue(comparison["semantic_model_equal"])
        self.assertTrue(comparison["validation_findings_equal"])

    def test_cli_sweep_is_per_profile_and_deterministic(self) -> None:
        cadmpeg = Path("target/debug/cadmpeg")
        if not cadmpeg.is_file():
            self.skipTest("the cadmpeg binary is not built")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "synthetic.ipt"
            path.write_bytes(synthetic_primary())
            document = evidence.locate_document(path.read_bytes(), 0, 1 << 20)
            result = evidence.cli_sweep(path, document.ordinal, cadmpeg, 30)
        self.assertEqual(len(result["runs"]), 4)
        self.assertTrue(
            all(
                run["inspect"]["status"] == 0
                and all(
                    item["status"] == (0 if run["mode"] == "salvage" else 1)
                    for item in run["decode"]
                )
                and all(
                    item["status"] == (0 if run["mode"] == "salvage" else 1)
                    for item in run["validate"]
                )
                for run in result["runs"]
            )
        )
        self.assertTrue(all(run["decode_deterministic"] for run in result["runs"]))
        self.assertTrue(all(run["validate_deterministic"] for run in result["runs"]))


if __name__ == "__main__":
    unittest.main()
