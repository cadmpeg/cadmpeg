// SPDX-License-Identifier: Apache-2.0
//! Feature-list census from selected Inventor `PmDc` tables.

use cadmpeg_container::compound::scan::{ScanCompound, ScanCompoundEntry};
use cadmpeg_core::decode::{bounded_len, u64_from_index, View};
use cadmpeg_core::scan::{
    DesignScan, FeatureCensus, InflateFraming, ScanBudget, ScanClock, ScanLimits, ScanReader,
    StopReason, Suppressed,
};

use crate::layout::{
    bulk_envelope as envelope, meta_body_prefix as prefix, meta_type_descriptor as descriptor,
};

fn malformed(what: &str) -> StopReason {
    StopReason::Malformed(what.into())
}
fn count(reader: &mut ScanReader<'_, '_>, width: usize) -> Result<usize, StopReason> {
    let count = reader.u32()?;
    bounded_len(u64::from(count), width, reader.remaining())
        .ok_or_else(|| malformed("Inventor count exceeds its extent"))
}
fn sized(reader: &mut ScanReader<'_, '_>, width: usize) -> Result<(), StopReason> {
    let count = count(reader, width)?;
    reader.take(
        count
            .checked_mul(width)
            .ok_or_else(|| malformed("Inventor sized field length"))?,
    )?;
    Ok(())
}
fn utf16(reader: &mut ScanReader<'_, '_>, budget: &ScanBudget<'_>) -> Result<String, StopReason> {
    let count = count(reader, 2)?;
    budget.retain(
        u64_from_index(count)
            .checked_mul(8)
            .ok_or(StopReason::RetainedBytes)?,
    )?;
    let mut units = Vec::new();
    for _ in 0..count {
        budget.check()?;
        units.push(reader.u16()?);
    }
    String::from_utf16(&units).map_err(|_| malformed("Inventor UTF16 field"))
}
struct RegistryRow {
    id: [u8; 16],
    major: u8,
}
fn registry(bytes: &[u8], budget: &ScanBudget<'_>) -> Result<Vec<RegistryRow>, StopReason> {
    let mut reader = ScanReader::retained(budget.view(bytes)?, budget);
    let rows = count(&mut reader, 76)?;
    let mut found = Vec::new();
    for _ in 0..rows {
        budget.check()?;
        utf16(&mut reader, budget)?;
        let id = reader
            .take(16)?
            .array()
            .ok_or_else(|| malformed("Inventor identifier"))?;
        reader.take(16)?;
        reader.u32()?;
        let objects = count(&mut reader, 49)?;
        reader.take(24)?;
        let kind = utf16(&mut reader, budget)?;
        reader.take(8)?;
        reader.take(2)?;
        let major = reader.u8()?;
        reader.take(5)?;
        reader.u32()?;
        let mut nodes = 0;
        for _ in 0..objects {
            budget.check()?;
            reader.take(45)?;
            nodes = match reader.u32()? {
                0 => 0,
                value => value
                    .checked_sub(1)
                    .ok_or_else(|| malformed("Inventor registry node count"))?,
            };
        }
        let nodes = bounded_len(u64::from(nodes), 22, reader.remaining())
            .ok_or_else(|| malformed("Inventor registry node count"))?;
        for _ in 0..nodes {
            budget.check()?;
            reader.take(22)?;
        }
        if kind == "PmDcSegmentType" {
            budget.retain(32)?;
            found.push(RegistryRow { id, major });
        }
    }
    Ok(found)
}

fn metadata_header(
    source: View<'_>,
    budget: &ScanBudget<'_>,
) -> Result<([u8; 16], usize, u8), StopReason> {
    let mut reader = ScanReader::retained(source, budget);
    sized(&mut reader, 1)?;
    reader.u16()?;
    reader.take(16)?;
    utf16(&mut reader, budget)?;
    let id = reader
        .take(16)?
        .array()
        .ok_or_else(|| malformed("Inventor identifier"))?;
    reader.take(12)?;
    sized(&mut reader, 1)?;
    sized(&mut reader, 1)?;
    let method = reader.u8()?;
    Ok((id, reader.position(), method))
}
fn expand(source: View<'_>, method: u8, budget: &ScanBudget<'_>) -> Result<Vec<u8>, StopReason> {
    let framing = match method {
        1 => InflateFraming::Zlib,
        2 => InflateFraming::Zstd,
        other => {
            return Err(StopReason::Unsupported(format!(
                "Inventor compression method {other}"
            )))
        }
    };
    let (bytes, consumed) = budget.inflate(source, framing)?;
    if consumed != source.window().len() {
        return Err(malformed("Inventor compressed member extent"));
    }
    Ok(bytes)
}
fn section<'a>(reader: &mut ScanReader<'a, '_>, width: usize) -> Result<View<'a>, StopReason> {
    let count = count(reader, width)?;
    let size = count
        .checked_mul(width)
        .ok_or_else(|| malformed("Inventor table length"))?;
    let payload = reader.take(size)?;
    let span = reader.u32()?;
    if u64::from(span)
        != u64_from_index(size)
            .checked_add(4)
            .ok_or_else(|| malformed("Inventor table span"))?
    {
        return Err(malformed("Inventor table span"));
    }
    Ok(payload)
}
fn tables<'a>(
    source: View<'a>,
    budget: &ScanBudget<'_>,
) -> Result<(View<'a>, Vec<[u8; 16]>), StopReason> {
    let mut reader = ScanReader::retained(source, budget);
    reader.take(prefix::LEN)?;
    let blocks = section(&mut reader, 4)?;
    section(&mut reader, 10)?;
    section(&mut reader, 28)?;
    let mut types = section(&mut reader, descriptor::LEN)?;
    let mut ids = Vec::new();
    while types.remaining() != 0 {
        budget.check()?;
        budget.retain(16)?;
        ids.push(
            types
                .array()
                .ok_or_else(|| malformed("Inventor type identifier"))?,
        );
        types.req_take(descriptor::LEN - 16)?;
    }
    Ok((blocks, ids))
}
fn trailer(reader: &mut ScanReader<'_, '_>, budget: &ScanBudget<'_>) -> Result<(), StopReason> {
    match reader.u8()? {
        0 => return Ok(()),
        1 => {}
        _ => return Err(malformed("Inventor record trailer presence")),
    }
    let count = reader.u32()?;
    if count & 0x8000_0000 != 0 {
        return Ok(());
    }
    for _ in 0..count {
        budget.check()?;
        sized(reader, 1)?;
        let width = match reader.u32()? {
            1 => 3,
            3 | 7 => 4,
            8 | 10 => 6,
            11 => 10,
            14 => {
                reader.take(2)?;
                sized(reader, 1)?;
                continue;
            }
            other => {
                return Err(StopReason::Unsupported(format!(
                    "Inventor record trailer type {other}"
                )))
            }
        };
        reader.take(width)?;
    }
    if reader.u16()? != 6 || reader.u16()? != 0x3000 {
        return Err(malformed("Inventor trailer reference list"));
    }
    let count = reader.u32()?;
    if count != 0 {
        reader.take(8)?;
    }
    for _ in 0..count {
        budget.check()?;
        sized(reader, 1)?;
        reader.take(4)?;
    }
    Ok(())
}
fn suppression(
    payload: View<'_>,
    major: u8,
    kind: &str,
    budget: &ScanBudget<'_>,
) -> Result<Suppressed, StopReason> {
    if !crate::pmdc::record_version_supported(major)
        || !matches!(
            kind,
            "feature" | "rectangular_pattern" | "mirror" | "sketch"
        )
    {
        return Ok(Suppressed::Unknown);
    }
    budget.retain(256)?;
    let mut cursor = crate::pmdc::Cursor::new(payload);
    let header = if kind == "sketch" {
        crate::pmdc::content_header(&mut cursor, major)
    } else {
        crate::pmdc::state_content_header(&mut cursor, major)
    };
    Ok(match header {
        Ok(header) if header.flags & crate::feature::history::SUPPRESSED_FLAG != 0 => {
            Suppressed::Yes
        }
        Ok(_) => Suppressed::No,
        Err(_) => Suppressed::Unknown,
    })
}

fn listing(
    metadata: &[u8],
    bulk: &[u8],
    major: u8,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
) -> Result<(), StopReason> {
    let meta = budget.view(metadata)?;
    let (_, at, method) = metadata_header(meta, budget)?;
    let body = expand(
        meta.child(at, meta.end())
            .ok_or_else(|| malformed("Inventor metadata body"))?,
        method,
        budget,
    )?;
    let (mut blocks, ids) = tables(budget.view(&body)?, budget)?;
    let mut member = budget.view(bulk)?;
    member.req_take(envelope::FORM)?;
    let form = member.req_u16_le()?;
    let method =
        u8::try_from(form >> 8).map_err(|_| malformed("Inventor bulk compression method"))?;
    let bytes = expand(member.req_take_child(member.remaining())?, method, budget)?;
    let mut reader = ScanReader::retained(budget.view(&bytes)?, budget);
    scan.census.get_or_insert_with(FeatureCensus::default);
    while blocks.remaining() != 0 {
        budget.check()?;
        let block = blocks.req_u32_le()?;
        if block & 0x8000_0000 == 0 {
            continue;
        }
        let size = usize::try_from(block & 0x7fff_ffff)
            .map_err(|_| malformed("Inventor feature record length"))?;
        let selector = usize::try_from(reader.u32()? & 0xff)
            .map_err(|_| malformed("Inventor record selector"))?;
        let id = ids
            .get(selector)
            .ok_or_else(|| malformed("Inventor record type descriptor"))?;
        let kind = crate::feature::scan_kind(*id);
        if let Some(kind) = kind {
            scan.feature(budget, kind, Suppressed::Unknown)?;
        }
        let payload = reader.take(size)?;
        if let Some(kind) = kind {
            let state = suppression(payload, major, kind, budget)?;
            let row = scan
                .census
                .as_mut()
                .and_then(|census| census.rows.last_mut())
                .ok_or_else(|| malformed("Inventor admitted row"))?;
            row.suppressed = state;
        }
        let length = reader.u32()?;
        if length != 0 && u64::from(length) != u64_from_index(size) {
            return Err(malformed("Inventor record trailing length"));
        }
        if major > 18 {
            trailer(&mut reader, budget)?;
        }
    }
    if reader.u32()? != u32::MAX || reader.remaining() != 0 {
        return Err(malformed("Inventor bulk trailer"));
    }
    Ok(())
}
fn document(
    source: View<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
) -> Result<(), StopReason> {
    let compound = ScanCompound::new(source, budget)?;
    let entry = compound
        .entries()
        .iter()
        .find(|entry| entry.path.eq_ignore_ascii_case("RSeStorage/RSeSegInfo"))
        .ok_or_else(|| StopReason::Unsupported("Inventor segment registry is absent".into()))?;
    scan.declared_size(budget, &entry.path, entry.declared_size)?;
    let rows = registry(&compound.open(entry)?, budget)?;
    let mut found = false;
    for entry in compound.entries() {
        budget.check()?;
        let Some((parent, name)) = entry.path.rsplit_once('/') else {
            continue;
        };
        let Some(token) = name.strip_prefix('M') else {
            continue;
        };
        if !parent.starts_with("RSeStorage") {
            continue;
        }
        let header = compound.prefix(entry, 16 * 1024)?;
        let (id, _, _) = metadata_header(budget.view(&header)?, budget)?;
        let Some(row) = rows.iter().find(|row| row.id == id) else {
            continue;
        };
        let bulk_name = format!("{parent}/B{token}");
        let bulk = compound
            .entries()
            .iter()
            .find(|entry| entry.path == bulk_name)
            .ok_or_else(|| malformed("Inventor PmDc bulk stream is absent"))?;
        scan.declared_size(budget, &entry.path, entry.declared_size)?;
        scan.declared_size(budget, &bulk.path, bulk.declared_size)?;
        listing(
            &compound.open(entry)?,
            &compound.open(bulk)?,
            row.major,
            budget,
            scan,
        )?;
        found = true;
    }
    if !found {
        return Err(StopReason::Unsupported(
            "Inventor PmDc listing is absent".into(),
        ));
    }
    scan.complete();
    Ok(())
}
impl crate::InventorCodec {
    /// Recognizes structurally reached compound stream names for this reader.
    pub fn scan_detect_compound(
        entries: &[ScanCompoundEntry],
        budget: &ScanBudget<'_>,
    ) -> Result<bool, StopReason> {
        for entry in entries {
            budget.check()?;
            if entry.path.eq_ignore_ascii_case("RSeStorage/RSeSegInfo")
                || crate::rse::database_band(&entry.path).is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Scans Inventor feature-list records in source order.
    pub fn scan_design(input: View<'_>, limits: ScanLimits, cancel: &dyn ScanClock) -> DesignScan {
        let budget = ScanBudget::new(limits, cancel);
        Self::scan_design_with_budget(input, &budget)
    }

    /// Scans with a meter shared by detection and nested readers.
    pub fn scan_design_with_budget(input: View<'_>, budget: &ScanBudget<'_>) -> DesignScan {
        let mut scan = DesignScan::new(Some(crate::dialect::registry_ids::FORMAT));
        let result = document(input, budget, &mut scan);
        scan.finish(budget, result);
        scan
    }
}
