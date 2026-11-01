// SPDX-License-Identifier: Apache-2.0
//! Native joins for neutral mirror constructions.

use std::collections::{hash_map::Entry, HashMap, HashSet};

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::patterns::PatternTransform;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::CadIr;

use super::{finding, NativeData};
use crate::design::scope::{BrepSegmentName, BrepSegmentRole};
use crate::feature::history::{build_records, HistoryPlacement, HistoryRecords, SUPPRESSED_FLAG};

pub(super) fn validate_mirrors(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<cadmpeg_ir::features::FeatureId>, CodecError> {
    if !data
        .pm_dc_pattern_features
        .iter()
        .any(crate::feature::mirror::is_mirror_record)
        && !ir.model.features.iter().any(is_mirror_construction)
    {
        return Ok(HashSet::new());
    }
    let selection_type = crate::pmdc::type_id_string(crate::feature::mirror::SELECTION_TYPE);
    let options_type = crate::pmdc::type_id_string(crate::feature::mirror::OPTIONS_TYPE);
    let instance_map_type = crate::pmdc::type_id_string(crate::feature::mirror::instance_map::TYPE);
    let generated_nodes = data
        .pm_dc_feature_properties
        .iter()
        .filter_map(|property| {
            let crate::feature::PmDcFeaturePropertyKind::Mirror {
                definition: crate::feature::mirror::Property::InstanceMap { result, .. },
            } = &property.kind
            else {
                return None;
            };
            Some((
                property.identity.segment_token.as_str(),
                result.record_ordinal()?,
            ))
        })
        .collect::<HashSet<_>>();
    let generic_type = crate::pmdc::type_id_string(crate::feature::FEATURE_TYPE);
    let index = crate::feature::mirror::MirrorIndex::new(
        &data.pm_dc_feature_properties,
        &data.pm_dc_transforms,
        &data.pm_dc_work_planes,
    )
    .with_generated_nodes(&data.pm_dc_features, &data.pm_dc_feature_labels)
    .with_record_lengths(
        data.records
            .iter()
            .filter(|record| {
                record.type_id == selection_type
                    || record.type_id == options_type
                    || record.type_id == instance_map_type
                    || (record.type_id == generic_type
                        && generated_nodes.contains(&(record.token.as_str(), record.ordinal)))
            })
            .map(|record| (record.token.as_str(), record.ordinal, record.payload_len)),
    );
    let patterns = unique_index(data.pm_dc_pattern_features.iter().map(|p| (p.id(), p)));
    let labels = unique_index(data.pm_dc_feature_labels.iter().filter_map(|l| {
        Some((
            (
                l.identity.segment_token.as_str(),
                l.header.owner.record_ordinal()?,
            ),
            l,
        ))
    }));
    let records = unique_index(
        data.pm_dc_features
            .iter()
            .map(|r| (&r.identity, r.id(), r.header.flags))
            .chain(
                data.pm_dc_pattern_features
                    .iter()
                    .map(|r| (&r.identity, r.id(), r.header.flags)),
            )
            .map(|(identity, native, flags)| {
                (
                    (identity.segment_token.as_str(), identity.record_ordinal),
                    (identity, native, flags),
                )
            }),
    );
    let features = unique_index(ir.model.features.iter().map(|f| (&f.id, f)));
    let results = unique_index(
        ir.model
            .feature_result_topologies
            .iter()
            .map(|r| (&r.output_of, r)),
    );
    let mut outputs = HashSet::new();
    let mut histories = HashMap::new();
    let segments = data
        .metadata
        .iter()
        .filter_map(|segment| {
            let role = match segment.kind.as_str() {
                "pm_brep" => BrepSegmentRole::Document,
                "mb_brep" => BrepSegmentRole::FlatPattern,
                _ => return None,
            };
            Some(BrepSegmentName {
                display_name: segment.display_name.clone(),
                role,
            })
        })
        .collect::<Vec<_>>();
    let history_records = HistoryRecords {
        features: &data.pm_dc_features,
        patterns: &data.pm_dc_pattern_features,
        labels: &data.pm_dc_feature_labels,
        terminators: &data.pm_dc_feature_terminators,
        sketches: &data.pm_dc_sketches,
        work_planes: &data.pm_dc_work_planes,
        roots: &data.pm_dc_model_roots,
        segments: &segments,
    };
    for feature in &ir.model.features {
        let source = feature
            .native_ref
            .as_ref()
            .and_then(|native| patterns.get(native))
            .and_then(Option::as_ref)
            .copied();
        if !requires_mirror_validation(feature, source) {
            continue;
        }
        outputs.insert(feature.id.clone());
        if let Some(source) = source {
            let token = source.identity.segment_token.as_str();
            if let Entry::Vacant(entry) = histories.entry(token) {
                entry.insert(build_records(ctx, token, &history_records)?);
            }
        }
        let admitted = (|| {
            let source = source?;
            let token = source.identity.segment_token.as_str();
            let history = histories.get(token)?;
            let placement = history.placement(source.identity.record_ordinal)?;
            if !matches_history(feature, placement, source.header.flags)
                || feature.dependencies.len() != source.participants.references().len()
            {
                return None;
            }
            let label = labels
                .get(&(token, source.identity.record_ordinal))?
                .as_ref()?;
            let result = results.get(&feature.id)?.as_ref()?;
            let mut dependencies = feature.dependencies.iter();
            Some(
                index.matches_projection(source, label, feature, result, |ordinal| {
                    let (identity, native, flags) = records.get(&(token, ordinal))?.as_ref()?;
                    let id = crate::feature::mirror::seed_feature_id(
                        history,
                        source.identity.record_ordinal,
                        identity,
                    )?;
                    let seed = features.get(&id)?.as_ref()?;
                    let before = history.placement(ordinal)?;
                    if !matches_history(seed, before, *flags)
                        || dependencies.next() != Some(&id)
                        || seed.native_ref.as_deref() != Some(native.as_str())
                    {
                        return None;
                    }
                    Some(id)
                }),
            )
        })() == Some(true);
        if !admitted {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor mirror seeds, plane, mode, or result do not match its PmDc records"
                    .into(),
                Some(feature.id.as_str().to_owned()),
            ));
        }
    }
    Ok(outputs)
}

fn requires_mirror_validation(
    feature: &Feature,
    source: Option<&crate::feature::PmDcPatternFeature>,
) -> bool {
    is_mirror_construction(feature)
        || (source.is_some_and(crate::feature::mirror::is_mirror_record)
            && !matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ))
}

fn is_mirror_construction(feature: &Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MirrorShape { .. })
    ) || matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. })
            if matches!(pattern.definition(), PatternTransform::Mirror { .. })
    )
}

fn matches_history(feature: &Feature, placement: HistoryPlacement, flags: u32) -> bool {
    let suppressed = flags & SUPPRESSED_FLAG != 0;
    let property = |key: &str| feature.source_properties.get(key).map(String::as_str);
    feature.ordinal == placement.ordinal
        && feature.suppressed == Some(suppressed || placement.below_end_of_part)
        && property("history") == (!placement.model_history).then_some("flat_pattern")
        && property("suppressed") == suppressed.then_some("true")
        && property("end_of_part") == placement.below_end_of_part.then_some("below")
}

#[cfg(test)]
mod tests;
