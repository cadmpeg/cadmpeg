//! Relation definition and profile locus resolution.

use super::grid::{quantize, GridPoint};
use super::markers::marker_is_geometry_locus;
use super::relation_geometry::{
    relation_operand_geometry_ref_matches, relation_uses_solver_line_operand,
    solver_line_geometry_ref_matches,
};
use super::relation_records::{relation_uses_dynamic_operands, relation_uses_solver_points};
use super::transforms::{
    charge_profile_marker_lookup, compatible_marker_transform_candidates, locus_entity, locus_key,
    marker_entities, marker_transforms_with_frame_fallback, sketch_entity_locus_points,
    sort_marker_entity_ids, MarkerEntityFilter, MarkerTransform, ProfileAxis,
};
use super::typed_relations::{
    line_endpoint_markers, relation_link_identifies_owner, relation_link_is_geometric_operand,
    relation_owner_markers, sketch_entity_contains_point,
};
use super::SKETCH_POINT_TOLERANCE;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::{index_from_u64, DecodeContext};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus,
};
use std::collections::{HashMap, HashSet};

// Relation geometry is projected after feature coordinates are rounded to the
// model-space quantum. Keep the ordinary identity comparison strict, but allow
// the bounded error that two independently rounded operand points can add to a
// stored dimensional value.
const RELATION_DIMENSION_RELATIVE_TOLERANCE: f64 = 1.0e-9;
const RELATION_GEOMETRY_QUANTUM_MM: f64 = 1.0e-8;
const RELATION_GEOMETRY_ABSOLUTE_TOLERANCE_MM: f64 = 2.0 * RELATION_GEOMETRY_QUANTUM_MM;
const EPS_RELATION_LOCI_SAME_DIMENSION_ANGLE_E9: f64 = 1e-9;
const EPS_RELATION_LOCI_MARKER_CENTER_DIMENSIONED_ENTITY_E8: f64 = 1e-8;
const EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9: f64 = 1e-9;

fn same_relation_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= RELATION_GEOMETRY_ABSOLUTE_TOLERANCE_MM
            .max(RELATION_DIMENSION_RELATIVE_TOLERANCE * left.abs().max(right.abs()).max(1.0))
}

pub(super) fn linked_single_arc_entity(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked arc entity";
    let mut has_link = false;
    for link in marker.links() {
        charge_relation_identity_work(ctx, [link.entity_ref.as_str(), marker.id()], 16, OPERATION)?;
        if relation_link_identifies_owner(marker, link) {
            continue;
        }
        has_link = true;
        charge_profile_marker_lookup(
            ctx,
            &link.entity_ref,
            markers_by_id,
            loci_by_marker,
            OPERATION,
        )?;
        if !matches!(
            markers_by_id
                .get(link.entity_ref.as_str())
                .map(|marker| marker.kind()),
            Some(SketchInputKind::Arc)
        ) {
            return Ok(None);
        }
    }
    if !has_link {
        return Ok(None);
    }
    let Some(entities) = linked_single_entities(ctx, marker, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    Ok(if entities.len() == 1 {
        entities.into_iter().next()
    } else {
        None
    })
}

pub(super) fn linked_single_ellipse_entity(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    let Some(entities) = linked_single_entities(ctx, marker, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    let [identity] = entities.as_slice() else {
        return Ok(None);
    };
    let Some(entity) = find_profile_entity(
        ctx,
        sketch_entities,
        identity,
        "resolve SLDPRT linked ellipse entity",
    )?
    else {
        return Ok(None);
    };
    if !matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Ellipse { .. }
    ) {
        return Ok(None);
    }
    Ok(entities.into_iter().next())
}

pub(super) fn linked_midpoint_operands(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked midpoint operands";
    for link in marker.links() {
        charge_relation_identity_work(ctx, [link.entity_ref.as_str(), marker.id()], 16, OPERATION)?;
    }
    let mut links = marker
        .links()
        .iter()
        .filter(|link| !relation_link_identifies_owner(marker, link));
    let (Some(first), Some(second), None) = (links.next(), links.next(), links.next()) else {
        return Ok(None);
    };
    let mut point = None;
    let mut entity = None;
    for link in [first, second] {
        charge_profile_marker_lookup(
            ctx,
            &link.entity_ref,
            markers_by_id,
            loci_by_marker,
            OPERATION,
        )?;
        let Some(linked_marker) = markers_by_id.get(link.entity_ref.as_str()) else {
            return Ok(None);
        };
        let Some(loci) = loci_by_marker.get(&link.entity_ref) else {
            return Ok(None);
        };
        let Some(locus) = unique_locus(ctx, loci)? else {
            return Ok(None);
        };
        match linked_marker.kind() {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint if point.is_none() => {
                point = Some(locus);
            }
            SketchInputKind::LineOrCircle | SketchInputKind::Arc if entity.is_none() => {
                entity = Some(match locus {
                    SketchLocus::Entity(entity)
                    | SketchLocus::Start(entity)
                    | SketchLocus::End(entity)
                    | SketchLocus::Center(entity) => entity,
                });
            }
            _ => return Ok(None),
        }
    }
    Ok(point.zip(entity))
}

pub(super) fn relation_operand_loci(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<Vec<SketchLocus>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT marker relation operand loci";
    let owners = relation_owner_markers(ctx, relation, markers_by_id)?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(markers_by_id.len()),
        OPERATION,
    )?;
    let marker_bytes = markers_by_id
        .keys()
        .try_fold(0u64, |bytes, key| {
            bytes.checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    for link in relation.links() {
        ctx.charge_work(
            marker_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(link.entity_ref.len()))
                .and_then(|bytes| {
                    bytes.checked_add(cadmpeg_core::decode::u64_from_index(relation.id().len()))
                })
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| work.checked_add(64))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
    }
    let mut loci = Vec::new();
    let mut locus_bytes = 0u64;
    for marker in relation
        .links()
        .iter()
        .filter(|link| relation_link_is_geometric_operand(relation, link, markers_by_id))
        .map(|link| link.entity_ref.as_str())
        .chain(owners.iter().map(|owner| owner.id()))
    {
        let Some(locus) = marker_point_locus(ctx, marker, markers_by_id, loci_by_marker)? else {
            return Ok(None);
        };
        let bytes = cadmpeg_core::decode::u64_from_index(locus_entity(&locus).as_str().len());
        ctx.charge_work(
            locus_bytes
                .checked_add(bytes)
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| {
                    work.checked_add(
                        cadmpeg_core::decode::u64_from_index(loci.len())
                            .checked_add(1)?
                            .checked_mul(cadmpeg_core::decode::u64_from_index(
                                std::mem::size_of::<SketchLocus>(),
                            ))?,
                    )
                })
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if loci.contains(&locus) {
            continue;
        }
        let next_bytes = locus_bytes
            .checked_add(bytes)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.reserve_vec(&mut loci, 1, OPERATION)?;
        loci.push(locus);
        locus_bytes = next_bytes;
    }
    Ok((!loci.is_empty()).then_some(loci))
}

pub(super) fn linked_single_entities(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<Vec<SketchEntityId>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT single linked entities";
    let mut result: Vec<SketchEntityId> = Vec::new();
    let mut bytes = 0u64;
    for link in marker.links() {
        charge_relation_identity_work(ctx, [link.entity_ref.as_str(), marker.id()], 16, OPERATION)?;
        if relation_link_identifies_owner(marker, link) {
            continue;
        }
        let entities = marker_entities(
            ctx,
            &link.entity_ref,
            markers_by_id,
            loci_by_marker,
            MarkerEntityFilter::All,
        )?;
        let [entity] = entities.as_slice() else {
            return Ok(None);
        };
        let length = cadmpeg_core::decode::u64_from_index(entity.as_str().len());
        ctx.charge_work(
            bytes
                .checked_add(length)
                .and_then(|bytes| bytes.checked_mul(8))
                .and_then(|work| {
                    work.checked_add(
                        cadmpeg_core::decode::u64_from_index(result.len())
                            .checked_add(1)?
                            .checked_mul(64)?,
                    )
                })
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if result.contains(entity) {
            continue;
        }
        bytes = bytes
            .checked_add(length)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.reserve_vec(&mut result, 1, OPERATION)?;
        if let Some(entity) = entities.into_iter().next() {
            result.push(entity);
        }
    }
    Ok(Some(result))
}

pub(super) fn relation_constraint_is_inactive(
    ctx: &DecodeContext<'_>,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    definition: &SketchConstraintDefinitionInput,
    sketch_entities: &[SketchEntity],
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.charge_work(256, "compare SLDPRT relation activity")?;
    let Some(parameter) = parameter else {
        return Ok(false);
    };
    let entity = |id: &SketchEntityId| {
        find_profile_entity(
            ctx,
            sketch_entities,
            id,
            "resolve SLDPRT relation activity entity",
        )
    };
    Ok(match definition {
        SketchConstraintDefinitionInput::DistanceLoci { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let measured = if let (Some(first), Some(second)) = (
                profile_locus_point_charged(
                    ctx,
                    first,
                    sketch_entities,
                    "resolve SLDPRT profile locus",
                )?,
                profile_locus_point_charged(
                    ctx,
                    second,
                    sketch_entities,
                    "resolve SLDPRT profile locus",
                )?,
            ) {
                Some((second.u - first.u).hypot(second.v - first.v))
            } else {
                let point_line = |point: &SketchLocus,
                                  line: &SketchLocus|
                 -> Result<Option<f64>, cadmpeg_core::CodecError> {
                    let Some(point) = profile_locus_point_charged(
                        ctx,
                        point,
                        sketch_entities,
                        "resolve SLDPRT relation activity locus",
                    )?
                    else {
                        return Ok(None);
                    };
                    let SketchLocus::Entity(line) = line else {
                        return Ok(None);
                    };
                    let Some(line) = entity(line)? else {
                        return Ok(None);
                    };
                    Ok(point_line_distance_value(point, line))
                };
                match point_line(first, second)? {
                    Some(value) => Some(value),
                    None => point_line(second, first)?,
                }
            };
            measured
                .is_some_and(|measured| !same_relation_dimension_length(measured, expected.get()))
        }
        SketchConstraintDefinitionInput::HorizontalDistance { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let (Some(first), Some(second)) = (
                profile_locus_point_charged(
                    ctx,
                    first,
                    sketch_entities,
                    "resolve SLDPRT profile locus",
                )?,
                profile_locus_point_charged(
                    ctx,
                    second,
                    sketch_entities,
                    "resolve SLDPRT profile locus",
                )?,
            ) else {
                return Ok(false);
            };
            !same_relation_dimension_length((second.u - first.u).abs(), expected.get())
        }
        SketchConstraintDefinitionInput::VerticalDistance { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let (Some(first), Some(second)) = (
                profile_locus_point_charged(
                    ctx,
                    first,
                    sketch_entities,
                    "resolve SLDPRT profile locus",
                )?,
                profile_locus_point_charged(
                    ctx,
                    second,
                    sketch_entities,
                    "resolve SLDPRT profile locus",
                )?,
            ) else {
                return Ok(false);
            };
            !same_relation_dimension_length((second.v - first.v).abs(), expected.get())
        }
        SketchConstraintDefinitionInput::Distance { entities, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let [first, second] = entities.as_slice() else {
                return Ok(true);
            };
            line_line_distance(
                match entity(first)? {
                    Some(entity) => entity,
                    None => return Ok(false),
                },
                match entity(second)? {
                    Some(entity) => entity,
                    None => return Ok(false),
                },
            )
            .is_some_and(|measured| !same_relation_dimension_length(measured, expected.get()))
        }
        SketchConstraintDefinitionInput::Angle { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            line_line_angle(
                match entity(first)? {
                    Some(entity) => entity,
                    None => return Ok(false),
                },
                match entity(second)? {
                    Some(entity) => entity,
                    None => return Ok(false),
                },
            )
            .is_some_and(|measured| !same_dimension_angle(measured, expected.get()))
        }
        SketchConstraintDefinitionInput::Radius { entity: id, .. }
        | SketchConstraintDefinitionInput::Diameter { entity: id, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let Some(entity) = entity(id)? else {
                return Ok(false);
            };
            let radius = match entity.geometry.definition() {
                SketchGeometryDefinition::Circle { radius, .. }
                | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                _ => return Ok(true),
            };
            let measured = if matches!(definition, SketchConstraintDefinitionInput::Diameter { .. })
            {
                radius * 2.0
            } else {
                radius
            };
            !same_dimension_length(measured, expected.get())
        }
        SketchConstraintDefinitionInput::RepeatedRadius { entities, .. }
        | SketchConstraintDefinitionInput::RepeatedDiameter { entities, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let diameter = matches!(
                definition,
                SketchConstraintDefinitionInput::RepeatedDiameter { .. }
            );
            let mut inactive = false;
            for id in entities {
                let Some(entity) = entity(id)? else {
                    return Ok(false);
                };
                let radius = match *entity.geometry.definition() {
                    SketchGeometryDefinition::Circle { radius, .. }
                    | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                    _ => return Ok(false),
                };
                ctx.charge_work(64, "compare SLDPRT repeated-radius activity")?;
                let measured = if diameter { radius * 2.0 } else { radius };
                inactive |= !same_dimension_length(measured, expected.get());
            }
            inactive
        }
        _ => false,
    })
}

pub(super) fn typed_relation_definition(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchConstraintDefinitionInput>, cadmpeg_core::CodecError> {
    typed_relation_definition_with_profile_axis(
        ctx,
        relation,
        parameter,
        crate::resolved_features::relation_loci::SketchRelationEntities {
            sketch,
            sketch_entities,
        },
        markers_by_id,
        loci_by_marker,
        None,
    )
}

#[derive(Clone, Copy)]
pub(super) struct SketchRelationEntities<'a> {
    pub sketch: &'a SketchId,
    pub sketch_entities: &'a [SketchEntity],
}

pub(super) fn typed_relation_definition_with_profile_axis(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    profile: SketchRelationEntities<'_>,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    profile_axis: Option<ProfileAxis>,
) -> Result<Option<SketchConstraintDefinitionInput>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "retain SLDPRT relation parameter identity";

    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    let SketchRelationEntities {
        sketch,
        sketch_entities,
    } = profile;
    let Some(parameter) = parameter else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(parameter.id.as_str().len())
            .checked_mul(4)
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        OPERATION,
    )?;
    let text = ctx.format_retained(format_args!("{}", parameter.id.as_str()), OPERATION)?;
    let parameter_id = cadmpeg_ir::features::ParameterId::mint(text)
        .map_err(cadmpeg_core::CodecError::malformed)?;
    macro_rules! resolved_or_none {
        ($candidate:expr) => {
            match $candidate {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let profile_axis = match relation.family {
        PointPointHorizontalDistance => Some(profile_axis.unwrap_or(ProfileAxis::U)),
        PointPointVerticalDistance => Some(profile_axis.unwrap_or(ProfileAxis::V)),
        _ => None,
    };
    let marker =
        |index: usize| relation_operand_marker(ctx, relation, index, sketch, markers_by_id);
    let dynamic = relation_uses_dynamic_operands(relation);
    let point = |index: usize| -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
        const SCAN: &str = "scan SLDPRT relation point identities";
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sketch_entities.len()),
            SCAN,
        )?;
        let geometry_work = sketch_entities.iter().try_fold(0u64, |work, entity| {
            cadmpeg_core::decode::u64_from_index(entity.geometry_ref.as_deref().map_or(0, str::len))
                .checked_add(cadmpeg_core::decode::u64_from_index(relation.id.len()))
                .and_then(|bytes| bytes.checked_add(1))
                .and_then(|bytes| bytes.checked_mul(8))
                .and_then(|bytes| work.checked_add(bytes))
                .ok_or_else(|| ctx.refuse_codec_limit(SCAN, u64::MAX - 1, u64::MAX))
        })?;
        ctx.charge_work(geometry_work, SCAN)?;
        if let Some(entity) = sketch_entities
            .iter()
            .find(|entity| {
                entity.geometry_ref.as_deref().is_some_and(|geometry_ref| {
                    relation_operand_geometry_ref_matches(geometry_ref, relation, index)
                })
            })
            .filter(|entity| {
                matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
            })
        {
            return super::transforms::SketchLocusRole::Entity
                .copy_locus(ctx, entity.id(), "retain SLDPRT relation point identity")
                .map(Some);
        }
        let Some(marker) = marker(index)? else {
            return Ok(None);
        };
        if matches!(
            relation.operands.get(index).map(|operand| operand.kind),
            Some(FeatureInputOperandKind::Native(
                NativeOperandTag::TAG_837B | NativeOperandTag::TAG_BC7C
            ))
        ) {
            if let Some(locus) = qualified_or_linked_point_locus(
                ctx,
                marker,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            )? {
                return Ok(Some(locus));
            }
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sketch_entities.len()),
            SCAN,
        )?;
        let native_work = sketch_entities.iter().try_fold(0u64, |work, entity| {
            cadmpeg_core::decode::u64_from_index(entity.native_ref.as_deref().map_or(0, str::len))
                .checked_add(cadmpeg_core::decode::u64_from_index(marker.len()))
                .and_then(|bytes| bytes.checked_add(1))
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|bytes| work.checked_add(bytes))
                .ok_or_else(|| ctx.refuse_codec_limit(SCAN, u64::MAX - 1, u64::MAX))
        })?;
        ctx.charge_work(native_work, SCAN)?;
        if let Some(entity) = sketch_entities.iter().find(|entity| {
            entity.native_ref.as_deref() == Some(marker)
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
        }) {
            return super::transforms::SketchLocusRole::Entity
                .copy_locus(ctx, entity.id(), "retain SLDPRT relation point identity")
                .map(Some);
        }
        if dynamic && dynamic_point_operand(relation, index) {
            dynamic_marker_point_locus(
                ctx,
                marker,
                sketch,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            )
        } else {
            marker_point_locus(ctx, marker, markers_by_id, loci_by_marker)
        }
    };
    let curve = |index: usize| -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
        match solver_line_entity(ctx, relation, index, sketch, sketch_entities)? {
            Some(entity) => Ok(Some(entity)),
            None => match marker(index)? {
                Some(marker) => single_marker_line_entity(
                    ctx,
                    marker,
                    markers_by_id,
                    loci_by_marker,
                    sketch_entities,
                ),
                None => Ok(None),
            },
        }
    };
    if relation_uses_solver_points(relation) && (point(0)?.is_none() || point(1)?.is_none()) {
        return Ok(None);
    }
    let dynamic_point_pair = if dynamic {
        match relation.family {
            PointPointDistance | PointPointHorizontalDistance | PointPointVerticalDistance => {
                unique_dynamic_marker_point_pair(
                    ctx,
                    relation,
                    sketch,
                    parameter,
                    (point(0)?, point(1)?),
                    &DynamicMarkerPointIndex {
                        sketch_entities,
                        markers_by_id,
                        loci_by_marker,
                        profile_axis,
                    },
                )?
            }
            _ => None,
        }
    } else {
        None
    };
    let dynamic_direct_point_pair = if dynamic
        && matches!(
            relation.family,
            PointPointDistance | PointPointHorizontalDistance | PointPointVerticalDistance
        )
        && dynamic_point_pair.is_none()
    {
        unique_dynamic_direct_point_roster_pair(
            ctx,
            relation,
            sketch,
            parameter,
            sketch_entities,
            markers_by_id,
            profile_axis,
        )?
    } else {
        None
    };
    // A family-scoped native tag is not a locus identity. If marker lookup
    // finds no resolved center carrier, the complete profile roster is the
    // defined witness; an ambiguous arc-center carrier must remain unresolved
    // instead of being bypassed by a coincidental whole-sketch distance.
    let mut roster_has_no_center = dynamic
        && dynamic_point_pair.is_none()
        && dynamic_direct_point_pair.is_none()
        && relation
            .operands
            .iter()
            .all(|operand| operand.entity_ref.is_none());
    if roster_has_no_center {
        for index in 0..2 {
            if let Some(marker) =
                relation_operand_marker(ctx, relation, index, sketch, markers_by_id)?
            {
                if dynamic_marker_center_candidates(
                    ctx,
                    marker,
                    sketch,
                    markers_by_id,
                    loci_by_marker,
                    sketch_entities,
                )?
                .is_some()
                {
                    roster_has_no_center = false;
                    break;
                }
            }
        }
    }
    let dynamic_roster_point_pair = if roster_has_no_center {
        match relation.family {
            PointPointDistance => {
                unique_profile_distance_loci_pair(ctx, sketch, parameter, sketch_entities)?
            }
            PointPointHorizontalDistance | PointPointVerticalDistance => {
                unique_profile_axis_distance_pair(
                    ctx,
                    sketch,
                    parameter,
                    sketch_entities,
                    resolved_or_none!(profile_axis),
                )?
            }
            _ => None,
        }
    } else {
        None
    };
    let dynamic_point_line_pair = if dynamic && relation.family == PointLineDistance {
        let known_point = point(0)?;
        let selected = if let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
            parameter.value.as_ref()
        {
            let point_candidates = match known_point {
                Some(point) => vec![point],
                None => match relation_operand_marker(ctx, relation, 0, sketch, markers_by_id)? {
                    Some(marker) => dynamic_marker_point_candidates(
                        ctx,
                        marker,
                        sketch,
                        markers_by_id,
                        loci_by_marker,
                        sketch_entities,
                    )?,
                    None => Vec::new(),
                },
            };
            let line_candidates = dynamic_line_operand_candidates(
                ctx,
                relation,
                1,
                sketch,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            )?;
            unique_point_line_candidate_pair(
                ctx,
                *expected,
                &point_candidates,
                &line_candidates,
                sketch_entities,
            )?
        } else {
            None
        };
        match selected {
            Some(pair) => Some(pair),
            None => unique_dynamic_roster_point_line_pair(
                ctx,
                relation,
                sketch,
                parameter,
                point(0)?,
                curve(1)?,
                sketch_entities,
            )?,
        }
    } else {
        None
    };
    let dynamic_line_distance_pair = if dynamic && relation.family == LineLineDistance {
        let selected = unique_dynamic_marker_line_distance_pair(
            ctx,
            relation,
            sketch,
            parameter,
            sketch_entities,
            markers_by_id,
            loci_by_marker,
        )?;
        match selected {
            Some(pair) => Some(pair),
            None => (|| -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(None);
            };
            let unique_partner = |known: &SketchEntityId| {
                unique_profile_matched_entity(ctx, sketch, known, sketch_entities, |known, candidate| {
                    line_line_distance(known, candidate).is_some_and(|measured| {
                        same_relation_dimension_length(measured, expected.get())
                    })
                })
            };
            let first = curve(0)?;
            let second = curve(1)?;
            Ok(match (first, second) {
                (Some(first), None) => { let Some(partner) = unique_partner(&first)? else { return Ok(None); }; Some((first, partner)) },
                (None, Some(second)) => unique_partner(&second)?.map(|partner| (partner, second)),
                _ => None,
            })
            })()?,
        }
    } else {
        None
    };
    // Dynamic line operands carry a family tag, not a stable line identity.
    // When marker and solver-line joins do not produce a pair, the complete
    // owner sketch is the remaining semantic scope. Accept it only when the
    // stored operands contain no explicit identity and exactly one pair meets
    // the native distance.
    let dynamic_roster_line_distance_pair = if dynamic
        && relation.family == LineLineDistance
        && dynamic_line_distance_pair.is_none()
    {
        unique_dynamic_roster_line_distance_pair(ctx, relation, sketch, parameter, sketch_entities)?
    } else {
        None
    };
    let dynamic_angle_pair = if dynamic && relation.family == Angle {
        let selected = unique_dynamic_marker_line_angle_pair(
            ctx,
            relation,
            sketch,
            parameter,
            sketch_entities,
            markers_by_id,
            loci_by_marker,
        )?;
        match selected {
            Some(pair) => Some(pair),
            None => (|| -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
            let first = curve(0)?;
            let second = curve(1)?;
            Ok(match (first, second) {
                (Some(first), None) => {
                    let Some(partner) = unique_dynamic_profile_line_angle_entity(ctx, sketch, &first, parameter, sketch_entities)? else { return Ok(None); };
                    Some((first, partner))
                },
                (None, Some(second)) => Some((
                    match unique_dynamic_profile_line_angle_entity(ctx, sketch, &second, parameter, sketch_entities)? {
                        Some(partner) => partner, None => return Ok(None),
                    },
                    second,
                )),
                _ => None,
            })
            })()?,
        }
    } else {
        None
    };
    // A family-scoped angular relation may carry only an unresolved relation
    // handle or an address that does not materialize a line entity. In that
    // case the complete owning sketch is the semantic scope; accept the pair
    // only when exactly one unordered line pair has the stored unoriented
    // angle. A resolved line remains authoritative and does not enter this
    // unrelated-pair fallback.
    let dynamic_roster_angle_pair = if dynamic
        && relation.family == Angle
        && dynamic_angle_pair.is_none()
        && curve(0)?.is_none()
        && curve(1)?.is_none()
    {
        unique_dynamic_roster_line_angle_pair(ctx, sketch, parameter, sketch_entities)?
    } else {
        None
    };
    if dynamic {
        let witnessed = match relation.family {
            PointPointDistance | PointPointHorizontalDistance | PointPointVerticalDistance => {
                dynamic_point_pair.is_some()
                    || dynamic_direct_point_pair.is_some()
                    || dynamic_roster_point_pair.is_some()
                    || (point(0)?.is_some() && point(1)?.is_some())
            }
            PointLineDistance => {
                dynamic_point_line_pair.is_some() || (point(0)?.is_some() && curve(1)?.is_some())
            }
            LineLineDistance => {
                dynamic_line_distance_pair.is_some()
                    || dynamic_roster_line_distance_pair.is_some()
                    || (curve(0)?.is_some() && curve(1)?.is_some())
            }
            Angle => {
                dynamic_angle_pair.is_some()
                    || dynamic_roster_angle_pair.is_some()
                    || (curve(0)?.is_some() && curve(1)?.is_some())
            }
            CircleDiameter => true,
        };
        if !witnessed {
            return Ok(None);
        }
    }
    Ok(match relation.family {
        PointPointDistance => {
            let first = point(0)?;
            let second = point(1)?;
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match dynamic_point_pair
                .or(dynamic_direct_point_pair)
                .or(dynamic_roster_point_pair)
            {
                Some(pair) => pair,
                None => match (first, second) {
                    (Some(first), Some(second)) => (first, second),
                    (Some(known), None) => match doubled_profile_distance_loci(
                        ctx,
                        relation,
                        (0, 1),
                        sketch,
                        parameter,
                        sketch_entities,
                        markers_by_id,
                    )? {
                        Some(pair) => pair,
                        None => {
                            let partner = resolved_or_none!(unique_profile_distance_locus(
                                ctx,
                                sketch,
                                &known,
                                parameter,
                                sketch_entities
                            )?);
                            (known, partner)
                        }
                    },
                    (None, Some(known)) => match doubled_profile_distance_loci(
                        ctx,
                        relation,
                        (1, 0),
                        sketch,
                        parameter,
                        sketch_entities,
                        markers_by_id,
                    )? {
                        Some(pair) => pair,
                        None => {
                            let partner = resolved_or_none!(unique_profile_distance_locus(
                                ctx,
                                sketch,
                                &known,
                                parameter,
                                sketch_entities
                            )?);
                            (partner, known)
                        }
                    },
                    (None, None) => {
                        resolved_or_none!(unique_profile_distance_loci_pair(
                            ctx,
                            sketch,
                            parameter,
                            sketch_entities
                        )?)
                    }
                },
            };
            if first == second {
                return Ok(None);
            }
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    resolved_or_none!(parameter.value.as_ref())
                else {
                    return Ok(None);
                };
                let first_point = resolved_or_none!(profile_locus_point_charged(
                    ctx,
                    &first,
                    sketch_entities,
                    "resolve SLDPRT profile locus"
                )?);
                let second_point = resolved_or_none!(profile_locus_point_charged(
                    ctx,
                    &second,
                    sketch_entities,
                    "resolve SLDPRT profile locus"
                )?);
                if !same_relation_dimension_length(
                    (second_point.u - first_point.u).hypot(second_point.v - first_point.v),
                    expected.get(),
                ) {
                    if dynamic {
                        return Ok(None);
                    }
                    let horizontal = same_dimension_length(
                        (second_point.u - first_point.u).abs(),
                        expected.get(),
                    );
                    let vertical = same_dimension_length(
                        (second_point.v - first_point.v).abs(),
                        expected.get(),
                    );
                    let projected_distance_operands = relation.operands.iter().all(|operand| {
                        operand.kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C)
                    });
                    if projected_distance_operands && horizontal != vertical {
                        return Ok(Some(if horizontal {
                            SketchConstraintDefinitionInput::HorizontalDistance {
                                first,
                                second,
                                parameter: parameter_id,
                            }
                        } else {
                            SketchConstraintDefinitionInput::VerticalDistance {
                                first,
                                second,
                                parameter: parameter_id,
                            }
                        }));
                    }
                    if authoritative {
                        return Ok(Some(SketchConstraintDefinitionInput::DistanceLoci {
                            first,
                            second,
                            parameter: parameter_id,
                        }));
                    }
                    (first, second) =
                        resolved_or_none!(unique_repaired_profile_distance_loci_pair(
                            ctx,
                            sketch,
                            &first,
                            &second,
                            parameter,
                            sketch_entities,
                        )?);
                }
            }
            Some(SketchConstraintDefinitionInput::DistanceLoci {
                first,
                second,
                parameter: parameter_id,
            })
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            let axis = resolved_or_none!(profile_axis);
            let first = point(0)?;
            let second = point(1)?;
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match dynamic_point_pair
                .or(dynamic_direct_point_pair)
                .or(dynamic_roster_point_pair)
            {
                Some(pair) => pair,
                None => match (first, second) {
                    (Some(first), Some(second)) => (first, second),
                    (Some(known), None) => {
                        let partner = resolved_or_none!(unique_profile_axis_distance_locus(
                            ctx,
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                            axis,
                        )?);
                        (known, partner)
                    }
                    (None, Some(known)) => (
                        resolved_or_none!(unique_profile_axis_distance_locus(
                            ctx,
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                            axis,
                        )?),
                        known,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_axis_distance_pair(
                            ctx,
                            sketch,
                            parameter,
                            sketch_entities,
                            axis
                        )?)
                    }
                },
            };
            if first == second {
                return Ok(None);
            }
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    resolved_or_none!(parameter.value.as_ref())
                else {
                    return Ok(None);
                };
                let first_point = resolved_or_none!(profile_locus_point_charged(
                    ctx,
                    &first,
                    sketch_entities,
                    "resolve SLDPRT profile locus"
                )?);
                let second_point = resolved_or_none!(profile_locus_point_charged(
                    ctx,
                    &second,
                    sketch_entities,
                    "resolve SLDPRT profile locus"
                )?);
                let measured = if axis == ProfileAxis::U {
                    (second_point.u - first_point.u).abs()
                } else {
                    (second_point.v - first_point.v).abs()
                };
                if !same_relation_dimension_length(measured, expected.get()) {
                    if dynamic {
                        return Ok(None);
                    }
                    if !authoritative {
                        (first, second) =
                            resolved_or_none!(unique_repaired_profile_axis_distance_pair(
                                ctx,
                                sketch,
                                &first,
                                &second,
                                parameter,
                                sketch_entities,
                                axis,
                            )?);
                    }
                }
            }
            Some(if axis == ProfileAxis::U {
                SketchConstraintDefinitionInput::HorizontalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                }
            } else {
                SketchConstraintDefinitionInput::VerticalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                }
            })
        }
        PointLineDistance => {
            let point = point(0)?;
            let line = curve(1)?;
            let authoritative = point.is_some() && line.is_some();
            let (mut point, mut line) = match dynamic_point_line_pair {
                Some(pair) => pair,
                None => match (point, line) {
                    (Some(point), Some(line)) => (point, line),
                    (Some(point), None) => {
                        let partner = resolved_or_none!(unique_profile_point_line_entity(
                            ctx,
                            sketch,
                            &point,
                            parameter,
                            sketch_entities,
                        )?);
                        (point, partner)
                    }
                    (None, Some(line)) => (
                        resolved_or_none!(unique_profile_line_point_locus(
                            ctx,
                            sketch,
                            &line,
                            parameter,
                            sketch_entities
                        )?),
                        line,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_point_line_pair(
                            ctx,
                            sketch,
                            parameter,
                            sketch_entities
                        )?)
                    }
                },
            };
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                resolved_or_none!(parameter.value.as_ref())
            else {
                return Ok(None);
            };
            let point_position = resolved_or_none!(profile_locus_point_charged(
                ctx,
                &point,
                sketch_entities,
                "resolve SLDPRT profile locus"
            )?);
            let line_entity =
                resolved_or_none!(sketch_entities.iter().find(|entity| entity.id() == &line));
            if !point_line_distance_value(point_position, line_entity)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
            {
                if dynamic {
                    return Ok(None);
                }
                if !authoritative {
                    (point, line) = resolved_or_none!(unique_repaired_profile_point_line_pair(
                        ctx,
                        sketch,
                        &point,
                        &line,
                        parameter,
                        sketch_entities,
                    )?);
                }
            }
            Some(SketchConstraintDefinitionInput::DistanceLoci {
                first: point,
                second: SketchLocus::Entity(line),
                parameter: parameter_id,
            })
        }
        LineLineDistance => {
            let operand_marker = |index: usize| -> Result<Option<&str>, cadmpeg_core::CodecError> {
                match relation_operand_marker(ctx, relation, index, sketch, markers_by_id)? {
                    Some(marker) => Ok(Some(marker)),
                    None => Ok(
                        relation_line_point_marker(ctx, relation, index, markers_by_id)?
                            .map(super::super::records::SketchInputEntity::id),
                    ),
                }
            };
            let first = match curve(0)? {
                Some(entity) => Some(entity),
                None => match operand_marker(0)? {
                    Some(marker) => single_marker_line_entity(
                        ctx,
                        marker,
                        markers_by_id,
                        loci_by_marker,
                        sketch_entities,
                    )?,
                    None => None,
                },
            };
            let second = match curve(1)? {
                Some(entity) => Some(entity),
                None => match operand_marker(1)? {
                    Some(marker) => single_marker_line_entity(
                        ctx,
                        marker,
                        markers_by_id,
                        loci_by_marker,
                        sketch_entities,
                    )?,
                    None => None,
                },
            };
            let authoritative =
                matches!((&first, &second), (Some(first), Some(second)) if first != second);
            let (mut first, mut second) =
                match dynamic_line_distance_pair.or(dynamic_roster_line_distance_pair) {
                    Some(pair) => pair,
                    None => match (first, second) {
                        (Some(first), Some(second)) => (first, second),
                        (Some(known), None) => {
                            let partner = if let Some(marker) =
                                relation_line_point_marker(ctx, relation, 1, markers_by_id)?
                            {
                                resolved_or_none!(unique_marker_line_distance_entity(
                                    ctx,
                                    marker.id(),
                                    SketchRelationEntities {
                                        sketch,
                                        sketch_entities
                                    },
                                    &known,
                                    parameter,
                                    markers_by_id,
                                    loci_by_marker
                                )?)
                            } else {
                                resolved_or_none!(unique_profile_line_distance_entity(
                                    ctx,
                                    sketch,
                                    &known,
                                    parameter,
                                    sketch_entities,
                                )?)
                            };
                            (known, partner)
                        }
                        (None, Some(known)) => (
                            if let Some(marker) =
                                relation_line_point_marker(ctx, relation, 0, markers_by_id)?
                            {
                                resolved_or_none!(unique_marker_line_distance_entity(
                                    ctx,
                                    marker.id(),
                                    SketchRelationEntities {
                                        sketch,
                                        sketch_entities
                                    },
                                    &known,
                                    parameter,
                                    markers_by_id,
                                    loci_by_marker
                                )?)
                            } else {
                                resolved_or_none!(unique_profile_line_distance_entity(
                                    ctx,
                                    sketch,
                                    &known,
                                    parameter,
                                    sketch_entities,
                                )?)
                            },
                            known,
                        ),
                        (None, None) => {
                            resolved_or_none!(unique_profile_line_distance_pair(
                                ctx,
                                sketch,
                                parameter,
                                sketch_entities
                            )?)
                        }
                    },
                };
            if first == second {
                let [first_operand, second_operand] = relation.operands.as_slice() else {
                    return Ok(None);
                };
                if first_operand.entity_index == second_operand.entity_index {
                    return Ok(None);
                }
                second = resolved_or_none!(unique_profile_line_distance_entity(
                    ctx,
                    sketch,
                    &first,
                    parameter,
                    sketch_entities,
                )?);
            }
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                resolved_or_none!(parameter.value.as_ref())
            else {
                return Ok(None);
            };
            let first_line =
                resolved_or_none!(sketch_entities.iter().find(|entity| entity.id() == &first));
            let second_line =
                resolved_or_none!(sketch_entities.iter().find(|entity| entity.id() == &second));
            if !line_line_distance(first_line, second_line)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
            {
                if dynamic {
                    return Ok(None);
                }
                if !authoritative {
                    (first, second) =
                        resolved_or_none!(unique_repaired_profile_line_distance_pair(
                            ctx,
                            sketch,
                            &first,
                            &second,
                            parameter,
                            sketch_entities,
                        )?);
                }
            }
            Some(SketchConstraintDefinitionInput::Distance {
                entities: vec![first, second],
                parameter: parameter_id,
            })
        }
        Angle => {
            let first = curve(0)?;
            let second = curve(1)?;
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match dynamic_angle_pair.or(dynamic_roster_angle_pair) {
                Some(pair) => pair,
                None => match (first, second) {
                    (Some(first), Some(second)) => (first, second),
                    (Some(known), None) => {
                        let partner = resolved_or_none!(unique_profile_line_angle_entity(
                            ctx,
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                        )?);
                        (known, partner)
                    }
                    (None, Some(known)) => (
                        resolved_or_none!(unique_profile_line_angle_entity(
                            ctx,
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                        )?),
                        known,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_line_angle_pair(
                            ctx,
                            sketch,
                            parameter,
                            sketch_entities
                        )?)
                    }
                },
            };
            if first == second {
                return Ok(None);
            }
            let cadmpeg_ir::features::ParameterValue::Angle(expected) =
                resolved_or_none!(parameter.value.as_ref())
            else {
                return Ok(None);
            };
            let first_line =
                resolved_or_none!(sketch_entities.iter().find(|entity| entity.id() == &first));
            let second_line =
                resolved_or_none!(sketch_entities.iter().find(|entity| entity.id() == &second));
            let angle = if dynamic {
                unoriented_line_line_angle(first_line, second_line)
            } else {
                line_line_angle(first_line, second_line)
            };
            if !angle.is_some_and(|measured| same_dimension_angle(measured, expected.get())) {
                if dynamic {
                    return Ok(None);
                }
                if !authoritative {
                    (first, second) = resolved_or_none!(unique_repaired_profile_line_angle_pair(
                        ctx,
                        sketch,
                        &first,
                        &second,
                        parameter,
                        sketch_entities,
                    )?);
                }
            }
            Some(SketchConstraintDefinitionInput::Angle {
                first,
                second,
                parameter: parameter_id,
            })
        }
        CircleDiameter => {
            const SCAN: &str = "scan SLDPRT dimensional circle identities";

            if let Some(entities) = repeated_dimensioned_circular_entities(
                ctx,
                relation,
                parameter,
                sketch,
                sketch_entities,
            )? {
                return Ok(Some(match parameter.display {
                    Some(cadmpeg_ir::features::DimensionDisplay::Radius) => {
                        SketchConstraintDefinitionInput::RepeatedRadius {
                            entities,
                            parameter: parameter_id,
                        }
                    }
                    Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                        SketchConstraintDefinitionInput::RepeatedDiameter {
                            entities,
                            parameter: parameter_id,
                        }
                    }
                    None => return Ok(None),
                }));
            }
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(sketch_entities.len()),
                SCAN,
            )?;
            let scan_work = sketch_entities.iter().try_fold(0u64, |work, entity| {
                let bytes = [
                    entity.sketch.as_str().len(),
                    sketch.as_str().len(),
                    entity.geometry_ref.as_deref().map_or(0, str::len),
                    relation.id.len(),
                ];
                bytes.into_iter().try_fold(work, |work, count| {
                    cadmpeg_core::decode::u64_from_index(count)
                        .checked_add(1)
                        .and_then(|bytes| bytes.checked_mul(8))
                        .and_then(|bytes| work.checked_add(bytes))
                        .ok_or_else(|| ctx.refuse_codec_limit(SCAN, u64::MAX - 1, u64::MAX))
                })
            })?;
            ctx.charge_work(scan_work, SCAN)?;
            let resolved_entity = if let Some(entity) = sketch_entities.iter().find(|entity| {
                entity.sketch == *sketch
                    && entity.geometry_ref.as_deref() == Some(relation.id.as_str())
                    && matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Circle { .. }
                            | SketchGeometryDefinition::Arc { .. }
                    )
            }) {
                Some(super::transforms::copy_sketch_entity_identity(
                    ctx,
                    entity.id(),
                    "retain SLDPRT dimensional circle identity",
                )?)
            } else if let Some(marker) = marker(0)? {
                match marker_center_dimensioned_entity(
                    ctx,
                    marker,
                    sketch,
                    sketch_entities,
                    parameter,
                )? {
                    Some(entity) => Some(entity),
                    None => {
                        if sketch_entities.is_empty() {
                            single_marker_entity(ctx, marker, markers_by_id, loci_by_marker)?
                        } else {
                            single_marker_circular_entity(
                                ctx,
                                marker,
                                markers_by_id,
                                loci_by_marker,
                                sketch_entities,
                            )?
                        }
                    }
                }
            } else {
                None
            };
            let authoritative = resolved_entity.is_some();
            let entity = resolved_or_none!(match resolved_entity {
                Some(entity) => Some(entity),
                None => unique_dimensioned_circle_entity(ctx, sketch, sketch_entities, parameter)?,
            });
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    resolved_or_none!(parameter.value.as_ref())
                else {
                    return Ok(None);
                };
                let geometry = &resolved_or_none!(sketch_entities
                    .iter()
                    .find(|candidate| candidate.id() == &entity))
                .geometry;
                let radius = match geometry.definition() {
                    SketchGeometryDefinition::Circle { radius, .. }
                    | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                    _ => return Ok(None),
                };
                let expected_radius = match parameter.display {
                    Some(cadmpeg_ir::features::DimensionDisplay::Radius) => expected.get(),
                    Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => expected.get() * 0.5,
                    None => return Ok(None),
                };
                if !same_dimension_length(radius, expected_radius) && !authoritative {
                    return Ok(None);
                }
            }
            match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => {
                    Some(SketchConstraintDefinitionInput::Radius {
                        entity,
                        parameter: parameter_id,
                    })
                }
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                    Some(SketchConstraintDefinitionInput::Diameter {
                        entity,
                        parameter: parameter_id,
                    })
                }
                None => None,
            }
        }
    })
}

fn solver_line_entity(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    index: usize,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT solver line identity";

    let Some(operand) = relation.operands.get(index) else {
        return Ok(None);
    };
    ctx.charge_work(16, "select SLDPRT solver line identity")?;
    if !relation_uses_solver_line_operand(relation, index) {
        return Ok(None);
    }
    if let Some(entity_ref) = operand.entity_ref.as_deref() {
        let mut selected = None;
        for entity in sketch_entities {
            charge_relation_identity_work(
                ctx,
                [
                    entity.sketch.as_str(),
                    sketch.as_str(),
                    entity.native_ref.as_deref().unwrap_or(""),
                    entity_ref,
                ],
                16,
                OPERATION,
            )?;
            if entity.sketch != *sketch
                || entity.native_ref.as_deref() != Some(entity_ref)
                || !matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            {
                continue;
            }
            if selected.is_some() {
                return Ok(None);
            }
            selected = Some(entity.id());
        }
        if let Some(entity) = selected {
            return super::transforms::copy_sketch_entity_identity(ctx, entity, OPERATION)
                .map(Some);
        }
        if !relation_uses_dynamic_operands(relation) {
            return Ok(None);
        }
    }
    let mut selected = None;
    for entity in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [
                entity.sketch.as_str(),
                sketch.as_str(),
                entity.geometry_ref.as_deref().unwrap_or(""),
                relation.feature_ref.as_str(),
            ],
            32,
            OPERATION,
        )?;
        if entity.sketch != *sketch
            || !entity.geometry_ref.as_deref().is_some_and(|geometry_ref| {
                solver_line_geometry_ref_matches(
                    geometry_ref,
                    &relation.feature_ref,
                    operand.entity_index,
                )
            })
            || !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some(entity.id());
    }
    selected
        .map(|entity| super::transforms::copy_sketch_entity_identity(ctx, entity, OPERATION))
        .transpose()
}

fn repeated_dimensioned_circular_entities(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Result<Option<Vec<SketchEntityId>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT repeated dimensioned circles";
    for (key, value) in &parameter.properties {
        charge_relation_identity_work(ctx, [key.as_str(), value.as_str()], 128, OPERATION)?;
    }
    charge_relation_identity_work(
        ctx,
        [
            relation.id.as_str(),
            relation.parameter_scalar_ref().unwrap_or(""),
            parameter.native_ref.as_deref().unwrap_or(""),
        ],
        16,
        OPERATION,
    )?;
    // Display-only scalar runs use the owner sketch's radius population.
    let repeated_display = relation.parameter_scalar_ref().is_none()
        && relation.scalar_refs().len() >= 2
        && relation.operands.len() == 1
        && parameter.native_ref.is_none()
        && super::relation_geometry::is_reference_relation_parameter(parameter)
        && parameter
            .properties
            .get(super::relation_geometry::RELATION_PARAMETER_ID_PROPERTY)
            == Some(&relation.id);
    let parameter_native_ref = parameter.native_ref.as_deref();
    if !repeated_display && relation.parameter_scalar_ref() != parameter_native_ref {
        return Ok(None);
    }
    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    if !(expected_radius.is_finite() && expected_radius > 0.0) {
        return Ok(None);
    }
    let matches = |entity: &SketchEntity| -> Result<bool, cadmpeg_core::CodecError> {
        charge_relation_identity_work(
            ctx,
            [
                entity.sketch.as_str(),
                sketch.as_str(),
                entity.geometry_ref.as_deref().unwrap_or(""),
                parameter_native_ref.unwrap_or(""),
            ],
            128,
            OPERATION,
        )?;
        if entity.sketch != *sketch
            || (!repeated_display && entity.geometry_ref.as_deref() != parameter_native_ref)
        {
            return Ok(false);
        }
        let radius = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { radius, .. }
            | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
            _ => return Ok(false),
        };
        Ok(same_dimension_length(radius, expected_radius))
    };
    let mut count = 0usize;
    for entity in sketch_entities {
        if matches(entity)? {
            count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
    }
    if count < 2 || count > relation.scalar_refs().len() {
        return Ok(None);
    }
    let mut entities = Vec::new();
    ctx.reserve_capacity(&mut entities, count, OPERATION)?;
    for entity in sketch_entities {
        if matches(entity)? {
            ctx.push_vec(&mut entities, super::transforms::copy_sketch_entity_identity(
                ctx,
                entity.id(),
                OPERATION,
            )?, OPERATION)?;
        }
    }
    Ok(Some(entities))
}

// Find the unique profile locus pair whose spanned dimension, measured by
// `measure` over the two loci points, equals the parameter length. Every
// unordered pair of canonical profile loci is a candidate; `measure` is the
// only axis of variation between the distance and axis-distance resolvers.
fn unique_profile_measured_loci_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT measured profile locus pair";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let loci = canonical_profile_loci(ctx, sketch, sketch_entities)?;
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    for (first_index, (first_point, first)) in loci.iter().enumerate() {
        for (second_point, second) in &loci[first_index + 1..] {
            let (selected_first, selected_second) = selected.map_or(("", ""), |(first, second)| {
                (locus_entity(first).as_str(), locus_entity(second).as_str())
            });
            charge_relation_identity_work(
                ctx,
                [
                    locus_entity(first).as_str(),
                    locus_entity(second).as_str(),
                    selected_first,
                    selected_second,
                ],
                256,
                OPERATION,
            )?;
            if !same_dimension_length(measure(first_point, second_point), distance.get()) {
                continue;
            }
            let pair = (first, second);
            if selected.is_some_and(|selected| selected != pair) {
                return Ok(None);
            }
            selected = Some(pair);
        }
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::SketchLocusRole::of_locus(first).copy_locus(
            ctx,
            locus_entity(first),
            OPERATION,
        )?,
        super::transforms::SketchLocusRole::of_locus(second).copy_locus(
            ctx,
            locus_entity(second),
            OPERATION,
        )?,
    )))
}

// Repair a candidate pair by resolving each supplied locus to its unique
// partner via `partner`, forming the sorted pair, and keeping it only when the
// two starting loci agree on exactly one pair.
fn unique_repaired_profile_pair(
    ctx: &DecodeContext<'_>,
    first: &SketchLocus,
    second: &SketchLocus,
    partner: impl Fn(&SketchLocus) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError>,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "repair SLDPRT measured profile locus pair";
    let mut selected: Option<(SketchLocus, SketchLocus)> = None;
    for known in [first, second] {
        let Some(partner) = partner(known)? else {
            continue;
        };
        let (selected_first, selected_second) =
            selected.as_ref().map_or(("", ""), |(first, second)| {
                (locus_entity(first).as_str(), locus_entity(second).as_str())
            });
        charge_relation_identity_work(
            ctx,
            [
                locus_entity(known).as_str(),
                locus_entity(&partner).as_str(),
                selected_first,
                selected_second,
            ],
            16,
            OPERATION,
        )?;
        let known = super::transforms::SketchLocusRole::of_locus(known).copy_locus(
            ctx,
            locus_entity(known),
            OPERATION,
        )?;
        let mut pair = [known, partner];
        ctx.sort_unstable_by(
            &mut pair,
            |value| value,
            |left, right| locus_key(left).cmp(&locus_key(right)),
            OPERATION,
        )?;
        let [first, second] = pair;
        let pair = (first, second);
        if selected.as_ref().is_some_and(|selected| selected != &pair) {
            return Ok(None);
        }
        selected = Some(pair);
    }
    Ok(selected)
}

// Find the unique profile locus at a given dimension from `known`, where
// `measure` reports the dimension between the known point and a candidate
// point. The known locus is excluded and, as for the pair resolvers, `measure`
// is the sole axis of variation between the straight-distance and axis-distance
// forms.
fn unique_profile_measured_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT measured profile locus";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(known_point) = profile_locus_point_charged(ctx, known, sketch_entities, OPERATION)?
    else {
        return Ok(None);
    };
    let mut selected: Option<SketchLocus> = None;
    for (candidate_point, candidate) in canonical_profile_loci(ctx, sketch, sketch_entities)? {
        let selected_id = selected
            .as_ref()
            .map_or("", |locus| locus_entity(locus).as_str());
        charge_relation_identity_work(
            ctx,
            [
                locus_entity(&candidate).as_str(),
                locus_entity(known).as_str(),
                selected_id,
            ],
            256,
            OPERATION,
        )?;
        if candidate == *known
            || !same_dimension_length(measure(&known_point, &candidate_point), distance.get())
        {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|selected| selected != &candidate)
        {
            return Ok(None);
        }
        selected = Some(candidate);
    }
    Ok(selected)
}

pub(super) fn unique_profile_distance_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    unique_profile_measured_locus(
        ctx,
        sketch,
        known,
        parameter,
        sketch_entities,
        |known, candidate| (candidate.u - known.u).hypot(candidate.v - known.v),
    )
}

pub(super) fn doubled_profile_distance_loci(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    operands: (usize, usize),
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const OPERATION: &str = "resolve SLDPRT doubled profile distance";
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(line_marker_id) =
        relation_operand_marker(ctx, relation, operands.0, sketch, markers_by_id)?
    else {
        return Ok(None);
    };
    let Some(center_marker_id) =
        relation_operand_marker(ctx, relation, operands.1, sketch, markers_by_id)?
    else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(markers_by_id.len()),
        OPERATION,
    )?;
    let key_bytes = markers_by_id
        .keys()
        .try_fold(0u64, |bytes, key| {
            bytes.checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(
        key_bytes
            .checked_mul(8)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        OPERATION,
    )?;
    charge_relation_identity_work(ctx, [line_marker_id, center_marker_id], 8, OPERATION)?;
    let Some(line_marker) = markers_by_id.get(line_marker_id) else {
        return Ok(None);
    };
    let Some(center_marker) = markers_by_id.get(center_marker_id) else {
        return Ok(None);
    };
    charge_relation_identity_work(
        ctx,
        [
            line_marker.feature_ref.as_deref().unwrap_or(""),
            center_marker.feature_ref.as_deref().unwrap_or(""),
            relation.feature_ref.as_str(),
        ],
        8,
        OPERATION,
    )?;
    if line_marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
        || center_marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
    {
        return Ok(None);
    }
    let mut center_is_distance_handle = false;
    for marker in markers_by_id.values() {
        charge_relation_identity_work(
            ctx,
            [
                marker.feature_ref.as_deref().unwrap_or(""),
                relation.feature_ref.as_str(),
            ],
            16,
            OPERATION,
        )?;
        if marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
            || marker.kind()
                != SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
        {
            continue;
        }
        let [link] = marker.links() else {
            continue;
        };
        charge_relation_identity_work(
            ctx,
            [link.entity_ref.as_str(), center_marker.id()],
            8,
            OPERATION,
        )?;
        if link.entity_ref == center_marker.id() {
            center_is_distance_handle = true;
            break;
        }
    }
    if !center_is_distance_handle {
        return Ok(None);
    }
    let Some(line_coordinates) = line_marker.coordinates_m else {
        return Ok(None);
    };
    let Some(center_coordinates) = center_marker.coordinates_m else {
        return Ok(None);
    };
    let [line_u, line_v] = line_coordinates.get();
    let [center_u, center_v] = center_coordinates.get();
    ctx.charge_work(128, OPERATION)?;
    if ![
        (center_u - line_u).abs() * NATIVE_TO_IR * 2.0,
        (center_v - line_v).abs() * NATIVE_TO_IR * 2.0,
    ]
    .into_iter()
    .any(|distance| same_dimension_length(distance, expected.get()))
    {
        return Ok(None);
    }
    let mut selected = None;
    for entity in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [
                entity.sketch.as_str(),
                sketch.as_str(),
                entity.native_ref.as_deref().unwrap_or(""),
                line_marker_id,
            ],
            128,
            OPERATION,
        )?;
        if entity.sketch != *sketch || entity.native_ref.as_deref() != Some(line_marker_id) {
            continue;
        }
        let SketchGeometryDefinition::Line { start, end } = *entity.geometry.definition() else {
            continue;
        };
        if !same_dimension_length((end.u - start.u).hypot(end.v - start.v), expected.get()) {
            continue;
        }
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some(entity.id());
    }
    let Some(identity) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::SketchLocusRole::Start.copy_locus(ctx, identity, OPERATION)?,
        super::transforms::SketchLocusRole::End.copy_locus(ctx, identity, OPERATION)?,
    )))
}

fn unique_repaired_profile_distance_loci_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    unique_repaired_profile_pair(ctx, first, second, |known| {
        unique_profile_distance_locus(ctx, sketch, known, parameter, sketch_entities)
    })
}

fn unique_profile_axis_distance_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    axis: ProfileAxis,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    unique_profile_measured_locus(
        ctx,
        sketch,
        known,
        parameter,
        sketch_entities,
        |known, candidate| {
            if axis == ProfileAxis::U {
                (candidate.u - known.u).abs()
            } else {
                (candidate.v - known.v).abs()
            }
        },
    )
}

fn unique_repaired_profile_axis_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    axis: ProfileAxis,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    unique_repaired_profile_pair(ctx, first, second, |known| {
        unique_profile_axis_distance_locus(ctx, sketch, known, parameter, sketch_entities, axis)
    })
}

fn unique_profile_axis_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    axis: ProfileAxis,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    unique_profile_measured_loci_pair(ctx, sketch, parameter, sketch_entities, |first, second| {
        if axis == ProfileAxis::U {
            (second.u - first.u).abs()
        } else {
            (second.v - first.v).abs()
        }
    })
}

fn unique_profile_distance_loci_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    unique_profile_measured_loci_pair(ctx, sketch, parameter, sketch_entities, |first, second| {
        (second.u - first.u).hypot(second.v - first.v)
    })
}

pub(super) fn canonical_profile_loci(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Result<Vec<(Point2, SketchLocus)>, cadmpeg_core::CodecError> {
    const QUANTUM: f64 = 1e-8;
    const OPERATION: &str = "collect SLDPRT canonical profile loci";
    let mut indexed = Vec::new();
    for (source_index, entity) in sketch_entities.iter().enumerate() {
        charge_relation_identity_work(
            ctx,
            [entity.sketch.as_str(), sketch.as_str()],
            256,
            OPERATION,
        )?;
        if entity.sketch != *sketch {
            continue;
        }
        for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
            ctx.reserve_vec(&mut indexed, 1, OPERATION)?;
            indexed.push((
                source_index,
                point,
                role.copy_locus(ctx, entity.id(), OPERATION)?,
            ));
        }
    }
    // Source order breaks equal geometric and identity keys without sort scratch.
    ctx.sort_unstable_by(
        &mut indexed,
        |value| value,
        |(left_index, left_point, left_locus), (right_index, right_point, right_locus)| {
            quantize(*left_point, QUANTUM)
                .cmp(&quantize(*right_point, QUANTUM))
                .then_with(|| locus_key(left_locus).cmp(&locus_key(right_locus)))
                .then_with(|| left_index.cmp(right_index))
        },
        OPERATION,
    )?;
    indexed.dedup_by(|(_, left_point, _), (_, right_point, _)| {
        quantize(*left_point, QUANTUM) == quantize(*right_point, QUANTUM)
    });
    let mut loci = Vec::new();
    ctx.reserve_vec(&mut loci, indexed.len(), OPERATION)?;
    for (_, point, locus) in indexed {
        loci.push((point, locus));
    }
    Ok(loci)
}

fn charge_relation_identity_work<const N: usize>(
    ctx: &DecodeContext<'_>,
    identities: [&str; N],
    scalar_work: u64,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    let work = identities
        .into_iter()
        .try_fold(scalar_work, |work, identity| {
            cadmpeg_core::decode::u64_from_index(identity.len())
                .checked_mul(4)
                .and_then(|bytes| work.checked_add(bytes))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
        })?;
    ctx.charge_work(work, operation)?;
    Ok(())
}

// Find the unique sketch entity, other than `known`, for which `matches`
// accepts the ordered pair `(known, candidate)`. The parameter kind guard and
// the measurement both live in `matches`, so the straight-distance and angle
// resolvers differ only in the closure they pass.
fn unique_profile_matched_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    sketch_entities: &[SketchEntity],
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT matched sketch entity";
    let mut known_entity = None;
    for entity in sketch_entities {
        charge_relation_identity_work(ctx, [entity.id().as_str(), known.as_str()], 8, OPERATION)?;
        if entity.id() == known {
            known_entity = Some(entity);
            break;
        }
    }
    let Some(known) = known_entity else {
        return Ok(None);
    };
    let mut selected: Option<&SketchEntityId> = None;
    for entity in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [
                entity.sketch.as_str(),
                sketch.as_str(),
                entity.id().as_str(),
                known.id().as_str(),
                selected.map_or("", SketchEntityId::as_str),
            ],
            256,
            OPERATION,
        )?;
        if entity.sketch != *sketch || entity.id() == known.id() || !matches(known, entity) {
            continue;
        }
        if selected.is_some_and(|selected| selected != entity.id()) {
            return Ok(None);
        }
        selected = Some(entity.id());
    }
    selected
        .map(|entity| super::transforms::copy_sketch_entity_identity(ctx, entity, OPERATION))
        .transpose()
}

// Select one distinct ordered identity pair from unordered line positions.
// Only line entities participate; `matches` supplies the measurement and comparison.
fn unique_profile_matched_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT matched sketch line pair";
    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
    for (first_index, first) in sketch_entities.iter().enumerate() {
        charge_relation_identity_work(ctx, [first.sketch.as_str(), sketch.as_str()], 8, OPERATION)?;
        if first.sketch != *sketch
            || !matches!(
                first.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        for second in &sketch_entities[first_index + 1..] {
            let (selected_first, selected_second) = selected.map_or(("", ""), |(first, second)| {
                (first.as_str(), second.as_str())
            });
            charge_relation_identity_work(
                ctx,
                [
                    second.sketch.as_str(),
                    sketch.as_str(),
                    first.id().as_str(),
                    second.id().as_str(),
                    selected_first,
                    selected_second,
                ],
                256,
                OPERATION,
            )?;
            if second.sketch != *sketch
                || !matches!(
                    second.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
                || !matches(first, second)
            {
                continue;
            }
            let pair = (first.id(), second.id());
            if selected.is_some_and(|selected| selected != pair) {
                return Ok(None);
            }
            selected = Some(pair);
        }
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::copy_sketch_entity_identity(ctx, first, OPERATION)?,
        super::transforms::copy_sketch_entity_identity(ctx, second, OPERATION)?,
    )))
}

// Resolve each supplied entity to its unique partner and sort its two identities.
// Keep one distinct pair across the two resolutions.
fn unique_repaired_entity_pair(
    ctx: &DecodeContext<'_>,
    first: &SketchEntityId,
    second: &SketchEntityId,
    partner: impl Fn(&SketchEntityId) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT repaired sketch entity pair";
    let mut selected: Option<(SketchEntityId, SketchEntityId)> = None;
    for known in [first, second] {
        let Some(partner) = partner(known)? else {
            continue;
        };
        charge_relation_identity_work(
            ctx,
            [
                known.as_str(),
                partner.as_str(),
                selected.as_ref().map_or("", |pair| pair.0.as_str()),
                selected.as_ref().map_or("", |pair| pair.1.as_str()),
            ],
            8,
            OPERATION,
        )?;
        let mut pair = [
            super::transforms::copy_sketch_entity_identity(ctx, known, OPERATION)?,
            partner,
        ];
        ctx.stable_sort_by(&mut pair, |value| value, Ord::cmp, OPERATION)?;
        let [first, second] = pair;
        let pair = (first, second);
        if selected.as_ref().is_some_and(|selected| selected != &pair) {
            return Ok(None);
        }
        selected = Some(pair);
    }
    Ok(selected)
}

fn unique_profile_line_distance_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, sketch_entities, |known, candidate| {
        line_line_distance(known, candidate)
            .is_some_and(|measured| same_dimension_length(measured, distance.get()))
    })
}

fn unique_marker_line_distance_entity(
    ctx: &DecodeContext<'_>,
    marker: &str,
    profile: SketchRelationEntities<'_>,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "locate SLDPRT dimensioned line marker";
    let SketchRelationEntities {
        sketch,
        sketch_entities,
    } = profile;

    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(marker_locus) = marker_point_locus(ctx, marker, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    for entity in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [entity.id().as_str(), locus_entity(&marker_locus).as_str()],
            16,
            OPERATION,
        )?;
    }
    let Some(marker_point) = profile_locus_point_charged(
        ctx,
        &marker_locus,
        sketch_entities,
        "resolve SLDPRT profile locus",
    )?
    else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, sketch_entities, |known, candidate| {
        matches!(
            candidate.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) && sketch_entity_contains_point(candidate, marker_point)
            && line_line_distance(known, candidate)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
    })
}

fn unique_profile_line_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_line_pair(ctx, sketch, sketch_entities, |first, second| {
        line_line_distance(first, second)
            .is_some_and(|measured| same_dimension_length(measured, distance.get()))
    })
}

fn unique_repaired_profile_line_distance_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    unique_repaired_entity_pair(ctx, first, second, |known| {
        unique_profile_line_distance_entity(ctx, sketch, known, parameter, sketch_entities)
    })
}

pub(super) fn line_line_distance(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometryDefinition::Line {
        start: first_start,
        end: first_end,
    } = first.geometry.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Line {
        start: second_start,
        end: second_end,
    } = second.geometry.definition()
    else {
        return None;
    };
    super::relation_records::line_line_distance(
        [[first_start.u, first_start.v], [first_end.u, first_end.v]],
        [
            [second_start.u, second_start.v],
            [second_end.u, second_end.v],
        ],
    )
}

fn unique_profile_line_angle_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, sketch_entities, |known, candidate| {
        line_line_angle(known, candidate)
            .is_some_and(|measured| same_dimension_angle(measured, angle.get()))
    })
}

#[derive(Clone, Copy)]
enum LinePairOrdering {
    Operand,
    Identity,
}

pub(super) fn find_profile_entity<'a>(
    ctx: &DecodeContext<'_>,
    entities: &'a [SketchEntity],
    id: &SketchEntityId,
    operation: &'static str,
) -> Result<Option<&'a SketchEntity>, cadmpeg_core::CodecError> {
    for entity in entities {
        charge_relation_identity_work(ctx, [entity.id().as_str(), id.as_str()], 8, operation)?;
        if entity.id() == id {
            return Ok(Some(entity));
        }
    }
    Ok(None)
}

fn select_dynamic_line_pair(
    ctx: &DecodeContext<'_>,
    first_candidates: &[SketchEntityId],
    second_candidates: &[SketchEntityId],
    entities: &[SketchEntity],
    ordering: LinePairOrdering,
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic line pair";
    if first_candidates.is_empty() || second_candidates.is_empty() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
    for first in first_candidates {
        let Some(first_entity) = find_profile_entity(ctx, entities, first, OPERATION)? else {
            return Ok(None);
        };
        for second in second_candidates {
            let (selected_first, selected_second) = selected.map_or(("", ""), |(first, second)| {
                (first.as_str(), second.as_str())
            });
            charge_relation_identity_work(
                ctx,
                [
                    first.as_str(),
                    second.as_str(),
                    selected_first,
                    selected_second,
                ],
                256,
                OPERATION,
            )?;
            if first == second {
                continue;
            }
            let Some(second_entity) = find_profile_entity(ctx, entities, second, OPERATION)? else {
                return Ok(None);
            };
            if !matches(first_entity, second_entity) {
                continue;
            }
            let pair = match ordering {
                LinePairOrdering::Operand => (first, second),
                LinePairOrdering::Identity => {
                    if first <= second {
                        (first, second)
                    } else {
                        (second, first)
                    }
                }
            };
            if selected.is_some_and(|selected| selected != pair) {
                return Ok(None);
            }
            selected = Some(pair);
        }
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::copy_sketch_entity_identity(ctx, first, OPERATION)?,
        super::transforms::copy_sketch_entity_identity(ctx, second, OPERATION)?,
    )))
}

fn unique_dynamic_marker_line_angle_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let first_candidates = dynamic_line_operand_candidates(
        ctx,
        relation,
        0,
        sketch,
        markers_by_id,
        loci_by_marker,
        sketch_entities,
    )?;
    let second_candidates = dynamic_line_operand_candidates(
        ctx,
        relation,
        1,
        sketch,
        markers_by_id,
        loci_by_marker,
        sketch_entities,
    )?;
    select_dynamic_line_pair(
        ctx,
        &first_candidates,
        &second_candidates,
        sketch_entities,
        LinePairOrdering::Operand,
        |first, second| {
            unoriented_line_line_angle(first, second)
                .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
        },
    )
}

fn unique_dynamic_profile_line_angle_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_entity(ctx, sketch, known, sketch_entities, |known, candidate| {
        unoriented_line_line_angle(known, candidate)
            .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
    })
}

fn unique_dynamic_roster_line_angle_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    unique_profile_matched_line_pair(ctx, sketch, sketch_entities, |first, second| {
        unoriented_line_line_angle(first, second)
            .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
    })
}

/// The sketch entities and marker indexes shared by the two operand loci of one
/// dynamic point-pair relation.
struct DynamicMarkerPointIndex<'a> {
    sketch_entities: &'a [SketchEntity],
    markers_by_id: &'a HashMap<&'a str, &'a SketchInputEntity>,
    loci_by_marker: &'a HashMap<String, Vec<SketchLocus>>,
    profile_axis: Option<ProfileAxis>,
}

fn unique_dynamic_marker_point_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    known: (Option<SketchLocus>, Option<SketchLocus>),
    index: &DynamicMarkerPointIndex<'_>,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic point pairs";

    let DynamicMarkerPointIndex {
        sketch_entities,
        markers_by_id,
        loci_by_marker,
        profile_axis,
    } = *index;
    let (known_first, known_second) = known;
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let measure = |first: &SketchLocus,
                   second: &SketchLocus|
     -> Result<Option<f64>, cadmpeg_core::CodecError> {
        let Some(first_point) =
            profile_locus_point_charged(ctx, first, sketch_entities, OPERATION)?
        else {
            return Ok(None);
        };
        let Some(second_point) =
            profile_locus_point_charged(ctx, second, sketch_entities, OPERATION)?
        else {
            return Ok(None);
        };
        ctx.charge_work(256, OPERATION)?;
        Ok(Some(match relation.family {
            FeatureInputRelationFamily::PointPointDistance => {
                (second_point.u - first_point.u).hypot(second_point.v - first_point.v)
            }
            FeatureInputRelationFamily::PointPointHorizontalDistance => {
                if profile_axis == Some(ProfileAxis::U) {
                    (second_point.u - first_point.u).abs()
                } else {
                    (second_point.v - first_point.v).abs()
                }
            }
            FeatureInputRelationFamily::PointPointVerticalDistance => {
                if profile_axis == Some(ProfileAxis::U) {
                    (second_point.u - first_point.u).abs()
                } else {
                    (second_point.v - first_point.v).abs()
                }
            }
            _ => return Ok(None),
        }))
    };
    if let (Some(first), Some(second)) = (&known_first, &known_second) {
        if measure(first, second)?
            .is_some_and(|value| same_relation_dimension_length(value, expected.get()))
        {
            return Ok(known_first.zip(known_second));
        }
    }
    let candidates = |index: usize,
                      known: Option<SketchLocus>|
     -> Result<Vec<SketchLocus>, cadmpeg_core::CodecError> {
        let mut candidates = known.into_iter().collect::<Vec<_>>();
        if let Some(marker) = relation_operand_marker(ctx, relation, index, sketch, markers_by_id)?
        {
            let additions = dynamic_marker_point_candidates(
                ctx,
                marker,
                sketch,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            )?;
            ctx.extend_vec(&mut candidates, additions, "append SLDPRT dynamic point candidates")?;
        }
        Ok(candidates)
    };
    let mut first_candidates = candidates(0, known_first)?;
    let mut second_candidates = candidates(1, known_second)?;
    deduplicate_physical_loci(ctx, &mut first_candidates, |locus| {
        profile_locus_point_charged(ctx, locus, sketch_entities, "resolve SLDPRT physical locus")
    })?;
    deduplicate_physical_loci(ctx, &mut second_candidates, |locus| {
        profile_locus_point_charged(ctx, locus, sketch_entities, "resolve SLDPRT physical locus")
    })?;
    if first_candidates.is_empty() || second_candidates.is_empty() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    for first in &first_candidates {
        for second in &second_candidates {
            let (selected_first, selected_second) = selected.map_or(("", ""), |(first, second)| {
                (locus_entity(first).as_str(), locus_entity(second).as_str())
            });
            charge_relation_identity_work(
                ctx,
                [
                    locus_entity(first).as_str(),
                    locus_entity(second).as_str(),
                    selected_first,
                    selected_second,
                ],
                16,
                OPERATION,
            )?;
            if first == second {
                continue;
            }
            if !measure(first, second)?
                .is_some_and(|value| same_relation_dimension_length(value, expected.get()))
            {
                continue;
            }
            let pair = if locus_key(first) <= locus_key(second) {
                (first, second)
            } else {
                (second, first)
            };
            if selected.is_some_and(|selected| selected != pair) {
                return Ok(None);
            }
            selected = Some(pair);
        }
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::SketchLocusRole::of_locus(first).copy_locus(
            ctx,
            locus_entity(first),
            OPERATION,
        )?,
        super::transforms::SketchLocusRole::of_locus(second).copy_locus(
            ctx,
            locus_entity(second),
            OPERATION,
        )?,
    )))
}

fn unique_dynamic_direct_point_roster_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    profile_axis: Option<ProfileAxis>,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT direct marker point roster";
    for operand in &relation.operands {
        ctx.charge_work(1, OPERATION)?;
        if operand.entity_ref.is_some() {
            return Ok(None);
        }
    }
    let is_direct_point = |marker: &SketchInputEntity| {
        marker.feature_ref.as_deref() == Some(relation.feature_ref.as_str())
            && marker.coordinates_m.is_some()
            && marker.links().is_empty()
            && matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(markers_by_id.len()),
        OPERATION,
    )?;
    let marker_bytes = markers_by_id.keys().try_fold(0u64, |bytes, key| {
        bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
    })?;
    for index in 0..2 {
        let Some(marker_id) = relation_operand_marker(ctx, relation, index, sketch, markers_by_id)?
        else {
            return Ok(None);
        };
        ctx.charge_work(
            marker_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(marker_id.len()))
                .and_then(|bytes| bytes.checked_mul(4))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let Some(marker) = markers_by_id.get(marker_id) else {
            return Ok(None);
        };
        charge_relation_identity_work(
            ctx,
            [
                marker.feature_ref.as_deref().unwrap_or(""),
                relation.feature_ref.as_str(),
            ],
            16,
            OPERATION,
        )?;
        if !is_direct_point(marker) {
            return Ok(None);
        }
    }
    let mut direct_marker_ids = HashSet::new();
    let mut source_bytes = 0u64;
    for marker in markers_by_id.values() {
        charge_relation_identity_work(
            ctx,
            [
                marker.feature_ref.as_deref().unwrap_or(""),
                relation.feature_ref.as_str(),
            ],
            16,
            OPERATION,
        )?;
        if !is_direct_point(marker) {
            continue;
        }
        let marker_id = marker.id();
        let bytes = cadmpeg_core::decode::u64_from_index(marker_id.len());
        if reserve_profile_locus_set_slot(
            ctx,
            &mut direct_marker_ids,
            &marker_id,
            source_bytes,
            bytes,
            OPERATION,
        )? {
            source_bytes = source_bytes
                .checked_add(bytes)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            direct_marker_ids.insert(marker_id);
        }
    }
    let mut loci = Vec::new();
    for entity in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [entity.sketch.as_str(), sketch.as_str()],
            16,
            OPERATION,
        )?;
        if entity.sketch != *sketch
            || !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
        {
            continue;
        }
        let Some(reference) = entity.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(
            source_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(reference.len()))
                .and_then(|bytes| bytes.checked_mul(4))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if !direct_marker_ids.contains(reference) {
            continue;
        }
        ctx.reserve_vec(&mut loci, 1, OPERATION)?;
        loci.push(super::transforms::SketchLocusRole::Entity.copy_locus(
            ctx,
            entity.id(),
            OPERATION,
        )?);
    }
    sort_profile_loci(ctx, &mut loci, OPERATION)?;
    deduplicate_physical_loci(ctx, &mut loci, |locus| {
        profile_locus_point_charged(ctx, locus, sketch_entities, OPERATION)
    })?;
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let measure = |first: &SketchLocus,
                   second: &SketchLocus|
     -> Result<Option<f64>, cadmpeg_core::CodecError> {
        let Some(first) = profile_locus_point_charged(ctx, first, sketch_entities, OPERATION)?
        else {
            return Ok(None);
        };
        let Some(second) = profile_locus_point_charged(ctx, second, sketch_entities, OPERATION)?
        else {
            return Ok(None);
        };
        ctx.charge_work(256, OPERATION)?;
        Ok(Some(match relation.family {
            FeatureInputRelationFamily::PointPointDistance => {
                (second.u - first.u).hypot(second.v - first.v)
            }
            FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance => {
                if profile_axis == Some(ProfileAxis::U) {
                    (second.u - first.u).abs()
                } else {
                    (second.v - first.v).abs()
                }
            }
            _ => return Ok(None),
        }))
    };
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    for (first_index, first) in loci.iter().enumerate() {
        for second in &loci[first_index + 1..] {
            let (selected_first, selected_second) = selected.map_or(("", ""), |(first, second)| {
                (locus_entity(first).as_str(), locus_entity(second).as_str())
            });
            charge_relation_identity_work(
                ctx,
                [
                    locus_entity(first).as_str(),
                    locus_entity(second).as_str(),
                    selected_first,
                    selected_second,
                ],
                16,
                OPERATION,
            )?;
            if !measure(first, second)?
                .is_some_and(|value| same_relation_dimension_length(value, expected.get()))
            {
                continue;
            }
            let pair = (first, second);
            if selected.is_some_and(|selected| selected != pair) {
                return Ok(None);
            }
            selected = Some(pair);
        }
    }
    let Some((first, second)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::SketchLocusRole::of_locus(first).copy_locus(
            ctx,
            locus_entity(first),
            OPERATION,
        )?,
        super::transforms::SketchLocusRole::of_locus(second).copy_locus(
            ctx,
            locus_entity(second),
            OPERATION,
        )?,
    )))
}

const DYNAMIC_POINT_LOCUS_QUANTUM: f64 = 1.0e-8;

fn deduplicate_physical_loci<T: cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    candidates: &mut Vec<T>,
    point: impl Fn(&T) -> Result<Option<Point2>, cadmpeg_core::CodecError>,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "deduplicate SLDPRT physical loci";
    let mut points = HashSet::new();
    let mut write = 0;
    for read in 0..candidates.len() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<T>())
                .checked_add(128)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let keep = if let Some(point) = point(&candidates[read])? {
            let key = quantize(point, DYNAMIC_POINT_LOCUS_QUANTUM);
            if reserve_profile_locus_set_slot(ctx, &mut points, &key, 0, 0, OPERATION)? {
                points.insert(key);
                true
            } else {
                false
            }
        } else {
            true
        };
        if keep {
            candidates.swap(write, read);
            write += 1;
        }
    }
    ctx.truncate_vec(candidates, write, "discard SLDPRT duplicate physical loci")?;
    Ok(())
}

/// The point-line operands a dynamic roster relation already knows.
enum KnownOperands {
    Point(SketchLocus),
    Line(SketchEntityId),
    Both(SketchLocus, SketchEntityId),
}

impl KnownOperands {
    fn resolve(point: Option<SketchLocus>, line: Option<SketchEntityId>) -> Option<Self> {
        match (point, line) {
            (Some(point), Some(line)) => Some(Self::Both(point, line)),
            (Some(point), None) => Some(Self::Point(point)),
            (None, Some(line)) => Some(Self::Line(line)),
            (None, None) => None,
        }
    }
}

fn unique_dynamic_roster_point_line_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    known_point: Option<SketchLocus>,
    known_line: Option<SketchEntityId>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic roster point-line pair";
    ctx.charge_work(16, OPERATION)?;
    let explicit_point = relation
        .operands
        .first()
        .is_some_and(|operand| operand.entity_ref.is_some());
    let explicit_line = relation
        .operands
        .get(1)
        .is_some_and(|operand| operand.entity_ref.is_some());
    if (explicit_point && known_point.is_none()) || (explicit_line && known_line.is_none()) {
        return Ok(None);
    }
    // Explicit identities narrow the geometry roster to the other operand.
    let Some(known) = KnownOperands::resolve(
        explicit_point.then_some(known_point).flatten(),
        explicit_line.then_some(known_line).flatten(),
    ) else {
        if explicit_point || explicit_line {
            return Ok(None);
        }
        return unique_roster_point_line_pair(ctx, sketch, parameter, sketch_entities);
    };
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let lines = collect_profile_lines(ctx, sketch, sketch_entities)?;
    let mut points = collect_profile_loci(ctx, sketch, sketch_entities)?;
    deduplicate_physical_loci(ctx, &mut points, |(_, locus)| {
        profile_locus_point_charged(ctx, locus, sketch_entities, OPERATION)
    })?;
    match known {
        KnownOperands::Point(point) => {
            let Some(position) =
                profile_locus_point_charged(ctx, &point, sketch_entities, OPERATION)?
            else {
                return Ok(None);
            };
            select_profile_point_line_pairs(ctx, *distance, &[(position, point)], &lines)
        }
        KnownOperands::Line(line) => {
            let Some(line_entity) = find_profile_entity(ctx, sketch_entities, &line, OPERATION)?
            else {
                return Ok(None);
            };
            resolve_profile_locus_positions(ctx, &mut points, sketch_entities)?;
            select_profile_point_line_pairs(ctx, *distance, &points, &[line_entity])
        }
        KnownOperands::Both(point, line) => {
            let Some(line_entity) = find_profile_entity(ctx, sketch_entities, &line, OPERATION)?
            else {
                return Ok(None);
            };
            let Some(position) =
                profile_locus_point_charged(ctx, &point, sketch_entities, OPERATION)?
            else {
                return Ok(None);
            };
            ctx.charge_work(256, OPERATION)?;
            Ok(point_line_distance_value(position, line_entity)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
                .then_some((point, line)))
        }
    }
}

fn unique_roster_point_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let mut loci = collect_profile_loci(ctx, sketch, sketch_entities)?;
    deduplicate_physical_loci(ctx, &mut loci, |(_, locus)| {
        profile_locus_point_charged(ctx, locus, sketch_entities, "resolve SLDPRT roster locus")
    })?;
    // The owning sketch roster supplies line witnesses for family-scoped operands.
    let lines = collect_profile_lines(ctx, sketch, sketch_entities)?;
    resolve_profile_locus_positions(ctx, &mut loci, sketch_entities)?;
    select_profile_point_line_pairs(ctx, *distance, &loci, &lines)
}

fn unique_point_line_candidate_pair(
    ctx: &DecodeContext<'_>,
    expected: cadmpeg_ir::scalar::Length,
    point_candidates: &[SketchLocus],
    line_candidates: &[SketchEntityId],
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic point-line pair";
    if point_candidates.is_empty() || line_candidates.is_empty() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchLocus, &SketchEntityId)> = None;
    for point in point_candidates {
        for entity in sketch_entities {
            charge_relation_identity_work(
                ctx,
                [entity.id().as_str(), locus_entity(point).as_str()],
                16,
                OPERATION,
            )?;
        }
        let Some(point_position) = profile_locus_point_charged(
            ctx,
            point,
            sketch_entities,
            "resolve SLDPRT profile locus",
        )?
        else {
            return Ok(None);
        };
        for line in line_candidates {
            let (selected_point, selected_line) = selected.map_or(("", ""), |(point, line)| {
                (locus_entity(point).as_str(), line.as_str())
            });
            charge_relation_identity_work(
                ctx,
                [
                    locus_entity(point).as_str(),
                    line.as_str(),
                    selected_point,
                    selected_line,
                ],
                256,
                OPERATION,
            )?;
            let Some(line_entity) = find_profile_entity(ctx, sketch_entities, line, OPERATION)?
            else {
                return Ok(None);
            };
            if !point_line_distance_value(point_position, line_entity)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
            {
                continue;
            }
            let pair = (point, line);
            if selected.is_some_and(|selected| selected != pair) {
                return Ok(None);
            }
            selected = Some(pair);
        }
    }
    let Some((point, line)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::SketchLocusRole::of_locus(point).copy_locus(
            ctx,
            locus_entity(point),
            OPERATION,
        )?,
        super::transforms::copy_sketch_entity_identity(ctx, line, OPERATION)?,
    )))
}

fn unique_dynamic_marker_line_distance_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let first_candidates = dynamic_line_operand_candidates(
        ctx,
        relation,
        0,
        sketch,
        markers_by_id,
        loci_by_marker,
        sketch_entities,
    )?;
    let second_candidates = dynamic_line_operand_candidates(
        ctx,
        relation,
        1,
        sketch,
        markers_by_id,
        loci_by_marker,
        sketch_entities,
    )?;
    select_dynamic_line_pair(
        ctx,
        &first_candidates,
        &second_candidates,
        sketch_entities,
        LinePairOrdering::Identity,
        |first, second| {
            line_line_distance(first, second)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
        },
    )
}

fn unique_dynamic_roster_line_distance_pair(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    for operand in &relation.operands {
        ctx.charge_work(1, "scan SLDPRT roster line relation operands")?;
        if operand.entity_ref.is_some() {
            return Ok(None);
        }
    }
    unique_profile_line_distance_pair(ctx, sketch, parameter, sketch_entities)
}

fn dynamic_line_operand_candidates(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    index: usize,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Vec<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "filter SLDPRT dynamic line operand candidates";

    let mut entities = if let Some(entity) =
        solver_line_entity(ctx, relation, index, sketch, sketch_entities)?
    {
        vec![entity]
    } else if let Some(marker) =
        match relation_operand_marker(ctx, relation, index, sketch, markers_by_id)? {
            Some(marker) => Some(marker),
            None => relation_line_point_marker(ctx, relation, index, markers_by_id)?
                .map(super::super::records::SketchInputEntity::id),
        }
    {
        dynamic_marker_line_candidates(ctx, marker, markers_by_id, loci_by_marker, sketch_entities)?
    } else {
        Vec::new()
    };
    let mut write = 0;
    for read in 0..entities.len() {
        let mut valid = false;
        for entity in sketch_entities {
            charge_relation_identity_work(
                ctx,
                [
                    entity.id().as_str(),
                    entities[read].as_str(),
                    entity.sketch.as_str(),
                    sketch.as_str(),
                ],
                16,
                OPERATION,
            )?;
            if entity.id() == &entities[read]
                && entity.sketch == *sketch
                && matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            {
                valid = true;
                break;
            }
        }
        if valid {
            entities.swap(write, read);
            write += 1;
        }
    }
    ctx.truncate_vec(&mut entities, write, OPERATION)?;
    if entities.len() > 1 {
        ctx.sort_unstable_by(&mut entities, |value| value, Ord::cmp, OPERATION)?;
        ctx.dedup_vec(&mut entities, "deduplicate SLDPRT marker entities")?;
    }
    Ok(entities)
}

fn dynamic_marker_point_candidates(
    ctx: &DecodeContext<'_>,
    marker: &str,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Vec<SketchLocus>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic marker points";
    let mut marker_ids = HashSet::new();
    collect_marker_identity_ids(ctx, marker, markers_by_id, &mut marker_ids)?;
    let centers = dynamic_marker_center_candidates(
        ctx,
        marker,
        sketch,
        markers_by_id,
        loci_by_marker,
        sketch_entities,
    )?;
    if let Some(centers) = centers.filter(|centers| !centers.is_empty()) {
        return Ok(if centers.len() == 1 {
            centers
        } else {
            Vec::new()
        });
    }
    let mut candidates = Vec::new();
    if let Some(locus) = marker_point_locus(ctx, marker, markers_by_id, loci_by_marker)? {
        if let Some(entity) =
            find_profile_entity(ctx, sketch_entities, locus_entity(&locus), OPERATION)?
        {
            charge_relation_identity_work(
                ctx,
                [entity.sketch.as_str(), sketch.as_str()],
                16,
                OPERATION,
            )?;
            if entity.sketch == *sketch
                && profile_locus_point_charged(
                    ctx,
                    &locus,
                    sketch_entities,
                    "resolve SLDPRT profile locus",
                )?
                .is_some()
            {
                ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                candidates.push(locus);
            }
        }
    }
    for entity in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [entity.sketch.as_str(), sketch.as_str()],
            16,
            OPERATION,
        )?;
        if entity.sketch != *sketch
            || !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
        {
            continue;
        }
        if dynamic_entity_has_marker_identity(ctx, entity, &marker_ids, OPERATION)? {
            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
            candidates.push(super::transforms::SketchLocusRole::Entity.copy_locus(
                ctx,
                entity.id(),
                OPERATION,
            )?);
        }
    }
    sort_profile_loci(ctx, &mut candidates, OPERATION)?;
    Ok(candidates)
}

fn dynamic_entity_has_marker_identity(
    ctx: &DecodeContext<'_>,
    entity: &SketchEntity,
    marker_ids: &HashSet<&str>,
    operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(marker_ids.len()),
        operation,
    )?;
    let source_bytes = marker_ids.iter().try_fold(0u64, |bytes, identity| {
        bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(identity.len()))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
    })?;
    for reference in entity
        .native_ref
        .as_deref()
        .into_iter()
        .chain(entity.geometry_ref.as_deref())
        .chain(entity.endpoint_refs.iter().map(std::string::String::as_str))
    {
        ctx.charge_work(
            source_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(reference.len()))
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
            operation,
        )?;
        if marker_ids.contains(reference) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn dynamic_marker_point_locus(
    ctx: &DecodeContext<'_>,
    marker: &str,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    match dynamic_marker_center_candidates(
        ctx,
        marker,
        sketch,
        markers_by_id,
        loci_by_marker,
        sketch_entities,
    )? {
        Some(candidates) if !candidates.is_empty() => Ok(if candidates.len() == 1 {
            candidates.into_iter().next()
        } else {
            None
        }),
        Some(_) | None => marker_point_locus(ctx, marker, markers_by_id, loci_by_marker),
    }
}

fn dynamic_marker_center_candidates(
    ctx: &DecodeContext<'_>,
    marker: &str,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<Vec<SketchLocus>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic marker centers";
    let mut marker_ids = HashSet::new();
    collect_marker_identity_ids(ctx, marker, markers_by_id, &mut marker_ids)?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(markers_by_id.len()),
        OPERATION,
    )?;
    let marker_key_bytes = markers_by_id.keys().try_fold(0u64, |bytes, identity| {
        bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(identity.len()))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
    })?;
    let mut has_arc_marker = false;
    let mut centers = Vec::new();
    for marker_id in marker_ids {
        ctx.charge_work(
            marker_key_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(marker_id.len()))
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| work.checked_add(16))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let Some(marker) = markers_by_id.get(marker_id) else {
            continue;
        };
        if !matches!(marker.kind(), SketchInputKind::Arc) {
            continue;
        }
        has_arc_marker = true;
        let Some(locus) = marker_point_locus(ctx, marker_id, markers_by_id, loci_by_marker)? else {
            continue;
        };
        let Some(entity) =
            find_profile_entity(ctx, sketch_entities, locus_entity(&locus), OPERATION)?
        else {
            continue;
        };
        charge_relation_identity_work(
            ctx,
            [entity.sketch.as_str(), sketch.as_str()],
            16,
            OPERATION,
        )?;
        if entity.sketch == *sketch
            && matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Arc { .. }
            )
        {
            ctx.reserve_vec(&mut centers, 1, OPERATION)?;
            centers.push(super::transforms::SketchLocusRole::Center.copy_locus(
                ctx,
                entity.id(),
                OPERATION,
            )?);
        }
    }
    if !has_arc_marker {
        return Ok(None);
    }
    sort_profile_loci(ctx, &mut centers, OPERATION)?;
    Ok(Some(centers))
}

fn dynamic_marker_line_candidates(
    ctx: &DecodeContext<'_>,
    marker: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Vec<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic marker lines";
    let mut marker_ids = HashSet::new();
    collect_marker_identity_ids(ctx, marker, markers_by_id, &mut marker_ids)?;
    let mut candidates = marker_entities(
        ctx,
        marker,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::Lines(sketch_entities),
    )?;
    for entity in sketch_entities {
        ctx.charge_work(16, OPERATION)?;
        if matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) && dynamic_entity_has_marker_identity(ctx, entity, &marker_ids, OPERATION)?
        {
            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
            candidates.push(super::transforms::copy_sketch_entity_identity(
                ctx,
                entity.id(),
                OPERATION,
            )?);
        }
    }
    if candidates.is_empty() {
        if let Some(entity) =
            single_marker_line_entity(ctx, marker, markers_by_id, loci_by_marker, sketch_entities)?
        {
            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
            candidates.push(entity);
        }
    }
    ctx.sort_unstable_by(&mut candidates, |value| value, Ord::cmp, OPERATION)?;
    ctx.dedup_vec(&mut candidates, "deduplicate SLDPRT marker line candidates")?;
    Ok(candidates)
}

fn collect_marker_identity_ids<'a>(
    ctx: &DecodeContext<'_>,
    marker_id: &'a str,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    marker_ids: &mut HashSet<&'a str>,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT dynamic marker identities";
    let _depth = ctx.enter_nested(OPERATION)?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(marker_ids.len()),
        OPERATION,
    )?;
    let source_bytes = marker_ids.iter().try_fold(0u64, |bytes, identity| {
        bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(identity.len()))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
    })?;
    if !reserve_profile_locus_set_slot(
        ctx,
        marker_ids,
        &marker_id,
        source_bytes,
        cadmpeg_core::decode::u64_from_index(marker_id.len()),
        OPERATION,
    )? {
        return Ok(());
    }
    marker_ids.insert(marker_id);
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(markers_by_id.len()),
        OPERATION,
    )?;
    let source_bytes = markers_by_id.keys().try_fold(0u64, |bytes, identity| {
        bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(identity.len()))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
    })?;
    ctx.charge_work(
        source_bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(marker_id.len()))
            .and_then(|bytes| bytes.checked_mul(4))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        OPERATION,
    )?;
    let Some(marker) = markers_by_id.get(marker_id) else {
        return Ok(());
    };
    for link in marker.links() {
        charge_relation_identity_work(
            ctx,
            [link.entity_ref.as_str(), marker_id, marker.id()],
            16,
            OPERATION,
        )?;
        if link.entity_ref != marker_id
            && (!matches!(marker.kind(), SketchInputKind::Relation(_))
                || !relation_link_identifies_owner(marker, link))
        {
            collect_marker_identity_ids(ctx, &link.entity_ref, markers_by_id, marker_ids)?;
        }
    }
    Ok(())
}

fn unique_profile_line_angle_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    unique_profile_matched_line_pair(ctx, sketch, sketch_entities, |first, second| {
        line_line_angle(first, second)
            .is_some_and(|measured| same_dimension_angle(measured, angle.get()))
    })
}

fn unique_repaired_profile_line_angle_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchEntityId, SketchEntityId)>, cadmpeg_core::CodecError> {
    unique_repaired_entity_pair(ctx, first, second, |known| {
        unique_profile_line_angle_entity(ctx, sketch, known, parameter, sketch_entities)
    })
}

pub(super) fn line_line_angle(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometryDefinition::Line {
        start: first_start,
        end: first_end,
    } = first.geometry.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Line {
        start: second_start,
        end: second_end,
    } = second.geometry.definition()
    else {
        return None;
    };
    super::relation_records::line_line_angle(
        [[first_start.u, first_start.v], [first_end.u, first_end.v]],
        [
            [second_start.u, second_start.v],
            [second_end.u, second_end.v],
        ],
    )
}

pub(super) fn unoriented_line_line_angle(
    first: &SketchEntity,
    second: &SketchEntity,
) -> Option<f64> {
    let angle = line_line_angle(first, second)?;
    Some(angle.min(std::f64::consts::PI - angle))
}

pub(super) fn same_dimension_angle(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= EPS_RELATION_LOCI_SAME_DIMENSION_ANGLE_E9 * left.abs().max(right.abs()).max(1.0)
}

fn collect_profile_loci(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Result<Vec<(Point2, SketchLocus)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT profile locus candidates";
    let mut result = Vec::new();
    for entity in entities {
        charge_relation_identity_work(
            ctx,
            [entity.sketch.as_str(), sketch.as_str()],
            256,
            OPERATION,
        )?;
        if entity.sketch != *sketch {
            continue;
        }
        for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
            ctx.reserve_vec(&mut result, 1, OPERATION)?;
            result.push((point, role.copy_locus(ctx, entity.id(), OPERATION)?));
        }
    }
    Ok(result)
}

fn collect_profile_lines<'a>(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &'a [SketchEntity],
) -> Result<Vec<&'a SketchEntity>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT profile line candidates";
    let mut result = Vec::new();
    for entity in entities {
        charge_relation_identity_work(
            ctx,
            [entity.sketch.as_str(), sketch.as_str()],
            16,
            OPERATION,
        )?;
        if entity.sketch == *sketch
            && matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            ctx.reserve_vec(&mut result, 1, OPERATION)?;
            result.push(entity);
        }
    }
    Ok(result)
}

fn resolve_profile_locus_positions(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<(Point2, SketchLocus)>,
    entities: &[SketchEntity],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT profile candidate coordinates";
    let mut write = 0;
    for read in 0..loci.len() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(Point2, SketchLocus)>()),
            OPERATION,
        )?;
        if let Some(point) = profile_locus_point_charged(ctx, &loci[read].1, entities, OPERATION)? {
            loci[read].0 = point;
            loci.swap(write, read);
            write += 1;
        }
    }
    ctx.truncate_vec(loci, write, OPERATION)?;
    Ok(())
}

fn select_profile_point_line_pairs(
    ctx: &DecodeContext<'_>,
    distance: cadmpeg_ir::scalar::Length,
    loci: &[(Point2, SketchLocus)],
    lines: &[&SketchEntity],
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT profile point-line pairs";
    let mut selected: Option<(&SketchLocus, &SketchEntityId)> = None;
    for (point, locus) in loci {
        for line in lines {
            let (selected_point, selected_line) = selected.map_or(("", ""), |(point, line)| {
                (locus_entity(point).as_str(), line.as_str())
            });
            charge_relation_identity_work(
                ctx,
                [
                    locus_entity(locus).as_str(),
                    line.id().as_str(),
                    selected_point,
                    selected_line,
                ],
                256,
                OPERATION,
            )?;
            if !point_line_distance_value(*point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
            {
                continue;
            }
            let pair = (locus, line.id());
            if selected.is_some_and(|selected| selected != pair) {
                return Ok(None);
            }
            selected = Some(pair);
        }
    }
    let Some((point, line)) = selected else {
        return Ok(None);
    };
    Ok(Some((
        super::transforms::SketchLocusRole::of_locus(point).copy_locus(
            ctx,
            locus_entity(point),
            OPERATION,
        )?,
        super::transforms::copy_sketch_entity_identity(ctx, line, OPERATION)?,
    )))
}

fn unique_profile_point_line_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    point: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT profile point-line entity";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(point) = profile_locus_point_charged(ctx, point, sketch_entities, OPERATION)? else {
        return Ok(None);
    };
    let mut selected: Option<&SketchEntityId> = None;
    for line in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [
                line.sketch.as_str(),
                sketch.as_str(),
                line.id().as_str(),
                selected.map_or("", |id| id.as_str()),
            ],
            256,
            OPERATION,
        )?;
        if line.sketch != *sketch
            || !point_line_distance_value(point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
        {
            continue;
        }
        if selected.is_some_and(|selected| selected != line.id()) {
            return Ok(None);
        }
        selected = Some(line.id());
    }
    selected
        .map(|id| super::transforms::copy_sketch_entity_identity(ctx, id, OPERATION))
        .transpose()
}

fn unique_profile_line_point_locus(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT profile line-point locus";
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let Some(line) = find_profile_entity(ctx, sketch_entities, line, OPERATION)? else {
        return Ok(None);
    };
    let mut selected: Option<SketchLocus> = None;
    for (point, locus) in collect_profile_loci(ctx, sketch, sketch_entities)? {
        charge_relation_identity_work(
            ctx,
            [
                locus_entity(&locus).as_str(),
                selected
                    .as_ref()
                    .map_or("", |locus| locus_entity(locus).as_str()),
            ],
            256,
            OPERATION,
        )?;
        if !point_line_distance_value(point, line)
            .is_some_and(|measured| same_dimension_length(measured, distance.get()))
        {
            continue;
        }
        if selected.as_ref().is_some_and(|selected| selected != &locus) {
            return Ok(None);
        }
        selected = Some(locus);
    }
    Ok(selected)
}

fn unique_profile_point_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let loci = collect_profile_loci(ctx, sketch, sketch_entities)?;
    let lines = collect_profile_lines(ctx, sketch, sketch_entities)?;
    select_profile_point_line_pairs(ctx, *distance, &loci, &lines)
}

fn unique_repaired_profile_point_line_pair(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    point: &SketchLocus,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "repair SLDPRT profile point-line pair";
    let mut selected = None;
    if let Some(candidate_line) =
        unique_profile_point_line_entity(ctx, sketch, point, parameter, sketch_entities)?
    {
        selected = Some((
            super::transforms::SketchLocusRole::of_locus(point).copy_locus(
                ctx,
                locus_entity(point),
                OPERATION,
            )?,
            candidate_line,
        ));
    }
    if let Some(candidate_point) =
        unique_profile_line_point_locus(ctx, sketch, line, parameter, sketch_entities)?
    {
        let (selected_point, selected_line) =
            selected.as_ref().map_or(("", ""), |(point, line)| {
                (locus_entity(point).as_str(), line.as_str())
            });
        charge_relation_identity_work(
            ctx,
            [
                locus_entity(&candidate_point).as_str(),
                line.as_str(),
                selected_point,
                selected_line,
            ],
            16,
            OPERATION,
        )?;
        let pair = (
            candidate_point,
            super::transforms::copy_sketch_entity_identity(ctx, line, OPERATION)?,
        );
        if selected.as_ref().is_some_and(|selected| selected != &pair) {
            return Ok(None);
        }
        selected = Some(pair);
    }
    Ok(selected)
}

pub(super) fn profile_locus_point_charged(
    ctx: &DecodeContext<'_>,
    locus: &SketchLocus,
    sketch_entities: &[SketchEntity],
    operation: &'static str,
) -> Result<Option<Point2>, cadmpeg_core::CodecError> {
    let Some(entity) = find_profile_entity(ctx, sketch_entities, locus_entity(locus), operation)?
    else {
        return Ok(None);
    };
    ctx.charge_work(32, operation)?;
    Ok(sketch_entity_locus_points(entity)
        .into_iter()
        .flatten()
        .find_map(|(point, role)| role.matches(locus).then_some(point)))
}

fn canonicalize_physical_loci(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
    sketch_entities: &[SketchEntity],
    quantum: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "canonicalize SLDPRT physical sketch loci";
    if loci.len() < 2 {
        return Ok(());
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sketch_entities.len()),
        OPERATION,
    )?;
    let entity_bytes = sketch_entities
        .iter()
        .try_fold(0u64, |sum, entity| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(
                entity.id().as_str().len(),
            ))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let mut first_point = None;
    let mut coincident = true;
    let mut minimum = 0;
    for (index, locus) in loci.iter().enumerate() {
        ctx.charge_work(
            entity_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    locus_entity(locus).as_str().len(),
                ))
                .and_then(|work| {
                    work.checked_add(cadmpeg_core::decode::u64_from_index(sketch_entities.len()))
                })
                .and_then(|work| work.checked_add(64))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let Some(point) = profile_locus_point_charged(
            ctx,
            locus,
            sketch_entities,
            "resolve SLDPRT profile locus",
        )?
        else {
            return Ok(());
        };
        let point = quantize(point, quantum);
        if let Some(first) = first_point {
            coincident &= point == first;
        } else {
            first_point = Some(point);
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(locus_key(locus).0.len())
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    locus_key(&loci[minimum]).0.len(),
                ))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if locus_key(locus) < locus_key(&loci[minimum]) {
            minimum = index;
        }
    }
    if coincident {
        loci.swap(0, minimum);
        ctx.truncate_vec(loci, 1, OPERATION)?;
    }
    Ok(())
}

pub(super) fn point_line_distance_value(point: Point2, line: &SketchEntity) -> Option<f64> {
    let SketchGeometryDefinition::Line { start, end } = line.geometry.definition() else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    (length > SKETCH_POINT_TOLERANCE).then(|| {
        ((point.u - start.u) * direction[1] - (point.v - start.v) * direction[0]).abs() / length
    })
}

fn collect_relation_marker_candidates<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    markers: &HashMap<&str, &'a SketchInputEntity>,
    matches: impl Fn(&SketchInputEntity) -> bool,
) -> Result<Vec<&'a SketchInputEntity>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT relation operand markers";
    let mut candidates = Vec::new();
    for marker in markers.values().copied() {
        charge_relation_identity_work(
            ctx,
            [
                marker.feature_ref.as_deref().unwrap_or(""),
                relation.feature_ref.as_str(),
            ],
            64,
            OPERATION,
        )?;
        if marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str()) || !matches(marker)
        {
            continue;
        }
        ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
        candidates.push(marker);
    }
    Ok(candidates)
}

pub(super) fn relation_operand_marker<'a>(
    ctx: &DecodeContext<'_>,
    relation: &'a FeatureInputRelationInstance,
    index: usize,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT relation operand marker";
    let Some(operand) = relation.operands.get(index) else {
        return Ok(None);
    };
    charge_relation_identity_work(ctx, [sketch.as_str()], 4, OPERATION)?;
    if sketch.as_str().contains("sketch#compact:") && operand.kind == FeatureInputOperandKind::D6 {
        let mut coordinate_handles =
            collect_relation_marker_candidates(ctx, relation, markers_by_id, |marker| {
                marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            })?;
        ctx.sort_unstable_by_key(
            &mut coordinate_handles,
            |value| value.offset(),
            Ord::cmp,
            OPERATION,
        )?;
        return Ok(coordinate_handles
            .get(usize::from(operand.entity_index))
            .map(|marker| marker.id()));
    }
    match operand.entity_ref.as_deref() {
        Some(marker) => Ok(Some(marker)),
        None => dynamic_relation_marker(ctx, relation, index, markers_by_id),
    }
}

fn dynamic_point_operand(relation: &FeatureInputRelationInstance, index: usize) -> bool {
    matches!(
        (relation.family, index),
        (
            FeatureInputRelationFamily::PointPointDistance
                | FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance,
            0 | 1
        ) | (FeatureInputRelationFamily::PointLineDistance, 0)
    )
}

fn dynamic_relation_marker<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    index: usize,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
    if !relation_uses_dynamic_operands(relation) {
        return Ok(None);
    }
    let point_role = dynamic_point_operand(relation, index);
    let line_role = matches!(
        (relation.family, index),
        (
            FeatureInputRelationFamily::LineLineDistance | FeatureInputRelationFamily::Angle,
            0 | 1
        ) | (FeatureInputRelationFamily::PointLineDistance, 1)
    );
    if !point_role && !line_role {
        return Ok(None);
    }
    let Some(operand) = relation.operands.get(index) else {
        return Ok(None);
    };
    let address = u32::from(operand.entity_index);
    let direct_kind = |marker: &SketchInputEntity| {
        if point_role {
            matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        } else {
            matches!(
                marker.kind(),
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
        }
    };
    let address_kind = |marker: &SketchInputEntity| {
        direct_kind(marker)
            || (point_role && matches!(marker.kind(), SketchInputKind::Arc))
            || matches!(marker.kind(), SketchInputKind::Relation(_))
    };
    let by_object = collect_relation_marker_candidates(ctx, relation, markers_by_id, |marker| {
        marker.object_index() == Some(address) && address_kind(marker)
    })?;
    if !by_object.is_empty() {
        if point_role {
            if let Some(marker) =
                unique_dynamic_marker(ctx, by_object.iter().copied(), direct_kind)?
            {
                return Ok(Some(marker));
            }
            if let Some(marker) = unique_dynamic_marker(ctx, by_object.iter().copied(), |marker| {
                matches!(marker.kind(), SketchInputKind::Arc)
            })? {
                return Ok(Some(marker));
            }
        }
        return unique_dynamic_marker(ctx, by_object.into_iter(), |_| true);
    }
    let by_local = collect_relation_marker_candidates(ctx, relation, markers_by_id, |marker| {
        marker.local_id() == Some(address) && address_kind(marker)
    })?;
    if !by_local.is_empty() {
        if point_role {
            if let Some(marker) = unique_dynamic_marker(ctx, by_local.iter().copied(), direct_kind)?
            {
                return Ok(Some(marker));
            }
            if let Some(marker) = unique_dynamic_marker(ctx, by_local.iter().copied(), |marker| {
                matches!(marker.kind(), SketchInputKind::Arc)
            })? {
                return Ok(Some(marker));
            }
        }
        return unique_dynamic_marker(ctx, by_local.into_iter(), |_| true);
    }
    let mut ordinal =
        collect_relation_marker_candidates(ctx, relation, markers_by_id, direct_kind)?;
    ctx.stable_sort_by(
        &mut ordinal,
        |value| value.id(),
        Ord::cmp,
        "sort SLDPRT ordinal operand markers",
    )?;
    ctx.stable_sort_by_key(
        &mut ordinal,
        |value| value.ordinal(),
        Ord::cmp,
        "sort SLDPRT ordinal operand markers",
    )?;
    ctx.stable_sort_by_key(
        &mut ordinal,
        |value| value.offset(),
        Ord::cmp,
        "sort SLDPRT ordinal operand markers",
    )?;
    Ok(ordinal
        .get(usize::from(operand.entity_index))
        .map(|marker| marker.id()))
}

fn unique_dynamic_marker<'a>(
    ctx: &DecodeContext<'_>,
    candidates: impl Iterator<Item = &'a SketchInputEntity>,
    matches: impl Fn(&SketchInputEntity) -> bool,
) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dynamic operand marker";
    let mut selected = None;
    for candidate in candidates {
        charge_relation_identity_work(
            ctx,
            [candidate.id(), selected.unwrap_or("")],
            64,
            OPERATION,
        )?;
        if !matches(candidate) {
            continue;
        }
        if selected.is_some_and(|id| id != candidate.id()) {
            return Ok(None);
        }
        selected = Some(candidate.id());
    }
    Ok(selected)
}

fn relation_line_point_marker<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    index: usize,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Option<&'a SketchInputEntity>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT relation line point marker";
    let Some(operand) = relation.operands.get(index) else {
        return Ok(None);
    };
    if operand.kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386)
        || operand.entity_ref.is_some()
    {
        return Ok(None);
    }
    let mut selected = None;
    for marker in markers_by_id.values().copied() {
        charge_relation_identity_work(
            ctx,
            [
                marker.feature_ref.as_deref().unwrap_or(""),
                relation.feature_ref.as_str(),
            ],
            64,
            OPERATION,
        )?;
        if marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
            || marker.local_id() != Some(u32::from(operand.entity_index))
            || marker.coordinates_m.is_none()
            || !matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        {
            continue;
        }
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some(marker);
    }
    Ok(selected)
}

fn unique_profile_entity<'a>(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    entities: &'a [SketchEntity],
    operation: &'static str,
    matches: impl Fn(&SketchEntity) -> Result<bool, cadmpeg_core::CodecError>,
) -> Result<Option<&'a SketchEntity>, cadmpeg_core::CodecError> {
    let mut selected = None;
    for entity in entities {
        charge_relation_identity_work(
            ctx,
            [entity.sketch.as_str(), sketch.as_str()],
            128,
            operation,
        )?;
        if entity.sketch != *sketch || !matches(entity)? {
            continue;
        }
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some(entity);
    }
    Ok(selected)
}

fn marker_center_dimensioned_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT marker-centered dimensioned circle";

    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    let center_entity = unique_profile_entity(ctx, sketch, sketch_entities, OPERATION, |entity| {
        charge_relation_identity_work(
            ctx,
            [entity.native_ref.as_deref().unwrap_or(""), marker_id],
            8,
            OPERATION,
        )?;
        Ok(entity.native_ref.as_deref() == Some(marker_id)
            && matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            ))
    })?;
    let Some(center_entity) = center_entity else {
        return Ok(None);
    };
    let SketchGeometryDefinition::Point { position: center } = center_entity.geometry.definition()
    else {
        return Ok(None);
    };
    let center = center.get();
    let selected = unique_profile_entity(ctx, sketch, sketch_entities, OPERATION, |entity| {
        let (candidate_center, radius) = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. } => (center.get(), radius.get()),
            _ => return Ok(false),
        };
        Ok(quantize(
            candidate_center,
            EPS_RELATION_LOCI_MARKER_CENTER_DIMENSIONED_ENTITY_E8,
        ) == quantize(
            center,
            EPS_RELATION_LOCI_MARKER_CENTER_DIMENSIONED_ENTITY_E8,
        ) && same_dimension_length(radius, expected_radius))
    })?;
    selected
        .map(|entity| super::transforms::copy_sketch_entity_identity(ctx, entity.id(), OPERATION))
        .transpose()
}

fn unique_dimensioned_circle_entity(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dimensioned circle";

    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    let selected = unique_profile_entity(ctx, sketch, sketch_entities, OPERATION, |entity| {
        let radius = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { radius, .. }
            | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
            _ => return Ok(false),
        };
        Ok(same_dimension_length(radius, expected_radius))
    })?;
    selected
        .map(|entity| super::transforms::copy_sketch_entity_identity(ctx, entity.id(), OPERATION))
        .transpose()
}

pub(super) fn same_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9 * left.abs().max(right.abs()).max(1.0)
}

pub(super) fn profile_axis_for_relation(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    transforms: Option<&[MarkerTransform]>,
) -> Result<Option<ProfileAxis>, cadmpeg_core::CodecError> {
    let (native_axis, default_axis) = match relation.family {
        FeatureInputRelationFamily::PointPointHorizontalDistance => (0, ProfileAxis::U),
        FeatureInputRelationFamily::PointPointVerticalDistance => (1, ProfileAxis::V),
        _ => return Ok(None),
    };
    let Some(transforms) = transforms else {
        return Ok(Some(default_axis));
    };
    if transforms.is_empty() {
        return Ok(Some(default_axis));
    }
    let mut first = None;
    let mut disagreement = false;
    for transform in transforms {
        ctx.charge_work(64, "resolve SLDPRT relation profile axis")?;
        let Some(axis) = transform.profile_axis_for_native(native_axis) else {
            return Ok(None);
        };
        if let Some(first) = first {
            if axis != first {
                disagreement = true;
            }
        } else {
            first = Some(axis);
        }
    }
    Ok(first.filter(|_| !disagreement))
}

fn qualified_point_loci<'a>(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    loci_by_marker: &'a HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<&'a [SketchLocus]>, cadmpeg_core::CodecError> {
    for (key, loci) in loci_by_marker {
        if ctx.strip_suffix(key, ":qualified-point", "strip SLDPRT qualified point suffix")?
            == Some(marker_id)
        {
            return Ok(Some(loci.as_slice()));
        }
    }
    Ok(None)
}

pub(super) fn marker_point_locus(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker point locus";
    charge_profile_marker_lookup(ctx, marker_id, markers_by_id, loci_by_marker, OPERATION)?;
    if let Some(loci) = qualified_point_loci(ctx, marker_id, loci_by_marker)? {
        if let Some(locus) = unique_locus(ctx, loci)? {
            return Ok(Some(locus));
        }
    }
    resolved_marker_locus(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        &mut HashSet::new(),
    )
}

fn qualified_or_linked_point_locus(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT qualified or linked point locus";
    charge_profile_marker_lookup(ctx, marker_id, markers_by_id, loci_by_marker, OPERATION)?;
    let Some(marker) = markers_by_id.get(marker_id) else {
        return Ok(None);
    };
    if matches!(
        marker.kind(),
        SketchInputKind::LineOrCircle | SketchInputKind::Arc
    ) {
        let mut selected = None;
        let mut ambiguous = false;
        for link in marker.links() {
            charge_relation_identity_work(
                ctx,
                [link.entity_ref.as_str(), marker_id],
                16,
                OPERATION,
            )?;
            if link.entity_ref == marker_id {
                continue;
            }
            charge_profile_marker_lookup(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                OPERATION,
            )?;
            if matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|marker| marker.kind()),
                Some(SketchInputKind::Relation(_))
            ) {
                continue;
            }
            let Some(locus) = resolved_marker_locus(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                &mut HashSet::new(),
            )?
            else {
                continue;
            };
            let Some(entity) =
                find_profile_entity(ctx, sketch_entities, locus_entity(&locus), OPERATION)?
            else {
                continue;
            };
            if !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            ) {
                continue;
            }
            charge_relation_identity_work(
                ctx,
                [
                    locus_entity(&locus).as_str(),
                    selected
                        .as_ref()
                        .map_or("", |locus| locus_entity(locus).as_str()),
                ],
                16,
                OPERATION,
            )?;
            if selected.as_ref().is_some_and(|selected| selected != &locus) {
                ambiguous = true;
            }
            selected = Some(locus);
        }
        if selected.is_some() && !ambiguous {
            return Ok(selected);
        }
    }
    charge_profile_marker_lookup(ctx, marker_id, markers_by_id, loci_by_marker, OPERATION)?;
    if let Some(loci) = qualified_point_loci(ctx, marker_id, loci_by_marker)? {
        return unique_locus(ctx, loci);
    }
    let Some(locus) = marker_point_locus(ctx, marker_id, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    let Some(entity) = find_profile_entity(ctx, sketch_entities, locus_entity(&locus), OPERATION)?
    else {
        return Ok(None);
    };
    Ok(matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Point { .. }
    )
    .then_some(locus))
}

#[cfg(test)]
pub(super) fn qualified_point_marker_key(marker_id: &str) -> String {
    format!("{marker_id}:qualified-point")
}

fn resolved_marker_locus<'a>(
    ctx: &DecodeContext<'_>,
    marker_id: &'a str,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<&'a str>,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    let Some(locus) =
        resolved_marker_locus_inner(ctx, marker_id, markers_by_id, loci_by_marker, visited)?
    else {
        return Ok(None);
    };
    super::transforms::SketchLocusRole::of_locus(locus)
        .copy_locus(
            ctx,
            locus_entity(locus),
            "retain SLDPRT resolved marker locus",
        )
        .map(Some)
}

fn resolved_marker_locus_inner<'a, 'loci>(
    ctx: &DecodeContext<'_>,
    marker_id: &'a str,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    loci_by_marker: &'loci HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<&'a str>,
) -> Result<Option<&'loci SketchLocus>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked point locus";
    let _nesting = ctx.enter_nested(OPERATION)?;
    charge_profile_marker_lookup(ctx, marker_id, markers_by_id, loci_by_marker, OPERATION)?;
    if let Some([locus]) = loci_by_marker.get(marker_id).map(Vec::as_slice) {
        return Ok(Some(locus));
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(visited.len()),
        OPERATION,
    )?;
    let path_bytes = visited
        .iter()
        .try_fold(0u64, |bytes, identity| {
            bytes.checked_add(cadmpeg_core::decode::u64_from_index(identity.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    if !reserve_profile_locus_set_slot(
        ctx,
        visited,
        &marker_id,
        path_bytes,
        cadmpeg_core::decode::u64_from_index(marker_id.len()),
        OPERATION,
    )? {
        return Ok(None);
    }
    visited.insert(marker_id);
    let result = (|| -> Result<Option<&'loci SketchLocus>, cadmpeg_core::CodecError> {
        let Some(marker) = markers_by_id.get(marker_id) else {
            return Ok(None);
        };
        let mut selected: Option<&SketchLocus> = None;
        let mut ambiguous = false;
        for link in marker.links() {
            charge_relation_identity_work(
                ctx,
                [link.entity_ref.as_str(), marker_id],
                16,
                OPERATION,
            )?;
            if link.entity_ref == marker_id {
                continue;
            }
            charge_profile_marker_lookup(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                OPERATION,
            )?;
            if matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|marker| marker.kind()),
                Some(SketchInputKind::Relation(_))
            ) {
                continue;
            }
            let Some(locus) = resolved_marker_locus_inner(
                ctx,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                visited,
            )?
            else {
                continue;
            };
            charge_relation_identity_work(
                ctx,
                [
                    locus_entity(locus).as_str(),
                    selected.map_or("", |locus| locus_entity(locus).as_str()),
                ],
                16,
                OPERATION,
            )?;
            if selected.is_some_and(|selected| selected != locus) {
                ambiguous = true;
            }
            selected = Some(locus);
        }
        Ok(selected.filter(|_| !ambiguous))
    })();
    let locus = result?;
    ctx.charge_work(
        path_bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(marker_id.len()))
            .and_then(|bytes| bytes.checked_mul(4))
            .and_then(|work| work.checked_add(16))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        OPERATION,
    )?;
    visited.remove(marker_id);
    Ok(locus)
}

fn unique_locus(
    ctx: &DecodeContext<'_>,
    loci: &[SketchLocus],
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    let [locus] = loci else {
        return Ok(None);
    };
    super::transforms::SketchLocusRole::of_locus(locus)
        .copy_locus(
            ctx,
            locus_entity(locus),
            "retain SLDPRT singleton marker locus",
        )
        .map(Some)
}

fn single_marker_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    let entities = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::All,
    )?;
    Ok(if entities.len() == 1 {
        entities.into_iter().next()
    } else {
        None
    })
}

fn single_marker_circular_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT single circular marker entity";
    let mut identities = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::All,
    )?;
    let mut count = 0;
    for index in 0..identities.len() {
        let Some(entity) =
            find_profile_entity(ctx, sketch_entities, &identities[index], OPERATION)?
        else {
            continue;
        };
        if !matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Arc { .. }
        ) {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<SketchEntityId>()),
            OPERATION,
        )?;
        identities.swap(count, index);
        count += 1;
    }
    ctx.truncate_vec(&mut identities, count, OPERATION)?;
    sort_marker_entity_ids(ctx, &mut identities, OPERATION)?;
    Ok(if identities.len() == 1 {
        identities.into_iter().next()
    } else {
        None
    })
}

pub(super) fn single_marker_line_entity(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT single marker line";
    let mut entities = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::Lines(sketch_entities),
    )?;
    ctx.sort_unstable_by(
        &mut entities,
        |value| value,
        Ord::cmp,
        "sort SLDPRT single marker line entities",
    )?;
    ctx.dedup_vec(&mut entities, "deduplicate SLDPRT marker entities")?;
    if entities.len() == 1 {
        return Ok(entities.into_iter().next());
    }
    charge_profile_marker_lookup(ctx, marker_id, markers_by_id, loci_by_marker, OPERATION)?;
    let Some(marker) = markers_by_id.get(marker_id) else {
        return Ok(None);
    };
    let fallback = || {
        unique_line_containing_marker_point(
            ctx,
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        )
    };
    let mut links = [None, None];
    let mut other = false;
    for link in marker.links() {
        charge_relation_identity_work(
            ctx,
            [link.entity_ref.as_str(), marker_id, marker.id()],
            16,
            OPERATION,
        )?;
        if link.entity_ref == marker_id
            || (matches!(marker.kind(), SketchInputKind::Relation(_))
                && relation_link_identifies_owner(marker, link))
        {
            continue;
        }
        if links[0].is_none() {
            links[0] = Some(link);
        } else if links[1].is_none() {
            links[1] = Some(link);
        } else {
            other = true;
        }
    }
    let [Some(first_link), Some(second_link)] = links else {
        return fallback();
    };
    if other {
        return fallback();
    }
    let Some(first_locus) =
        marker_point_locus(ctx, &first_link.entity_ref, markers_by_id, loci_by_marker)?
    else {
        return fallback();
    };
    let Some(second_locus) =
        marker_point_locus(ctx, &second_link.entity_ref, markers_by_id, loci_by_marker)?
    else {
        return fallback();
    };
    let Some(first_entity) =
        find_profile_entity(ctx, sketch_entities, locus_entity(&first_locus), OPERATION)?
    else {
        return fallback();
    };
    let sketch = &first_entity.sketch;
    let Some(second_entity) =
        find_profile_entity(ctx, sketch_entities, locus_entity(&second_locus), OPERATION)?
    else {
        return fallback();
    };
    charge_relation_identity_work(
        ctx,
        [second_entity.sketch.as_str(), sketch.as_str()],
        16,
        OPERATION,
    )?;
    if second_entity.sketch != *sketch {
        return fallback();
    }
    let Some(first) = profile_locus_point_charged(ctx, &first_locus, sketch_entities, OPERATION)?
    else {
        return Ok(None);
    };
    let Some(second) = profile_locus_point_charged(ctx, &second_locus, sketch_entities, OPERATION)?
    else {
        return Ok(None);
    };
    ctx.charge_work(64, OPERATION)?;
    if same_dimension_length(first.u, second.u) && same_dimension_length(first.v, second.v) {
        return fallback();
    }
    match unique_profile_line_through_points(ctx, sketch, sketch_entities, &[first, second])? {
        Some(identity) => Ok(Some(identity)),
        None => fallback(),
    }
}

fn unique_line_containing_marker_point(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT line through marker point";
    charge_profile_marker_lookup(ctx, marker_id, markers_by_id, loci_by_marker, OPERATION)?;
    let Some(marker) = markers_by_id.get(marker_id) else {
        return Ok(None);
    };
    if !matches!(
        marker.kind(),
        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
    ) {
        return Ok(None);
    }
    let Some(locus) = marker_point_locus(ctx, marker_id, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    let Some(point) = profile_locus_point_charged(ctx, &locus, sketch_entities, OPERATION)? else {
        return Ok(None);
    };
    let Some(entity) = find_profile_entity(ctx, sketch_entities, locus_entity(&locus), OPERATION)?
    else {
        return Ok(None);
    };
    unique_profile_line_through_points(ctx, &entity.sketch, sketch_entities, &[point])
}

fn unique_profile_line_through_points(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    points: &[Point2],
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT profile line through points";
    let mut selected: Option<&SketchEntityId> = None;
    for entity in sketch_entities {
        charge_relation_identity_work(
            ctx,
            [
                entity.sketch.as_str(),
                sketch.as_str(),
                entity.id().as_str(),
                selected.map_or("", |identity| identity.as_str()),
            ],
            16,
            OPERATION,
        )?;
        if entity.sketch != *sketch
            || !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(points.len())
                .checked_mul(128)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if !points
            .iter()
            .all(|point| sketch_entity_contains_point(entity, *point))
        {
            continue;
        }
        if selected.is_some_and(|selected| selected != entity.id()) {
            return Ok(None);
        }
        selected = Some(entity.id());
    }
    selected
        .map(|identity| super::transforms::copy_sketch_entity_identity(ctx, identity, OPERATION))
        .transpose()
}

fn reserve_profile_locus_map_slot<K, V>(
    ctx: &DecodeContext<'_>,
    map: &mut HashMap<K, V>,
    key: &K,
    source_key_bytes: u64,
    query_bytes: u64,
    operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError>
where
    K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost,
{
    ctx.charge_work(
        source_key_bytes
            .checked_add(query_bytes)
            .and_then(|work| work.checked_mul(4))
            .and_then(|work| {
                work.checked_add(
                    cadmpeg_core::decode::u64_from_index(map.len())
                        .checked_add(1)?
                        .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                            K,
                            V,
                        )>(
                        )))?,
                )
            })
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let new_key = !map.contains_key(key);
    ctx.admit_hash_map_entry(map, key, operation)?;
    Ok(new_key)
}

fn collect_profile_locus_map<K, V>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = (K, V)>,
    key_len: impl Fn(&K) -> usize,
    operation: &'static str,
) -> Result<HashMap<K, V>, cadmpeg_core::CodecError>
where
    K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost,
{
    let mut result = HashMap::new();
    let mut key_bytes = 0u64;
    for (key, value) in entries {
        let bytes = cadmpeg_core::decode::u64_from_index(key_len(&key));
        if reserve_profile_locus_map_slot(ctx, &mut result, &key, key_bytes, bytes, operation)? {
            key_bytes = key_bytes
                .checked_add(bytes)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        }
        result.insert(key, value);
    }
    Ok(result)
}

fn reserve_profile_locus_set_slot<K>(
    ctx: &DecodeContext<'_>,
    set: &mut HashSet<K>,
    key: &K,
    source_key_bytes: u64,
    query_bytes: u64,
    operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError>
where
    K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost,
{
    ctx.charge_work(
        source_key_bytes
            .checked_add(query_bytes)
            .and_then(|work| work.checked_mul(4))
            .and_then(|work| {
                work.checked_add(
                    cadmpeg_core::decode::u64_from_index(set.len())
                        .checked_add(1)?
                        .checked_mul(cadmpeg_core::decode::u64_from_index(
                            std::mem::size_of::<K>(),
                        ))?,
                )
            })
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let new_key = !set.contains(key);
    if new_key {
        ctx.reserve_set(set, 1, operation)?;
    }
    Ok(new_key)
}

fn append_profile_endpoint_locus(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
    entity: &SketchEntityId,
    role: super::transforms::SketchLocusRole,
    source_entity_bytes: u64,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_work(
        source_entity_bytes
            .checked_add(cadmpeg_core::decode::u64_from_index(entity.as_str().len()))
            .and_then(|work| work.checked_mul(4))
            .and_then(|work| {
                work.checked_add(
                    cadmpeg_core::decode::u64_from_index(loci.len())
                        .checked_add(1)?
                        .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                            SketchLocus,
                        >(
                        )))?,
                )
            })
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    if !loci
        .iter()
        .any(|locus| role.matches(locus) && locus_entity(locus) == entity)
    {
        ctx.reserve_vec(loci, 1, operation)?;
        loci.push(role.copy_locus(ctx, entity, operation)?);
    }
    Ok(())
}

fn append_transformed_profile_locus(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
    role: super::transforms::SketchLocusRole,
    entity: &SketchEntityId,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(loci.len())
            .checked_add(1)
            .and_then(|work| {
                work.checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    SketchLocus,
                >()))
            })
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    ctx.reserve_vec(loci, 1, operation)?;
    loci.push(role.copy_locus(ctx, entity, operation)?);
    Ok(())
}

fn sort_profile_loci(
    ctx: &DecodeContext<'_>,
    loci: &mut Vec<SketchLocus>,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    // Equal sort keys identify equal locus values.
    ctx.sort_unstable_by(
        loci.as_mut_slice(),
        |value| value,
        |left, right| locus_key(left).cmp(&locus_key(right)),
        operation,
    )?;
    ctx.dedup_vec(loci, "deduplicate SLDPRT profile loci")?;
    Ok(())
}

fn collect_profile_locus_set<K>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = K>,
    key_len: impl Fn(&K) -> usize,
    operation: &'static str,
) -> Result<HashSet<K>, cadmpeg_core::CodecError>
where
    K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost,
{
    let mut result = HashSet::new();
    let mut key_bytes = 0u64;
    for key in entries {
        let bytes = cadmpeg_core::decode::u64_from_index(key_len(&key));
        if reserve_profile_locus_set_slot(ctx, &mut result, &key, key_bytes, bytes, operation)? {
            key_bytes = key_bytes
                .checked_add(bytes)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            result.insert(key);
        }
    }
    Ok(result)
}

pub(super) fn profile_loci_by_marker(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> Result<HashMap<String, Vec<SketchLocus>>, cadmpeg_core::CodecError> {
    const BUILD_OPERATION: &str = "build SLDPRT profile marker loci";
    const RESULT_OPERATION: &str = "build SLDPRT native marker locus results";
    const ENDPOINT_OPERATION: &str = "build SLDPRT endpoint marker locus results";
    const GROUP_OPERATION: &str = "group SLDPRT transformed profile markers";

    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1e-8;
    const INDEX_OPERATION: &str = "index SLDPRT profile marker identities";
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(features.len())
            .checked_add(cadmpeg_core::decode::u64_from_index(sketch_entities.len()))
            .and_then(|work| work.checked_mul(64))
            .ok_or_else(|| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?,
        INDEX_OPERATION,
    )?;
    for lane in lanes {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(lane.relation_instances.len())
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    lane.sketch_entities.len(),
                ))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?,
            INDEX_OPERATION,
        )?;
        for relation in &lane.relation_instances {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(relation.operands.len()),
                INDEX_OPERATION,
            )?;
        }
    }
    let qualified_point_markers = collect_profile_locus_set(
        ctx,
        lanes
            .iter()
            .flat_map(|lane| &lane.relation_instances)
            .flat_map(|relation| &relation.operands)
            .filter(|operand| {
                matches!(
                    operand.kind,
                    FeatureInputOperandKind::D6
                        | FeatureInputOperandKind::Native(
                            NativeOperandTag::TAG_80CC
                                | NativeOperandTag::TAG_8152
                                | NativeOperandTag::TAG_81B2
                                | NativeOperandTag::TAG_837B
                                | NativeOperandTag::TAG_8AB6
                                | NativeOperandTag::TAG_8DCB
                                | NativeOperandTag::TAG_929D
                                | NativeOperandTag::TAG_BC7C
                                | NativeOperandTag::TAG_BD69
                        )
                )
            })
            .filter_map(|operand| operand.entity_ref.as_deref()),
        |key: &&str| key.len(),
        "index SLDPRT qualified profile markers",
    )?;

    let sketches_by_feature = collect_profile_locus_map(
        ctx,
        features.iter().filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                },
            ) = feature.evaluation.definition()
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        }),
        |key: &&str| key.len(),
        "index SLDPRT feature profile sketches",
    )?;
    let mut profile_locus_storage = ctx.reserve_scoped(0, BUILD_OPERATION)?;
    let mut profile_loci = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let mut line_midpoints = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let geometry_by_entity = collect_profile_locus_map(
        ctx,
        sketch_entities
            .iter()
            .map(|entity| (entity.id(), &entity.geometry)),
        |key: &&SketchEntityId| key.as_str().len(),
        "index SLDPRT profile geometry identities",
    )?;
    let transforms =
        marker_transform_candidates_by_feature(ctx, features, sketches, sketch_entities, lanes)?;
    let markers_by_id = collect_profile_locus_map(
        ctx,
        lanes
            .iter()
            .flat_map(|lane| &lane.sketch_entities)
            .map(|marker| (marker.id(), marker)),
        |key: &&str| key.len(),
        "index SLDPRT profile markers",
    )?;
    let native_point_markers_with_nonpoint_carrier = collect_profile_locus_set(
        ctx,
        sketch_entities
            .iter()
            .filter(|entity| {
                !matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
            })
            .filter_map(|entity| entity.native_ref.as_deref()),
        |key: &&str| key.len(),
        "index SLDPRT nonpoint profile carriers",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sketch_entities.len()),
        BUILD_OPERATION,
    )?;
    let sketch_key_bytes = sketch_entities
        .iter()
        .try_fold(0u64, |sum, entity| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(
                entity.sketch.as_str().len(),
            ))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(BUILD_OPERATION, u64::MAX - 1, u64::MAX))?;
    for entity in sketch_entities {
        let points = sketch_entity_locus_points(entity);
        let count = points.iter().flatten().count();
        if count != 0 {
            let loci = ctx.entry_hash_map(&mut profile_loci, &entity.sketch, BUILD_OPERATION)?.or_default();
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(loci.len())
                    .checked_add(cadmpeg_core::decode::u64_from_index(count))
                    .and_then(|work| {
                        work.checked_mul(cadmpeg_core::decode::u64_from_index(
                            std::mem::size_of::<(Point2, SketchLocus)>(),
                        ))
                    })
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(BUILD_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                BUILD_OPERATION,
            )?;
            profile_locus_storage.with_storage(|| ctx.reserve_capacity(loci, count, BUILD_OPERATION))?;
            for (point, role) in points.into_iter().flatten() {
                profile_locus_storage.with_storage(|| ctx.push_vec(loci, (point, role.copy_locus(ctx, entity.id(), BUILD_OPERATION)?), BUILD_OPERATION))?;
            }
        }
        if let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() {
            let midpoints = ctx.entry_hash_map(&mut line_midpoints, &entity.sketch, BUILD_OPERATION)?.or_default();
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(midpoints.len())
                    .checked_add(1)
                    .and_then(|work| {
                        work.checked_mul(cadmpeg_core::decode::u64_from_index(
                            std::mem::size_of::<(Point2, SketchLocus)>(),
                        ))
                    })
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(BUILD_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                BUILD_OPERATION,
            )?;
            ctx.reserve_vec(midpoints, 1, BUILD_OPERATION)?;
            midpoints.push((
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
                super::transforms::SketchLocusRole::Entity.copy_locus(
                    ctx,
                    entity.id(),
                    BUILD_OPERATION,
                )?,
            ));
        }
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(markers_by_id.len())
            .checked_add(cadmpeg_core::decode::u64_from_index(
                native_point_markers_with_nonpoint_carrier.len(),
            ))
            .ok_or_else(|| ctx.refuse_codec_limit(RESULT_OPERATION, u64::MAX - 1, u64::MAX))?,
        RESULT_OPERATION,
    )?;
    let marker_key_bytes = markers_by_id
        .keys()
        .try_fold(0u64, |sum, marker| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(marker.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(RESULT_OPERATION, u64::MAX - 1, u64::MAX))?;
    let carrier_key_bytes = native_point_markers_with_nonpoint_carrier
        .iter()
        .try_fold(0u64, |sum, marker| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(marker.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(RESULT_OPERATION, u64::MAX - 1, u64::MAX))?;
    let result_key_byte_bound = cadmpeg_core::decode::u64_from_index(markers_by_id.len())
        .checked_mul(cadmpeg_core::decode::u64_from_index(
            ":qualified-point".len(),
        ))
        .and_then(|bytes| bytes.checked_add(marker_key_bytes))
        .and_then(|bytes| bytes.checked_mul(2))
        .ok_or_else(|| ctx.refuse_codec_limit(RESULT_OPERATION, u64::MAX - 1, u64::MAX))?;
    let mut result = HashMap::<String, Vec<SketchLocus>>::new();
    for entity in sketch_entities {
        ctx.charge_work(
            marker_key_bytes
                .checked_add(carrier_key_bytes)
                .and_then(|bytes| {
                    bytes.checked_add(cadmpeg_core::decode::u64_from_index(
                        entity.id().as_str().len(),
                    ))
                })
                .and_then(|bytes| {
                    bytes.checked_add(cadmpeg_core::decode::u64_from_index(
                        entity.native_ref.as_deref().map_or(0, str::len),
                    ))
                })
                .and_then(|bytes| {
                    bytes.checked_add(cadmpeg_core::decode::u64_from_index(
                        entity.geometry_ref.as_deref().map_or(0, str::len),
                    ))
                })
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| work.checked_add(64))
                .ok_or_else(|| ctx.refuse_codec_limit(RESULT_OPERATION, u64::MAX - 1, u64::MAX))?,
            RESULT_OPERATION,
        )?;
        let (marker, qualified_point) =
            if let Some(marker) = entity.native_ref.as_ref() {
                (
                    marker,
                    matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ) && native_point_markers_with_nonpoint_carrier.contains(marker.as_str()),
                )
            } else {
                let Some(reference) = entity.geometry_ref.as_ref().filter(|reference| {
                    reference.starts_with("sldprt:feature-input:sketch-entity#")
                }) else {
                    continue;
                };
                (
                    reference,
                    matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ),
                )
            };
        if !markers_by_id.contains_key(marker.as_str()) {
            continue;
        }
        let role = if entity.id().as_str().contains("sketch-entity#compact:")
            && matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            ) {
            super::transforms::SketchLocusRole::Start
        } else if markers_by_id.get(marker.as_str()).is_some_and(|marker| {
            matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        }) && matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. }
                | SketchGeometryDefinition::Arc { .. }
                | SketchGeometryDefinition::Ellipse { .. }
        ) {
            super::transforms::SketchLocusRole::Center
        } else {
            super::transforms::SketchLocusRole::Entity
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(marker.len())
                .checked_add(32)
                .and_then(|work| work.checked_mul(4))
                .ok_or_else(|| ctx.refuse_codec_limit(RESULT_OPERATION, u64::MAX - 1, u64::MAX))?,
            RESULT_OPERATION,
        )?;
        let marker = if qualified_point {
            ctx.format_retained(format_args!("{marker}:qualified-point"), RESULT_OPERATION)?
        } else {
            ctx.format_retained(format_args!("{marker}"), RESULT_OPERATION)?
        };
        reserve_profile_locus_map_slot(
            ctx,
            &mut result,
            &marker,
            result_key_byte_bound,
            cadmpeg_core::decode::u64_from_index(marker.len()),
            RESULT_OPERATION,
        )?;
        let mut loci = Vec::new();
        ctx.reserve_vec(&mut loci, 1, RESULT_OPERATION)?;
        loci.push(role.copy_locus(ctx, entity.id(), RESULT_OPERATION)?);
        result.insert(marker, loci);
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sketch_entities.len())
            .checked_add(cadmpeg_core::decode::u64_from_index(
                qualified_point_markers.len(),
            ))
            .ok_or_else(|| ctx.refuse_codec_limit(ENDPOINT_OPERATION, u64::MAX - 1, u64::MAX))?,
        ENDPOINT_OPERATION,
    )?;
    let source_entity_bytes = sketch_entities
        .iter()
        .try_fold(0u64, |sum, entity| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(
                entity.id().as_str().len(),
            ))
        })
        .and_then(|bytes| bytes.checked_mul(3))
        .ok_or_else(|| ctx.refuse_codec_limit(ENDPOINT_OPERATION, u64::MAX - 1, u64::MAX))?;
    let qualified_marker_bytes = qualified_point_markers
        .iter()
        .try_fold(0u64, |sum, marker| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(marker.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(ENDPOINT_OPERATION, u64::MAX - 1, u64::MAX))?;
    let mut endpoint_marker_keys = HashSet::new();
    for entity in sketch_entities {
        let [start, end] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        for (marker, role) in [
            (start, super::transforms::SketchLocusRole::Start),
            (end, super::transforms::SketchLocusRole::End),
        ] {
            ctx.charge_work(
                marker_key_bytes
                    .checked_add(qualified_marker_bytes)
                    .and_then(|bytes| {
                        bytes.checked_add(cadmpeg_core::decode::u64_from_index(marker.len()))
                    })
                    .and_then(|bytes| bytes.checked_mul(4))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(ENDPOINT_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                ENDPOINT_OPERATION,
            )?;
            if !markers_by_id.contains_key(marker.as_str()) {
                continue;
            }
            let qualified = qualified_point_markers.contains(marker.as_str());
            for qualified_key in [false, true] {
                if qualified_key && !qualified {
                    continue;
                }
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(marker.len())
                        .checked_add(32)
                        .and_then(|work| work.checked_mul(8))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(ENDPOINT_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    ENDPOINT_OPERATION,
                )?;
                let key = if qualified_key {
                    ctx.format_retained(
                        format_args!("{marker}:qualified-point"),
                        ENDPOINT_OPERATION,
                    )?
                } else {
                    ctx.format_retained(format_args!("{marker}"), ENDPOINT_OPERATION)?
                };
                reserve_profile_locus_set_slot(
                    ctx,
                    &mut endpoint_marker_keys,
                    &key,
                    result_key_byte_bound,
                    cadmpeg_core::decode::u64_from_index(key.len()),
                    ENDPOINT_OPERATION,
                )?;
                endpoint_marker_keys
                    .insert(ctx.format_retained(format_args!("{key}"), ENDPOINT_OPERATION)?);
                append_profile_endpoint_locus(
                    ctx,
                    ctx.entry_hash_map(&mut result, key, ENDPOINT_OPERATION)?.or_default(),
                    entity.id(),
                    role,
                    source_entity_bytes,
                    ENDPOINT_OPERATION,
                )?;
            }
        }
    }
    for marker in endpoint_marker_keys {
        if let Some(loci) = ctx.get_mut_hash_map(&mut result, &marker, "lookup SLDPRT marker endpoint loci")? {
            canonicalize_physical_loci(ctx, loci, sketch_entities, QUANTUM)?;
        }
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sketches_by_feature.len())
            .checked_add(cadmpeg_core::decode::u64_from_index(transforms.len()))
            .ok_or_else(|| ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX))?,
        GROUP_OPERATION,
    )?;
    let feature_key_bytes = sketches_by_feature
        .keys()
        .try_fold(0u64, |sum, key| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX))?;
    let transform_key_bytes = transforms
        .keys()
        .try_fold(0u64, |sum, key| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX))?;
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            ctx.charge_work(1, GROUP_OPERATION)?;
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            ctx.charge_work(
                feature_key_bytes
                    .checked_add(cadmpeg_core::decode::u64_from_index(feature.len()))
                    .and_then(|bytes| bytes.checked_mul(4))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                GROUP_OPERATION,
            )?;
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                let group = ctx.entry_hash_map(&mut markers_by_feature, feature, GROUP_OPERATION)?.or_default();
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(group.len())
                        .checked_add(1)
                        .and_then(|work| {
                            work.checked_mul(cadmpeg_core::decode::u64_from_index(
                                std::mem::size_of::<&SketchInputEntity>(),
                            ))
                        })
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    GROUP_OPERATION,
                )?;
                ctx.reserve_vec(group, 1, GROUP_OPERATION)?;
                group.push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            ctx.charge_work(
                feature_key_bytes
                    .checked_add(cadmpeg_core::decode::u64_from_index(feature.len()))
                    .and_then(|bytes| bytes.checked_mul(4))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                GROUP_OPERATION,
            )?;
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            ctx.charge_work(
                sketch_key_bytes
                    .checked_add(cadmpeg_core::decode::u64_from_index(sketch.as_str().len()))
                    .and_then(|bytes| bytes.checked_mul(4))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                GROUP_OPERATION,
            )?;
            let Some(loci) = profile_loci.get(sketch) else {
                continue;
            };
            ctx.charge_work(
                transform_key_bytes
                    .checked_add(cadmpeg_core::decode::u64_from_index(feature.len()))
                    .and_then(|bytes| bytes.checked_mul(4))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                GROUP_OPERATION,
            )?;
            let transforms = transforms
                .get(feature)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let point_key_bytes = cadmpeg_core::decode::u64_from_index(loci.len())
                .checked_mul(64)
                .ok_or_else(|| ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX))?;
            let mut loci_by_point = HashMap::<GridPoint, Vec<SketchLocus>>::new();
            for (point, locus) in loci {
                let point = quantize(*point, QUANTUM);
                reserve_profile_locus_map_slot(
                    ctx,
                    &mut loci_by_point,
                    &point,
                    point_key_bytes,
                    64,
                    GROUP_OPERATION,
                )?;
                let bucket = loci_by_point.entry(point).or_default();
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(bucket.len())
                        .checked_add(1)
                        .and_then(|work| {
                            work.checked_mul(cadmpeg_core::decode::u64_from_index(
                                std::mem::size_of::<SketchLocus>(),
                            ))
                        })
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(GROUP_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    GROUP_OPERATION,
                )?;
                ctx.reserve_vec(bucket, 1, GROUP_OPERATION)?;
                bucket.push(
                    super::transforms::SketchLocusRole::of_locus(locus).copy_locus(
                        ctx,
                        locus_entity(locus),
                        GROUP_OPERATION,
                    )?,
                );
            }
            for marker in markers {
                const TRANSFORM_OPERATION: &str = "resolve SLDPRT transformed marker loci";
                ctx.charge_work(
                    qualified_marker_bytes
                        .checked_add(cadmpeg_core::decode::u64_from_index(marker.id().len()))
                        .and_then(|bytes| bytes.checked_mul(4))
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    TRANSFORM_OPERATION,
                )?;
                let qualified_point = qualified_point_markers.contains(marker.id());
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(marker.id().len())
                        .checked_add(32)
                        .and_then(|work| work.checked_mul(4))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    TRANSFORM_OPERATION,
                )?;
                let result_key = if qualified_point {
                    ctx.format_retained(
                        format_args!("{}:qualified-point", marker.id()),
                        TRANSFORM_OPERATION,
                    )?
                } else {
                    ctx.format_retained(format_args!("{}", marker.id()), TRANSFORM_OPERATION)?
                };
                ctx.charge_work(
                    result_key_byte_bound
                        .checked_add(cadmpeg_core::decode::u64_from_index(result_key.len()))
                        .and_then(|bytes| bytes.checked_mul(4))
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    TRANSFORM_OPERATION,
                )?;
                if result.contains_key(&result_key) {
                    continue;
                }
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(sketch.as_str().len()),
                    TRANSFORM_OPERATION,
                )?;
                if qualified_point && sketch.as_str().contains("sketch#compact:") {
                    continue;
                }
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                ctx.charge_work(64, TRANSFORM_OPERATION)?;
                let primary_geometry_locus = usize::try_from(marker.offset())
                    .ok()
                    .is_some_and(|offset| marker_is_geometry_locus(&lane.native_payload, offset));
                let point = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(transforms.len())
                        .checked_mul(64)
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    TRANSFORM_OPERATION,
                )?;
                let translated_points = collect_profile_locus_set(
                    ctx,
                    transforms
                        .iter()
                        .filter_map(|transform| transform.apply(point)),
                    |_| 64,
                    TRANSFORM_OPERATION,
                )?;
                let mut marker_loci = Vec::new();
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(translated_points.len())
                        .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                            Vec<SketchLocus>,
                        >(
                        )))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "collect SLDPRT transformed marker loci",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?,
                    "collect SLDPRT transformed marker loci",
                )?;
                ctx.reserve_vec(
                    &mut marker_loci,
                    translated_points.len(),
                    "collect SLDPRT transformed marker loci",
                )?;
                for translated in translated_points {
                    ctx.charge_work(
                        point_key_bytes
                            .checked_add(64)
                            .and_then(|bytes| bytes.checked_mul(4))
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                            })?,
                        TRANSFORM_OPERATION,
                    )?;
                    let mut translated_loci = Vec::new();
                    for locus in loci_by_point
                        .get(&GridPoint::from(translated))
                        .into_iter()
                        .flatten()
                    {
                        ctx.charge_work(
                            source_entity_bytes
                                .checked_add(cadmpeg_core::decode::u64_from_index(
                                    locus_entity(locus).as_str().len(),
                                ))
                                .and_then(|bytes| bytes.checked_mul(4))
                                .and_then(|work| work.checked_add(64))
                                .ok_or_else(|| {
                                    ctx.refuse_codec_limit(
                                        TRANSFORM_OPERATION,
                                        u64::MAX - 1,
                                        u64::MAX,
                                    )
                                })?,
                            TRANSFORM_OPERATION,
                        )?;
                        if !geometry_by_entity
                            .get(locus_entity(locus))
                            .is_some_and(|geometry| marker_accepts_locus(marker.kind(), geometry))
                        {
                            continue;
                        }
                        let role = if !qualified_point
                            && matches!(
                                marker.kind(),
                                SketchInputKind::LineOrCircle | SketchInputKind::Arc
                            ) {
                            super::transforms::SketchLocusRole::Entity
                        } else {
                            super::transforms::SketchLocusRole::of_locus(locus)
                        };
                        append_transformed_profile_locus(
                            ctx,
                            &mut translated_loci,
                            role,
                            locus_entity(locus),
                            TRANSFORM_OPERATION,
                        )?;
                    }
                    if translated_loci.is_empty() && marker.kind() == SketchInputKind::LineOrCircle
                    {
                        ctx.charge_work(
                            sketch_key_bytes
                                .checked_add(cadmpeg_core::decode::u64_from_index(
                                    sketch.as_str().len(),
                                ))
                                .and_then(|bytes| bytes.checked_mul(4))
                                .and_then(|work| work.checked_add(1))
                                .ok_or_else(|| {
                                    ctx.refuse_codec_limit(
                                        TRANSFORM_OPERATION,
                                        u64::MAX - 1,
                                        u64::MAX,
                                    )
                                })?,
                            TRANSFORM_OPERATION,
                        )?;
                        for (point, locus) in line_midpoints.get(sketch).into_iter().flatten() {
                            ctx.charge_work(64, TRANSFORM_OPERATION)?;
                            if quantize(*point, QUANTUM) == translated {
                                append_transformed_profile_locus(
                                    ctx,
                                    &mut translated_loci,
                                    super::transforms::SketchLocusRole::of_locus(locus),
                                    locus_entity(locus),
                                    TRANSFORM_OPERATION,
                                )?;
                            }
                        }
                    }
                    if translated_loci.is_empty()
                        && primary_geometry_locus
                        && marker.kind() == SketchInputKind::LineOrCircle
                    {
                        ctx.charge_work(
                            sketch_key_bytes.checked_mul(2).ok_or_else(|| {
                                ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                            })?,
                            TRANSFORM_OPERATION,
                        )?;
                        for entity in sketch_entities {
                            ctx.charge_work(256, TRANSFORM_OPERATION)?;
                            if entity.sketch != **sketch {
                                continue;
                            }
                            let SketchGeometryDefinition::Line { start, end } =
                                entity.geometry.definition()
                            else {
                                continue;
                            };
                            if point_on_quantized_segment(
                                translated,
                                quantize(start.get(), QUANTUM),
                                quantize(end.get(), QUANTUM),
                            ) {
                                append_transformed_profile_locus(
                                    ctx,
                                    &mut translated_loci,
                                    super::transforms::SketchLocusRole::Entity,
                                    entity.id(),
                                    TRANSFORM_OPERATION,
                                )?;
                            }
                        }
                    }
                    sort_profile_loci(ctx, &mut translated_loci, TRANSFORM_OPERATION)?;
                    if qualified_point {
                        canonicalize_physical_loci(
                            ctx,
                            &mut translated_loci,
                            sketch_entities,
                            QUANTUM,
                        )?;
                    }
                    if !translated_loci.is_empty() {
                        marker_loci.push(translated_loci);
                    }
                }
                let Some(first) = marker_loci.first() else {
                    continue;
                };
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(marker_loci.len()),
                    TRANSFORM_OPERATION,
                )?;
                let mut comparison_work = 0u64;
                for candidate in &marker_loci {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(candidate.len()),
                        TRANSFORM_OPERATION,
                    )?;
                    for locus in candidate {
                        comparison_work = comparison_work
                            .checked_add(cadmpeg_core::decode::u64_from_index(
                                locus_key(locus).0.len(),
                            ))
                            .and_then(|work| work.checked_add(1))
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                            })?;
                    }
                }
                ctx.charge_work(
                    comparison_work.checked_mul(2).ok_or_else(|| {
                        ctx.refuse_codec_limit(TRANSFORM_OPERATION, u64::MAX - 1, u64::MAX)
                    })?,
                    TRANSFORM_OPERATION,
                )?;
                if marker_loci.iter().all(|candidate| candidate == first) {
                    if let Some(loci) = marker_loci.into_iter().next() {
                        reserve_profile_locus_map_slot(
                            ctx,
                            &mut result,
                            &result_key,
                            result_key_byte_bound,
                            cadmpeg_core::decode::u64_from_index(result_key.len()),
                            TRANSFORM_OPERATION,
                        )?;
                        result.insert(result_key, loci);
                    }
                }
            }
        }
    }

    for marker in markers_by_id.values().copied() {
        const PAIR_OPERATION: &str = "resolve SLDPRT endpoint marker profile entity";
        ctx.charge_work(1, PAIR_OPERATION)?;
        if marker.kind() != SketchInputKind::LineOrCircle {
            continue;
        }
        ctx.charge_work(
            result_key_byte_bound
                .checked_add(cadmpeg_core::decode::u64_from_index(marker.id().len()))
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(PAIR_OPERATION, u64::MAX - 1, u64::MAX))?,
            PAIR_OPERATION,
        )?;
        if result.contains_key(marker.id()) {
            continue;
        }
        let endpoints = line_endpoint_markers(ctx, marker, &markers_by_id)?;
        let (Some(feature), [first, second]) =
            (marker.feature_ref.as_deref(), endpoints.as_slice())
        else {
            continue;
        };
        ctx.charge_work(
            feature_key_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(feature.len()))
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(PAIR_OPERATION, u64::MAX - 1, u64::MAX))?,
            PAIR_OPERATION,
        )?;
        let (Some(sketch_id), Some(first), Some(second)) = (
            sketches_by_feature.get(feature),
            first.coordinates_m,
            second.coordinates_m,
        ) else {
            continue;
        };
        ctx.charge_work(128, PAIR_OPERATION)?;
        let first_native = quantize(
            Point2::new(first[0] * NATIVE_TO_IR, first[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        let second_native = quantize(
            Point2::new(second[0] * NATIVE_TO_IR, second[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        ctx.charge_work(
            transform_key_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(feature.len()))
                .and_then(|bytes| bytes.checked_mul(4))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(PAIR_OPERATION, u64::MAX - 1, u64::MAX))?,
            PAIR_OPERATION,
        )?;
        let candidates = transforms
            .get(feature)
            .map(Vec::as_slice)
            .unwrap_or_default();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(candidates.len())
                .checked_mul(128)
                .ok_or_else(|| ctx.refuse_codec_limit(PAIR_OPERATION, u64::MAX - 1, u64::MAX))?,
            PAIR_OPERATION,
        )?;
        let endpoint_pairs = collect_profile_locus_set(
            ctx,
            candidates.iter().filter_map(|transform| {
                Some((
                    transform.apply(first_native)?,
                    transform.apply(second_native)?,
                ))
            }),
            |_| 128,
            PAIR_OPERATION,
        )?;
        if endpoint_pairs.is_empty() {
            continue;
        }
        let mut selected: Option<&SketchEntityId> = None;
        let mut complete = true;
        for (start, end) in endpoint_pairs {
            ctx.charge_work(
                sketch_key_bytes.checked_mul(4).ok_or_else(|| {
                    ctx.refuse_codec_limit(PAIR_OPERATION, u64::MAX - 1, u64::MAX)
                })?,
                PAIR_OPERATION,
            )?;
            let mut candidate = None;
            for entity in sketch_entities {
                ctx.charge_work(256, PAIR_OPERATION)?;
                if entity.sketch != **sketch_id {
                    continue;
                }
                let SketchGeometryDefinition::Line {
                    start: candidate_start,
                    end: candidate_end,
                } = entity.geometry.definition()
                else {
                    continue;
                };
                let candidate_start = quantize(candidate_start.get(), QUANTUM);
                let candidate_end = quantize(candidate_end.get(), QUANTUM);
                if (candidate_start == start && candidate_end == end)
                    || (candidate_start == end && candidate_end == start)
                {
                    if candidate.is_some() {
                        complete = false;
                        break;
                    }
                    candidate = Some(entity.id());
                }
            }
            let Some(candidate) = candidate else {
                complete = false;
                break;
            };
            if !complete {
                break;
            }
            if let Some(previous) = selected {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(previous.as_str().len())
                        .checked_add(cadmpeg_core::decode::u64_from_index(
                            candidate.as_str().len(),
                        ))
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(PAIR_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    PAIR_OPERATION,
                )?;
                if previous != candidate {
                    complete = false;
                    break;
                }
            } else {
                selected = Some(candidate);
            }
        }
        if complete {
            if let Some(entity) = selected {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(marker.id().len())
                        .checked_mul(4)
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(PAIR_OPERATION, u64::MAX - 1, u64::MAX)
                        })?,
                    PAIR_OPERATION,
                )?;
                let key = ctx.format_retained(format_args!("{}", marker.id()), PAIR_OPERATION)?;
                reserve_profile_locus_map_slot(
                    ctx,
                    &mut result,
                    &key,
                    result_key_byte_bound,
                    cadmpeg_core::decode::u64_from_index(key.len()),
                    PAIR_OPERATION,
                )?;
                let mut loci = Vec::new();
                ctx.reserve_vec(&mut loci, 1, PAIR_OPERATION)?;
                loci.push(super::transforms::SketchLocusRole::Entity.copy_locus(
                    ctx,
                    entity,
                    PAIR_OPERATION,
                )?);
                result.insert(key, loci);
            }
        }
    }
    let entities_by_id = collect_profile_locus_map(
        ctx,
        sketch_entities.iter().map(|entity| (entity.id(), entity)),
        |key: &&SketchEntityId| key.as_str().len(),
        "index SLDPRT linked profile entities",
    )?;
    loop {
        const OPERATION: &str = "collect SLDPRT linked endpoint loci";
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(result.len()),
            OPERATION,
        )?;
        let mut key_bytes = result
            .keys()
            .try_fold(0u64, |sum, key| {
                sum.checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
            })
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        let mut additions = Vec::new();
        for marker in markers_by_id.values() {
            ctx.charge_work(
                key_bytes
                    .checked_add(cadmpeg_core::decode::u64_from_index(marker.id().len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            if marker.coordinates_m.is_some()
                || !matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                || result.contains_key(marker.id())
            {
                continue;
            }
            let Some(locus) = unique_linked_endpoint_locus(
                ctx,
                marker,
                &markers_by_id,
                &result,
                &entities_by_id,
                QUANTUM,
            )?
            else {
                continue;
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(marker.id().len())
                    .checked_mul(2)
                    .and_then(|work| {
                        work.checked_add(
                            cadmpeg_core::decode::u64_from_index(additions.len()).checked_mul(
                                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                                    String,
                                    Vec<SketchLocus>,
                                )>(
                                )),
                            )?,
                        )
                    })
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            ctx.reserve_vec(&mut additions, 1, OPERATION)?;
            let key = ctx.format_retained(format_args!("{}", marker.id()), OPERATION)?;
            let mut loci = Vec::new();
            ctx.reserve_vec(&mut loci, 1, OPERATION)?;
            loci.push(locus);
            additions.push((key, loci));
        }
        if additions.is_empty() {
            break;
        }
        ctx.charge_work(
            key_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(result.len()))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        ctx.reserve_map(&mut result, additions.len(), OPERATION)?;
        for (key, loci) in additions {
            ctx.charge_work(
                key_bytes
                    .checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            key_bytes = key_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(key.len()))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            result.insert(key, loci);
        }
    }
    Ok(result)
}

pub(super) fn unique_linked_endpoint_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities_by_id: &HashMap<&SketchEntityId, &SketchEntity>,
    quantum: f64,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT linked endpoint locus";
    if marker.links().len() < 2 {
        return Ok(None);
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(entities_by_id.len()),
        OPERATION,
    )?;
    let entity_key_bytes = entities_by_id
        .keys()
        .try_fold(0u64, |sum, id| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(id.as_str().len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let mut group_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut groups = Vec::<
        HashMap<GridPoint, Vec<(&SketchEntityId, super::transforms::SketchLocusRole)>>,
    >::new();
    group_storage.with_storage(|| ctx.reserve_capacity(&mut groups, marker.links().len(), OPERATION))?;
    let mut sketch: Option<&cadmpeg_ir::sketches::SketchId> = None;
    for link in marker.links() {
        let entities = marker_entities(
            ctx,
            &link.entity_ref,
            markers_by_id,
            loci_by_marker,
            MarkerEntityFilter::All,
        )?;
        if entities.is_empty() {
            return Ok(None);
        }
        let mut endpoints =
            HashMap::<GridPoint, Vec<(&SketchEntityId, super::transforms::SketchLocusRole)>>::new();
        for entity_id in entities {
            ctx.charge_work(
                entity_key_bytes
                    .checked_add(cadmpeg_core::decode::u64_from_index(
                        entity_id.as_str().len(),
                    ))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            let Some(entity) = entities_by_id.get(&entity_id) else {
                return Ok(None);
            };
            if let Some(previous) = sketch {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(entity.sketch.as_str().len())
                        .checked_add(cadmpeg_core::decode::u64_from_index(
                            previous.as_str().len(),
                        ))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                if previous != &entity.sketch {
                    return Ok(None);
                }
            } else {
                sketch = Some(&entity.sketch);
            }
            for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
                if matches!(role, super::transforms::SketchLocusRole::Center) {
                    continue;
                }
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(endpoints.len())
                        .checked_add(1)
                        .and_then(|work| work.checked_mul(64))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                let point = quantize(point, quantum);
                ctx.admit_hash_map_entry(&mut endpoints, &point, OPERATION)?;
                let loci = endpoints.entry(point).or_default();
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(loci.len())
                        .checked_add(1)
                        .and_then(|work| {
                            work.checked_mul(cadmpeg_core::decode::u64_from_index(
                                std::mem::size_of::<(
                                    &SketchEntityId,
                                    super::transforms::SketchLocusRole,
                                )>(),
                            ))
                        })
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                ctx.reserve_vec(loci, 1, OPERATION)?;
                loci.push((entity.id(), role));
            }
        }
        if endpoints.is_empty() {
            return Ok(None);
        }
        group_storage.with_storage(|| ctx.push_vec(&mut groups, endpoints, OPERATION))?;
    }
    let Some(first) = groups.first() else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(first.len())
            .checked_mul(cadmpeg_core::decode::u64_from_index(groups.len()))
            .and_then(|work| work.checked_mul(64))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        OPERATION,
    )?;
    let mut shared = first
        .keys()
        .filter(|point| groups[1..].iter().all(|group| group.contains_key(point)));
    let Some(point) = shared.next() else {
        return Ok(None);
    };
    if shared.next().is_some() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchEntityId, super::transforms::SketchLocusRole)> = None;
    for group in &groups {
        ctx.charge_work(64, OPERATION)?;
        for locus in group.get(point).into_iter().flatten() {
            let candidate_key = (locus.0.as_str(), locus.1);
            let other_bytes = selected.map_or(0, |other| other.0.as_str().len());
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(candidate_key.0.len())
                    .checked_add(cadmpeg_core::decode::u64_from_index(other_bytes))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            if selected.is_none_or(|other| candidate_key < (other.0.as_str(), other.1)) {
                selected = Some(*locus);
            }
        }
    }
    let Some((entity, role)) = selected else {
        return Ok(None);
    };
    Ok(Some(role.copy_locus(ctx, entity, OPERATION)?))
}

fn point_on_quantized_segment(point: (i64, i64), start: GridPoint, end: GridPoint) -> bool {
    let (Some(start), Some(end)) = (start.cells(), end.cells()) else {
        return false;
    };
    let ab = (
        i128::from(end.0) - i128::from(start.0),
        i128::from(end.1) - i128::from(start.1),
    );
    let ap = (
        i128::from(point.0) - i128::from(start.0),
        i128::from(point.1) - i128::from(start.1),
    );
    let Some(cross) =
        ab.0.checked_mul(ap.1)
            .and_then(|left| left.checked_sub(ab.1.checked_mul(ap.0)?))
    else {
        return false;
    };
    let Some(projection) =
        ab.0.checked_mul(ap.0)
            .and_then(|left| left.checked_add(ab.1.checked_mul(ap.1)?))
    else {
        return false;
    };
    let Some(squared_length) =
        ab.0.checked_mul(ab.0)
            .and_then(|left| left.checked_add(ab.1.checked_mul(ab.1)?))
    else {
        return false;
    };
    squared_length != 0 && cross == 0 && (0..=squared_length).contains(&projection)
}

pub(super) fn marker_transform_candidates_by_feature<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &'a [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> Result<HashMap<&'a str, Vec<MarkerTransform>>, cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = RELATION_GEOMETRY_QUANTUM_MM;
    const OPERATION: &str = "index SLDPRT marker transform candidates";

    let mut sketches_by_feature = HashMap::<&str, &SketchId>::new();
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(
            u64::try_from(native_ref.len())
                .ok()
                .and_then(|len| len.checked_add(1))
                .and_then(|work| work.checked_mul(4))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if !sketches_by_feature.contains_key(native_ref) {
            ctx.charge_collection_items(1, OPERATION)?;
            if sketches_by_feature.len() == sketches_by_feature.capacity() {
                for key in sketches_by_feature.keys() {
                    ctx.charge_work(
                        u64::try_from(key.len())
                            .ok()
                            .and_then(|len| len.checked_add(1))
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                            })?,
                        OPERATION,
                    )?;
                }
            }
            sketches_by_feature
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        sketches_by_feature.insert(native_ref, sketch);
    }
    let mut result = HashMap::<&str, Vec<MarkerTransform>>::new();
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            ctx.charge_work(1, OPERATION)?;
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            ctx.charge_work(
                u64::try_from(feature.len())
                    .ok()
                    .and_then(|len| len.checked_add(1))
                    .and_then(|work| work.checked_mul(4))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                let markers = ctx.entry_hash_map(&mut markers_by_feature, feature, OPERATION)?.or_default();
                ctx.reserve_vec(markers, 1, OPERATION)?;
                markers.push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            ctx.charge_work(
                u64::try_from(feature.len())
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            let Some((canonical_feature, sketch)) = sketches_by_feature.get_key_value(feature)
            else {
                continue;
            };
            for entity in sketch_entities {
                let work = entity
                    .sketch
                    .as_str()
                    .len()
                    .checked_add(sketch.as_str().len())
                    .and_then(|len| len.checked_add(1))
                    .and_then(|len| u64::try_from(len).ok())
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            if !sketch_entities
                .iter()
                .any(|entity| entity.sketch == **sketch)
            {
                continue;
            }
            let mut directly_bound = HashMap::<GridPoint, HashSet<GridPoint>>::new();
            for marker in &markers {
                ctx.charge_work(64, OPERATION)?;
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                for entity in sketch_entities {
                    let work = entity
                        .sketch
                        .as_str()
                        .len()
                        .checked_add(sketch.as_str().len())
                        .and_then(|len| {
                            len.checked_add(entity.native_ref.as_deref().map_or(0, str::len))
                        })
                        .and_then(|len| len.checked_add(marker.id().len()))
                        .and_then(|len| len.checked_add(64))
                        .and_then(|len| u64::try_from(len).ok())
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                    ctx.charge_work(work, OPERATION)?;
                    if entity.sketch != **sketch
                        || entity.native_ref.as_deref() != Some(marker.id())
                    {
                        continue;
                    }
                    let anchors = match *entity.geometry.definition() {
                        SketchGeometryDefinition::Point { position } => vec![position.get()],
                        _ => marker_geometry_anchors(marker.kind(), &entity.geometry),
                    };
                    for anchor in anchors {
                        insert_compatible_locus(
                            ctx,
                            &mut directly_bound,
                            native,
                            quantize(anchor, QUANTUM),
                        )?;
                    }
                }
            }
            let compatible = |primary_only: bool| -> Result<
                HashMap<GridPoint, HashSet<GridPoint>>,
                cadmpeg_core::CodecError,
            > {
                let mut points = HashMap::<GridPoint, HashSet<GridPoint>>::new();
                for marker in &markers {
                    ctx.charge_work(64, OPERATION)?;
                    if !matches!(
                        marker.kind(),
                        SketchInputKind::Point
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                            | SketchInputKind::ConstrainedPoint
                    ) {
                        continue;
                    }
                    let Some([u, v]) = marker
                        .coordinates_m
                        .map(cadmpeg_ir::units::FiniteVector::get)
                    else {
                        continue;
                    };
                    if primary_only
                        && index_from_u64(marker.offset()).is_none_or(|offset| {
                            !marker_is_geometry_locus(&lane.native_payload, offset)
                        })
                    {
                        continue;
                    }
                    let marker_point =
                        quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    for entity in sketch_entities {
                        let work = entity
                            .sketch
                            .as_str()
                            .len()
                            .checked_add(sketch.as_str().len())
                            .and_then(|len| len.checked_add(64))
                            .and_then(|len| u64::try_from(len).ok())
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                            })?;
                        ctx.charge_work(work, OPERATION)?;
                        if entity.sketch != **sketch {
                            continue;
                        }
                        if primary_only {
                            for (point, _) in
                                sketch_entity_locus_points(entity).into_iter().flatten()
                            {
                                if marker_accepts_locus(marker.kind(), &entity.geometry) {
                                    insert_compatible_locus(
                                        ctx,
                                        &mut points,
                                        marker_point,
                                        quantize(point, QUANTUM),
                                    )?;
                                }
                            }
                        } else {
                            for point in marker_geometry_anchors(marker.kind(), &entity.geometry) {
                                insert_compatible_locus(
                                    ctx,
                                    &mut points,
                                    marker_point,
                                    quantize(point, QUANTUM),
                                )?;
                            }
                        }
                    }
                }
                Ok(points)
            };
            let direct = compatible_marker_transform_candidates(ctx, &directly_bound)?;
            let primary = compatible_marker_transform_candidates(ctx, &compatible(true)?)?;
            let fallback = compatible_marker_transform_candidates(ctx, &compatible(false)?)?;
            let candidates = if direct.len() == 1 {
                direct
            } else if primary.len() == 1 || fallback.is_empty() {
                primary
            } else {
                fallback
            };
            for candidate in sketches {
                let work = candidate
                    .id
                    .as_str()
                    .len()
                    .checked_add(sketch.as_str().len())
                    .and_then(|len| len.checked_add(1))
                    .and_then(|len| u64::try_from(len).ok())
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            let candidates =
                if let Some(sketch) = sketches.iter().find(|candidate| candidate.id == **sketch) {
                    marker_transforms_with_frame_fallback(candidates, sketch, QUANTUM)
                } else {
                    candidates
                };
            if !candidates.is_empty() {
                ctx.charge_work(
                    u64::try_from(feature.len())
                        .ok()
                        .and_then(|len| len.checked_add(1))
                        .and_then(|work| work.checked_mul(4))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                if !result.contains_key(canonical_feature) {
                    ctx.charge_collection_items(1, OPERATION)?;
                    if result.len() == result.capacity() {
                        for key in result.keys() {
                            ctx.charge_work(
                                u64::try_from(key.len())
                                    .ok()
                                    .and_then(|len| len.checked_add(1))
                                    .ok_or_else(|| {
                                        ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                                    })?,
                                OPERATION,
                            )?;
                        }
                    }
                    result
                        .try_reserve(1)
                        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                }
                result.insert(*canonical_feature, candidates);
            }
        }
    }
    Ok(result)
}

fn insert_compatible_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    points: &mut HashMap<GridPoint, HashSet<GridPoint>>,
    marker: GridPoint,
    locus: GridPoint,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "index SLDPRT compatible marker loci";
    ctx.charge_work(64, OPERATION)?;
    ctx.admit_hash_map_entry(points, &marker, OPERATION)?;
    let loci = points.entry(marker).or_default();
    ctx.insert_hash_set(loci, locus, OPERATION)?;
    Ok(())
}

fn marker_geometry_anchors(kind: SketchInputKind, geometry: &SketchGeometry) -> Vec<Point2> {
    match (kind, geometry.definition()) {
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometryDefinition::Point { position },
        ) => vec![position.get()],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometryDefinition::Line { start, end },
        ) => vec![start.get(), end.get()],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometryDefinition::Circle { center, .. }
            | SketchGeometryDefinition::Arc { center, .. }
            | SketchGeometryDefinition::Ellipse { center, .. },
        ) => vec![center.get()],
        (SketchInputKind::LineOrCircle, SketchGeometryDefinition::Line { start, end }) => {
            vec![
                start.get(),
                end.get(),
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
            ]
        }
        (
            SketchInputKind::LineOrCircle,
            SketchGeometryDefinition::Circle { center, .. }
            | SketchGeometryDefinition::Ellipse { center, .. },
        )
        | (SketchInputKind::Arc, SketchGeometryDefinition::Arc { center, .. }) => {
            vec![center.get()]
        }
        _ => Vec::new(),
    }
}

pub(super) fn marker_accepts_locus(kind: SketchInputKind, geometry: &SketchGeometry) -> bool {
    match kind {
        SketchInputKind::Arc => {
            matches!(geometry.definition(), SketchGeometryDefinition::Arc { .. })
        }
        SketchInputKind::LineOrCircle => matches!(
            geometry.definition(),
            SketchGeometryDefinition::Line { .. }
                | SketchGeometryDefinition::Circle { .. }
                | SketchGeometryDefinition::Ellipse { .. }
        ),
        SketchInputKind::Point
        | SketchInputKind::ConstrainedPoint
        | SketchInputKind::Relation(_)
        | SketchInputKind::Native(_)
        | SketchInputKind::NativeHandle(_) => true,
    }
}

#[cfg(test)]
mod relation_loci_tests;
