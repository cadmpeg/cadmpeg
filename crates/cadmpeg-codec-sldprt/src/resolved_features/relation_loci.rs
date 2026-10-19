//! Relation definition and profile locus resolution.

use super::grid::{quantize, GridPoint};
use super::markers::marker_is_geometry_locus;
use super::relation_geometry::{
    relation_operand_geometry_ref_matches, relation_uses_solver_line_operand,
    solver_line_geometry_ref_matches,
};
use super::relation_records::{relation_uses_dynamic_operands, relation_uses_solver_points};
use super::transforms::{
    compatible_marker_transform_candidates, locus_entity, locus_key, marker_entities,
    marker_transforms_with_frame_fallback, sketch_entity_locus_points, sort_marker_entity_ids,
    MarkerEntityFilter, MarkerTransform, ProfileAxis,
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
    let mut has_link = false;
    for link in marker.links() {
        if relation_link_identifies_owner(marker, link) {
            continue;
        }
        has_link = true;
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
    let Some(entity) = find_profile_entity(sketch_entities, identity) else {
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
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<(SketchLocus, SketchEntityId)> {
    let mut links = marker
        .links()
        .iter()
        .filter(|link| !relation_link_identifies_owner(marker, link));
    let (Some(first), Some(second), None) = (links.next(), links.next(), links.next()) else {
        return None;
    };
    let mut point = None;
    let mut entity = None;
    for link in [first, second] {
        let linked_marker = markers_by_id.get(link.entity_ref.as_str())?;
        let loci = loci_by_marker.get(&link.entity_ref)?;
        let locus = unique_locus(loci)?;
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
            _ => return None,
        }
    }
    point.zip(entity)
}

pub(super) fn relation_operand_loci(
    ctx: &DecodeContext<'_>,
    relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<Vec<SketchLocus>>, cadmpeg_core::CodecError> {
    let owners = relation_owner_markers(relation, markers_by_id);
    let mut loci = Vec::new();
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
        if loci.contains(&locus) {
            continue;
        }
        loci.push(locus);
    }
    Ok((!loci.is_empty()).then_some(loci))
}

pub(super) fn linked_single_entities(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<Vec<SketchEntityId>>, cadmpeg_core::CodecError> {
    let mut result: Vec<SketchEntityId> = Vec::new();
    for link in marker.links() {
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
        if result.contains(entity) {
            continue;
        }
        if let Some(entity) = entities.into_iter().next() {
            result.push(entity);
        }
    }
    Ok(Some(result))
}

pub(super) fn relation_constraint_is_inactive(
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    definition: &SketchConstraintDefinitionInput,
    sketch_entities: &[SketchEntity],
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(parameter) = parameter else {
        return Ok(false);
    };
    let entity = |id: &SketchEntityId| find_profile_entity(sketch_entities, id);
    Ok(match definition {
        SketchConstraintDefinitionInput::DistanceLoci { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return Ok(true);
            };
            let measured = if let (Some(first), Some(second)) = (
                profile_locus_point(first, sketch_entities),
                profile_locus_point(second, sketch_entities),
            ) {
                Some((second.u - first.u).hypot(second.v - first.v))
            } else {
                let point_line = |point: &SketchLocus,
                                  line: &SketchLocus|
                 -> Result<Option<f64>, cadmpeg_core::CodecError> {
                    let Some(point) = profile_locus_point(point, sketch_entities) else {
                        return Ok(None);
                    };
                    let SketchLocus::Entity(line) = line else {
                        return Ok(None);
                    };
                    let Some(line) = entity(line) else {
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
                profile_locus_point(first, sketch_entities),
                profile_locus_point(second, sketch_entities),
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
                profile_locus_point(first, sketch_entities),
                profile_locus_point(second, sketch_entities),
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
                match entity(first) {
                    Some(entity) => entity,
                    None => return Ok(false),
                },
                match entity(second) {
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
                match entity(first) {
                    Some(entity) => entity,
                    None => return Ok(false),
                },
                match entity(second) {
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
            let Some(entity) = entity(id) else {
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
                let Some(entity) = entity(id) else {
                    return Ok(false);
                };
                let radius = match *entity.geometry.definition() {
                    SketchGeometryDefinition::Circle { radius, .. }
                    | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
                    _ => return Ok(false),
                };
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
    let parameter_id = parameter.id.clone();
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
    let marker = |index: usize| relation_operand_marker(relation, index, sketch, markers_by_id);
    let dynamic = relation_uses_dynamic_operands(relation);
    let point = |index: usize| -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
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
            return Ok(Some(
                super::transforms::SketchLocusRole::Entity.copy_locus(entity.id()),
            ));
        }
        let Some(marker) = marker(index) else {
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
        if let Some(entity) = sketch_entities.iter().find(|entity| {
            entity.native_ref.as_deref() == Some(marker)
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
        }) {
            return Ok(Some(
                super::transforms::SketchLocusRole::Entity.copy_locus(entity.id()),
            ));
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
        match solver_line_entity(relation, index, sketch, sketch_entities) {
            Some(entity) => Ok(Some(entity)),
            None => match marker(index) {
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
            if let Some(marker) = relation_operand_marker(relation, index, sketch, markers_by_id) {
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
                unique_profile_distance_loci_pair(sketch, parameter, sketch_entities)
            }
            PointPointHorizontalDistance | PointPointVerticalDistance => {
                unique_profile_axis_distance_pair(
                    sketch,
                    parameter,
                    sketch_entities,
                    resolved_or_none!(profile_axis),
                )
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
                None => match relation_operand_marker(relation, 0, sketch, markers_by_id) {
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
                *expected,
                &point_candidates,
                &line_candidates,
                sketch_entities,
            )
        } else {
            None
        };
        match selected {
            Some(pair) => Some(pair),
            None => unique_dynamic_roster_point_line_pair(
                relation,
                sketch,
                parameter,
                point(0)?,
                curve(1)?,
                sketch_entities,
            ),
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
                unique_profile_matched_entity( sketch, known, sketch_entities, |known, candidate| {
                    line_line_distance(known, candidate).is_some_and(|measured| {
                        same_relation_dimension_length(measured, expected.get())
                    })
                })
            };
            let first = curve(0)?;
            let second = curve(1)?;
            Ok(match (first, second) {
                (Some(first), None) => { let Some(partner) = unique_partner(&first) else { return Ok(None); }; Some((first, partner)) },
                (None, Some(second)) => unique_partner(&second).map(|partner| (partner, second)),
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
    let dynamic_roster_line_distance_pair =
        if dynamic && relation.family == LineLineDistance && dynamic_line_distance_pair.is_none() {
            unique_dynamic_roster_line_distance_pair(relation, sketch, parameter, sketch_entities)
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
                    let Some(partner) = unique_dynamic_profile_line_angle_entity( sketch, &first, parameter, sketch_entities) else { return Ok(None); };
                    Some((first, partner))
                },
                (None, Some(second)) => Some((
                    match unique_dynamic_profile_line_angle_entity( sketch, &second, parameter, sketch_entities) {
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
        unique_dynamic_roster_line_angle_pair(sketch, parameter, sketch_entities)
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
                        relation,
                        (0, 1),
                        sketch,
                        parameter,
                        sketch_entities,
                        markers_by_id,
                    ) {
                        Some(pair) => pair,
                        None => {
                            let partner = resolved_or_none!(unique_profile_distance_locus(
                                sketch,
                                &known,
                                parameter,
                                sketch_entities
                            ));
                            (known, partner)
                        }
                    },
                    (None, Some(known)) => match doubled_profile_distance_loci(
                        relation,
                        (1, 0),
                        sketch,
                        parameter,
                        sketch_entities,
                        markers_by_id,
                    ) {
                        Some(pair) => pair,
                        None => {
                            let partner = resolved_or_none!(unique_profile_distance_locus(
                                sketch,
                                &known,
                                parameter,
                                sketch_entities
                            ));
                            (partner, known)
                        }
                    },
                    (None, None) => {
                        resolved_or_none!(unique_profile_distance_loci_pair(
                            sketch,
                            parameter,
                            sketch_entities
                        ))
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
                let first_point = resolved_or_none!(profile_locus_point(&first, sketch_entities));
                let second_point = resolved_or_none!(profile_locus_point(&second, sketch_entities));
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
                            sketch,
                            &first,
                            &second,
                            parameter,
                            sketch_entities,
                        ));
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
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                            axis,
                        ));
                        (known, partner)
                    }
                    (None, Some(known)) => (
                        resolved_or_none!(unique_profile_axis_distance_locus(
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                            axis,
                        )),
                        known,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_axis_distance_pair(
                            sketch,
                            parameter,
                            sketch_entities,
                            axis
                        ))
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
                let first_point = resolved_or_none!(profile_locus_point(&first, sketch_entities));
                let second_point = resolved_or_none!(profile_locus_point(&second, sketch_entities));
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
                                sketch,
                                &first,
                                &second,
                                parameter,
                                sketch_entities,
                                axis,
                            ));
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
                            sketch,
                            &point,
                            parameter,
                            sketch_entities,
                        ));
                        (point, partner)
                    }
                    (None, Some(line)) => (
                        resolved_or_none!(unique_profile_line_point_locus(
                            sketch,
                            &line,
                            parameter,
                            sketch_entities
                        )),
                        line,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_point_line_pair(
                            sketch,
                            parameter,
                            sketch_entities
                        ))
                    }
                },
            };
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                resolved_or_none!(parameter.value.as_ref())
            else {
                return Ok(None);
            };
            let point_position = resolved_or_none!(profile_locus_point(&point, sketch_entities));
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
                        sketch,
                        &point,
                        &line,
                        parameter,
                        sketch_entities,
                    ));
                }
            }
            Some(SketchConstraintDefinitionInput::DistanceLoci {
                first: point,
                second: SketchLocus::Entity(line),
                parameter: parameter_id,
            })
        }
        LineLineDistance => {
            let operand_marker = |index: usize| -> Option<&str> {
                match relation_operand_marker(relation, index, sketch, markers_by_id) {
                    Some(marker) => Some(marker),
                    None => relation_line_point_marker(relation, index, markers_by_id)
                        .map(super::super::records::SketchInputEntity::id),
                }
            };
            let first = match curve(0)? {
                Some(entity) => Some(entity),
                None => match operand_marker(0) {
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
                None => match operand_marker(1) {
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
            let (mut first, mut second) = match dynamic_line_distance_pair
                .or(dynamic_roster_line_distance_pair)
            {
                Some(pair) => pair,
                None => match (first, second) {
                    (Some(first), Some(second)) => (first, second),
                    (Some(known), None) => {
                        let partner = if let Some(marker) =
                            relation_line_point_marker(relation, 1, markers_by_id)
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
                                sketch,
                                &known,
                                parameter,
                                sketch_entities,
                            ))
                        };
                        (known, partner)
                    }
                    (None, Some(known)) => (
                        if let Some(marker) = relation_line_point_marker(relation, 0, markers_by_id)
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
                                sketch,
                                &known,
                                parameter,
                                sketch_entities,
                            ))
                        },
                        known,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_line_distance_pair(
                            sketch,
                            parameter,
                            sketch_entities
                        ))
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
                    sketch,
                    &first,
                    parameter,
                    sketch_entities,
                ));
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
                            sketch,
                            &first,
                            &second,
                            parameter,
                            sketch_entities,
                        ));
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
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                        ));
                        (known, partner)
                    }
                    (None, Some(known)) => (
                        resolved_or_none!(unique_profile_line_angle_entity(
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                        )),
                        known,
                    ),
                    (None, None) => {
                        resolved_or_none!(unique_profile_line_angle_pair(
                            sketch,
                            parameter,
                            sketch_entities
                        ))
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
                        sketch,
                        &first,
                        &second,
                        parameter,
                        sketch_entities,
                    ));
                }
            }
            Some(SketchConstraintDefinitionInput::Angle {
                first,
                second,
                parameter: parameter_id,
            })
        }
        CircleDiameter => {
            if let Some(entities) =
                repeated_dimensioned_circular_entities(relation, parameter, sketch, sketch_entities)
            {
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
            let resolved_entity = if let Some(entity) = sketch_entities.iter().find(|entity| {
                entity.sketch == *sketch
                    && entity.geometry_ref.as_deref() == Some(relation.id.as_str())
                    && matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Circle { .. }
                            | SketchGeometryDefinition::Arc { .. }
                    )
            }) {
                Some(entity.id().clone())
            } else if let Some(marker) = marker(0) {
                match marker_center_dimensioned_entity(marker, sketch, sketch_entities, parameter)?
                {
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
                None => unique_dimensioned_circle_entity(sketch, sketch_entities, parameter)?,
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
    relation: &FeatureInputRelationInstance,
    index: usize,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let operand = relation.operands.get(index)?;
    if !relation_uses_solver_line_operand(relation, index) {
        return None;
    }
    if let Some(entity_ref) = operand.entity_ref.as_deref() {
        let mut selected = None;
        for entity in sketch_entities {
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
                return None;
            }
            selected = Some(entity.id());
        }
        if let Some(entity) = selected {
            return Some(entity.clone());
        }
        if !relation_uses_dynamic_operands(relation) {
            return None;
        }
    }
    let mut selected = None;
    for entity in sketch_entities {
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
            return None;
        }
        selected = Some(entity.id());
    }
    selected.cloned()
}

fn repeated_dimensioned_circular_entities(
    relation: &FeatureInputRelationInstance,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Option<Vec<SketchEntityId>> {
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
        return None;
    }
    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return None;
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return None,
    };
    if !(expected_radius.is_finite() && expected_radius > 0.0) {
        return None;
    }
    let matches = |entity: &SketchEntity| -> bool {
        if entity.sketch != *sketch
            || (!repeated_display && entity.geometry_ref.as_deref() != parameter_native_ref)
        {
            return false;
        }
        let radius = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { radius, .. }
            | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
            _ => return false,
        };
        same_dimension_length(radius, expected_radius)
    };
    let mut count = 0usize;
    for entity in sketch_entities {
        if matches(entity) {
            count += 1;
        }
    }
    if count < 2 || count > relation.scalar_refs().len() {
        return None;
    }
    let mut entities = Vec::new();
    for entity in sketch_entities {
        if matches(entity) {
            entities.push(entity.id().clone());
        }
    }
    Some(entities)
}

// Find the unique profile locus pair whose spanned dimension, measured by
// `measure` over the two loci points, equals the parameter length. Every
// unordered pair of canonical profile loci is a candidate; `measure` is the
// only axis of variation between the distance and axis-distance resolvers.
fn unique_profile_measured_loci_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Option<(SketchLocus, SketchLocus)> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    let loci = canonical_profile_loci(sketch, sketch_entities);
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    for (first_index, (first_point, first)) in loci.iter().enumerate() {
        for (second_point, second) in &loci[first_index + 1..] {
            if !same_dimension_length(measure(first_point, second_point), distance.get()) {
                continue;
            }
            let pair = (first, second);
            if selected.is_some_and(|selected| selected != pair) {
                return None;
            }
            selected = Some(pair);
        }
    }
    let (first, second) = selected?;
    Some((
        super::transforms::SketchLocusRole::of_locus(first).copy_locus(locus_entity(first)),
        super::transforms::SketchLocusRole::of_locus(second).copy_locus(locus_entity(second)),
    ))
}

// Repair a candidate pair by resolving each supplied locus to its unique
// partner via `partner`, forming the sorted pair, and keeping it only when the
// two starting loci agree on exactly one pair.
fn unique_repaired_profile_pair(
    first: &SketchLocus,
    second: &SketchLocus,
    partner: impl Fn(&SketchLocus) -> Option<SketchLocus>,
) -> Option<(SketchLocus, SketchLocus)> {
    let mut selected: Option<(SketchLocus, SketchLocus)> = None;
    for known in [first, second] {
        let Some(partner) = partner(known) else {
            continue;
        };
        let known =
            super::transforms::SketchLocusRole::of_locus(known).copy_locus(locus_entity(known));
        let mut pair = [known, partner];
        pair.sort_unstable_by(|left, right| locus_key(left).cmp(&locus_key(right)));
        let [first, second] = pair;
        let pair = (first, second);
        if selected.as_ref().is_some_and(|selected| selected != &pair) {
            return None;
        }
        selected = Some(pair);
    }
    selected
}

// Find the unique profile locus at a given dimension from `known`, where
// `measure` reports the dimension between the known point and a candidate
// point. The known locus is excluded and, as for the pair resolvers, `measure`
// is the sole axis of variation between the straight-distance and axis-distance
// forms.
fn unique_profile_measured_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Option<SketchLocus> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    let known_point = profile_locus_point(known, sketch_entities)?;
    let mut selected: Option<SketchLocus> = None;
    for (candidate_point, candidate) in canonical_profile_loci(sketch, sketch_entities) {
        if candidate == *known
            || !same_dimension_length(measure(&known_point, &candidate_point), distance.get())
        {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|selected| selected != &candidate)
        {
            return None;
        }
        selected = Some(candidate);
    }
    selected
}

pub(super) fn unique_profile_distance_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchLocus> {
    unique_profile_measured_locus(
        sketch,
        known,
        parameter,
        sketch_entities,
        |known, candidate| (candidate.u - known.u).hypot(candidate.v - known.v),
    )
}

pub(super) fn doubled_profile_distance_loci(
    relation: &FeatureInputRelationInstance,
    operands: (usize, usize),
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> Option<(SketchLocus, SketchLocus)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return None;
    };
    let line_marker_id = relation_operand_marker(relation, operands.0, sketch, markers_by_id)?;
    let center_marker_id = relation_operand_marker(relation, operands.1, sketch, markers_by_id)?;
    let line_marker = markers_by_id.get(line_marker_id)?;
    let center_marker = markers_by_id.get(center_marker_id)?;
    if line_marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
        || center_marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
    {
        return None;
    }
    let mut center_is_distance_handle = false;
    for marker in markers_by_id.values() {
        if marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
            || marker.kind()
                != SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
        {
            continue;
        }
        let [link] = marker.links() else {
            continue;
        };
        if link.entity_ref == center_marker.id() {
            center_is_distance_handle = true;
            break;
        }
    }
    if !center_is_distance_handle {
        return None;
    }
    let line_coordinates = line_marker.coordinates_m?;
    let center_coordinates = center_marker.coordinates_m?;
    let [line_u, line_v] = line_coordinates.get();
    let [center_u, center_v] = center_coordinates.get();
    if ![
        (center_u - line_u).abs() * NATIVE_TO_IR * 2.0,
        (center_v - line_v).abs() * NATIVE_TO_IR * 2.0,
    ]
    .into_iter()
    .any(|distance| same_dimension_length(distance, expected.get()))
    {
        return None;
    }
    let mut selected = None;
    for entity in sketch_entities {
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
            return None;
        }
        selected = Some(entity.id());
    }
    let identity = selected?;
    Some((
        super::transforms::SketchLocusRole::Start.copy_locus(identity),
        super::transforms::SketchLocusRole::End.copy_locus(identity),
    ))
}

fn unique_repaired_profile_distance_loci_pair(
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchLocus)> {
    unique_repaired_profile_pair(first, second, |known| {
        unique_profile_distance_locus(sketch, known, parameter, sketch_entities)
    })
}

fn unique_profile_axis_distance_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    axis: ProfileAxis,
) -> Option<SketchLocus> {
    unique_profile_measured_locus(
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
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    axis: ProfileAxis,
) -> Option<(SketchLocus, SketchLocus)> {
    unique_repaired_profile_pair(first, second, |known| {
        unique_profile_axis_distance_locus(sketch, known, parameter, sketch_entities, axis)
    })
}

fn unique_profile_axis_distance_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    axis: ProfileAxis,
) -> Option<(SketchLocus, SketchLocus)> {
    unique_profile_measured_loci_pair(sketch, parameter, sketch_entities, |first, second| {
        if axis == ProfileAxis::U {
            (second.u - first.u).abs()
        } else {
            (second.v - first.v).abs()
        }
    })
}

fn unique_profile_distance_loci_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchLocus)> {
    unique_profile_measured_loci_pair(sketch, parameter, sketch_entities, |first, second| {
        (second.u - first.u).hypot(second.v - first.v)
    })
}

pub(super) fn canonical_profile_loci(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Vec<(Point2, SketchLocus)> {
    const QUANTUM: f64 = 1e-8;
    let mut indexed = Vec::new();
    for (source_index, entity) in sketch_entities.iter().enumerate() {
        if entity.sketch != *sketch {
            continue;
        }
        for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
            indexed.push((source_index, point, role.copy_locus(entity.id())));
        }
    }
    // Source order breaks equal geometric and identity keys without sort scratch.
    indexed.sort_unstable_by(
        |(left_index, left_point, left_locus), (right_index, right_point, right_locus)| {
            quantize(*left_point, QUANTUM)
                .cmp(&quantize(*right_point, QUANTUM))
                .then_with(|| locus_key(left_locus).cmp(&locus_key(right_locus)))
                .then_with(|| left_index.cmp(right_index))
        },
    );
    indexed.dedup_by(|(_, left_point, _), (_, right_point, _)| {
        quantize(*left_point, QUANTUM) == quantize(*right_point, QUANTUM)
    });
    let mut loci = Vec::new();
    for (_, point, locus) in indexed {
        loci.push((point, locus));
    }
    loci
}

// Find the unique sketch entity, other than `known`, for which `matches`
// accepts the ordered pair `(known, candidate)`. The parameter kind guard and
// the measurement both live in `matches`, so the straight-distance and angle
// resolvers differ only in the closure they pass.
fn unique_profile_matched_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    sketch_entities: &[SketchEntity],
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Option<SketchEntityId> {
    let mut known_entity = None;
    for entity in sketch_entities {
        if entity.id() == known {
            known_entity = Some(entity);
            break;
        }
    }
    let known = known_entity?;
    let mut selected: Option<&SketchEntityId> = None;
    for entity in sketch_entities {
        if entity.sketch != *sketch || entity.id() == known.id() || !matches(known, entity) {
            continue;
        }
        if selected.is_some_and(|selected| selected != entity.id()) {
            return None;
        }
        selected = Some(entity.id());
    }
    selected.cloned()
}

// Select one distinct ordered identity pair from unordered line positions.
// Only line entities participate; `matches` supplies the measurement and comparison.
fn unique_profile_matched_line_pair(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Option<(SketchEntityId, SketchEntityId)> {
    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
    for (first_index, first) in sketch_entities.iter().enumerate() {
        if first.sketch != *sketch
            || !matches!(
                first.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        for second in &sketch_entities[first_index + 1..] {
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
                return None;
            }
            selected = Some(pair);
        }
    }
    let (first, second) = selected?;
    Some((first.clone(), second.clone()))
}

// Resolve each supplied entity to its unique partner and sort its two identities.
// Keep one distinct pair across the two resolutions.
fn unique_repaired_entity_pair(
    first: &SketchEntityId,
    second: &SketchEntityId,
    partner: impl Fn(&SketchEntityId) -> Option<SketchEntityId>,
) -> Option<(SketchEntityId, SketchEntityId)> {
    let mut selected: Option<(SketchEntityId, SketchEntityId)> = None;
    for known in [first, second] {
        let Some(partner) = partner(known) else {
            continue;
        };
        let mut pair = [known.clone(), partner];
        pair.sort_by(Ord::cmp);
        let [first, second] = pair;
        let pair = (first, second);
        if selected.as_ref().is_some_and(|selected| selected != &pair) {
            return None;
        }
        selected = Some(pair);
    }
    selected
}

fn unique_profile_line_distance_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    unique_profile_matched_entity(sketch, known, sketch_entities, |known, candidate| {
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
    let Some(marker_point) = profile_locus_point(&marker_locus, sketch_entities) else {
        return Ok(None);
    };
    Ok(unique_profile_matched_entity(
        sketch,
        known,
        sketch_entities,
        |known, candidate| {
            matches!(
                candidate.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            ) && sketch_entity_contains_point(candidate, marker_point)
                && line_line_distance(known, candidate)
                    .is_some_and(|measured| same_dimension_length(measured, distance.get()))
        },
    ))
}

fn unique_profile_line_distance_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    unique_profile_matched_line_pair(sketch, sketch_entities, |first, second| {
        line_line_distance(first, second)
            .is_some_and(|measured| same_dimension_length(measured, distance.get()))
    })
}

fn unique_repaired_profile_line_distance_pair(
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    unique_repaired_entity_pair(first, second, |known| {
        unique_profile_line_distance_entity(sketch, known, parameter, sketch_entities)
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
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = parameter.value.as_ref() else {
        return None;
    };
    unique_profile_matched_entity(sketch, known, sketch_entities, |known, candidate| {
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
    entities: &'a [SketchEntity],
    id: &SketchEntityId,
) -> Option<&'a SketchEntity> {
    entities.iter().find(|entity| entity.id() == id)
}

fn select_dynamic_line_pair(
    first_candidates: &[SketchEntityId],
    second_candidates: &[SketchEntityId],
    entities: &[SketchEntity],
    ordering: LinePairOrdering,
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Option<(SketchEntityId, SketchEntityId)> {
    if first_candidates.is_empty() || second_candidates.is_empty() {
        return None;
    }
    let mut selected: Option<(&SketchEntityId, &SketchEntityId)> = None;
    for first in first_candidates {
        let first_entity = find_profile_entity(entities, first)?;
        for second in second_candidates {
            if first == second {
                continue;
            }
            let second_entity = find_profile_entity(entities, second)?;
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
                return None;
            }
            selected = Some(pair);
        }
    }
    let (first, second) = selected?;
    Some((first.clone(), second.clone()))
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
    Ok(select_dynamic_line_pair(
        &first_candidates,
        &second_candidates,
        sketch_entities,
        LinePairOrdering::Operand,
        |first, second| {
            unoriented_line_line_angle(first, second)
                .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
        },
    ))
}

fn unique_dynamic_profile_line_angle_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return None;
    };
    unique_profile_matched_entity(sketch, known, sketch_entities, |known, candidate| {
        unoriented_line_line_angle(known, candidate)
            .is_some_and(|measured| same_dimension_angle(measured, expected.get()))
    })
}

fn unique_dynamic_roster_line_angle_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) = parameter.value.as_ref()
    else {
        return None;
    };
    unique_profile_matched_line_pair(sketch, sketch_entities, |first, second| {
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
        let Some(first_point) = profile_locus_point(first, sketch_entities) else {
            return Ok(None);
        };
        let Some(second_point) = profile_locus_point(second, sketch_entities) else {
            return Ok(None);
        };
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
        if let Some(marker) = relation_operand_marker(relation, index, sketch, markers_by_id) {
            let additions = dynamic_marker_point_candidates(
                ctx,
                marker,
                sketch,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            )?;
            candidates.extend(additions);
        }
        Ok(candidates)
    };
    let mut first_candidates = candidates(0, known_first)?;
    let mut second_candidates = candidates(1, known_second)?;
    deduplicate_physical_loci(&mut first_candidates, |locus| {
        profile_locus_point(locus, sketch_entities)
    });
    deduplicate_physical_loci(&mut second_candidates, |locus| {
        profile_locus_point(locus, sketch_entities)
    });
    if first_candidates.is_empty() || second_candidates.is_empty() {
        return Ok(None);
    }
    let mut selected: Option<(&SketchLocus, &SketchLocus)> = None;
    for first in &first_candidates {
        for second in &second_candidates {
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
        super::transforms::SketchLocusRole::of_locus(first).copy_locus(locus_entity(first)),
        super::transforms::SketchLocusRole::of_locus(second).copy_locus(locus_entity(second)),
    )))
}

fn unique_dynamic_direct_point_roster_pair(
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    profile_axis: Option<ProfileAxis>,
) -> Result<Option<(SketchLocus, SketchLocus)>, cadmpeg_core::CodecError> {
    for operand in &relation.operands {
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
    for index in 0..2 {
        let Some(marker_id) = relation_operand_marker(relation, index, sketch, markers_by_id)
        else {
            return Ok(None);
        };
        let Some(marker) = markers_by_id.get(marker_id) else {
            return Ok(None);
        };
        if !is_direct_point(marker) {
            return Ok(None);
        }
    }
    let mut direct_marker_ids = HashSet::new();
    for marker in markers_by_id.values() {
        if !is_direct_point(marker) {
            continue;
        }
        let marker_id = marker.id();
        direct_marker_ids.insert(marker_id);
    }
    let mut positions = HashMap::<&SketchEntityId, Point2>::new();
    let mut loci = Vec::new();
    for entity in sketch_entities {
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
        if !direct_marker_ids.contains(reference) {
            continue;
        }
        let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
            continue;
        };
        positions.insert(entity.id(), position.get());
        loci.push(super::transforms::SketchLocusRole::Entity.copy_locus(entity.id()));
    }
    sort_profile_loci(&mut loci);
    let point = |locus: &SketchLocus| positions.get(locus_entity(locus)).copied();
    deduplicate_physical_loci(&mut loci, point);
    let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) = parameter.value.as_ref()
    else {
        return Ok(None);
    };
    let measure = |first: &SketchLocus,
                   second: &SketchLocus|
     -> Result<Option<f64>, cadmpeg_core::CodecError> {
        let Some(first) = point(first) else {
            return Ok(None);
        };
        let Some(second) = point(second) else {
            return Ok(None);
        };
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
        super::transforms::SketchLocusRole::of_locus(first).copy_locus(locus_entity(first)),
        super::transforms::SketchLocusRole::of_locus(second).copy_locus(locus_entity(second)),
    )))
}

const DYNAMIC_POINT_LOCUS_QUANTUM: f64 = 1.0e-8;

fn deduplicate_physical_loci<T>(candidates: &mut Vec<T>, point: impl Fn(&T) -> Option<Point2>) {
    let mut points = HashSet::new();
    let mut write = 0;
    for read in 0..candidates.len() {
        let keep = if let Some(point) = point(&candidates[read]) {
            let key = quantize(point, DYNAMIC_POINT_LOCUS_QUANTUM);
            points.insert(key)
        } else {
            true
        };
        if keep {
            candidates.swap(write, read);
            write += 1;
        }
    }
    candidates.truncate(write);
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
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    known_point: Option<SketchLocus>,
    known_line: Option<SketchEntityId>,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let explicit_point = relation
        .operands
        .first()
        .is_some_and(|operand| operand.entity_ref.is_some());
    let explicit_line = relation
        .operands
        .get(1)
        .is_some_and(|operand| operand.entity_ref.is_some());
    if (explicit_point && known_point.is_none()) || (explicit_line && known_line.is_none()) {
        return None;
    }
    // Explicit identities narrow the geometry roster to the other operand.
    let Some(known) = KnownOperands::resolve(
        explicit_point.then_some(known_point).flatten(),
        explicit_line.then_some(known_line).flatten(),
    ) else {
        if explicit_point || explicit_line {
            return None;
        }
        return unique_roster_point_line_pair(sketch, parameter, sketch_entities);
    };
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    let lines = collect_profile_lines(sketch, sketch_entities);
    let mut points = collect_profile_loci(sketch, sketch_entities);
    deduplicate_physical_loci(&mut points, |(_, locus)| {
        profile_locus_point(locus, sketch_entities)
    });
    match known {
        KnownOperands::Point(point) => {
            let position = profile_locus_point(&point, sketch_entities)?;
            select_profile_point_line_pairs(*distance, &[(position, point)], &lines)
        }
        KnownOperands::Line(line) => {
            let line_entity = find_profile_entity(sketch_entities, &line)?;
            resolve_profile_locus_positions(&mut points, sketch_entities);
            select_profile_point_line_pairs(*distance, &points, &[line_entity])
        }
        KnownOperands::Both(point, line) => {
            let line_entity = find_profile_entity(sketch_entities, &line)?;
            let position = profile_locus_point(&point, sketch_entities)?;
            point_line_distance_value(position, line_entity)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
                .then_some((point, line))
        }
    }
}

fn unique_roster_point_line_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    let mut loci = collect_profile_loci(sketch, sketch_entities);
    deduplicate_physical_loci(&mut loci, |(_, locus)| {
        profile_locus_point(locus, sketch_entities)
    });
    // The owning sketch roster supplies line witnesses for family-scoped operands.
    let lines = collect_profile_lines(sketch, sketch_entities);
    resolve_profile_locus_positions(&mut loci, sketch_entities);
    select_profile_point_line_pairs(*distance, &loci, &lines)
}

fn unique_point_line_candidate_pair(
    expected: cadmpeg_ir::scalar::Length,
    point_candidates: &[SketchLocus],
    line_candidates: &[SketchEntityId],
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    if point_candidates.is_empty() || line_candidates.is_empty() {
        return None;
    }
    let mut selected: Option<(&SketchLocus, &SketchEntityId)> = None;
    for point in point_candidates {
        let point_position = profile_locus_point(point, sketch_entities)?;
        for line in line_candidates {
            let line_entity = find_profile_entity(sketch_entities, line)?;
            if !point_line_distance_value(point_position, line_entity)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
            {
                continue;
            }
            let pair = (point, line);
            if selected.is_some_and(|selected| selected != pair) {
                return None;
            }
            selected = Some(pair);
        }
    }
    let (point, line) = selected?;
    Some((
        super::transforms::SketchLocusRole::of_locus(point).copy_locus(locus_entity(point)),
        line.clone(),
    ))
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
    Ok(select_dynamic_line_pair(
        &first_candidates,
        &second_candidates,
        sketch_entities,
        LinePairOrdering::Identity,
        |first, second| {
            line_line_distance(first, second)
                .is_some_and(|measured| same_relation_dimension_length(measured, expected.get()))
        },
    ))
}

fn unique_dynamic_roster_line_distance_pair(
    relation: &FeatureInputRelationInstance,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    for operand in &relation.operands {
        if operand.entity_ref.is_some() {
            return None;
        }
    }
    unique_profile_line_distance_pair(sketch, parameter, sketch_entities)
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
    let mut entities = if let Some(entity) =
        solver_line_entity(relation, index, sketch, sketch_entities)
    {
        vec![entity]
    } else if let Some(marker) =
        match relation_operand_marker(relation, index, sketch, markers_by_id) {
            Some(marker) => Some(marker),
            None => relation_line_point_marker(relation, index, markers_by_id)
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
    entities.truncate(write);
    if entities.len() > 1 {
        entities.sort_unstable_by(Ord::cmp);
        entities.dedup();
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
        if let Some(entity) = find_profile_entity(sketch_entities, locus_entity(&locus)) {
            if entity.sketch == *sketch && profile_locus_point(&locus, sketch_entities).is_some() {
                candidates.push(locus);
            }
        }
    }
    for entity in sketch_entities {
        if entity.sketch != *sketch
            || !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
        {
            continue;
        }
        if dynamic_entity_has_marker_identity(entity, &marker_ids) {
            candidates.push(super::transforms::SketchLocusRole::Entity.copy_locus(entity.id()));
        }
    }
    sort_profile_loci(&mut candidates);
    Ok(candidates)
}

fn dynamic_entity_has_marker_identity(entity: &SketchEntity, marker_ids: &HashSet<&str>) -> bool {
    for reference in entity
        .native_ref
        .as_deref()
        .into_iter()
        .chain(entity.geometry_ref.as_deref())
        .chain(entity.endpoint_refs.iter().map(std::string::String::as_str))
    {
        if marker_ids.contains(reference) {
            return true;
        }
    }
    false
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
    let mut marker_ids = HashSet::new();
    collect_marker_identity_ids(ctx, marker, markers_by_id, &mut marker_ids)?;
    let mut has_arc_marker = false;
    let mut centers = Vec::new();
    for marker_id in marker_ids {
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
        let Some(entity) = find_profile_entity(sketch_entities, locus_entity(&locus)) else {
            continue;
        };
        if entity.sketch == *sketch
            && matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Arc { .. }
            )
        {
            centers.push(super::transforms::SketchLocusRole::Center.copy_locus(entity.id()));
        }
    }
    if !has_arc_marker {
        return Ok(None);
    }
    sort_profile_loci(&mut centers);
    Ok(Some(centers))
}

fn dynamic_marker_line_candidates(
    ctx: &DecodeContext<'_>,
    marker: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Result<Vec<SketchEntityId>, cadmpeg_core::CodecError> {
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
        if matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ) && dynamic_entity_has_marker_identity(entity, &marker_ids)
        {
            candidates.push(entity.id().clone());
        }
    }
    if candidates.is_empty() {
        if let Some(entity) =
            single_marker_line_entity(ctx, marker, markers_by_id, loci_by_marker, sketch_entities)?
        {
            candidates.push(entity);
        }
    }
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
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
    if !marker_ids.insert(marker_id) {
        return Ok(());
    }
    let Some(marker) = markers_by_id.get(marker_id) else {
        return Ok(());
    };
    for link in marker.links() {
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
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = parameter.value.as_ref() else {
        return None;
    };
    unique_profile_matched_line_pair(sketch, sketch_entities, |first, second| {
        line_line_angle(first, second)
            .is_some_and(|measured| same_dimension_angle(measured, angle.get()))
    })
}

fn unique_repaired_profile_line_angle_pair(
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    unique_repaired_entity_pair(first, second, |known| {
        unique_profile_line_angle_entity(sketch, known, parameter, sketch_entities)
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
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Vec<(Point2, SketchLocus)> {
    let mut result = Vec::new();
    for entity in entities {
        if entity.sketch != *sketch {
            continue;
        }
        for (point, role) in sketch_entity_locus_points(entity).into_iter().flatten() {
            result.push((point, role.copy_locus(entity.id())));
        }
    }
    result
}

fn collect_profile_lines<'a>(
    sketch: &SketchId,
    entities: &'a [SketchEntity],
) -> Vec<&'a SketchEntity> {
    let mut result = Vec::new();
    for entity in entities {
        if entity.sketch == *sketch
            && matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            result.push(entity);
        }
    }
    result
}

fn resolve_profile_locus_positions(
    loci: &mut Vec<(Point2, SketchLocus)>,
    entities: &[SketchEntity],
) {
    let mut write = 0;
    for read in 0..loci.len() {
        if let Some(point) = profile_locus_point(&loci[read].1, entities) {
            loci[read].0 = point;
            loci.swap(write, read);
            write += 1;
        }
    }
    loci.truncate(write);
}

fn select_profile_point_line_pairs(
    distance: cadmpeg_ir::scalar::Length,
    loci: &[(Point2, SketchLocus)],
    lines: &[&SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let mut selected: Option<(&SketchLocus, &SketchEntityId)> = None;
    for (point, locus) in loci {
        for line in lines {
            if !point_line_distance_value(*point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
            {
                continue;
            }
            let pair = (locus, line.id());
            if selected.is_some_and(|selected| selected != pair) {
                return None;
            }
            selected = Some(pair);
        }
    }
    let (point, line) = selected?;
    Some((
        super::transforms::SketchLocusRole::of_locus(point).copy_locus(locus_entity(point)),
        line.clone(),
    ))
}

fn unique_profile_point_line_entity(
    sketch: &SketchId,
    point: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    let point = profile_locus_point(point, sketch_entities)?;
    let mut selected: Option<&SketchEntityId> = None;
    for line in sketch_entities {
        if line.sketch != *sketch
            || !point_line_distance_value(point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.get()))
        {
            continue;
        }
        if selected.is_some_and(|selected| selected != line.id()) {
            return None;
        }
        selected = Some(line.id());
    }
    selected.cloned()
}

fn unique_profile_line_point_locus(
    sketch: &SketchId,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchLocus> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    let line = find_profile_entity(sketch_entities, line)?;
    let mut selected: Option<SketchLocus> = None;
    for (point, locus) in collect_profile_loci(sketch, sketch_entities) {
        if !point_line_distance_value(point, line)
            .is_some_and(|measured| same_dimension_length(measured, distance.get()))
        {
            continue;
        }
        if selected.as_ref().is_some_and(|selected| selected != &locus) {
            return None;
        }
        selected = Some(locus);
    }
    selected
}

fn unique_profile_point_line_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) = parameter.value.as_ref()
    else {
        return None;
    };
    let loci = collect_profile_loci(sketch, sketch_entities);
    let lines = collect_profile_lines(sketch, sketch_entities);
    select_profile_point_line_pairs(*distance, &loci, &lines)
}

fn unique_repaired_profile_point_line_pair(
    sketch: &SketchId,
    point: &SketchLocus,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let mut selected = None;
    if let Some(candidate_line) =
        unique_profile_point_line_entity(sketch, point, parameter, sketch_entities)
    {
        selected = Some((
            super::transforms::SketchLocusRole::of_locus(point).copy_locus(locus_entity(point)),
            candidate_line,
        ));
    }
    if let Some(candidate_point) =
        unique_profile_line_point_locus(sketch, line, parameter, sketch_entities)
    {
        let pair = (candidate_point, line.clone());
        if selected.as_ref().is_some_and(|selected| selected != &pair) {
            return None;
        }
        selected = Some(pair);
    }
    selected
}

pub(super) fn profile_locus_point(
    locus: &SketchLocus,
    sketch_entities: &[SketchEntity],
) -> Option<Point2> {
    let entity = find_profile_entity(sketch_entities, locus_entity(locus))?;
    sketch_entity_locus_points(entity)
        .into_iter()
        .flatten()
        .find_map(|(point, role)| role.matches(locus).then_some(point))
}

fn canonicalize_physical_loci(
    loci: &mut Vec<SketchLocus>,
    entities_by_id: &HashMap<&SketchEntityId, &SketchEntity>,
    quantum: f64,
) {
    if loci.len() < 2 {
        return;
    }
    let mut first_point = None;
    let mut coincident = true;
    let mut minimum = 0;
    for (index, locus) in loci.iter().enumerate() {
        let Some(entity) = entities_by_id.get(locus_entity(locus)) else {
            return;
        };
        let Some(point) = sketch_entity_locus_points(entity)
            .into_iter()
            .flatten()
            .find_map(|(point, role)| role.matches(locus).then_some(point))
        else {
            return;
        };
        let point = quantize(point, quantum);
        if let Some(first) = first_point {
            coincident &= point == first;
        } else {
            first_point = Some(point);
        }
        if locus_key(locus) < locus_key(&loci[minimum]) {
            minimum = index;
        }
    }
    if coincident {
        loci.swap(0, minimum);
        loci.truncate(1);
    }
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
    relation: &FeatureInputRelationInstance,
    markers: &HashMap<&str, &'a SketchInputEntity>,
    matches: impl Fn(&SketchInputEntity) -> bool,
) -> Vec<&'a SketchInputEntity> {
    let mut candidates = Vec::new();
    for marker in markers.values().copied() {
        if marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str()) || !matches(marker)
        {
            continue;
        }
        candidates.push(marker);
    }
    candidates
}

pub(super) fn relation_operand_marker<'a>(
    relation: &'a FeatureInputRelationInstance,
    index: usize,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<&'a str> {
    let operand = relation.operands.get(index)?;
    if sketch.as_str().contains("sketch#compact:") && operand.kind == FeatureInputOperandKind::D6 {
        let mut coordinate_handles =
            collect_relation_marker_candidates(relation, markers_by_id, |marker| {
                marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            });
        coordinate_handles.sort_unstable_by_key(|left| left.offset());
        return coordinate_handles
            .get(usize::from(operand.entity_index))
            .map(|marker| marker.id());
    }
    match operand.entity_ref.as_deref() {
        Some(marker) => Some(marker),
        None => dynamic_relation_marker(relation, index, markers_by_id),
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
    relation: &FeatureInputRelationInstance,
    index: usize,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<&'a str> {
    if !relation_uses_dynamic_operands(relation) {
        return None;
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
        return None;
    }
    let operand = relation.operands.get(index)?;
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
    let by_object = collect_relation_marker_candidates(relation, markers_by_id, |marker| {
        marker.object_index() == Some(address) && address_kind(marker)
    });
    if !by_object.is_empty() {
        if point_role {
            if let Some(marker) = unique_dynamic_marker(by_object.iter().copied(), direct_kind) {
                return Some(marker);
            }
            if let Some(marker) = unique_dynamic_marker(by_object.iter().copied(), |marker| {
                matches!(marker.kind(), SketchInputKind::Arc)
            }) {
                return Some(marker);
            }
        }
        return unique_dynamic_marker(by_object.into_iter(), |_| true);
    }
    let by_local = collect_relation_marker_candidates(relation, markers_by_id, |marker| {
        marker.local_id() == Some(address) && address_kind(marker)
    });
    if !by_local.is_empty() {
        if point_role {
            if let Some(marker) = unique_dynamic_marker(by_local.iter().copied(), direct_kind) {
                return Some(marker);
            }
            if let Some(marker) = unique_dynamic_marker(by_local.iter().copied(), |marker| {
                matches!(marker.kind(), SketchInputKind::Arc)
            }) {
                return Some(marker);
            }
        }
        return unique_dynamic_marker(by_local.into_iter(), |_| true);
    }
    let mut ordinal = collect_relation_marker_candidates(relation, markers_by_id, direct_kind);
    ordinal.sort_unstable_by(|left, right| {
        left.offset()
            .cmp(&right.offset())
            .then_with(|| left.ordinal().cmp(&right.ordinal()))
            .then_with(|| left.id().cmp(right.id()))
    });
    ordinal
        .get(usize::from(operand.entity_index))
        .map(|marker| marker.id())
}

fn unique_dynamic_marker<'a>(
    candidates: impl Iterator<Item = &'a SketchInputEntity>,
    matches: impl Fn(&SketchInputEntity) -> bool,
) -> Option<&'a str> {
    let mut selected = None;
    for candidate in candidates {
        if !matches(candidate) {
            continue;
        }
        if selected.is_some_and(|id| id != candidate.id()) {
            return None;
        }
        selected = Some(candidate.id());
    }
    selected
}

fn relation_line_point_marker<'a>(
    relation: &FeatureInputRelationInstance,
    index: usize,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<&'a SketchInputEntity> {
    let operand = relation.operands.get(index)?;
    if operand.kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386)
        || operand.entity_ref.is_some()
    {
        return None;
    }
    let mut selected = None;
    for marker in markers_by_id.values().copied() {
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
            return None;
        }
        selected = Some(marker);
    }
    selected
}

fn unique_profile_entity<'a>(
    sketch: &SketchId,
    entities: &'a [SketchEntity],
    matches: impl Fn(&SketchEntity) -> Result<bool, cadmpeg_core::CodecError>,
) -> Result<Option<&'a SketchEntity>, cadmpeg_core::CodecError> {
    let mut selected = None;
    for entity in entities {
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
    marker_id: &str,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    let center_entity = unique_profile_entity(sketch, sketch_entities, |entity| {
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
    let selected = unique_profile_entity(sketch, sketch_entities, |entity| {
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
    Ok(selected.map(|entity| entity.id().clone()))
}

fn unique_dimensioned_circle_entity(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
        None => return Ok(None),
    };
    let selected = unique_profile_entity(sketch, sketch_entities, |entity| {
        let radius = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { radius, .. }
            | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
            _ => return Ok(false),
        };
        Ok(same_dimension_length(radius, expected_radius))
    })?;
    Ok(selected.map(|entity| entity.id().clone()))
}

pub(super) fn same_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9 * left.abs().max(right.abs()).max(1.0)
}

pub(super) fn profile_axis_for_relation(
    relation: &FeatureInputRelationInstance,
    transforms: Option<&[MarkerTransform]>,
) -> Option<ProfileAxis> {
    let (native_axis, default_axis) = match relation.family {
        FeatureInputRelationFamily::PointPointHorizontalDistance => (0, ProfileAxis::U),
        FeatureInputRelationFamily::PointPointVerticalDistance => (1, ProfileAxis::V),
        _ => return None,
    };
    let Some(transforms) = transforms else {
        return Some(default_axis);
    };
    if transforms.is_empty() {
        return Some(default_axis);
    }
    let mut first = None;
    let mut disagreement = false;
    for transform in transforms {
        let axis = transform.profile_axis_for_native(native_axis)?;
        if let Some(first) = first {
            if axis != first {
                disagreement = true;
            }
        } else {
            first = Some(axis);
        }
    }
    first.filter(|_| !disagreement)
}

fn qualified_point_loci<'a>(
    marker_id: &str,
    loci_by_marker: &'a HashMap<String, Vec<SketchLocus>>,
) -> Option<&'a [SketchLocus]> {
    loci_by_marker.iter().find_map(|(key, loci)| {
        (key.strip_suffix(":qualified-point") == Some(marker_id)).then_some(loci.as_slice())
    })
}

pub(super) fn marker_point_locus(
    ctx: &DecodeContext<'_>,
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    if let Some(loci) = qualified_point_loci(marker_id, loci_by_marker) {
        if let Some(locus) = unique_locus(loci) {
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
            if link.entity_ref == marker_id {
                continue;
            }
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
            let Some(entity) = find_profile_entity(sketch_entities, locus_entity(&locus)) else {
                continue;
            };
            if !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            ) {
                continue;
            }
            if selected.as_ref().is_some_and(|selected| selected != &locus) {
                ambiguous = true;
            }
            selected = Some(locus);
        }
        if selected.is_some() && !ambiguous {
            return Ok(selected);
        }
    }
    if let Some(loci) = qualified_point_loci(marker_id, loci_by_marker) {
        return Ok(unique_locus(loci));
    }
    let Some(locus) = marker_point_locus(ctx, marker_id, markers_by_id, loci_by_marker)? else {
        return Ok(None);
    };
    let Some(entity) = find_profile_entity(sketch_entities, locus_entity(&locus)) else {
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
    Ok(Some(
        super::transforms::SketchLocusRole::of_locus(locus).copy_locus(locus_entity(locus)),
    ))
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
    if let Some([locus]) = loci_by_marker.get(marker_id).map(Vec::as_slice) {
        return Ok(Some(locus));
    }
    if !visited.insert(marker_id) {
        return Ok(None);
    }
    let result = (|| -> Result<Option<&'loci SketchLocus>, cadmpeg_core::CodecError> {
        let Some(marker) = markers_by_id.get(marker_id) else {
            return Ok(None);
        };
        let mut selected: Option<&SketchLocus> = None;
        let mut ambiguous = false;
        for link in marker.links() {
            if link.entity_ref == marker_id {
                continue;
            }
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
            if selected.is_some_and(|selected| selected != locus) {
                ambiguous = true;
            }
            selected = Some(locus);
        }
        Ok(selected.filter(|_| !ambiguous))
    })();
    let locus = result?;
    visited.remove(marker_id);
    Ok(locus)
}

fn unique_locus(loci: &[SketchLocus]) -> Option<SketchLocus> {
    let [locus] = loci else {
        return None;
    };
    Some(super::transforms::SketchLocusRole::of_locus(locus).copy_locus(locus_entity(locus)))
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
    let mut identities = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::All,
    )?;
    let mut count = 0;
    for index in 0..identities.len() {
        let Some(entity) = find_profile_entity(sketch_entities, &identities[index]) else {
            continue;
        };
        if !matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Arc { .. }
        ) {
            continue;
        }
        identities.swap(count, index);
        count += 1;
    }
    identities.truncate(count);
    sort_marker_entity_ids(&mut identities);
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
    let mut entities = marker_entities(
        ctx,
        marker_id,
        markers_by_id,
        loci_by_marker,
        MarkerEntityFilter::Lines(sketch_entities),
    )?;
    entities.sort_unstable_by(Ord::cmp);
    entities.dedup();
    if entities.len() == 1 {
        return Ok(entities.into_iter().next());
    }
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
    let Some(first_entity) = find_profile_entity(sketch_entities, locus_entity(&first_locus))
    else {
        return fallback();
    };
    let sketch = &first_entity.sketch;
    let Some(second_entity) = find_profile_entity(sketch_entities, locus_entity(&second_locus))
    else {
        return fallback();
    };
    if second_entity.sketch != *sketch {
        return fallback();
    }
    let Some(first) = profile_locus_point(&first_locus, sketch_entities) else {
        return Ok(None);
    };
    let Some(second) = profile_locus_point(&second_locus, sketch_entities) else {
        return Ok(None);
    };
    if same_dimension_length(first.u, second.u) && same_dimension_length(first.v, second.v) {
        return fallback();
    }
    match unique_profile_line_through_points(sketch, sketch_entities, &[first, second]) {
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
    let Some(point) = profile_locus_point(&locus, sketch_entities) else {
        return Ok(None);
    };
    let Some(entity) = find_profile_entity(sketch_entities, locus_entity(&locus)) else {
        return Ok(None);
    };
    Ok(unique_profile_line_through_points(
        &entity.sketch,
        sketch_entities,
        &[point],
    ))
}

fn unique_profile_line_through_points(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    points: &[Point2],
) -> Option<SketchEntityId> {
    let mut selected: Option<&SketchEntityId> = None;
    for entity in sketch_entities {
        if entity.sketch != *sketch
            || !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        if !points
            .iter()
            .all(|point| sketch_entity_contains_point(entity, *point))
        {
            continue;
        }
        if selected.is_some_and(|selected| selected != entity.id()) {
            return None;
        }
        selected = Some(entity.id());
    }
    selected.cloned()
}

fn append_profile_endpoint_locus(
    loci: &mut Vec<SketchLocus>,
    entity: &SketchEntityId,
    role: super::transforms::SketchLocusRole,
) {
    for locus in loci.iter() {
        if role.matches(locus) && locus_entity(locus) == entity {
            return;
        }
    }
    loci.push(role.copy_locus(entity));
}

fn append_transformed_profile_locus(
    loci: &mut Vec<SketchLocus>,
    role: super::transforms::SketchLocusRole,
    entity: &SketchEntityId,
) {
    loci.push(role.copy_locus(entity));
}

fn sort_profile_loci(loci: &mut Vec<SketchLocus>) {
    // Equal sort keys identify equal locus values.
    loci.sort_unstable_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    loci.dedup();
}

pub(super) fn profile_loci_by_marker(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> Result<HashMap<String, Vec<SketchLocus>>, cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1e-8;

    let qualified_point_markers = lanes
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
        .filter_map(|operand| operand.entity_ref.as_deref())
        .collect::<HashSet<_>>();
    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                },
            ) = feature.evaluation.definition()
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let mut profile_loci = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let mut line_midpoints = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let entities_by_id = sketch_entities
        .iter()
        .map(|entity| (entity.id(), entity))
        .collect::<HashMap<_, _>>();
    let transforms =
        marker_transform_candidates_by_feature(features, sketches, sketch_entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id(), marker))
        .collect::<HashMap<_, _>>();
    let native_point_markers_with_nonpoint_carrier = sketch_entities
        .iter()
        .filter(|entity| {
            !matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
        })
        .filter_map(|entity| entity.native_ref.as_deref())
        .collect::<HashSet<_>>();
    for entity in sketch_entities {
        let points = sketch_entity_locus_points(entity);
        let count = points.iter().flatten().count();
        if count != 0 {
            let loci = profile_loci.entry(&entity.sketch).or_default();
            for (point, role) in points.into_iter().flatten() {
                loci.push((point, role.copy_locus(entity.id())));
            }
        }
        if let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() {
            let midpoints = line_midpoints.entry(&entity.sketch).or_default();
            midpoints.push((
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
                super::transforms::SketchLocusRole::Entity.copy_locus(entity.id()),
            ));
        }
    }
    let mut result = HashMap::<String, Vec<SketchLocus>>::new();
    for entity in sketch_entities {
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
        let marker = if qualified_point {
            format!("{marker}:qualified-point")
        } else {
            marker.clone()
        };
        let loci = vec![role.copy_locus(entity.id())];
        result.insert(marker, loci);
    }
    let mut endpoint_marker_keys = HashSet::new();
    for entity in sketch_entities {
        let [start, end] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        for (marker, role) in [
            (start, super::transforms::SketchLocusRole::Start),
            (end, super::transforms::SketchLocusRole::End),
        ] {
            if !markers_by_id.contains_key(marker.as_str()) {
                continue;
            }
            let qualified = qualified_point_markers.contains(marker.as_str());
            for qualified_key in [false, true] {
                if qualified_key && !qualified {
                    continue;
                }
                let key = if qualified_key {
                    format!("{marker}:qualified-point")
                } else {
                    marker.clone()
                };
                endpoint_marker_keys.insert(key.clone());
                append_profile_endpoint_locus(result.entry(key).or_default(), entity.id(), role);
            }
        }
    }
    for marker in endpoint_marker_keys {
        if let Some(loci) = result.get_mut(&marker) {
            canonicalize_physical_loci(loci, &entities_by_id, QUANTUM);
        }
    }
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                let group = markers_by_feature.entry(feature).or_default();
                group.push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            let Some(loci) = profile_loci.get(sketch) else {
                continue;
            };
            let transforms = transforms
                .get(feature)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mut loci_by_point = HashMap::<GridPoint, Vec<SketchLocus>>::new();
            for (point, locus) in loci {
                let point = quantize(*point, QUANTUM);
                let bucket = loci_by_point.entry(point).or_default();
                bucket.push(
                    super::transforms::SketchLocusRole::of_locus(locus)
                        .copy_locus(locus_entity(locus)),
                );
            }
            for marker in markers {
                let qualified_point = qualified_point_markers.contains(marker.id());
                let result_key = if qualified_point {
                    format!("{}:qualified-point", marker.id())
                } else {
                    marker.id().to_string()
                };
                if result.contains_key(&result_key) {
                    continue;
                }
                if qualified_point && sketch.as_str().contains("sketch#compact:") {
                    continue;
                }
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let primary_geometry_locus = usize::try_from(marker.offset())
                    .ok()
                    .is_some_and(|offset| marker_is_geometry_locus(&lane.native_payload, offset));
                let point = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let translated_points = transforms
                    .iter()
                    .filter_map(|transform| transform.apply(point))
                    .collect::<HashSet<_>>();
                let mut marker_loci = Vec::new();
                for translated in translated_points {
                    let mut translated_loci = Vec::new();
                    for locus in loci_by_point
                        .get(&GridPoint::from(translated))
                        .into_iter()
                        .flatten()
                    {
                        if !entities_by_id
                            .get(locus_entity(locus))
                            .is_some_and(|entity| {
                                marker_accepts_locus(marker.kind(), &entity.geometry)
                            })
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
                            &mut translated_loci,
                            role,
                            locus_entity(locus),
                        );
                    }
                    if translated_loci.is_empty() && marker.kind() == SketchInputKind::LineOrCircle
                    {
                        for (point, locus) in line_midpoints.get(sketch).into_iter().flatten() {
                            if quantize(*point, QUANTUM) == translated {
                                append_transformed_profile_locus(
                                    &mut translated_loci,
                                    super::transforms::SketchLocusRole::of_locus(locus),
                                    locus_entity(locus),
                                );
                            }
                        }
                    }
                    if translated_loci.is_empty()
                        && primary_geometry_locus
                        && marker.kind() == SketchInputKind::LineOrCircle
                    {
                        for entity in sketch_entities {
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
                                    &mut translated_loci,
                                    super::transforms::SketchLocusRole::Entity,
                                    entity.id(),
                                );
                            }
                        }
                    }
                    sort_profile_loci(&mut translated_loci);
                    if qualified_point {
                        canonicalize_physical_loci(&mut translated_loci, &entities_by_id, QUANTUM);
                    }
                    if !translated_loci.is_empty() {
                        marker_loci.push(translated_loci);
                    }
                }
                let Some(first) = marker_loci.first() else {
                    continue;
                };
                if marker_loci.iter().all(|candidate| candidate == first) {
                    if let Some(loci) = marker_loci.into_iter().next() {
                        result.insert(result_key, loci);
                    }
                }
            }
        }
    }

    for marker in markers_by_id.values().copied() {
        if marker.kind() != SketchInputKind::LineOrCircle {
            continue;
        }
        if result.contains_key(marker.id()) {
            continue;
        }
        let endpoints = line_endpoint_markers(marker, &markers_by_id);
        let (Some(feature), [first, second]) =
            (marker.feature_ref.as_deref(), endpoints.as_slice())
        else {
            continue;
        };
        let (Some(sketch_id), Some(first), Some(second)) = (
            sketches_by_feature.get(feature),
            first.coordinates_m,
            second.coordinates_m,
        ) else {
            continue;
        };
        let first_native = quantize(
            Point2::new(first[0] * NATIVE_TO_IR, first[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        let second_native = quantize(
            Point2::new(second[0] * NATIVE_TO_IR, second[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        let candidates = transforms
            .get(feature)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let endpoint_pairs = candidates
            .iter()
            .filter_map(|transform| {
                Some((
                    transform.apply(first_native)?,
                    transform.apply(second_native)?,
                ))
            })
            .collect::<HashSet<_>>();
        if endpoint_pairs.is_empty() {
            continue;
        }
        let mut selected: Option<&SketchEntityId> = None;
        let mut complete = true;
        for (start, end) in endpoint_pairs {
            let mut candidate = None;
            for entity in sketch_entities {
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
                let key = marker.id().to_string();
                let loci = vec![super::transforms::SketchLocusRole::Entity.copy_locus(entity)];
                result.insert(key, loci);
            }
        }
    }
    let entities_by_id = sketch_entities
        .iter()
        .map(|entity| (entity.id(), entity))
        .collect::<HashMap<_, _>>();
    loop {
        let mut additions = Vec::new();
        for marker in markers_by_id.values() {
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
            additions.push((marker.id().to_string(), vec![locus]));
        }
        if additions.is_empty() {
            break;
        }
        for (key, loci) in additions {
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
    if marker.links().len() < 2 {
        return Ok(None);
    }
    let mut groups = Vec::<
        HashMap<GridPoint, Vec<(&SketchEntityId, super::transforms::SketchLocusRole)>>,
    >::new();
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
            let Some(entity) = entities_by_id.get(&entity_id) else {
                return Ok(None);
            };
            if let Some(previous) = sketch {
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
                let point = quantize(point, quantum);
                let loci = endpoints.entry(point).or_default();
                loci.push((entity.id(), role));
            }
        }
        if endpoints.is_empty() {
            return Ok(None);
        }
        groups.push(endpoints);
    }
    let Some(first) = groups.first() else {
        return Ok(None);
    };
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
        for locus in group.get(point).into_iter().flatten() {
            let candidate_key = (locus.0.as_str(), locus.1);
            if selected.is_none_or(|other| candidate_key < (other.0.as_str(), other.1)) {
                selected = Some(*locus);
            }
        }
    }
    let Some((entity, role)) = selected else {
        return Ok(None);
    };
    Ok(Some(role.copy_locus(entity)))
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
    features: &'a [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> HashMap<&'a str, Vec<MarkerTransform>> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = RELATION_GEOMETRY_QUANTUM_MM;

    let mut entities_by_sketch = HashMap::<&SketchId, Vec<&SketchEntity>>::new();
    let mut entities_by_native = HashMap::<(&SketchId, &str), Vec<&SketchEntity>>::new();
    for entity in sketch_entities {
        entities_by_sketch
            .entry(&entity.sketch)
            .or_default()
            .push(entity);
        if let Some(reference) = entity.native_ref.as_deref() {
            entities_by_native
                .entry((&entity.sketch, reference))
                .or_default()
                .push(entity);
        }
    }
    let mut sketches_by_feature = HashMap::<&str, &SketchId>::new();
    for feature in features {
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
        sketches_by_feature.insert(native_ref, sketch);
    }
    let mut result = HashMap::<&str, Vec<MarkerTransform>>::new();
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                let markers = markers_by_feature.entry(feature).or_default();
                markers.push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            let Some((canonical_feature, sketch)) = sketches_by_feature.get_key_value(feature)
            else {
                continue;
            };
            let Some(sketch_entities) = entities_by_sketch.get(*sketch) else {
                continue;
            };
            let mut directly_bound = HashMap::<GridPoint, HashSet<GridPoint>>::new();
            for marker in &markers {
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                for entity in entities_by_native
                    .get(&(*sketch, marker.id()))
                    .into_iter()
                    .flatten()
                {
                    let anchors = match *entity.geometry.definition() {
                        SketchGeometryDefinition::Point { position } => vec![position.get()],
                        _ => marker_geometry_anchors(marker.kind(), &entity.geometry),
                    };
                    for anchor in anchors {
                        insert_compatible_locus(
                            &mut directly_bound,
                            native,
                            quantize(anchor, QUANTUM),
                        );
                    }
                }
            }
            let compatible = |primary_only: bool| -> HashMap<GridPoint, HashSet<GridPoint>> {
                let mut points = HashMap::<GridPoint, HashSet<GridPoint>>::new();
                for marker in &markers {
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
                        if entity.sketch != **sketch {
                            continue;
                        }
                        if primary_only {
                            for (point, _) in
                                sketch_entity_locus_points(entity).into_iter().flatten()
                            {
                                if marker_accepts_locus(marker.kind(), &entity.geometry) {
                                    insert_compatible_locus(
                                        &mut points,
                                        marker_point,
                                        quantize(point, QUANTUM),
                                    );
                                }
                            }
                        } else {
                            for point in marker_geometry_anchors(marker.kind(), &entity.geometry) {
                                insert_compatible_locus(
                                    &mut points,
                                    marker_point,
                                    quantize(point, QUANTUM),
                                );
                            }
                        }
                    }
                }
                points
            };
            let direct = compatible_marker_transform_candidates(&directly_bound);
            let candidates = if direct.len() == 1 {
                direct
            } else {
                let primary_points = compatible(true);
                let primary = compatible_marker_transform_candidates(&primary_points);
                if primary.len() == 1 {
                    primary
                } else {
                    let fallback_points = compatible(false);
                    let fallback = compatible_marker_transform_candidates(&fallback_points);
                    if fallback.is_empty() {
                        primary
                    } else {
                        fallback
                    }
                }
            };
            let candidates =
                if let Some(sketch) = sketches.iter().find(|candidate| candidate.id == **sketch) {
                    marker_transforms_with_frame_fallback(candidates, sketch, QUANTUM)
                } else {
                    candidates
                };
            if !candidates.is_empty() {
                result.insert(*canonical_feature, candidates);
            }
        }
    }
    result
}

fn insert_compatible_locus(
    points: &mut HashMap<GridPoint, HashSet<GridPoint>>,
    marker: GridPoint,
    locus: GridPoint,
) {
    points.entry(marker).or_default().insert(locus);
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
