// SPDX-License-Identifier: Apache-2.0
//! Feature census from Fusion Design timeline tables.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_container::scan::ScanZip;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::scan::{
    DesignScan, FeatureCensus, ScanBudget, ScanClock, ScanLimits, StopReason, Suppressed,
};

use crate::design::decode::meta::{
    is_supported_feature_timeline_type, local_reference, parse_feature_timeline_record,
    FEATURE_TIMELINE_TYPE_GUID, FEATURE_TIMELINE_TYPE_VERSIONS,
};

const META_STORAGE_PER_BYTE: u64 = 32;
const TIMELINE_STORAGE_PER_BYTE: u64 = 32;
const PARSER_FIXED_STORAGE: u64 = 1024;

fn malformed(what: &str) -> StopReason {
    StopReason::Malformed(what.into())
}

fn admit_parser(
    source: View<'_>,
    storage_per_byte: u64,
    budget: &ScanBudget<'_>,
) -> Result<(), StopReason> {
    let length = u64_from_index(source.window().len());
    budget.source(length)?;
    budget.retain(
        length
            .checked_mul(storage_per_byte)
            .and_then(|size| size.checked_add(PARSER_FIXED_STORAGE))
            .ok_or(StopReason::RetainedBytes)?,
    )
}

struct Meta<'a> {
    kinds: HashMap<u64, Vec<&'a str>>,
    timelines: BTreeMap<u64, String>,
    /// Timeline entities whose registration the decoder reports as unread.
    unregistered: bool,
}

fn metadata<'a>(
    source: &'a crate::metastream::MetaStream,
    budget: &ScanBudget<'_>,
) -> Result<Meta<'a>, StopReason> {
    let mut kinds: HashMap<u64, Vec<&str>> = HashMap::new();
    let mut timelines = BTreeMap::new();
    let mut unregistered = false;
    for (ordinal, registration) in source.types.iter().enumerate() {
        budget.check()?;
        let guid = registration.type_guid.as_str();
        let is_timeline = guid.eq_ignore_ascii_case(FEATURE_TIMELINE_TYPE_GUID);
        // The decoder reports a timeline of an unsupported version, an incompatible
        // registration or an unrepresentable class tag as unread and reads the rest.
        let readable = !is_timeline
            || (FEATURE_TIMELINE_TYPE_VERSIONS.contains(&registration.version)
                && is_supported_feature_timeline_type(registration));
        for id in registration.entities.values() {
            budget.check()?;
            budget.retain(160)?;
            kinds.entry(*id).or_default().push(guid);
            if is_timeline {
                let class = u64_from_index(ordinal)
                    .checked_add(256)
                    .filter(|class| *class <= 999);
                match class {
                    Some(class) if readable => {
                        timelines.insert(*id, class.to_string());
                    }
                    _ => unregistered = true,
                }
            }
        }
    }
    Ok(Meta {
        kinds,
        timelines,
        unregistered,
    })
}

fn timeline(
    source: View<'_>,
    expected: (&str, u64),
    stream: &str,
    meta: &Meta<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
    depth: u64,
) -> Result<bool, StopReason> {
    admit_parser(source, TIMELINE_STORAGE_PER_BYTE, budget)?;
    let Some((timeline, _)) = parse_feature_timeline_record(
        source.window(),
        stream,
        0..source.window().len(),
        expected,
        0,
        &meta.kinds,
    ) else {
        // The decoder reports a record that misses its exact frame as unread.
        return Ok(false);
    };
    budget.check()?;
    scan.census.get_or_insert_with(FeatureCensus::default);
    for item in timeline.frame().items() {
        let visit = budget.feature()?;
        let mut at = usize::try_from(item.offset)
            .ok()
            .and_then(|offset| offset.checked_sub(1))
            .ok_or_else(|| malformed("Fusion timeline item offset"))?;
        budget.source(u64_from_index(
            source
                .window()
                .len()
                .checked_sub(at)
                .ok_or_else(|| malformed("Fusion timeline item offset"))?
                .min(crate::bytes::TYPED_LOCAL_REFERENCE_LEN),
        ))?;
        let reference = crate::bytes::take_reference(source.window(), &mut at)
            .ok_or_else(|| malformed("Fusion timeline reference"))?;
        let target = local_reference(&reference, &meta.kinds)
            .ok_or_else(|| malformed("Fusion timeline local reference"))?;
        let (_, inline) = reference
            .local()
            .ok_or_else(|| malformed("Fusion timeline local reference"))?;
        if target == expected.1 {
            budget.depth(depth.checked_add(1).ok_or(StopReason::NestingDepth)?)?;
            return Err(malformed("Fusion timeline references itself"));
        }
        let mut kind: Option<crate::records::feature::scope::DesignFeatureKind> = None;
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
                    if kind
                        .as_ref()
                        .is_some_and(|old| old.as_str() != candidate.as_str())
                    {
                        kind = None;
                        break;
                    }
                    kind = Some(candidate);
                }
            }
        }
        scan.record_feature(
            visit,
            kind.as_ref().map_or(
                "unknown",
                crate::records::feature::scope::DesignFeatureKind::as_str,
            ),
            Suppressed::Unknown,
        )?;
    }
    Ok(true)
}

fn document(
    source: View<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
    depth: u64,
    physical: bool,
    unread: &mut bool,
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
            // The decoder reports a member without a readable timeline and continues.
            if document(
                budget.view(&bytes)?,
                budget,
                scan,
                nested_depth,
                false,
                unread,
            )? {
                found = true;
            } else {
                *unread = true;
            }
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
        let source = budget.view(&bytes)?;
        admit_parser(source, META_STORAGE_PER_BYTE, budget)?;
        let parsed = crate::metastream::parse(source.window(), &entry.name)
            .map_err(|error| malformed(&error.to_string()))?;
        let meta = metadata(&parsed, budget)?;
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
        let mut previous = None;
        let mut ids = BTreeMap::<u64, u64>::new();
        for record in &parsed.records {
            budget.check()?;
            budget.retain(64)?;
            if previous.is_some_and(|at| at >= record.bulk_offset) {
                return Err(malformed("Fusion primary record index order"));
            }
            *ids.entry(record.entity_id).or_default() += 1;
            previous = Some(record.bulk_offset);
        }
        *unread |= meta.unregistered;
        for (ordinal, record) in parsed.records.iter().enumerate() {
            let (id, offset) = (record.entity_id, record.bulk_offset);
            budget.check()?;
            let Some(class) = meta.timelines.get(&id) else {
                continue;
            };
            if ids.get(&id) != Some(&1) {
                // No unique primary record-index entry: unread in the decoder.
                *unread = true;
                continue;
            }
            let end = parsed
                .records
                .get(ordinal + 1)
                .map_or(u64_from_index(bytes.len()), |row| row.bulk_offset);
            let frame = usize::try_from(offset)
                .ok()
                .zip(usize::try_from(end).ok())
                .filter(|(start, end)| start < end)
                .and_then(|(start, end)| source.child(start, end));
            let Some(frame) = frame else {
                *unread = true;
                continue;
            };
            if timeline(frame, (class, id), &bulk.name, &meta, budget, scan, depth)? {
                found = true;
            } else {
                *unread = true;
            }
        }
        let absent = meta
            .timelines
            .keys()
            .filter(|id| !ids.contains_key(id))
            .count();
        *unread |= absent != 0;
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
        let mut unread = false;
        let result = document(input, budget, &mut scan, 1, true, &mut unread).and_then(|found| {
            if unread {
                // Readable rows stay listed; the listing is not claimed complete.
                Err(malformed("Fusion timeline records are unread"))
            } else if found {
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
