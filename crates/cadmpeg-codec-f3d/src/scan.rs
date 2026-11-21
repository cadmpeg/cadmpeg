// SPDX-License-Identifier: Apache-2.0
//! Feature census from Fusion Design timeline tables.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::scan::ScanZip;
use cadmpeg_core::decode::{bounded_len, u64_from_index, View};
use cadmpeg_core::scan::{
    DesignScan, FeatureCensus, ScanBudget, ScanClock, ScanLimits, ScanReader, StopReason,
    Suppressed,
};

use crate::design::decode::meta::{FEATURE_TIMELINE_TYPE_GUID, FEATURE_TIMELINE_TYPE_VERSIONS};

fn malformed(what: &str) -> StopReason {
    StopReason::Malformed(what.into())
}

fn length(reader: &mut ScanReader<'_, '_>, stride: usize) -> Result<usize, StopReason> {
    let count = reader.u32()?;
    bounded_len(u64::from(count), stride, reader.remaining())
        .ok_or_else(|| malformed("Fusion table count exceeds its extent"))
}

fn text<'a>(reader: &mut ScanReader<'a, '_>) -> Result<&'a str, StopReason> {
    let length = length(reader, 1)?;
    std::str::from_utf8(reader.take(length)?.window()).map_err(|_| malformed("Fusion ASCII field"))
}

fn skip_text16(reader: &mut ScanReader<'_, '_>) -> Result<(), StopReason> {
    let count = length(reader, 2)?;
    reader.take(
        count
            .checked_mul(2)
            .ok_or_else(|| malformed("Fusion UTF16 length"))?,
    )?;
    Ok(())
}

struct Meta<'a> {
    kinds: BTreeMap<u64, Vec<&'a str>>,
    timelines: BTreeMap<u64, String>,
    records: Vec<(u64, u64)>,
}

fn metadata<'a>(source: View<'a>, budget: &ScanBudget<'_>) -> Result<Meta<'a>, StopReason> {
    let mut reader = ScanReader::retained(source, budget);
    text(&mut reader)?;
    reader.take(4)?;
    skip_text16(&mut reader)?;
    let magic = reader.u32()?;
    reader.take(if magic == crate::metastream::MODERN_SERIALIZER_MAGIC {
        12
    } else {
        4
    })?;
    text(&mut reader)?;
    text(&mut reader)?;
    reader.take(8)?;
    let type_count = length(&mut reader, 20)?;
    let mut kinds: BTreeMap<u64, Vec<&str>> = BTreeMap::new();
    let mut timelines = BTreeMap::new();
    for ordinal in 0..type_count {
        budget.check()?;
        let guid = text(&mut reader)?;
        let base = text(&mut reader)?;
        let version = reader.u32()?;
        let module = text(&mut reader)?;
        let count = length(&mut reader, 8)?;
        let is_timeline = guid.eq_ignore_ascii_case(FEATURE_TIMELINE_TYPE_GUID);
        if is_timeline && !FEATURE_TIMELINE_TYPE_VERSIONS.contains(&version) {
            return Err(StopReason::Unsupported(format!(
                "Fusion timeline version {version}"
            )));
        }
        if is_timeline
            && (!base
                .eq_ignore_ascii_case(crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID)
                || module != crate::records::entity_header::DESIGN_MODULE_FUSION)
        {
            return Err(malformed("Fusion timeline registration"));
        }
        for _ in 0..count {
            budget.check()?;
            budget.retain(160)?;
            let id = reader.u64()?;
            kinds.entry(id).or_default().push(guid);
            if is_timeline {
                let class = u64_from_index(ordinal)
                    .checked_add(256)
                    .ok_or_else(|| malformed("Fusion class index"))?;
                timelines.insert(id, class.to_string());
            }
        }
    }
    let named = length(&mut reader, 8)?;
    reader.take(
        named
            .checked_mul(8)
            .ok_or_else(|| malformed("Fusion named table length"))?,
    )?;
    let count = length(&mut reader, 16)?;
    let mut records = Vec::new();
    let mut previous = None;
    let mut ids = BTreeSet::new();
    for _ in 0..count {
        budget.check()?;
        budget.retain(128)?;
        let id = reader.u64()?;
        let offset = reader.u64()?;
        if previous.is_some_and(|at| at >= offset) || !ids.insert(id) {
            return Err(malformed("Fusion primary record index order or identity"));
        }
        previous = Some(offset);
        records.push((id, offset));
    }
    Ok(Meta {
        kinds,
        timelines,
        records,
    })
}

fn reference<'a>(
    source: View<'a>,
    reader: &mut ScanReader<'a, '_>,
    budget: &ScanBudget<'_>,
) -> Result<crate::bytes::Reference<&'a str, crate::bytes::utf16::Utf16View<'a>>, StopReason> {
    budget.check()?;
    let mut at = reader.position() - source.start();
    let value = crate::bytes::take_reference(source.window(), &mut at)
        .ok_or_else(|| malformed("Fusion timeline reference"))?;
    reader.seek(
        source
            .start()
            .checked_add(at)
            .ok_or_else(|| malformed("Fusion reference offset"))?,
    )?;
    Ok(value)
}

fn timeline(
    source: View<'_>,
    id: u64,
    class: &str,
    meta: &Meta<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
    depth: u64,
) -> Result<(), StopReason> {
    let mut reader = ScanReader::retained(source, budget);
    if text(&mut reader)? != class || reader.u64()? != id {
        return Err(malformed("Fusion timeline identity"));
    }
    text(&mut reader)?;
    if reader.take(2)?.window() != [0, 0] {
        return Err(malformed("Fusion timeline prefix"));
    }
    if reference(source, &mut reader, budget)?.local().is_none() {
        return Err(malformed("Fusion timeline context"));
    }
    let count = length(&mut reader, 1)?;
    scan.census.get_or_insert_with(FeatureCensus::default);
    let mut visited = BTreeSet::new();
    for _ in 0..count {
        budget.check()?;
        let visit = budget.feature()?;
        let item = reference(source, &mut reader, budget)?;
        if matches!(item, crate::bytes::Reference::Null) {
            continue;
        }
        let kind = if let Some((target, inline)) = item.local() {
            if target == id {
                budget.depth(depth.checked_add(1).ok_or(StopReason::NestingDepth)?)?;
                return Err(malformed("Fusion timeline references itself"));
            }
            budget.retain(64)?;
            if !visited.insert(target) {
                return Err(malformed("Fusion timeline repeats a feature"));
            }
            let mut found: Option<crate::records::feature::scope::DesignFeatureKind> = None;
            if let Some(registered) = meta.kinds.get(&target) {
                for guid in registered {
                    budget.check()?;
                    if inline.is_some_and(|inline| !inline.eq_ignore_ascii_case(guid)) {
                        continue;
                    }
                    budget.retain(128)?;
                    if let Some(candidate) =
                        crate::design::decode::scopes::parameter_scope::scan_kind(guid)
                    {
                        if found
                            .as_ref()
                            .is_some_and(|old| old.as_str() != candidate.as_str())
                        {
                            found = None;
                            break;
                        }
                        found = Some(candidate);
                    }
                }
            }
            found
        } else {
            None
        };
        scan.record_feature(
            visit,
            kind.as_ref().map_or(
                "unknown",
                crate::records::feature::scope::DesignFeatureKind::as_str,
            ),
            Suppressed::Unknown,
        )?;
    }
    if reader.remaining() != 0 {
        return Err(malformed("Fusion timeline trailing bytes"));
    }
    Ok(())
}

fn document(
    source: View<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
    depth: u64,
    physical: bool,
) -> Result<bool, StopReason> {
    budget.depth(depth)?;
    let mut archive = ScanZip::new(source, budget, physical)?;
    let entries = archive.entries()?;
    let mut found = false;
    for entry in &entries {
        budget.check()?;
        if entry.name.rsplit_once('.').is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("f3d") || extension.eq_ignore_ascii_case("f3z")
        }) {
            let nested_depth = depth.checked_add(1).ok_or(StopReason::NestingDepth)?;
            budget.depth(nested_depth)?;
            scan.declared_size(budget, &entry.name, entry.declared_size)?;
            let bytes = archive.open(entry.ordinal)?;
            if !document(budget.view(&bytes)?, budget, scan, nested_depth, false)? {
                return Err(StopReason::Unsupported(
                    "Fusion child timeline listing is absent".into(),
                ));
            }
            found = true;
            continue;
        }
        let Some(prefix) = entry.name.strip_suffix("MetaStream.dat") else {
            continue;
        };
        let segment = prefix
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("");
        if !(segment == "Design"
            || segment == "Design1"
            || segment.starts_with("FusionDesignSegmentType"))
        {
            continue;
        }
        scan.declared_size(budget, &entry.name, entry.declared_size)?;
        let bytes = archive.open(entry.ordinal)?;
        let meta = metadata(budget.view(&bytes)?, budget)?;
        if meta.timelines.is_empty() {
            continue;
        }
        let bulk = entries
            .iter()
            .find(|entry| entry.name.strip_prefix(prefix) == Some("BulkStream.dat"))
            .ok_or_else(|| malformed("Fusion timeline BulkStream is absent"))?;
        scan.declared_size(budget, &bulk.name, bulk.declared_size)?;
        let bytes = archive.open(bulk.ordinal)?;
        let source = budget.view(&bytes)?;
        let mut seen = BTreeSet::new();
        for (ordinal, &(id, offset)) in meta.records.iter().enumerate() {
            budget.check()?;
            let Some(class) = meta.timelines.get(&id) else {
                continue;
            };
            let end = meta
                .records
                .get(ordinal + 1)
                .map_or(u64_from_index(bytes.len()), |row| row.1);
            let start = usize::try_from(offset).map_err(|_| malformed("Fusion record offset"))?;
            let end = usize::try_from(end).map_err(|_| malformed("Fusion record end"))?;
            let frame = source
                .child(start, end)
                .ok_or_else(|| malformed("Fusion timeline extent"))?;
            timeline(frame, id, class, &meta, budget, scan, depth)?;
            seen.insert(id);
            found = true;
        }
        if seen.len() != meta.timelines.len() {
            return Err(malformed("Fusion timeline has no primary record"));
        }
    }
    Ok(found)
}

impl crate::F3dCodec {
    /// Scans Fusion timeline records under shared host-controlled limits.
    pub fn scan_design(input: View<'_>, limits: ScanLimits, cancel: &dyn ScanClock) -> DesignScan {
        let budget = ScanBudget::new(limits, cancel);
        Self::scan_design_with_budget(input, &budget)
    }

    /// Scans with a meter shared by detection and nested readers.
    pub fn scan_design_with_budget(input: View<'_>, budget: &ScanBudget<'_>) -> DesignScan {
        let mut scan = DesignScan::new(Some(crate::dialect::FORMAT));
        let result = document(input, budget, &mut scan, 1, true).and_then(|found| {
            if found {
                scan.complete();
                Ok(())
            } else {
                Err(StopReason::Unsupported(
                    "Fusion timeline listing is absent".into(),
                ))
            }
        });
        scan.finish(budget, result);
        scan
    }
}
