//! Ordered loft sections and guide curves of `moBlend_c` records.

use super::component_paths::{component_feature_precedes, is_profile_feature_object};
use super::scalars::FeatureObjectNames;
use super::selections::{component_child_prefix_at, component_profile_record_at};
use super::sweeps::read::{class_body, wire_contact_end};
use crate::classification::{native_object_class, NativeClassKind};
use crate::layout::{loft_section_tail as tail, sweep_component_suffix as suffix};
use crate::loss::SldprtLossCode;
use crate::records::{Feature, FeatureHistory, FeatureInputLane, HistoryFeatures, ObjectId};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::profiles::PathRef;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, LoftGuidance};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

/// Prefix of the native reference that keeps one loft guide curve.
pub(crate) const LOFT_GUIDE_PREFIX: &str = "sldprt:feature-input:loft-guide:";

const COUNT: usize = std::mem::size_of::<u16>();
const SECTION_COUNT: usize = std::mem::size_of::<u32>() + COUNT;
const MIN_SECTIONS: u16 = 2;

/// The section and guide operands of one loft record.
#[derive(Debug, Clone, PartialEq)]
struct Operands {
    /// Section source IDs in stored order.
    sections: Vec<u32>,
    /// The guide-curve count, or why the guide curves do not read.
    guides: Result<usize, &'static str>,
}

/// A `moGeneralCurveRef_w` that holds a `moCompProfile_c`: the referenced
/// source ID and the offset after the operand.
fn section_at(payload: &[u8], offset: usize) -> Option<(u32, usize)> {
    let child = class_body(payload, offset, "moGeneralCurveRef_w")?;
    let prefix = class_body(payload, child, "moCompProfile_c")?;
    let (source, record_end) = component_profile_record_at(payload, prefix)?;
    let suffix_start = record_end.checked_add(tail::LEN)?;
    let end = suffix_start.checked_add(suffix::LEN)?;
    let frame = payload.get(record_end..suffix_start)?;
    let closing = payload.get(suffix_start..end)?;
    (frame[tail::ZEROS..tail::UNIT] == tail::ZEROS_VALUE
        && View::f64_le_at(frame, tail::UNIT)? == 1.0
        && frame[tail::BODY_FLAG] == tail::BODY_FLAG_VALUE
        && closing[suffix::SENTINEL..suffix::ZEROS] == suffix::SENTINEL_VALUE
        && closing[suffix::ZEROS..] == suffix::ZEROS_VALUE)
        .then_some((source, end))
}

/// A guide curve: a `moWireContact_c` followed by a `moGeneralCurveRef_w`
/// whose `moCompEntGroup_c` child opens with the ten-byte child prefix and
/// holds no section source record.
fn guide_at(payload: &[u8], offset: usize) -> bool {
    (|| {
        let body = class_body(payload, offset, "moWireContact_c")?;
        let reference = wire_contact_end(payload, body)?;
        let child = class_body(payload, reference, "moGeneralCurveRef_w")?;
        let prefix = class_body(payload, child, "moCompEntGroup_c")?;
        (component_child_prefix_at(payload, prefix)
            && component_profile_record_at(payload, prefix).is_none())
        .then_some(())
    })()
    .is_some()
}

fn closes_component(payload: &[u8], end: usize) -> bool {
    end.checked_sub(suffix::LEN)
        .and_then(|start| payload.get(start..end))
        .is_some_and(|closing| {
            closing[suffix::SENTINEL..suffix::ZEROS] == suffix::SENTINEL_VALUE
                && closing[suffix::ZEROS..] == suffix::ZEROS_VALUE
        })
}

/// The guide-curve count at `count`, when its guide curves follow it
/// consecutively: the first immediately, and each other immediately after
/// the component-reference suffix that closes the one before it.
fn guide_count(payload: &[u8], count: usize) -> Option<usize> {
    let declared = usize::from(View::u16_le_at(payload, count)?);
    let mut at = count.checked_add(COUNT)?;
    for index in 0..declared {
        if index > 0 {
            at = (at + 1..payload.len()).find(|&next| guide_at(payload, next))?;
            if !closes_component(payload, at) {
                return None;
            }
        } else if !guide_at(payload, at) {
            return None;
        }
    }
    Some(declared)
}

/// The operands of the loft whose name record is at `start`. A section
/// operand lies before `bound`; the object ends at the first name record
/// after its first section operand, `next_name`.
fn operands(
    payload: &[u8],
    start: usize,
    bound: usize,
    next_name: impl Fn(usize) -> usize,
) -> Option<Operands> {
    let first = (start.checked_add(SECTION_COUNT)?..bound.min(payload.len()))
        .find(|&at| section_at(payload, at).is_some())?;
    let payload = payload.get(..next_name(first).min(bound))?;
    let count = View::u16_le_at(payload, first - COUNT)?;
    if count < MIN_SECTIONS || View::u32_le_at(payload, first - SECTION_COUNT)? != u32::from(count)
    {
        return None;
    }
    let mut sections = Vec::new();
    let mut cursor = first;
    for _ in 0..count {
        let (source, next) = section_at(payload, cursor)?;
        sections.push(source);
        cursor = next;
    }
    Some(Operands {
        sections,
        guides: guide_count(payload, cursor).ok_or("its guide-curve records are unreadable"),
    })
}

/// The first name record after `offset` whose object `ends` selects.
fn next_name(lane: &FeatureInputLane, offset: usize, ends: impl Fn(Option<u32>) -> bool) -> usize {
    lane.names
        .iter()
        .filter(|name| ends(name.object_id.and_then(ObjectId::value)))
        .filter_map(|name| usize::try_from(name.offset).ok())
        .filter(|at| *at > offset)
        .min()
        .unwrap_or(lane.native_payload.len())
}

enum Unread {
    /// No lane holds the loft record.
    Absent,
    /// Why the loft keeps no sections.
    Reason(&'static str),
}

/// The operands every lane that holds `feature` agrees on. Lanes that agree
/// on the sections and differ on the guide curves give the sections.
fn consensus(
    feature: &Feature,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<Operands, Unread> {
    let Some(source) = feature.source_value() else {
        return Err(Unread::Reason("it has no object identifier"));
    };
    let held = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|candidate| is_profile_feature_object(candidate))
        .filter_map(Feature::source_value)
        .filter(|id| *id < source)
        .collect::<HashSet<_>>();
    let mut votes = Vec::new();
    for lane in lanes {
        let names = FeatureObjectNames::new(&lane.names);
        if !names.has_candidate(feature) {
            continue;
        }
        let reading = names
            .of(feature)
            .and_then(|name| usize::try_from(name.offset).ok())
            .and_then(|start| {
                let bound = next_name(lane, start, |id| {
                    id.is_none_or(|id| id > source || !held.contains(&id))
                });
                operands(&lane.native_payload, start, bound, |first| {
                    next_name(lane, first, |_| true)
                })
            });
        votes.push(reading);
    }
    let Some(first) = votes.first() else {
        return Err(Unread::Absent);
    };
    let Some(first) = first.clone() else {
        return Err(Unread::Reason("its section records are unreadable"));
    };
    if votes.iter().any(|reading| {
        reading
            .as_ref()
            .is_none_or(|reading| reading.sections != first.sections)
    }) {
        return Err(Unread::Reason(
            "its configurations do not agree on its section records",
        ));
    }
    let guides = if votes
        .iter()
        .flatten()
        .all(|reading| reading.guides == first.guides)
    {
        first.guides
    } else {
        Err("its configurations do not agree on its guide-curve records")
    };
    Ok(Operands {
        sections: first.sections,
        guides,
    })
}

fn section_producer(
    source: u32,
    consumer: &Feature,
    histories: &[FeatureHistory],
) -> Result<(), String> {
    let mut matches = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| {
            feature.source_value() == Some(source) && feature.parent == consumer.parent
        });
    let Some(producer) = matches.next() else {
        return Err(format!("section {source} names no feature of its history"));
    };
    if matches.next().is_some() {
        return Err(format!("section {source} names several features"));
    }
    if !is_profile_feature_object(producer) {
        return Err(format!("section {source} names no sketch"));
    }
    if !component_feature_precedes(producer.into(), consumer.into()) {
        return Err(format!("section {source} names a later feature"));
    }
    Ok(())
}

/// The `Profiles` value that names `operands`' sections by source ID.
fn profiles_value(operands: &Operands) -> String {
    operands
        .sections
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn is_loft(feature: &Feature) -> bool {
    native_object_class(feature.input_class.as_deref().unwrap_or_default()) == NativeClassKind::Loft
}

fn states_operands(feature: &Feature) -> bool {
    feature.properties.contains_key("Profiles") || feature.properties.contains_key("Guides")
}

/// Add the `Profiles` of loft records that state no operands.
pub(crate) fn enrich_history_loft_inputs(
    histories: &mut [FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut fills = Vec::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        if !is_loft(feature) || states_operands(feature) {
            continue;
        }
        let unresolved = |reason: &str| {
            SldprtLossCode::FeatureLoftSectionsUnresolved
                .note(format!("{} keeps no sections: {reason}.", feature.id))
        };
        let operands = match consensus(feature, histories, lanes) {
            Ok(operands) => operands,
            Err(Unread::Absent) => continue,
            Err(Unread::Reason(reason)) => {
                losses.push(unresolved(reason));
                continue;
            }
        };
        if let Err(reason) = operands
            .sections
            .iter()
            .try_for_each(|section| section_producer(*section, feature, histories))
        {
            losses.push(unresolved(&reason));
            continue;
        }
        if operands.sections.len() > usize::from(MIN_SECTIONS) {
            losses.push(SldprtLossCode::FeatureLoftSectionOrderAssumed.note(format!(
                "{} takes its {} sections in stored order.",
                feature.id,
                operands.sections.len()
            )));
        }
        fills.push((feature.id.clone(), profiles_value(&operands)));
    }
    for (id, profiles) in fills {
        if let Some(feature) = histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|feature| feature.id == id)
        {
            feature
                .properties
                .insert(cadmpeg_core::nonblank_literal!("Profiles"), profiles);
        }
    }
    losses
}

/// The native references that keep the `count` guide curves of the loft
/// record with object identifier `source`.
pub(crate) fn retained_guides(source: u32, count: usize) -> Vec<PathRef> {
    (0..count)
        .map(|index| PathRef::Native(format!("{LOFT_GUIDE_PREFIX}{source}:{index}/{count}")))
        .collect()
}

/// The section source IDs a stated `Profiles` value names.
fn stated_sections(value: &str) -> Option<Vec<u32>> {
    value
        .split(',')
        .map(|source| source.trim().parse().ok())
        .collect()
}

/// The guide curves a loft record stores, or why its loft keeps none.
fn record_guides(
    record: &Feature,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Option<Result<Vec<PathRef>, &'static str>> {
    let operands = match consensus(record, histories, lanes) {
        Ok(operands) => operands,
        Err(Unread::Absent) => return None,
        Err(Unread::Reason(reason)) => return Some(Err(reason)),
    };
    if record
        .properties
        .get("Profiles")
        .is_some_and(|stated| stated_sections(stated).as_ref() != Some(&operands.sections))
    {
        return Some(Err("its stated sections differ from its section records"));
    }
    let source = record.source_value()?;
    Some(operands.guides.map(|count| retained_guides(source, count)))
}

/// Keep the guide curves of lofts whose records store them, as native
/// references. A loft whose history states its guides keeps its definition.
/// A feature of `stated` with the same definition already carries the loss.
pub(crate) fn bind_guides(
    features: &mut [cadmpeg_ir::features::Feature],
    stated: &[cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    losses: &mut Vec<LossNote>,
) {
    let records = HistoryFeatures::new(histories);
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: LoftGuidance::Guides(guides),
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if sections.is_empty() || !guides.is_empty() {
            continue;
        }
        let Some(record) = feature
            .native_ref
            .as_deref()
            .and_then(|native| records.get(native))
        else {
            continue;
        };
        if !is_loft(record) || record.properties.contains_key("Guides") {
            continue;
        }
        let Some(reading) = record_guides(record, histories, lanes) else {
            continue;
        };
        let note = match reading {
            Ok(retained) if retained.is_empty() => continue,
            Ok(retained) => {
                let note = SldprtLossCode::FeatureLoftGuidesRetained.note(format!(
                    "{} retains its {} guide curve records.",
                    record.id,
                    retained.len()
                ));
                feature.evaluation.edit(|definition, _| {
                    if let FeatureDefinition::Operation(FeatureOperation::Loft {
                        guidance, ..
                    }) = definition
                    {
                        *guidance = LoftGuidance::Guides(retained);
                    }
                });
                note
            }
            Err(reason) => SldprtLossCode::FeatureLoftGuidesUnresolved
                .note(format!("{} keeps no guide curves: {reason}.", record.id)),
        };
        if !stated.iter().any(|known| {
            known.id == feature.id
                && known.evaluation.definition() == feature.evaluation.definition()
        }) {
            losses.push(note);
        }
    }
}

/// How a loft's guide curves relate to the native references its record
/// retains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetainedGuides {
    /// The guides hold no retained native reference.
    Absent,
    /// The guides are the references the record retains.
    Unchanged,
    /// The guides hold retained native references the record does not give.
    Changed,
}

/// How `guides` relate to the references the loft record `existing` retains.
pub(crate) fn retained_guide_state(
    existing: Option<&Feature>,
    guides: &[PathRef],
) -> RetainedGuides {
    if !guides.iter().any(|path| {
        matches!(path, PathRef::Native(reference) if reference.starts_with(LOFT_GUIDE_PREFIX))
    }) {
        return RetainedGuides::Absent;
    }
    let retained = existing
        .filter(|record| is_loft(record) && !record.properties.contains_key("Guides"))
        .and_then(Feature::source_value)
        .map(|source| retained_guides(source, guides.len()));
    if retained.as_deref() == Some(guides) {
        RetainedGuides::Unchanged
    } else {
        RetainedGuides::Changed
    }
}

/// Whether the `Profiles` value `written` restates the decoded sections of
/// the loft record `existing`, whose properties state none.
pub(crate) fn restates_record_sections(
    existing: Option<&Feature>,
    stated: Option<&str>,
    written: &str,
) -> bool {
    existing.is_some_and(|record| is_loft(record) && !record.properties.contains_key("Profiles"))
        && stated == Some(written)
}

#[cfg(test)]
mod tests;
