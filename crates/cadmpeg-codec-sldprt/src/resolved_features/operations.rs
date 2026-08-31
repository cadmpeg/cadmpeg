//! Boolean operation codes for extrusion, revolution and sweep.

use super::is_class_token;
use super::scalars::feature_object_name;
use crate::classification::{classify, FeatureClass};
use crate::layout::extrusion_sparse_operation_trailer as sparse_tr;
use crate::records::{Feature, FeatureInputLane, FeatureInputName};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition};
use std::collections::HashMap;

pub(crate) const SPLIT_LINE_MODE_PROPERTY: &str = "SplitLineMode";
pub(crate) const SPLIT_LINE_PROJECTION_MODE: &str = "Projection";
pub(crate) const SPLIT_LINE_TOOL_PROPERTY: &str = "SplitLineTool";

pub(super) fn repeated_class_token(payload: &[u8], name_offset: usize) -> Option<u16> {
    let start = name_offset.checked_sub(2)?;
    View::u16_le_at(payload, start)
}

/// Selects one operation code from byte-valid layout candidates.
///
/// A declared padding selects at most one versioned candidate. Without a
/// declaration, all byte-valid candidates must agree; their order is not
/// evidence and cannot choose the operation.
fn consistent_operation_code(
    mut candidates: impl Iterator<Item = u32>,
    padding_declared: bool,
) -> Option<u32> {
    let first = candidates.next()?;
    if !padding_declared && candidates.any(|candidate| candidate != first) {
        return None;
    }
    Some(first)
}

pub(super) fn feature_operation_code(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
    class: Option<&str>,
    form_padding: Option<usize>,
) -> Option<u32> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let direct_class = lane
        .classes
        .iter()
        .find(|class| class.offset + 6 + class.name.len() as u64 == name.offset);
    if let Some(class) = direct_class {
        let class_offset = usize::try_from(class.offset).ok()?;
        if lane
            .native_payload
            .get(class_offset.checked_sub(4)?..class_offset)
            == Some(&[0xff; 4])
        {
            return Some(u32::MAX);
        }
        let candidates = [8usize, 4]
            .into_iter()
            .filter(|padding| form_padding.is_none_or(|expected| expected == *padding))
            .filter_map(|padding| {
                let code_offset = class_offset.checked_sub(4 + padding)?;
                if !lane
                    .native_payload
                    .get(code_offset + 4..class_offset)?
                    .iter()
                    .all(|byte| *byte == 0)
                {
                    return None;
                }
                View::u32_le_at(&lane.native_payload, code_offset)
            });
        return consistent_operation_code(candidates, form_padding.is_some());
    }

    let repeated_token = repeated_class_token(&lane.native_payload, name_offset)?;
    if !is_class_token(repeated_token) {
        return None;
    }
    if let Some(code_offset) = name_offset.checked_sub(14).filter(|code_offset| {
        repeated_token == 0x8000
            && lane.native_payload.get(code_offset + 4..code_offset + 8) == Some(&[0; 4])
    }) {
        return View::u32_le_at(&lane.native_payload, code_offset);
    }

    let paddings: &[usize] = if class == Some("moICE_c") {
        &[8, 4, 0]
    } else {
        &[8, 4]
    };
    let candidates = paddings.iter().copied().filter_map(|padding| {
        if padding != 0 && form_padding.is_some_and(|expected| expected != padding) {
            return None;
        }
        let code_offset = name_offset.checked_sub(6 + padding)?;
        if !lane
            .native_payload
            .get(code_offset + 4..name_offset - 2)?
            .iter()
            .all(|byte| *byte == 0)
        {
            return None;
        }
        View::u32_le_at(&lane.native_payload, code_offset)
    });
    consistent_operation_code(candidates, form_padding.is_some())
}

pub(super) fn revolution_operation(class: Option<&str>, code: u32) -> Option<BooleanOp> {
    match (class, code) {
        (Some("moRevolution_c"), 5 | 6 | 11 | 60 | 20_322 | 22_016) => Some(BooleanOp::NewBody),
        (Some("moRevolution_c"), 8) => Some(BooleanOp::Join),
        (Some("moRevCut_c"), _) => Some(BooleanOp::Cut),
        _ => None,
    }
}

pub(super) fn extrusion_operation(class: Option<&str>, code: u32) -> Option<BooleanOp> {
    match (class, code) {
        (Some("moExtrusion_c"), 1 | 4 | 82) | (Some("moICE_c"), 6 | 21 | 0x3ee4_f8b5) | (_, 3) => {
            Some(BooleanOp::Join)
        }
        (Some("moICE_c"), 0 | 1 | 2 | 5 | 7 | 10 | 14 | 15 | 22_993 | u32::MAX) => {
            Some(BooleanOp::Cut)
        }
        _ => None,
    }
}

/// Bind operation discriminators shared by geometry and metadata decode.
pub(crate) fn bind_feature_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) {
    bind_extrusion_operations(features, histories, lanes, form_padding);
    bind_revolution_operations(features, histories, lanes, form_padding);
    bind_sweep_operations(features, histories, lanes, form_padding);
}

/// Project revolution Boolean form words from declared and compact objects.
pub(crate) fn bind_revolution_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let FeatureDefinition::Revolve { op, .. } = &mut feature.definition else {
            continue;
        };
        if *op != BooleanOp::Unresolved {
            continue;
        }
        let Some(history) = feature
            .native_ref
            .as_deref()
            .and_then(|native| history_features.get(native).copied())
        else {
            continue;
        };
        let mut operations = lanes.iter().filter_map(|lane| {
            let name = feature_object_name(history, lane)?;
            revolution_operation(
                history.input_class.as_deref(),
                feature_operation_code(lane, name, history.input_class.as_deref(), form_padding)?,
            )
        });
        let Some(first) = operations.next() else {
            continue;
        };
        if operations.all(|operation| operation == first) {
            *op = first;
        }
    }
}

/// Project compact solid-sweep Boolean operation discriminators.
pub(crate) fn bind_sweep_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Solid { op },
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if *op != BooleanOp::Unresolved {
            continue;
        }
        let Some(history) = feature
            .native_ref
            .as_deref()
            .and_then(|native| history_features.get(native).copied())
        else {
            continue;
        };
        let mut operations = lanes.iter().filter_map(|lane| {
            let name = feature_object_name(history, lane)?;
            match (
                history.input_class.as_deref(),
                feature_operation_code(lane, name, history.input_class.as_deref(), form_padding)?,
            ) {
                (Some("moSweep_c"), 15) => Some(BooleanOp::Join),
                _ => None,
            }
        });
        let Some(first) = operations.next() else {
            continue;
        };
        if operations.all(|operation| operation == first) {
            *op = first;
        }
    }
}

/// Inline extrusion trailer fields: the family word and operation byte.
pub(super) fn feature_inline_operation_fields(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
) -> Option<(u16, u8)> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let name_bytes = name.value.encode_utf16().count().checked_mul(2)?;
    let trailer = name_offset.checked_add(6 + name_bytes)?;
    let bytes = lane.native_payload.get(trailer..trailer + 19)?;
    let terminated = bytes[sparse_tr::SPARSE_ZERO_PREFIX..19] == [0xff, 0xfe, 0xff]
        || lane
            .native_payload
            .get(trailer + sparse_tr::SPARSE_ZERO_PREFIX..trailer + sparse_tr::LEN)
            .is_some_and(|suffix| {
                (suffix[..sparse_tr::SPARSE_MARKER - sparse_tr::SPARSE_ZERO_PREFIX] == [0; 6]
                    && suffix[sparse_tr::SPARSE_MARKER - sparse_tr::SPARSE_ZERO_PREFIX
                        ..sparse_tr::FIRST_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX]
                        == [1, 0]
                    && suffix[sparse_tr::FIRST_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX
                        ..sparse_tr::OPTIONAL_IDENTITY - sparse_tr::SPARSE_ZERO_PREFIX]
                        != [0, 0]
                    && suffix[sparse_tr::OPTIONAL_IDENTITY - sparse_tr::SPARSE_ZERO_PREFIX
                        ..sparse_tr::ZERO_BEFORE_FINAL_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX]
                        != [0xff; 4]
                    // The common sparse form places its second token at +38.
                    // Older files use u64(1) in the otherwise-zero field at +30.
                    && (suffix[sparse_tr::ZERO_BEFORE_FINAL_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX
                        ..sparse_tr::FINAL_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX]
                        == [0; 8]
                        || suffix[sparse_tr::ZERO_BEFORE_FINAL_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX
                            ..sparse_tr::FINAL_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX]
                            == [1, 0, 0, 0, 0, 0, 0, 0])
                    && suffix[sparse_tr::FINAL_TOKEN - sparse_tr::SPARSE_ZERO_PREFIX..]
                        != [0, 0])
                    // A compact continuation retains a secondary family word
                    // in the first two bytes before the marker.
                    || (suffix[..4] == [0; 4]
                        && matches!(
                            View::u16_le_at(suffix, 4),
                            Some(0x00b2 | 0x00b3)
                        )
                        && suffix[6..8] == [1, 0]
                        && suffix[8..12] != [0; 4]
                        && suffix[12..22] == [0; 10]
                        && suffix[22..24] != [0, 0])
                    || (suffix[..4] == [0, 0, 1, 0]
                        && suffix[4..8] != [0; 4]
                        && suffix[8..18] == [0; 10]
                        && suffix[18..20] != [0, 0]
                        && suffix[20..24] == [0; 4])
            });
    if bytes[sparse_tr::ZERO_HEADER..sparse_tr::FAMILY] != [0; 4]
        || bytes[sparse_tr::OBJECT_ID..sparse_tr::ZERO_AFTER_OBJECT]
            != name.object_id?.to_le_bytes()
        || bytes[sparse_tr::ZERO_AFTER_OBJECT..sparse_tr::SPARSE_ZERO_PREFIX] != [0; 4]
        || !terminated
        || !matches!(bytes[sparse_tr::OPERATION], 0 | 2)
    {
        return None;
    }
    Some((
        View::u16_le_at(bytes, sparse_tr::FAMILY)?,
        bytes[sparse_tr::OPERATION],
    ))
}

/// Project an inline Boolean operation from a recognized complete family.
pub(super) fn feature_inline_operation(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
) -> Option<BooleanOp> {
    match feature_inline_operation_fields(lane, name)? {
        (0x0140, 0) => Some(BooleanOp::Join),
        (0x01ca, 0 | 2) => Some(BooleanOp::Cut),
        _ => None,
    }
}

/// Project the feature-input operation discriminator onto typed extrusions.
pub(crate) fn bind_extrusion_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let history_by_id = history_features
        .iter()
        .map(|feature| (feature.id.as_str(), *feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let FeatureDefinition::Extrude { op, .. } = &mut feature.definition else {
            continue;
        };
        if *op != BooleanOp::Unresolved {
            continue;
        }
        let Some(history) = feature
            .native_ref
            .as_deref()
            .and_then(|native| history_by_id.get(native).copied())
        else {
            continue;
        };
        let mut operations = lanes.iter().filter_map(|lane| {
            let name = feature_object_name(history, lane)?;
            if let Some(operation) = feature_inline_operation(lane, name) {
                return Some(operation);
            }
            extrusion_operation(
                history.input_class.as_deref(),
                feature_operation_code(lane, name, history.input_class.as_deref(), form_padding)?,
            )
        });
        let Some(first) = operations.next() else {
            continue;
        };
        if operations.all(|operation| operation == first) {
            *op = first;
        }
    }
}

#[derive(Clone, Copy)]
enum OperationKind {
    Extrusion,
    Revolution,
}

/// Preserve a Boolean operation that is invariant across a configuration lane
/// when that lane carries no independent operation carrier.
pub(crate) fn inherit_configuration_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    base_features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) {
    let history_by_id = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let base_definitions = base_features
        .iter()
        .map(|feature| (&feature.id, &feature.definition))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(base_definition) = base_definitions.get(&feature.id) else {
            continue;
        };
        let Some(history) = feature
            .native_ref
            .as_deref()
            .and_then(|native| history_by_id.get(native).copied())
        else {
            continue;
        };
        let operation_kind = match (&feature.definition, *base_definition) {
            (
                FeatureDefinition::Extrude { op, .. },
                FeatureDefinition::Extrude { op: base_op, .. },
            ) if *op == BooleanOp::Unresolved && *base_op != BooleanOp::Unresolved => {
                OperationKind::Extrusion
            }
            (
                FeatureDefinition::Revolve { op, .. },
                FeatureDefinition::Revolve { op: base_op, .. },
            ) if *op == BooleanOp::Unresolved && *base_op != BooleanOp::Unresolved => {
                OperationKind::Revolution
            }
            _ => continue,
        };
        if lanes
            .iter()
            .any(|lane| operation_carrier_present(operation_kind, history, lane, form_padding))
        {
            continue;
        }
        match (&mut feature.definition, operation_kind, *base_definition) {
            (
                FeatureDefinition::Extrude { op, .. },
                OperationKind::Extrusion,
                FeatureDefinition::Extrude { op: base_op, .. },
            )
            | (
                FeatureDefinition::Revolve { op, .. },
                OperationKind::Revolution,
                FeatureDefinition::Revolve { op: base_op, .. },
            ) if *op == BooleanOp::Unresolved && *base_op != BooleanOp::Unresolved => {
                *op = *base_op;
            }
            _ => {}
        }
    }
}

fn operation_carrier_present(
    kind: OperationKind,
    feature: &Feature,
    lane: &FeatureInputLane,
    form_padding: Option<usize>,
) -> bool {
    let source_matches = feature
        .source_id
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
        .map(|source_id| {
            lane.names
                .iter()
                .filter(|name| name.object_id == Some(source_id))
                .collect::<Vec<_>>()
        })
        .filter(|matches| !matches.is_empty());
    let candidates = source_matches.unwrap_or_else(|| {
        lane.names
            .iter()
            .filter(|name| name.value == feature.name)
            .collect::<Vec<_>>()
    });
    let [name] = candidates.as_slice() else {
        return candidates.len() > 1;
    };
    if matches!(kind, OperationKind::Extrusion)
        && feature_inline_operation_fields(lane, name).is_some()
    {
        return true;
    }
    let Some(code) =
        feature_operation_code(lane, name, feature.input_class.as_deref(), form_padding)
    else {
        return false;
    };
    matches!(kind, OperationKind::Revolution)
        || !(code == 11
            && matches!(
                feature.input_class.as_deref(),
                Some("moExtrusion_c" | "moICE_c" | "moCut_c")
            ))
}

/// Establish projected split-line mode and source sketch from each
/// `moPLine_c` feature-input object while native dimension values remain raw.
pub(crate) fn enrich_history_split_lines(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let mut observations = HashMap::<String, (bool, bool)>::new();
    for lane in lanes {
        let mut objects = features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, (start, feature)) in objects.iter().enumerate() {
            if feature.input_class.as_deref() != Some("moPLine_c") {
                continue;
            }
            let end = objects
                .get(index + 1)
                .map_or(lane.native_payload.len() as u64, |(offset, _)| *offset);
            let project_classes = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moPLineProject_c" && class.offset >= *start && class.offset < end
                })
                .count();
            let observation = observations.entry(feature.id.clone()).or_default();
            if project_classes == 1 {
                observation.0 = true;
            } else {
                observation.1 = true;
            }
        }
    }
    let tools = histories
        .iter()
        .flat_map(|history| {
            history.features.iter().filter_map(|feature| {
                (observations.get(&feature.id) == Some(&(true, false)))
                    .then(|| split_line_source_sketch(feature, &history.features))
                    .flatten()
                    .map(|tool| (feature.id.clone(), tool.id.clone()))
            })
        })
        .collect::<HashMap<_, _>>();
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if observations.get(&feature.id) == Some(&(true, false)) {
            feature.properties.insert(
                SPLIT_LINE_MODE_PROPERTY.into(),
                SPLIT_LINE_PROJECTION_MODE.into(),
            );
            if let Some(tool) = tools.get(&feature.id) {
                feature
                    .properties
                    .insert(SPLIT_LINE_TOOL_PROPERTY.into(), tool.clone());
            }
        }
    }
}

fn split_line_source_sketch<'a>(
    feature: &Feature,
    history_features: &'a [Feature],
) -> Option<&'a Feature> {
    if feature.parameters.is_empty() {
        return None;
    }
    let source = feature.source_id.as_deref()?.parse::<u32>().ok()?;
    let mut candidates = history_features.iter().filter(|candidate| {
        candidate.parent == feature.parent
            && classify(candidate) == Some(FeatureClass::Sketch)
            && candidate.input_class.as_deref() == Some("moProfileFeature_c")
            && candidate.parameters == feature.parameters
            && candidate
                .source_id
                .as_deref()
                .and_then(|value| value.parse::<u32>().ok())
                .is_some_and(|candidate_source| candidate_source > 0 && candidate_source < source)
    });
    let tool = candidates.next()?;
    candidates.next().is_none().then_some(tool)
}

#[cfg(test)]
mod operations_tests;
