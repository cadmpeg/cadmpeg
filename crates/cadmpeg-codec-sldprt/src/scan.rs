// SPDX-License-Identifier: Apache-2.0
//! Selective `SolidWorks` history-stream census.

mod xml;

use cadmpeg_container::compound::scan::{ScanCompound, ScanCompoundEntry};
use cadmpeg_core::decode::{index_from_u32, u64_from_index, View};
use cadmpeg_core::scan::{
    DesignScan, InflateFraming, ScanBudget, ScanClock, ScanLimits, ScanReader, StopReason,
};

use crate::layout::{
    block_frame_header as block, outer_header as outer, zlb_wrapper_header as zlb,
};

fn malformed(what: &str) -> StopReason {
    StopReason::Malformed(what.into())
}

fn history_name(name: &str) -> bool {
    name.as_bytes()
        .windows(b"keywords".len())
        .any(|window| window.eq_ignore_ascii_case(b"keywords"))
}

fn wrapped(
    bytes: &[u8],
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
) -> Result<bool, StopReason> {
    let mut source = budget.view(bytes)?;
    if bytes.starts_with(&zlb::MAGIC_VALUE) {
        source.req_take(zlb::UNCOMPRESSED_SIZE)?;
        let declared = source.req_u32_le()?;
        let size = index_from_u32(source.req_u32_le()?);
        scan.declared_size(budget, "SolidWorks history XML", u64::from(declared))?;
        let member = source.req_take_child(size)?;
        let (expanded, consumed) = budget.inflate(member, InflateFraming::Zlib)?;
        if consumed != size || u64_from_index(expanded.len()) != u64::from(declared) {
            return Err(malformed("SolidWorks history wrapper extent"));
        }
        return xml::census(budget.view(&expanded)?, budget, scan);
    }
    xml::census(source, budget, scan)
}

fn compound(
    source: View<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
) -> Result<bool, StopReason> {
    let compound = ScanCompound::new(source, budget)?;
    let mut found = false;
    for entry in compound.entries() {
        budget.check()?;
        if !history_name(&entry.path) {
            continue;
        }
        scan.declared_size(budget, &entry.path, entry.declared_size)?;
        let bytes = compound.open(entry)?;
        found |= wrapped(&bytes, budget, scan)?;
    }
    Ok(found)
}

fn native(
    source: View<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
) -> Result<bool, StopReason> {
    budget.depth(1)?;
    let mut reader = ScanReader::source(source, budget);
    let header = reader.take(outer::LEN)?;
    let version = View::u32_be_at(header.window(), outer::VERSION)
        .ok_or_else(|| StopReason::Truncated("SolidWorks version".into()))?;
    budget.retain(32)?;
    scan.source_version = Some(version.to_string());
    let mut found = false;
    while reader.remaining() >= block::MARKER_VALUE.len() {
        budget.check()?;
        let at = reader.position();
        if reader.take(block::MARKER_VALUE.len())?.window() != block::MARKER_VALUE {
            reader.seek(
                at.checked_add(1)
                    .ok_or_else(|| malformed("SolidWorks marker position"))?,
            )?;
            continue;
        }
        reader.seek(at)?;
        let header = reader.take(block::LEN)?;
        let bytes = header.window();
        let compressed = View::u32_le_at(bytes, block::COMP_SZ)
            .ok_or_else(|| malformed("SolidWorks compressed size"))?;
        let declared = View::u32_le_at(bytes, block::UNCOMP_SZ)
            .ok_or_else(|| malformed("SolidWorks expanded size"))?;
        let name_size = index_from_u32(
            View::u32_le_at(bytes, block::PRE_SZ)
                .ok_or_else(|| malformed("SolidWorks name size"))?,
        );
        let end = reader
            .position()
            .checked_add(name_size)
            .and_then(|at| at.checked_add(index_from_u32(compressed)))
            .ok_or_else(|| malformed("SolidWorks block extent"))?;
        if end > source.end() {
            return Err(StopReason::Truncated(
                "SolidWorks history block extent".into(),
            ));
        }
        if compressed == 0 {
            reader.seek(
                at.checked_add(1)
                    .ok_or_else(|| malformed("SolidWorks marker position"))?,
            )?;
            continue;
        }
        let name = reader.take(name_size)?;
        budget.retain(u64_from_index(name_size))?;
        let mut decoded = String::new();
        for byte in name.window() {
            budget.check()?;
            decoded.push(char::from(byte.rotate_left(4)));
        }
        if !history_name(&decoded) {
            reader.seek(end)?;
            continue;
        }
        scan.declared_size(budget, &decoded, u64::from(declared))?;
        let compressed = reader.take(index_from_u32(compressed))?;
        let (expanded, consumed) = budget.inflate(compressed, InflateFraming::Raw)?;
        let crc = View::u32_le_at(bytes, block::CRC32)
            .ok_or_else(|| malformed("SolidWorks block checksum"))?;
        if consumed != compressed.window().len()
            || expanded.len() != index_from_u32(declared)
            || crc32fast::hash(&expanded) != crc
        {
            return Err(malformed("SolidWorks history block size or checksum"));
        }
        found |= wrapped(&expanded, budget, scan)?;
    }
    Ok(found)
}

impl crate::SldprtCodec {
    /// Recognizes structurally reached compound stream names for this reader.
    pub fn scan_detect_compound(
        entries: &[ScanCompoundEntry],
        budget: &ScanBudget<'_>,
    ) -> Result<bool, StopReason> {
        for entry in entries {
            budget.check()?;
            if entry
                .path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("ISolidWorksInformation"))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Scans the `SolidWorks` feature listing without opening geometry streams.
    pub fn scan_design(input: View<'_>, limits: ScanLimits, cancel: &dyn ScanClock) -> DesignScan {
        let budget = ScanBudget::new(limits, cancel);
        Self::scan_design_with_budget(input, &budget)
    }

    /// Scans with a meter shared by detection and nested readers.
    pub fn scan_design_with_budget(input: View<'_>, budget: &ScanBudget<'_>) -> DesignScan {
        let mut scan = DesignScan::new(Some(crate::dialect::FORMAT));
        let result = if input
            .window()
            .starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])
        {
            compound(input, budget, &mut scan)
        } else {
            native(input, budget, &mut scan)
        };
        let result = result.and_then(|found| {
            if found {
                scan.complete();
                Ok(())
            } else {
                Err(StopReason::Unsupported(
                    "SolidWorks Keywords history is absent".into(),
                ))
            }
        });
        scan.finish(budget, result);
        scan
    }
}
