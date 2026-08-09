//! Boolean operation codes for extrusion, revolution and sweep.

use super::scalars::feature_object_name;
use crate::records::{FeatureInputLane, FeatureInputName};
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormCodePadding {
    Four,
    Eight,
}

impl FormCodePadding {
    fn bytes(self) -> usize {
        match self {
            Self::Four => 4,
            Self::Eight => 8,
        }
    }
}

pub(crate) fn form_code_padding(sw_version: Option<&str>) -> Option<FormCodePadding> {
    let version = sw_version?.parse::<u32>().ok()?;
    (version > 0).then_some(if version >= 12_000 {
        FormCodePadding::Eight
    } else {
        FormCodePadding::Four
    })
}

pub(super) fn repeated_class_token(payload: &[u8], name_offset: usize) -> Option<u16> {
    let start = name_offset.checked_sub(2)?;
    Some(u16::from_le_bytes(
        payload.get(start..name_offset)?.try_into().ok()?,
    ))
}

pub(super) fn feature_operation_code(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
    class: Option<&str>,
    form_padding: Option<FormCodePadding>,
) -> Option<u32> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let direct_class = lane
        .classes
        .iter()
        .find(|class| class.offset + 6 + class.name.len() as u64 == name.offset);
    let code_offset = if let Some(class) = direct_class {
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
            .filter(|padding| form_padding.is_none_or(|expected| expected.bytes() == *padding))
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
                let code = u32::from_le_bytes(
                    lane.native_payload
                        .get(code_offset..code_offset + 4)?
                        .try_into()
                        .ok()?,
                );
                Some((code_offset, code))
            })
            .collect::<Vec<_>>();
        if form_padding.is_none() {
            // A zero form code makes four- and eight-byte padding both match. A
            // different candidate code is not a byte-level discriminator.
            match candidates.as_slice() {
                [(code_offset, _)] => *code_offset,
                [(first_offset, first_code), (_, second_code)] if first_code == second_code => {
                    *first_offset
                }
                _ => return None,
            }
        } else {
            candidates.first().map(|(code_offset, _)| *code_offset)?
        }
    } else {
        let repeated_token = repeated_class_token(&lane.native_payload, name_offset)?;
        if repeated_token & 0x8000 == 0 || repeated_token == u16::MAX {
            return None;
        }
        let compact_instance = name_offset.checked_sub(14).filter(|code_offset| {
            repeated_token == 0x8000
                && lane.native_payload.get(code_offset + 4..code_offset + 8) == Some(&[0; 4])
        });
        compact_instance.or_else(|| {
            let paddings: &[usize] = if class == Some("moICE_c") {
                &[8, 4, 0]
            } else {
                &[8, 4]
            };
            paddings.iter().copied().find_map(|padding| {
                if padding != 0 && form_padding.is_some_and(|expected| expected.bytes() != padding)
                {
                    return None;
                }
                let code_offset = name_offset.checked_sub(6 + padding)?;
                lane.native_payload
                    .get(code_offset + 4..name_offset - 2)?
                    .iter()
                    .all(|byte| *byte == 0)
                    .then_some(code_offset)
            })
        })?
    };
    Some(u32::from_le_bytes(
        lane.native_payload
            .get(code_offset..code_offset + 4)?
            .try_into()
            .ok()?,
    ))
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

/// Project revolution Boolean form words from declared and compact objects.
pub(crate) fn bind_revolution_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<FormCodePadding>,
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
    form_padding: Option<FormCodePadding>,
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

/// Inline extrusion trailer fields: the low byte of the family word and the
/// operation byte. The family word is `0x0140` for `moExtrusion_c` objects
/// and `0x01ca` for `moICE_c` objects.
pub(super) fn feature_inline_operation_fields(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
) -> Option<(u8, u8)> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let name_bytes = name.value.encode_utf16().count().checked_mul(2)?;
    let trailer = name_offset.checked_add(6 + name_bytes)?;
    let bytes = lane.native_payload.get(trailer..trailer + 19)?;
    let terminated = bytes[16..19] == [0xff, 0xfe, 0xff]
        || lane
            .native_payload
            .get(trailer + 16..trailer + 40)
            .is_some_and(|suffix| {
                (suffix[..6] == [0; 6]
                    && suffix[6..8] == [1, 0]
                    && suffix[8..10] != [0, 0]
                    && suffix[10..22] == [0; 12]
                    && suffix[22..24] != [0, 0])
                    || (suffix[..4] == [0, 0, 1, 0]
                        && suffix[4..8] != [0; 4]
                        && suffix[8..18] == [0; 10]
                        && suffix[18..20] != [0, 0]
                        && suffix[20..24] == [0; 4])
            });
    if bytes[..4] != [0; 4]
        || bytes[5] != 1
        || bytes[8..12] != name.object_id?.to_le_bytes()
        || bytes[12..16] != [0; 4]
        || !terminated
        || !matches!(bytes[6], 0 | 2)
    {
        return None;
    }
    Some((bytes[4], bytes[6]))
}

/// Inline Boolean operation, when the trailer carries one. A zero operation
/// byte on an `moICE_c` object is not an operation carrier; those objects use
/// class-scoped form semantics instead.
pub(super) fn feature_inline_operation(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
) -> Option<BooleanOp> {
    match feature_inline_operation_fields(lane, name)? {
        (0x40, 0) => Some(BooleanOp::Join),
        (0xca, 2) => Some(BooleanOp::Cut),
        _ => None,
    }
}

pub(super) fn class_scoped_extrusion_operation(
    feature: &crate::records::Feature,
    features: &[&crate::records::Feature],
    lane: &FeatureInputLane,
    name: &FeatureInputName,
    form_padding: Option<FormCodePadding>,
) -> Option<BooleanOp> {
    if feature.input_class.as_deref() != Some("moICE_c")
        || feature_inline_operation_fields(lane, name) != Some((0xca, 0))
        || !lane.classes.iter().any(|class| {
            class.name == "moICE_c" && class.offset + 6 + class.name.len() as u64 == name.offset
        })
    {
        return None;
    }
    let siblings = features
        .iter()
        .copied()
        .filter(|candidate| {
            candidate.id != feature.id && candidate.input_class == feature.input_class
        })
        .collect::<Vec<_>>();
    if siblings.len() < 2 {
        return None;
    }
    let mut operations = siblings.iter().map(|sibling| {
        let sibling_name = feature_object_name(sibling, lane)?;
        extrusion_operation(
            sibling.input_class.as_deref(),
            feature_operation_code(
                lane,
                sibling_name,
                sibling.input_class.as_deref(),
                form_padding,
            )?,
        )
    });
    let operation = operations.next()??;
    operations
        .all(|candidate| candidate == Some(operation))
        .then_some(operation)
}

/// Project the feature-input operation discriminator onto typed extrusions.
pub(crate) fn bind_extrusion_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<FormCodePadding>,
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
            if let Some(operation) = class_scoped_extrusion_operation(
                history,
                &history_features,
                lane,
                name,
                form_padding,
            ) {
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

#[cfg(test)]
mod operations_tests;
