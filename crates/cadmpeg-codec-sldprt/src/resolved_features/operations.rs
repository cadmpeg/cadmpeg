//! Boolean operation codes for extrusion, revolution and sweep.

use super::is_class_token;
use super::scalars::{lane_object_names, NameLookup, ObjectNames};
use super::{classes_within, sorted_classes};
use crate::classification::{classify, FeatureClass};
use crate::layout::extrusion_sparse_operation_trailer as sparse_tr;
use crate::records::ObjectId;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation};
use std::collections::HashMap;

/// Each history feature by id; a repeated id keeps its last feature.
fn history_features_by_id<'h>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    histories: &'h [FeatureHistory],
    operation: &'static str,
) -> Result<HashMap<&'h str, &'h Feature>, CodecError> {
    let mut index = HashMap::new();
    for history in ctx.admit_iter(histories, operation)? {
        for feature in ctx.admit_iter(&history.features, operation)? {
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut index, feature.id.as_str(), feature, operation)
            })?;
        }
    }
    Ok(index)
}

/// One lane's class declarations by the offset of the object name that
/// directly follows each; the first declaration at an offset wins.
type DirectClasses<'l> = HashMap<u64, &'l FeatureInputClass>;

fn direct_classes<'l>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    lanes: &'l [FeatureInputLane],
) -> Result<Vec<DirectClasses<'l>>, CodecError> {
    const OPERATION: &str = "index SLDPRT operation classes";
    let mut indexes = Vec::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        let mut index = HashMap::new();
        for class in ctx.admit_iter(&lane.classes, OPERATION)? {
            let Some(name_offset) = class
                .offset
                .checked_add(6)
                .and_then(|offset| offset.checked_add(u64_from_index(class.name.len())))
            else {
                continue;
            };
            storage.with_storage(|| {
                ctx.entry_hash_map(&mut index, name_offset, OPERATION)
                    .map(|slot| {
                        slot.or_insert(class);
                    })
            })?;
        }
        storage.with_storage(|| ctx.push_vec(&mut indexes, index, OPERATION))?;
    }
    Ok(indexes)
}

/// The indexes every operation binder reads, built once per call.
struct OperationIndexes<'h, 'l, 'ctx> {
    history_by_id: HashMap<&'h str, &'h Feature>,
    direct_classes: Vec<DirectClasses<'l>>,
    object_names: Vec<ObjectNames<'l, 'ctx>>,
}

impl<'h, 'l, 'ctx> OperationIndexes<'h, 'l, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        storage: &mut ScopedReservation<'_>,
        histories: &'h [FeatureHistory],
        lanes: &'l [FeatureInputLane],
    ) -> Result<Self, CodecError> {
        Ok(Self {
            history_by_id: history_features_by_id(
                ctx,
                storage,
                histories,
                "index SLDPRT operation history",
            )?,
            direct_classes: direct_classes(ctx, storage, lanes)?,
            object_names: lane_object_names(ctx, storage, lanes)?,
        })
    }

    fn history(
        &self,
        ctx: &DecodeContext<'_>,
        native_ref: Option<&str>,
    ) -> Result<Option<&'h Feature>, CodecError> {
        let Some(native) = native_ref else {
            return Ok(None);
        };
        Ok(ctx
            .get_hash_map(
                &self.history_by_id,
                native,
                "find SLDPRT operation history feature",
            )?
            .copied())
    }
}

/// The one operation every lane that projects one agrees on. `project`
/// receives each lane with its direct classes and the lane name of `feature`.
fn agreed_lane_operation<T: Copy + PartialEq>(
    ctx: &DecodeContext<'_>,
    lanes: &[FeatureInputLane],
    indexes: &OperationIndexes<'_, '_, '_>,
    feature: &Feature,
    mut project: impl FnMut(
        &FeatureInputLane,
        &DirectClasses<'_>,
        &FeatureInputName,
    ) -> Result<Option<T>, CodecError>,
    operation: &'static str,
) -> Result<Option<T>, CodecError> {
    let mut step = |((lane, direct), names): (
        (&FeatureInputLane, &DirectClasses<'_>),
        &ObjectNames<'_, '_>,
    )| {
        match names.of(ctx, feature)? {
            Some(name) => project(lane, direct, name),
            None => Ok(None),
        }
    };
    let mut remaining = lanes
        .iter()
        .zip(&indexes.direct_classes)
        .zip(&indexes.object_names);
    let Some(first) = ctx.find_map(&mut remaining, &mut step, operation)? else {
        return Ok(None);
    };
    let disagrees = ctx.any_by(
        &mut remaining,
        |lane| Ok(step(lane)?.is_some_and(|candidate| candidate != first)),
        operation,
    )?;
    Ok((!disagrees).then_some(first))
}

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

#[cfg(test)]
fn feature_operation_code(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
    class: Option<&str>,
    form_padding: Option<usize>,
) -> Option<u32> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut storage = ctx.reserve_scoped(0, "test operation classes").unwrap();
    let direct = direct_classes(&ctx, &mut storage, std::slice::from_ref(lane)).unwrap();
    operation_code(&ctx, lane, &direct[0], name, class, form_padding).unwrap()
}

fn operation_code(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    direct_classes: &DirectClasses<'_>,
    name: &FeatureInputName,
    class: Option<&str>,
    form_padding: Option<usize>,
) -> Result<Option<u32>, CodecError> {
    let direct_class = ctx
        .get_hash_map(
            direct_classes,
            &name.offset,
            "find SLDPRT operation direct class",
        )?
        .copied();
    Ok(direct_operation_code(
        lane,
        direct_class,
        name,
        class,
        form_padding,
    ))
}

fn direct_operation_code(
    lane: &FeatureInputLane,
    direct_class: Option<&FeatureInputClass>,
    name: &FeatureInputName,
    class: Option<&str>,
    form_padding: Option<usize>,
) -> Option<u32> {
    let name_offset = usize::try_from(name.offset).ok()?;
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

fn revolution_operation(class: Option<&str>, code: u32) -> Option<BooleanOp> {
    match (class, code) {
        (Some("moRevolution_c"), 5 | 6 | 11 | 60 | 20_322 | 22_016) => Some(BooleanOp::NewBody),
        (Some("moRevolution_c"), 8) => Some(BooleanOp::Join),
        (Some("moRevCut_c"), _) => Some(BooleanOp::Cut),
        _ => None,
    }
}

fn extrusion_operation(class: Option<&str>, code: u32) -> Option<BooleanOp> {
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
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "SLDPRT operation indexes")?;
    let indexes = OperationIndexes::new(ctx, &mut storage, histories, lanes)?;
    bind_extrusion_operations_with(ctx, features, &indexes, lanes, form_padding)?;
    bind_revolution_operations_with(ctx, features, &indexes, lanes, form_padding)?;
    bind_sweep_operations_with(ctx, features, &indexes, lanes, form_padding)
}

/// Project revolution Boolean form words from declared and compact objects.
pub(crate) fn bind_revolution_operations(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "SLDPRT operation indexes")?;
    let indexes = OperationIndexes::new(ctx, &mut storage, histories, lanes)?;
    bind_revolution_operations_with(ctx, features, &indexes, lanes, form_padding)
}

fn bind_revolution_operations_with(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    indexes: &OperationIndexes<'_, '_, '_>,
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    for feature in ctx.admit_iter(features, "bind SLDPRT revolution operations")? {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Revolve {
                op: BooleanOp::Unresolved,
                ..
            })
        ) {
            continue;
        }
        let Some(history) = indexes.history(ctx, feature.native_ref.as_deref())? else {
            continue;
        };
        let class = history.input_class.as_deref();
        let Some(resolved) = agreed_lane_operation(
            ctx,
            lanes,
            indexes,
            history,
            |lane, direct, name| {
                Ok(
                    operation_code(ctx, lane, direct, name, class, form_padding)?
                        .and_then(|code| revolution_operation(class, code)),
                )
            },
            "bind SLDPRT revolution operations",
        )?
        else {
            continue;
        };
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }) = definition {
                *op = resolved;
            }
        });
    }
    Ok(())
}

/// Project compact solid-sweep Boolean operation discriminators.
pub(crate) fn bind_sweep_operations(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "SLDPRT operation indexes")?;
    let indexes = OperationIndexes::new(ctx, &mut storage, histories, lanes)?;
    bind_sweep_operations_with(ctx, features, &indexes, lanes, form_padding)
}

fn bind_sweep_operations_with(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    indexes: &OperationIndexes<'_, '_, '_>,
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    for feature in ctx.admit_iter(features, "bind SLDPRT sweep operations")? {
        let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        if !matches!(
            shape.mode(),
            cadmpeg_ir::features::SweepMode::Unresolved { .. }
        ) {
            continue;
        }
        let Some(history) = indexes.history(ctx, feature.native_ref.as_deref())? else {
            continue;
        };
        let class = history.input_class.as_deref();
        let Some(resolved) = agreed_lane_operation(
            ctx,
            lanes,
            indexes,
            history,
            |lane, direct, name| {
                Ok(
                    match (
                        class,
                        operation_code(ctx, lane, direct, name, class, form_padding)?,
                    ) {
                        (Some("moSweep_c"), Some(15)) => Some(BooleanOp::Join),
                        _ => None,
                    },
                )
            },
            "bind SLDPRT sweep operations",
        )?
        else {
            continue;
        };
        let op = match resolved {
            BooleanOp::Join => cadmpeg_ir::features::SolidSweepOperation::Join,
            BooleanOp::Cut => cadmpeg_ir::features::SolidSweepOperation::Cut,
            BooleanOp::Intersect => cadmpeg_ir::features::SolidSweepOperation::Intersect,
            BooleanOp::NewBody => cadmpeg_ir::features::SolidSweepOperation::NewBody,
            BooleanOp::Unresolved => continue,
        };
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            else {
                return;
            };
            // The lane always names a solid result, which admits every
            // cross-section, so the shape is built once from the sections it
            // already holds.
            let (section, sections) =
                std::mem::replace(shape, cadmpeg_ir::features::SweepShape::unresolved(None))
                    .into_sections();
            *shape = cadmpeg_ir::features::SweepShape::solid_sections(op, section, sections);
        });
    }
    Ok(())
}

/// Inline extrusion trailer fields: the family word and operation byte.
pub(super) fn feature_inline_operation_fields(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    name: &FeatureInputName,
) -> Result<Option<(u16, u8)>, CodecError> {
    let name_units = ctx.admit_iter(name.value.as_str(), "measure SLDPRT inline operation name")?.encode_utf16().count();
    Ok((|| {
    let name_offset = usize::try_from(name.offset).ok()?;
    let name_bytes = name_units.checked_mul(2)?;
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
            != name.object_id.and_then(ObjectId::value)?.to_le_bytes()
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
    })())
}

/// Project an inline Boolean operation from a recognized complete family.
fn feature_inline_operation(ctx: &DecodeContext<'_>, lane: &FeatureInputLane, name: &FeatureInputName) -> Result<Option<BooleanOp>, CodecError> {
    Ok(match feature_inline_operation_fields(ctx, lane, name)? {
        Some((0x0140, 0)) => Some(BooleanOp::Join),
        Some((0x01ca, 0 | 2)) => Some(BooleanOp::Cut),
        _ => None,
    })
}

/// Project the feature-input operation discriminator onto typed extrusions.
pub(crate) fn bind_extrusion_operations(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "SLDPRT operation indexes")?;
    let indexes = OperationIndexes::new(ctx, &mut storage, histories, lanes)?;
    bind_extrusion_operations_with(ctx, features, &indexes, lanes, form_padding)
}

fn bind_extrusion_operations_with(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    indexes: &OperationIndexes<'_, '_, '_>,
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    for feature in ctx.admit_iter(features, "bind SLDPRT extrusion operations")? {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Unresolved,
                ..
            })
        ) {
            continue;
        }
        let Some(history) = indexes.history(ctx, feature.native_ref.as_deref())? else {
            continue;
        };
        let class = history.input_class.as_deref();
        let Some(resolved) = agreed_lane_operation(
            ctx,
            lanes,
            indexes,
            history,
            |lane, direct, name| {
                if let Some(operation) = feature_inline_operation(ctx, lane, name)? {
                    return Ok(Some(operation));
                }
                Ok(
                    operation_code(ctx, lane, direct, name, class, form_padding)?
                        .and_then(|code| extrusion_operation(class, code)),
                )
            },
            "bind SLDPRT extrusion operations",
        )?
        else {
            continue;
        };
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition {
                *op = resolved;
            }
        });
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum OperationKind {
    Extrusion,
    Revolution,
}

/// Preserve a Boolean operation that is invariant across a configuration lane
/// when that lane carries no independent operation carrier.
pub(crate) fn inherit_configuration_operations(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    base_features: &[cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    form_padding: Option<usize>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "inherit SLDPRT configuration operations";
    let mut storage = ctx.reserve_scoped(0, "SLDPRT operation indexes")?;
    let indexes = OperationIndexes::new(ctx, &mut storage, histories, lanes)?;
    let mut base_operations = HashMap::new();
    for feature in ctx.admit_iter(base_features, "index SLDPRT base definitions")? {
        let base = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. })
                if *op != BooleanOp::Unresolved =>
            {
                Some((OperationKind::Extrusion, *op))
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. })
                if *op != BooleanOp::Unresolved =>
            {
                Some((OperationKind::Revolution, *op))
            }
            _ => None,
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut base_operations,
                &feature.id,
                base,
                "index SLDPRT base definitions",
            )
        })?;
    }
    for feature in ctx.admit_iter(features, OPERATION)? {
        let Some(&Some((kind, base_op))) =
            ctx.get_hash_map(&base_operations, &feature.id, OPERATION)?
        else {
            continue;
        };
        let unresolved = match (kind, feature.evaluation.definition()) {
            (
                OperationKind::Extrusion,
                FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }),
            )
            | (
                OperationKind::Revolution,
                FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }),
            ) => *op == BooleanOp::Unresolved,
            _ => false,
        };
        if !unresolved {
            continue;
        }
        let Some(history) = indexes.history(ctx, feature.native_ref.as_deref())? else {
            continue;
        };
        if ctx.any_by(
            lanes
                .iter()
                .zip(&indexes.direct_classes)
                .zip(&indexes.object_names),
            |((lane, direct), names)| {
                operation_carrier_present(ctx, kind, history, lane, direct, names, form_padding)
            },
            OPERATION,
        )? {
            continue;
        }
        feature
            .evaluation
            .edit(|definition, _| match (kind, definition) {
                (
                    OperationKind::Extrusion,
                    FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }),
                )
                | (
                    OperationKind::Revolution,
                    FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }),
                ) => *op = base_op,
                _ => {}
            });
    }
    Ok(())
}

fn operation_carrier_present(
    ctx: &DecodeContext<'_>,
    kind: OperationKind,
    feature: &Feature,
    lane: &FeatureInputLane,
    direct_classes: &DirectClasses<'_>,
    object_names: &ObjectNames<'_, '_>,
    form_padding: Option<usize>,
) -> Result<bool, CodecError> {
    let name = match object_names.lookup(ctx, feature)? {
        NameLookup::Absent => return Ok(false),
        NameLookup::Repeated => return Ok(true),
        NameLookup::One(name) => name,
    };
    if matches!(kind, OperationKind::Extrusion)
        && feature_inline_operation_fields(ctx, lane, name)?.is_some()
    {
        return Ok(true);
    }
    let Some(code) = operation_code(
        ctx,
        lane,
        direct_classes,
        name,
        feature.input_class.as_deref(),
        form_padding,
    )?
    else {
        return Ok(false);
    };
    Ok(matches!(kind, OperationKind::Revolution)
        || !(code == 11
            && matches!(
                feature.input_class.as_deref(),
                Some("moExtrusion_c" | "moICE_c" | "moCut_c")
            )))
}

/// Establish projected split-line mode and source sketch from each
/// `moPLine_c` feature-input object while native dimension values remain raw.
pub(crate) fn enrich_history_split_lines(
    ctx: &DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OBSERVATION: &str = "index SLDPRT split-line observations";
    let mut storage = ctx.reserve_scoped(0, "SLDPRT split-line workspace")?;
    let mut observations = HashMap::<String, (bool, bool)>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT split-line lanes")? {
        let project_classes = sorted_classes(
            ctx,
            &mut storage,
            lane,
            "moPLineProject_c",
            "index SLDPRT split-line projection classes",
        )?;
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut objects = Vec::new();
        for history in ctx.admit_iter(&*histories, "scan SLDPRT split-line objects")? {
            for feature in ctx.admit_iter(&history.features, "scan SLDPRT split-line objects")? {
                if let Some(name) = object_names.of(ctx, feature)? {
                    storage.with_storage(|| {
                        ctx.push_vec(
                            &mut objects,
                            (name.offset, feature),
                            "collect SLDPRT split-line objects",
                        )
                    })?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut objects,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT split-line objects",
        )?;
        for (index, (start, feature)) in ctx
            .admit_iter(&objects, "scan SLDPRT split-line objects")?
            .enumerate()
        {
            if feature.input_class.as_deref() != Some("moPLine_c") {
                continue;
            }
            let end = objects
                .get(index + 1)
                .map_or(u64_from_index(lane.native_payload.len()), |(offset, _)| {
                    *offset
                });
            let projected = classes_within(
                ctx,
                &project_classes,
                *start,
                end,
                "find SLDPRT split-line projection classes",
            )?
            .len()
                == 1;
            if let Some(observation) =
                ctx.get_mut_hash_map(&mut observations, feature.id.as_str(), OBSERVATION)?
            {
                if projected {
                    observation.0 = true;
                } else {
                    observation.1 = true;
                }
            } else {
                let key =
                    storage.with_storage(|| ctx.copy_retained_text(&feature.id, OBSERVATION))?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut observations,
                        key,
                        (projected, !projected),
                        OBSERVATION,
                    )
                })?;
            }
        }
    }
    let projected = |ctx: &DecodeContext<'_>, feature: &Feature| {
        Ok::<_, CodecError>(
            ctx.get_hash_map(&observations, feature.id.as_str(), OBSERVATION)?
                == Some(&(true, false)),
        )
    };
    let mut tools = HashMap::<String, String>::new();
    for history in ctx.admit_iter(&*histories, "resolve SLDPRT split-line sources")? {
        for feature in ctx.admit_iter(&history.features, "resolve SLDPRT split-line sources")? {
            if !projected(ctx, feature)? {
                continue;
            }
            let Some(tool) = split_line_source_sketch(ctx, feature, &history.features)? else {
                continue;
            };
            let value = storage.with_storage(|| {
                ctx.copy_retained_text(&tool.id, "retain SLDPRT split-line tool ID")
            })?;
            if let Some(existing) = ctx.get_mut_hash_map(
                &mut tools,
                feature.id.as_str(),
                "index SLDPRT split-line tools",
            )? {
                *existing = value;
            } else {
                let key = storage.with_storage(|| {
                    ctx.copy_retained_text(&feature.id, "retain SLDPRT split-line tool key")
                })?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut tools, key, value, "index SLDPRT split-line tools")
                })?;
            }
        }
    }
    for history in ctx.admit_iter(&mut *histories, "bind SLDPRT split-line mode")? {
        for feature in ctx.admit_iter(&mut history.features, "bind SLDPRT split-line mode")? {
            if !projected(ctx, feature)? {
                continue;
            }
            ctx.insert_btree_map(
                &mut feature.properties,
                cadmpeg_core::nonblank_const!(SPLIT_LINE_MODE_PROPERTY),
                SPLIT_LINE_PROJECTION_MODE.into(),
                "bind SLDPRT split-line mode",
            )?;
            if let Some(tool) =
                ctx.get_hash_map(&tools, feature.id.as_str(), "find SLDPRT split-line tool")?
            {
                let tool =
                    ctx.copy_retained_text(tool, "retain SLDPRT split-line tool property")?;
                ctx.insert_btree_map(
                    &mut feature.properties,
                    cadmpeg_core::nonblank_const!(SPLIT_LINE_TOOL_PROPERTY),
                    tool,
                    "bind SLDPRT split-line tool",
                )?;
            }
        }
    }
    Ok(())
}

/// The one earlier profile sketch in the same history that repeats a split
/// line's parameters.
fn split_line_source_sketch<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &Feature,
    history_features: &'a [Feature],
) -> Result<Option<&'a Feature>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "find SLDPRT split line source sketch";
    if feature.parameters.is_empty() {
        return Ok(None);
    }
    let Some(source) = feature.source_value() else {
        return Ok(None);
    };
    let candidate_matches = |candidate: &&'a Feature| {
        Ok(classify(candidate) == Some(FeatureClass::Sketch)
            && candidate.input_class.as_deref() == Some("moProfileFeature_c")
            && candidate
                .source_value()
                .is_some_and(|candidate_source| candidate_source > 0 && candidate_source < source)
            && ctx.equal(&candidate.parent, &feature.parent, OPERATION)?
            && crate::records::equal_text_maps(
                ctx,
                &candidate.parameters,
                &feature.parameters,
                OPERATION,
            )?)
    };
    let mut candidates = history_features.iter();
    let Some(tool) = ctx.find_by(&mut candidates, &candidate_matches, OPERATION)? else {
        return Ok(None);
    };
    Ok(ctx
        .find_by(&mut candidates, &candidate_matches, OPERATION)?
        .is_none()
        .then_some(tool))
}

#[cfg(test)]
mod operations_tests;
