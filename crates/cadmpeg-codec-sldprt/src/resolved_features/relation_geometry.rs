//! Relation point and solved geometry projection.

use super::curves::{slot_curve_and_center_indices, SlotReferences};
use super::endpoints::{inferred_point_coordinates_by_index, legacy_undetailed_profile_line};
use super::grid::{quantize, GridPoint};
use super::markers::{
    marker_is_geometry_locus, marker_native_code, spatial_relation_marker_coordinates,
};
use super::names::operand_kind_name;
use super::operands::{
    coordinate_line_endpoints_with_linked_point, linked_coordinate_line_endpoints,
};
use super::relation_loci::{
    entity_locus_point, find_profile_entity, line_line_angle, line_line_distance,
    marker_point_locus, marker_transform_candidates_by_feature, marker_transform_candidates_in,
    point_line_distance_value, profile_axis_for_relation, profile_loci_in,
    relation_constraint_is_inactive_in, relation_definition, relation_operand_marker_in,
    same_dimension_angle, same_dimension_length, unoriented_line_line_angle, ProfileEntities,
    RelationIndex,
};
use super::relation_records::{
    circle_dimension_handle_driver, relation_uses_dynamic_operands, relation_uses_solver_points,
};
use super::transforms::{
    marker_entities, sketch_entity_locus_points, sketch_frame_marker_transform, MarkerEntityFilter,
    ProfileAxis,
};
use super::typed_relations::{
    current_undetailed_bounded_curve_is_line, marker_curve_endpoint_markers_in,
    marker_relation_definition, marker_relation_is_inactive_in, CurveMarkers, RelationMarkers,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity,
    SketchInputKind, SketchRelationKind,
};
use cadmpeg_core::convert::f64_from_i64;
use cadmpeg_core::decode::{
    index_from_u64, u64_from_index, DecodeContext, ScopedReservation, View,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchLocus, SketchNativeOperand,
    SpatialSketch, SpatialSketchConstraint, SpatialSketchConstraintDefinitionInput,
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition,
};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) const RELATION_PARAMETER_ID_PROPERTY: &str = "sldprt_relation_id";
pub(super) const RELATION_PARAMETER_ROLE_PROPERTY: &str = "sldprt_relation_parameter_role";
pub(super) const RELATION_PARAMETER_ROLE_REFERENCE: &str = "reference";
pub(super) const RELATION_DISPLAY_SCALAR_ID_PROPERTY: &str = "sldprt_display_scalar_id";

// Identity::mint moves each formatted String and scans its grammar bytes without temporary storage.

pub(crate) fn is_reference_relation_parameter(
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> bool {
    parameter
        .properties
        .get(RELATION_PARAMETER_ROLE_PROPERTY)
        .map(String::as_str)
        == Some(RELATION_PARAMETER_ROLE_REFERENCE)
}

/// Looks up the reference role under decode admission. Writer paths use the
/// context-free predicate above.
pub(super) fn is_reference_relation_parameter_in(
    ctx: &DecodeContext<'_>,
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(ctx
        .get_btree_map(
            &parameter.properties,
            RELATION_PARAMETER_ROLE_PROPERTY,
            "resolve SLDPRT relation parameter role",
        )?
        .map(String::as_str)
        == Some(RELATION_PARAMETER_ROLE_REFERENCE))
}

fn relation_native_kind(family: FeatureInputRelationFamily) -> cadmpeg_core::text::NonBlankString {
    use cadmpeg_core::nonblank_literal;
    match family {
        FeatureInputRelationFamily::LineLineDistance => nonblank_literal!("sgLLDist"),
        FeatureInputRelationFamily::PointPointDistance => nonblank_literal!("sgPntPntDist"),
        FeatureInputRelationFamily::PointLineDistance => nonblank_literal!("sgPntLineDist"),
        FeatureInputRelationFamily::PointPointHorizontalDistance => {
            nonblank_literal!("sgPntPntHorDist")
        }
        FeatureInputRelationFamily::PointPointVerticalDistance => {
            nonblank_literal!("sgPntPntVertDist")
        }
        FeatureInputRelationFamily::Angle => nonblank_literal!("sgAnglDim"),
        FeatureInputRelationFamily::CircleDiameter => nonblank_literal!("sgCircleDim"),
    }
}

fn spatial_point_line_distance(point: Point3, start: Point3, end: Point3) -> Option<f64> {
    let direction = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
    let length = direction.norm();
    if !length.is_finite() || length == 0.0 {
        return None;
    }
    let offset = Vector3::new(point.x - start.x, point.y - start.y, point.z - start.z);
    Some(
        Vector3::new(
            offset.y * direction.z - offset.z * direction.y,
            offset.z * direction.x - offset.x * direction.z,
            offset.x * direction.y - offset.y * direction.x,
        )
        .norm()
            / length,
    )
}

fn ensure_spatial_relation_point(
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SpatialSketchEntity>,
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    marker: &crate::records::SketchInputEntity,
    position: Point3,
) -> Result<Option<SpatialSketchEntityId>, cadmpeg_core::CodecError> {
    let mut remaining = entities.iter();
    let mut next = || {
        ctx.find_by(
            &mut remaining,
            |entity| {
                Ok(ctx.equal(
                    &entity.sketch,
                    sketch,
                    "compare SLDPRT spatial relation point sketches",
                )? && ctx.equal(
                    &entity.native_ref.as_deref(),
                    &Some(marker.id()),
                    "compare SLDPRT spatial relation point markers",
                )?)
            },
            "find existing SLDPRT spatial relation point",
        )
    };
    let first = next()?;
    if next()?.is_some() {
        return Ok(None);
    }
    if first.is_some_and(|entity| {
        !matches!(*entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Point { position: candidate } if candidate == position
        )
    }) {
        return Ok(None);
    }
    if let Some(entity) = first {
        return copy_spatial_entity_id(ctx, entity.id()).map(Some);
    }
    let id_text = ctx.format_retained(
        format_args!("{}:relation-point:{}", sketch.as_str(), marker.offset()),
        "format SLDPRT spatial relation point identity",
    )?;
    ctx.charge_work(
        u64_from_index(id_text.len()),
        "validate SLDPRT spatial relation point identity",
    )?;
    let Ok(id) = SpatialSketchEntityId::mint(id_text) else {
        return Ok(None);
    };
    let entity_id = copy_spatial_entity_id(ctx, &id)?;
    let sketch_id = copy_spatial_sketch_id(ctx, sketch)?;
    let native_ref = ctx.format_retained(
        format_args!("{}", marker.id()),
        "copy SLDPRT spatial relation marker identity",
    )?;
    let Some(geometry) =
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point { position }).ok()
    else {
        return Ok(None);
    };
    ctx.reserve_vec(entities, 1, "append SLDPRT spatial relation point")?;
    entities.push(
        SpatialSketchEntity::new(entity_id, sketch_id, geometry)
            .with_construction(true)
            .with_native_ref(Some(native_ref)),
    );
    Ok(Some(id))
}

fn copy_spatial_entity_id(
    ctx: &DecodeContext<'_>,
    id: &SpatialSketchEntityId,
) -> Result<SpatialSketchEntityId, cadmpeg_core::CodecError> {
    id.try_clone_for_decode(ctx, "copy SLDPRT spatial entity identity")
}

fn copy_spatial_sketch_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::sketches::SpatialSketchId,
) -> Result<cadmpeg_ir::sketches::SpatialSketchId, cadmpeg_core::CodecError> {
    id.try_clone_for_decode(ctx, "copy SLDPRT spatial sketch identity")
}

#[derive(Default)]
struct SpatialRelationMarkerRoster<'a> {
    points: Vec<(&'a SketchInputEntity, Point3)>,
    lines: Vec<(&'a SketchInputEntity, Point3)>,
}

/// Raw spatial carriers grouped by feature, with every occurrence retained.
struct SpatialRelationMarkers<'a> {
    by_feature: BTreeMap<&'a str, SpatialRelationMarkerRoster<'a>>,
}

impl<'a> SpatialRelationMarkers<'a> {
    fn new<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        lane: &'a FeatureInputLane,
    ) -> Result<(Self, ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
        const OPERATION: &str = "index SLDPRT spatial relation markers";
        ctx.with_scoped_storage(OPERATION, || {
            let mut by_feature = BTreeMap::<_, SpatialRelationMarkerRoster<'_>>::new();
            for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                let (Some(feature), Ok(offset)) = (
                    marker.feature_ref.as_deref(),
                    usize::try_from(marker.offset()),
                ) else {
                    continue;
                };
                let Some(code) = marker_native_code(&lane.native_payload, offset) else {
                    continue;
                };
                let point = matches!(code, 2..=5);
                if !(point || code == 0 && marker.object_index().is_some()) {
                    continue;
                }
                let Some(coordinates) =
                    spatial_relation_marker_coordinates(&lane.native_payload, offset)
                else {
                    continue;
                };
                let roster = ctx
                    .entry_btree_map(&mut by_feature, feature, OPERATION)?
                    .or_default();
                let candidates = if point {
                    &mut roster.points
                } else {
                    &mut roster.lines
                };
                ctx.push_vec(candidates, (marker, coordinates), OPERATION)?;
            }
            for (_, roster) in ctx.admit_iter(&mut by_feature, OPERATION)? {
                ctx.sort_unstable_by_key(
                    &mut roster.points,
                    |(marker, _)| marker.offset(),
                    Ord::cmp,
                    OPERATION,
                )?;
                ctx.sort_unstable_by_key(
                    &mut roster.lines,
                    |(marker, _)| marker.offset(),
                    Ord::cmp,
                    OPERATION,
                )?;
            }
            Ok(Self { by_feature })
        })
    }
}

fn spatial_relation_point_line_entities(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    markers: &SpatialRelationMarkers<'_>,
    entities: &mut Vec<SpatialSketchEntity>,
) -> Result<Option<(SpatialSketchEntityId, SpatialSketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(value) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected = match value {
        cadmpeg_ir::features::ParameterValue::Length(length) => length.get().abs(),
        _ => return Ok(None),
    };
    let Some(roster) = ctx.get_btree_map(
        &markers.by_feature,
        relation.feature_ref.as_str(),
        "resolve SLDPRT spatial relation markers",
    )?
    else {
        return Ok(None);
    };
    let point_markers = &roster.points;
    let Some(point_operand) = relation.operands.first() else {
        return Ok(None);
    };
    let point_marker_by_reference = if let Some(entity_ref) = point_operand.entity_ref.as_deref() {
        let mut found_index = None;
        let mut steps = point_markers.iter().enumerate();
        while let Some((index, (marker, _))) =
            ctx.next_charged(&mut steps, "resolve SLDPRT spatial point operand")?
        {
            if ctx.equal(
                marker.id(),
                entity_ref,
                "compare SLDPRT spatial point operand identities",
            )? {
                found_index = Some(index);
                break;
            }
        }
        found_index.and_then(|index| point_markers.get(index))
    } else {
        None
    };
    let Some(point_marker) = point_marker_by_reference
        .or_else(|| point_markers.get(usize::from(point_operand.entity_index)))
    else {
        return Ok(None);
    };
    let (point_marker, point) = *point_marker;

    let line_markers = &roster.lines;
    let mut pairs = line_markers.chunks_exact(2);
    let mut next_match = || {
        ctx.find_map(
            &mut pairs,
            |pair| {
                let ((first_marker, first), (second_marker, second)) = (pair[0], pair[1]);
                Ok((first != second
                    && spatial_point_line_distance(point, first, second)
                        .is_some_and(|distance| same_dimension_length(distance, expected)))
                .then_some((first_marker, first, second_marker, second)))
            },
            "select SLDPRT spatial relation line pair",
        )
    };
    let (Some((start_marker, start, end_marker, end)), None) = (next_match()?, next_match()?)
    else {
        return Ok(None);
    };

    let Some(start_id) = ensure_spatial_relation_point(ctx, entities, sketch, start_marker, start)?
    else {
        return Ok(None);
    };
    let Some(end_id) = ensure_spatial_relation_point(ctx, entities, sketch, end_marker, end)?
    else {
        return Ok(None);
    };
    let Some(point_id) = ensure_spatial_relation_point(ctx, entities, sketch, point_marker, point)?
    else {
        return Ok(None);
    };
    let mut existing_line_id = None;
    let mut steps = entities.as_slice().iter();
    while let Some(entity) =
        ctx.next_charged(&mut steps, "find existing SLDPRT spatial relation line")?
    {
        if !ctx.equal(
            &entity.sketch,
            sketch,
            "compare SLDPRT spatial relation line sketches",
        )? || !matches!(
            *entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Line { .. }
        ) {
            continue;
        }
        let matches_endpoints = match entity.endpoint_refs.as_slice() {
            [first, second] => {
                (ctx.equal(
                    first.as_str(),
                    start_id.as_str(),
                    "compare SLDPRT spatial line endpoints",
                )? && ctx.equal(
                    second.as_str(),
                    end_id.as_str(),
                    "compare SLDPRT spatial line endpoints",
                )?) || (ctx.equal(
                    first.as_str(),
                    end_id.as_str(),
                    "compare SLDPRT spatial line endpoints",
                )? && ctx.equal(
                    second.as_str(),
                    start_id.as_str(),
                    "compare SLDPRT spatial line endpoints",
                )?)
            }
            _ => false,
        };
        if matches_endpoints {
            existing_line_id = Some(entity.id());
            break;
        }
    }
    let line_id = if let Some(line_id) = existing_line_id {
        copy_spatial_entity_id(ctx, line_id)?
    } else {
        let id_text = ctx.format_retained(
            format_args!("{}:relation-line", relation.id),
            "format SLDPRT spatial relation line identity",
        )?;
        ctx.charge_work(
            u64_from_index(id_text.len()),
            "validate SLDPRT spatial relation line identity",
        )?;
        let Ok(id) = SpatialSketchEntityId::mint(id_text) else {
            return Ok(None);
        };
        let entity_id = copy_spatial_entity_id(ctx, &id)?;
        let sketch_id = copy_spatial_sketch_id(ctx, sketch)?;
        let geometry_ref = ctx.format_retained(
            format_args!("{}:relation-line", relation.id),
            "copy SLDPRT spatial relation line reference",
        )?;
        let start_ref = ctx.format_retained(
            format_args!("{}", start_id.as_str()),
            "copy SLDPRT spatial line endpoint",
        )?;
        let end_ref = ctx.format_retained(
            format_args!("{}", end_id.as_str()),
            "copy SLDPRT spatial line endpoint",
        )?;
        let Some(geometry) =
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .ok()
        else {
            return Ok(None);
        };
        ctx.reserve_vec(entities, 1, "append SLDPRT spatial relation line")?;
        entities.push(
            SpatialSketchEntity::new(entity_id, sketch_id, geometry)
                .with_construction(true)
                .with_geometry_ref(Some(geometry_ref))
                .with_endpoint_refs(vec![start_ref, end_ref]),
        );
        id
    };
    Ok(Some((point_id, line_id)))
}

/// Project spatial-feature relation records while retaining native records
/// whenever the source marker roster cannot identify complete neutral geometry.
pub(crate) fn project_spatial_relation_bindings(
    ctx: &DecodeContext<'_>,
    constraints: &mut Vec<SpatialSketchConstraint>,
    entities: &mut Vec<SpatialSketchEntity>,
    sketches: &[SpatialSketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut spatial_sketch_ids_storage =
        ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut spatial_sketch_ids = HashSet::new();
    for sketch in ctx.admit_iter(sketches, "index SLDPRT spatial sketch identities")? {
        let operation = "index SLDPRT spatial sketch identities";
        spatial_sketch_ids_storage
            .with_storage(|| ctx.insert_hash_set(&mut spatial_sketch_ids, &sketch.id, operation))?;
    }
    let mut sketches_by_feature_storage =
        ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut sketches_by_feature = HashMap::new();
    for feature in ctx.admit_iter(features, "index SLDPRT spatial sketches by feature")? {
        let operation = "index SLDPRT spatial sketches by feature";
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::SpatialSketch {
                sketch: Some(sketch),
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        if !ctx.contains_hash_set(
            &spatial_sketch_ids,
            sketch,
            "check indexed SLDPRT spatial sketch identity",
        )? {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        sketches_by_feature_storage.with_storage(|| {
            ctx.insert_hash_map(&mut sketches_by_feature, native_ref, sketch, operation)
        })?;
    }
    let relation_parameters_scope = ctx
        .with_scoped_storage("SLDPRT spatial relation parameter ownership", || {
            owned_relation_parameters(ctx, features, parameters, lanes)
        })?;
    let mut parameters_by_id_storage = ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut parameters_by_id = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "index SLDPRT spatial relation parameters")? {
        let operation = "index SLDPRT spatial relation parameters";
        parameters_by_id_storage.with_storage(|| {
            ctx.insert_hash_map(&mut parameters_by_id, &parameter.id, parameter, operation)
        })?;
    }
    let mut constraints_by_native_ref_storage =
        ctx.reserve_scoped(0, "index SLDPRT spatial relation constraints")?;
    let mut constraints_by_native_ref = HashMap::<String, usize>::new();
    for (index, constraint) in ctx
        .admit_iter(&*constraints, "index SLDPRT spatial relation constraints")?
        .enumerate()
    {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            let operation = "index SLDPRT spatial relation constraints";
            if !ctx.contains_key_hash_map(&constraints_by_native_ref, native_ref, operation)? {
                let key = ctx.format_scoped_text(
                    &mut constraints_by_native_ref_storage,
                    format_args!("{native_ref}"),
                    "copy SLDPRT spatial constraint reference",
                )?;
                constraints_by_native_ref_storage.with_storage(|| {
                    ctx.insert_hash_map(&mut constraints_by_native_ref, key, index, operation)
                })?;
            }
        }
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT spatial relation lanes")? {
        let spatial_markers = std::cell::OnceCell::new();
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "split SLDPRT spatial relation lane identity")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in ctx.admit_iter(
            &lane.relation_instances,
            "scan SLDPRT spatial relation instances",
        )? {
            let Some(sketch) = ctx.get_hash_map(
                &sketches_by_feature,
                relation.feature_ref.as_str(),
                "resolve SLDPRT spatial relation sketch",
            )?
            else {
                continue;
            };
            let parameter_id = ctx
                .get_hash_map(
                    &relation_parameters_scope.0,
                    &relation.id,
                    "resolve SLDPRT spatial relation parameter ownership",
                )?
                .and_then(Option::as_ref);
            let parameter = if let Some(parameter_id) = parameter_id {
                ctx.get_hash_map(
                    &parameters_by_id,
                    parameter_id,
                    "resolve SLDPRT spatial relation parameter",
                )?
                .copied()
            } else {
                None
            };
            let typed_definition =
                if relation.family == FeatureInputRelationFamily::PointLineDistance {
                    if let (Some(parameter), Some(parameter_id)) = (parameter, parameter_id) {
                        let projected = if matches!(
                            parameter.value.as_ref(),
                            Some(cadmpeg_ir::features::ParameterValue::Length(_))
                        ) {
                            let (markers, _) = match spatial_markers.get() {
                                Some(markers) => markers,
                                None => {
                                    let built = SpatialRelationMarkers::new(ctx, lane)?;
                                    spatial_markers.get_or_init(|| built)
                                }
                            };
                            spatial_relation_point_line_entities(
                                ctx, relation, sketch, parameter, markers, entities,
                            )?
                        } else {
                            None
                        };
                        projected.map(|(point, line)| (point, line, parameter_id))
                    } else {
                        None
                    }
                } else {
                    None
                };
            let native_kind = relation_native_kind(relation.family);
            let definition = if let Some((point, line, parameter_id)) = typed_definition {
                SpatialSketchConstraintDefinitionInput::PointLineDistance {
                    point,
                    line,
                    parameter: copy_relation_parameter_id(ctx, parameter_id)?,
                }
            } else {
                let mut operands = Vec::new();
                for operand in ctx.admit_iter(
                    &relation.operands,
                    "collect SLDPRT spatial relation operands",
                )? {
                    let native_ref = operand
                        .entity_ref
                        .as_deref()
                        .map(|reference| {
                            ctx.format_retained(
                                format_args!("{reference}"),
                                "copy SLDPRT spatial relation operand reference",
                            )
                        })
                        .transpose()?;
                    ctx.reserve_vec(&mut operands, 1, "collect SLDPRT spatial relation operands")?;
                    operands.push(SketchNativeOperand {
                        native_kind: operand_kind_name(ctx, operand.kind)?,
                        field: None,
                        object_index: Some(u32::from(operand.entity_index)),
                        native_ref,
                    });
                }
                SpatialSketchConstraintDefinitionInput::Native {
                    native_kind,
                    native_state: None,
                    parameter: parameter_id
                        .map(|id| copy_relation_parameter_id(ctx, id))
                        .transpose()?,
                    operands,
                }
            };
            let Ok(definition) =
                cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(definition)
            else {
                continue;
            };
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:spatial-sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                ),
                "format SLDPRT spatial relation constraint identity",
            )?;
            ctx.charge_work(
                u64_from_index(id_text.len()),
                "validate SLDPRT spatial relation constraint identity",
            )?;
            let projected = SpatialSketchConstraint {
                id: match SketchConstraintId::mint(id_text) {
                    Ok(id) => id,
                    Err(_) => continue,
                },
                sketch: copy_spatial_sketch_id(ctx, sketch)?,
                definition,
                native_ref: Some(ctx.format_retained(
                    format_args!("{}", relation.id),
                    "copy SLDPRT spatial relation reference",
                )?),
            };
            if let Some(index) = ctx
                .get_hash_map(
                    &constraints_by_native_ref,
                    relation.id.as_str(),
                    "find existing SLDPRT spatial relation constraint",
                )?
                .copied()
            {
                if matches!(
                    constraints[index].definition.kind(),
                    SpatialSketchConstraintDefinitionInput::Native { .. }
                ) && !matches!(
                    projected.definition.kind(),
                    SpatialSketchConstraintDefinitionInput::Native { .. }
                ) {
                    constraints[index] = projected;
                }
            } else {
                let operation = "index SLDPRT spatial relation constraints";
                let key = ctx.format_scoped_text(
                    &mut constraints_by_native_ref_storage,
                    format_args!("{}", relation.id),
                    "copy SLDPRT spatial constraint reference",
                )?;
                constraints_by_native_ref_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut constraints_by_native_ref,
                        key,
                        constraints.len(),
                        operation,
                    )
                })?;
                ctx.reserve_vec(constraints, 1, "append SLDPRT spatial relation constraint")?;
                constraints.push(projected);
            }
        }
    }
    Ok(())
}

/// Materialize relation-addressed point geometry omitted from selected profile streams.
pub(crate) fn project_relation_point_geometry(
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketch_records = std::cell::OnceCell::new();
    let mut sketches_by_feature_storage =
        ctx.reserve_scoped(0, "index SLDPRT relation-point sketches")?;
    let mut sketches_by_feature = HashMap::new();
    for feature in ctx.admit_iter(features, "index SLDPRT relation-point sketches")? {
        let operation = "index SLDPRT relation-point sketches";
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
        let sketch_id = sketch;
        sketches_by_feature_storage.with_storage(|| {
            ctx.insert_hash_map(&mut sketches_by_feature, native_ref, sketch_id, operation)
        })?;
    }
    let (transforms, _transforms_storage) = ctx
        .with_scoped_storage("SLDPRT relation transform workspace", || {
            marker_transform_candidates_by_feature(ctx, features, sketches, entities, lanes)
        })?;
    let mut markers_by_id_storage = ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut markers_by_id = HashMap::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT relation-point lanes")? {
        for marker in
            ctx.admit_iter(&lane.sketch_entities, "index SLDPRT relation-point markers")?
        {
            let operation = "index SLDPRT relation-point markers";
            markers_by_id_storage.with_storage(|| {
                ctx.insert_hash_map(&mut markers_by_id, marker.id(), marker, operation)
            })?;
        }
    }
    let mut point_operands = HashSet::new();
    let mut curve_operands = HashSet::new();
    let mut referenced = HashSet::new();
    let mut link_storage = ctx.reserve_scoped(0, "SLDPRT relation marker link workspace")?;
    let mut pending = Vec::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT relation-point operand lanes")? {
        for relation in ctx.admit_iter(
            &lane.relation_instances,
            "scan SLDPRT relation-point operands",
        )? {
            let count = match relation.family {
                FeatureInputRelationFamily::PointPointDistance
                | FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance => 2,
                FeatureInputRelationFamily::PointLineDistance => 1,
                _ => 0,
            };
            for operand in ctx
                .admit_iter(&relation.operands, "index SLDPRT relation-point operands")?
                .take(count)
            {
                let Some(id) = operand.entity_ref.as_deref() else {
                    continue;
                };
                ctx.insert_hash_set(
                    &mut point_operands,
                    id,
                    "index SLDPRT relation-point operands",
                )?;
            }
            let first = match relation.family {
                FeatureInputRelationFamily::LineLineDistance
                | FeatureInputRelationFamily::Angle => 0,
                FeatureInputRelationFamily::PointLineDistance => 1,
                _ => relation.operands.len(),
            };
            for operand in ctx
                .admit_iter(&relation.operands, "index SLDPRT relation-curve operands")?
                .skip(first)
            {
                let Some(id) = operand.entity_ref.as_deref() else {
                    continue;
                };
                ctx.insert_hash_set(
                    &mut curve_operands,
                    id,
                    "index SLDPRT relation-curve operands",
                )?;
            }
            for operand in ctx.admit_iter(
                &relation.operands,
                "index SLDPRT referenced relation operands",
            )? {
                let Some(id) = operand.entity_ref.as_deref() else {
                    continue;
                };
                if ctx.insert_hash_set(
                    &mut referenced,
                    id,
                    "index SLDPRT referenced relation markers",
                )? {
                    link_storage.with_storage(|| {
                        ctx.push_vec(&mut pending, id, "index SLDPRT referenced relation markers")
                    })?;
                }
            }
        }
        for marker in
            ctx.admit_iter(&lane.sketch_entities, "index SLDPRT relation marker roster")?
        {
            if !matches!(marker.kind(), SketchInputKind::Relation(_)) {
                continue;
            }
            let id = marker.id();
            if ctx.insert_hash_set(
                &mut referenced,
                id,
                "index SLDPRT referenced relation markers",
            )? {
                link_storage.with_storage(|| {
                    ctx.push_vec(&mut pending, id, "index SLDPRT referenced relation markers")
                })?;
            }
        }
    }
    // A marker linked to a referenced marker is referenced too, in either
    // direction of the link: walk the closure over the link graph once.
    let mut links_by_marker = HashMap::<&str, Vec<&str>>::new();
    for lane in ctx.admit_iter(lanes, "index SLDPRT relation marker links")? {
        for marker in ctx.admit_iter(&lane.sketch_entities, "index SLDPRT relation marker links")? {
            // A repeated identity keeps the marker the identity index kept.
            if !ctx
                .get_hash_map(
                    &markers_by_id,
                    marker.id(),
                    "index SLDPRT relation marker links",
                )?
                .is_some_and(|kept| std::ptr::eq(*kept, marker))
            {
                continue;
            }
            for link in ctx.admit_iter(marker.links(), "index SLDPRT relation marker links")? {
                for (from, to) in [
                    (marker.id(), link.entity_ref.as_str()),
                    (link.entity_ref.as_str(), marker.id()),
                ] {
                    link_storage.with_storage(|| {
                        ctx.push_hash_group(
                            &mut links_by_marker,
                            from,
                            to,
                            "index SLDPRT relation marker links",
                            "index SLDPRT relation marker links",
                        )
                    })?;
                }
            }
        }
    }
    loop {
        ctx.charge_work(1, "expand SLDPRT referenced relation markers")?;
        let Some(id) = pending.pop() else {
            break;
        };
        let Some(adjacent) = ctx.get_hash_map(
            &links_by_marker,
            id,
            "expand SLDPRT referenced relation markers",
        )?
        else {
            continue;
        };
        for &adjacent in ctx.admit_iter(adjacent, "expand SLDPRT referenced relation markers")? {
            if ctx.insert_hash_set(
                &mut referenced,
                adjacent,
                "index SLDPRT referenced relation markers",
            )? {
                link_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut pending,
                        adjacent,
                        "expand SLDPRT referenced relation markers",
                    )
                })?;
            }
        }
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT relation geometry lanes")? {
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "split SLDPRT solved-point lane identity")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        for marker in ctx.admit_iter(
            &lane.sketch_entities,
            "scan SLDPRT relation geometry markers",
        )? {
            let qualified_point = ctx.contains_hash_set(
                &point_operands,
                marker.id(),
                "check SLDPRT relation point operand",
            )?;
            let mut has_existing_point = false;
            let mut steps = entities.iter();
            while let Some(entity) =
                ctx.next_charged(&mut steps, "check existing SLDPRT relation points")?
            {
                if !matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                ) {
                    continue;
                }
                if ctx.equal(
                    &entity.native_ref.as_deref(),
                    &Some(marker.id()),
                    "match SLDPRT relation point native reference",
                )? || ctx.equal(
                    &entity.geometry_ref.as_deref(),
                    &Some(marker.id()),
                    "match SLDPRT relation point geometry reference",
                )? {
                    has_existing_point = true;
                    break;
                }
            }
            if !ctx.contains_hash_set(
                &referenced,
                marker.id(),
                "check SLDPRT relation point reference",
            )? || !(qualified_point
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
                || matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                ))
                || has_existing_point
            {
                continue;
            }
            let has_endpoint_reference = ctx.any_by(
                &*entities,
                |entity| {
                    ctx.any_by(
                        &entity.endpoint_refs,
                        |reference| {
                            ctx.equal(
                                reference.as_str(),
                                marker.id(),
                                "match SLDPRT relation endpoint reference",
                            )
                        },
                        "check SLDPRT relation point endpoint references",
                    )
                },
                "check SLDPRT relation point endpoint references",
            )?;
            if has_endpoint_reference {
                continue;
            }
            let (Some(feature), Some([u, v])) = (
                marker.feature_ref.as_deref(),
                marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get),
            ) else {
                continue;
            };
            let Some(sketch) = ctx
                .get_hash_map(
                    &sketches_by_feature,
                    feature,
                    "resolve SLDPRT relation point sketch",
                )?
                .copied()
            else {
                continue;
            };
            if ctx.contains_text(
                sketch.as_str(),
                "sketch#compact:",
                "check compact SLDPRT sketch identity",
            )? && index_from_u64(marker.offset())
                .is_none_or(|offset| !marker_is_geometry_locus(&lane.native_payload, offset))
            {
                continue;
            }
            let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
            let mut unique_position = None;
            let mut ambiguous = false;
            if let Some(transforms) = ctx.get_hash_map(
                &transforms,
                feature,
                "resolve SLDPRT relation point transforms",
            )? {
                for transform in
                    ctx.admit_iter(transforms, "scan SLDPRT relation-point transforms")?
                {
                    let Some(position) = transform.apply(native) else {
                        continue;
                    };
                    if unique_position.is_some_and(|previous| previous != position) {
                        ambiguous = true;
                    } else if unique_position.is_none() {
                        unique_position = Some(position);
                    }
                }
            }
            let position = if ambiguous || unique_position.is_none() {
                let (records, _) = match sketch_records.get() {
                    Some(records) => records,
                    None => {
                        let built = relation_sketch_records(ctx, sketches)?;
                        sketch_records.get_or_init(|| built)
                    }
                };
                let selected_sketch = ctx
                    .get_hash_map(records, sketch, "find SLDPRT relation-point sketch frame")?
                    .copied();
                selected_sketch
                    .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                    .and_then(|transform| transform.apply(native))
            } else {
                unique_position
            };
            let Some(position) = position else {
                continue;
            };
            let (Some(position_u), Some(position_v)) =
                (f64_from_i64(position.0), f64_from_i64(position.1))
            else {
                continue;
            };
            let position = Point2::new(position_u * QUANTUM, position_v * QUANTUM);
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-entity#relation-point:{lane_key}:{}",
                    marker.offset()
                ),
                "format SLDPRT relation-point entity identity",
            )?;
            ctx.charge_work(
                u64_from_index(id_text.len()),
                "validate SLDPRT relation-point entity identity",
            )?;
            let Ok(id) = SketchEntityId::mint(id_text) else {
                continue;
            };
            let Ok(geometry) =
                SketchGeometry::try_from(SketchGeometryDefinition::Point { position })
            else {
                continue;
            };
            let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
            let native_ref = if matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                Some(ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT relation-point native reference",
                )?)
            } else {
                None
            };
            let geometry_ref = if qualified_point
                && matches!(
                    marker.kind(),
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                ) {
                Some(ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT relation-point geometry reference",
                )?)
            } else {
                None
            };
            ctx.reserve_vec(entities, 1, "append SLDPRT relation point")?;
            entities.push(
                SketchEntity::new(id, sketch_id, geometry)
                    .with_construction(true)
                    .with_native_ref(native_ref)
                    .with_geometry_ref(geometry_ref),
            );
        }
        let mut markers_by_id_storage =
            ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
        let mut markers_by_id = HashMap::new();
        let mut marker_roster_storage =
            ctx.reserve_scoped(0, "collect SLDPRT relation-line marker roster")?;
        let mut marker_roster = Vec::new();
        for marker in ctx.admit_iter(&lane.sketch_entities, "index SLDPRT relation-line markers")? {
            let operation = "index SLDPRT relation-line markers";
            markers_by_id_storage.with_storage(|| {
                ctx.insert_hash_map(&mut markers_by_id, marker.id(), marker, operation)
            })?;
            marker_roster_storage
                .with_storage(|| ctx.push_vec(&mut marker_roster, marker, operation))?;
        }
        let curve_markers = std::cell::OnceCell::new();
        let geometry =
            crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                ctx,
                &marker_roster,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    ctx,
                    &lane.native_payload,
                )?,
            )?;
        for marker in ctx.admit_iter(&lane.sketch_entities, "scan SLDPRT relation-line markers")? {
            let marker_offset = usize::try_from(marker.offset()).ok();
            let undetailed_arc_line = marker.kind() == SketchInputKind::Arc
                && marker_offset.is_some_and(|offset| {
                    current_undetailed_bounded_curve_is_line(&lane.native_payload, offset)
                        || legacy_undetailed_profile_line(&lane.native_payload, offset)
                });
            let curve_is_operand = ctx.contains_hash_set(
                &curve_operands,
                marker.id(),
                "check SLDPRT relation curve operand",
            )?;
            let mut self_linked = false;
            let mut steps = marker.links().iter();
            while let Some(link) =
                ctx.next_charged(&mut steps, "check self-linked SLDPRT curve handle")?
            {
                if ctx.equal(
                    link.entity_ref.as_str(),
                    marker.id(),
                    "match SLDPRT self-linked curve handle",
                )? {
                    self_linked = true;
                    break;
                }
            }
            let self_linked_curve_handle =
                curve_is_operand && marker.coordinates_m.is_some() && self_linked && {
                    let mut endpoints = 0;
                    let mut steps = marker.links().iter();
                    while let Some(link) =
                        ctx.next_charged(&mut steps, "count linked SLDPRT curve endpoints")?
                    {
                        if ctx.equal(
                            link.entity_ref.as_str(),
                            marker.id(),
                            "exclude SLDPRT self-linked curve handle",
                        )? {
                            continue;
                        }
                        let Some(linked) = ctx.get_hash_map(
                            &markers_by_id,
                            link.entity_ref.as_str(),
                            "resolve linked SLDPRT curve endpoint",
                        )?
                        else {
                            continue;
                        };
                        if linked.coordinates_m.is_some() {
                            endpoints += 1;
                            if endpoints == 2 {
                                break;
                            }
                        }
                    }
                    endpoints == 1
                };
            let linked_curve_handle = curve_is_operand
                && !self_linked
                && (linked_coordinate_line_endpoints(ctx, marker, &markers_by_id)?.is_some()
                    || coordinate_line_endpoints_with_linked_point(ctx, marker, &markers_by_id)?
                        .is_some());
            if !ctx.contains_hash_set(
                &referenced,
                marker.id(),
                "check SLDPRT relation-line marker reference",
            )? || !(marker.kind() == SketchInputKind::LineOrCircle
                || undetailed_arc_line
                || self_linked_curve_handle
                || linked_curve_handle)
                || {
                    let mut present = false;
                    let mut steps = entities.iter();
                    while let Some(entity) =
                        ctx.next_charged(&mut steps, "check existing SLDPRT relation line handle")?
                    {
                        if ctx.equal(
                            &entity.native_ref.as_deref(),
                            &Some(marker.id()),
                            "match existing SLDPRT relation line handle",
                        )? {
                            present = true;
                            break;
                        }
                    }
                    present
                }
            {
                continue;
            }
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            let Some(sketch) = ctx
                .get_hash_map(
                    &sketches_by_feature,
                    feature,
                    "resolve SLDPRT relation line sketch",
                )?
                .copied()
            else {
                continue;
            };
            let (curve_index, _) = match curve_markers.get() {
                Some(index) => index,
                None => {
                    let built = CurveMarkers::new(ctx, &marker_roster)?;
                    curve_markers.get_or_init(|| built)
                }
            };
            let (mut endpoints, mut endpoints_storage) =
                ctx.with_scoped_storage("SLDPRT relation-line endpoint workspace", || {
                    marker_curve_endpoint_markers_in(
                        ctx,
                        &lane.native_payload,
                        marker,
                        &markers_by_id,
                        curve_index,
                        &geometry,
                    )
                })?;
            if endpoints.len() != 2 && linked_curve_handle {
                let linked_endpoints =
                    match linked_coordinate_line_endpoints(ctx, marker, &markers_by_id)? {
                        Some(endpoints) => Some(endpoints),
                        None => coordinate_line_endpoints_with_linked_point(
                            ctx,
                            marker,
                            &markers_by_id,
                        )?,
                    };
                endpoints = endpoints_storage.with_storage(|| {
                    Ok::<_, cadmpeg_core::CodecError>(match linked_endpoints {
                        Some(linked_endpoints) => ctx.collect_vec(
                            linked_endpoints,
                            "collect SLDPRT linked relation-line endpoints",
                        )?,
                        None => ctx.collect_vec(
                            std::iter::empty(),
                            "collect SLDPRT linked relation-line endpoints",
                        )?,
                    })
                })?;
            }
            if endpoints.len() != 2 {
                ctx.clear_vec(&mut endpoints, "clear SLDPRT relation-line endpoints")?;
                if self_linked_curve_handle {
                    endpoints_storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut endpoints,
                            1,
                            "collect SLDPRT relation-line fallback endpoints",
                        )
                    })?;
                    endpoints.push(marker);
                }
                for link in
                    ctx.admit_iter(marker.links(), "scan SLDPRT relation-line fallback links")?
                {
                    let Some(endpoint) = ctx
                        .get_hash_map(
                            &markers_by_id,
                            link.entity_ref.as_str(),
                            "resolve SLDPRT relation-line endpoint",
                        )?
                        .copied()
                    else {
                        continue;
                    };
                    if ctx.equal(
                        endpoint.id(),
                        marker.id(),
                        "exclude SLDPRT relation-line self endpoint",
                    )? || !ctx.equal(
                        &endpoint.feature_ref.as_deref(),
                        &marker.feature_ref.as_deref(),
                        "match SLDPRT relation-line endpoint feature",
                    )? || endpoint.coordinates_m.is_none()
                    {
                        continue;
                    }
                    let mut already_present_point = false;
                    let mut steps = entities.iter_mut();
                    while let Some(entity) = ctx.next_charged(
                        &mut steps,
                        "find existing SLDPRT relation-line endpoint point",
                    )? {
                        if !ctx.equal(
                            &entity.sketch,
                            sketch,
                            "match SLDPRT relation-line endpoint sketch",
                        )? || !matches!(
                            *entity.geometry.definition(),
                            SketchGeometryDefinition::Point { .. }
                        ) {
                            continue;
                        }
                        if ctx.equal(
                            &entity.native_ref.as_deref(),
                            &Some(endpoint.id()),
                            "match SLDPRT relation-line endpoint native reference",
                        )? || ctx.equal(
                            &entity.geometry_ref.as_deref(),
                            &Some(endpoint.id()),
                            "match SLDPRT relation-line endpoint geometry reference",
                        )? {
                            already_present_point = true;
                            break;
                        }
                    }
                    if !already_present_point {
                        continue;
                    }
                    endpoints_storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut endpoints,
                            1,
                            "collect SLDPRT relation-line fallback endpoints",
                        )
                    })?;
                    endpoints.push(endpoint);
                }
                ctx.sort_unstable_by_key(
                    &mut endpoints,
                    |value| value.offset(),
                    Ord::cmp,
                    "sort SLDPRT relation-line fallback endpoints",
                )?;
                ctx.dedup_by_key(
                    &mut endpoints,
                    |endpoint| Ok(endpoint.id()),
                    "deduplicate SLDPRT relation-line endpoints",
                )?;
            }
            let [first_marker, second_marker] = endpoints.as_slice() else {
                continue;
            };
            let (Some(first), Some(second)) =
                (first_marker.coordinates_m, second_marker.coordinates_m)
            else {
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
            let mut unique_candidate = None;
            let mut ambiguous = false;
            if let Some(transforms) = ctx.get_hash_map(
                &transforms,
                feature,
                "resolve SLDPRT relation-line transforms",
            )? {
                for transform in
                    ctx.admit_iter(transforms, "scan SLDPRT relation-line transforms")?
                {
                    let (Some(start), Some(end)) = (
                        transform.apply(first_native),
                        transform.apply(second_native),
                    ) else {
                        continue;
                    };
                    let candidate = (start, end);
                    if unique_candidate.is_some_and(|previous| previous != candidate) {
                        ambiguous = true;
                    } else if unique_candidate.is_none() {
                        unique_candidate = Some(candidate);
                    }
                }
            }
            let Some((start, end)) = unique_candidate.filter(|_| !ambiguous) else {
                continue;
            };
            if start == end {
                continue;
            }
            let (Some(start_u), Some(start_v), Some(end_u), Some(end_v)) = (
                f64_from_i64(start.0),
                f64_from_i64(start.1),
                f64_from_i64(end.0),
                f64_from_i64(end.1),
            ) else {
                continue;
            };
            let start = Point2::new(start_u * QUANTUM, start_v * QUANTUM);
            let end = Point2::new(end_u * QUANTUM, end_v * QUANTUM);
            let mut already_present = false;
            let mut steps = entities.iter();
            while let Some(entity) =
                ctx.next_charged(&mut steps, "check existing SLDPRT relation lines")?
            {
                if !ctx.equal(
                    &entity.sketch,
                    sketch,
                    "match SLDPRT existing relation line sketch",
                )? {
                    continue;
                }
                let matches_line = match entity.geometry.definition() {
                    SketchGeometryDefinition::Line {
                        start: existing_start,
                        end: existing_end,
                    } => {
                        (quantize(existing_start.get(), QUANTUM) == quantize(start, QUANTUM)
                            && quantize(existing_end.get(), QUANTUM) == quantize(end, QUANTUM))
                            || (quantize(existing_start.get(), QUANTUM) == quantize(end, QUANTUM)
                                && quantize(existing_end.get(), QUANTUM)
                                    == quantize(start, QUANTUM))
                    }
                    _ => false,
                };
                if matches_line {
                    already_present = true;
                    break;
                }
            }
            if already_present {
                continue;
            }
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-entity#relation-line:{lane_key}:{}",
                    marker.offset()
                ),
                "format SLDPRT relation-line entity identity",
            )?;
            ctx.charge_work(
                u64_from_index(id_text.len()),
                "validate SLDPRT relation-line entity identity",
            )?;
            let Ok(id) = SketchEntityId::mint(id_text) else {
                continue;
            };
            let Ok(geometry) =
                SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
            else {
                continue;
            };
            let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
            let native_ref = if matches!(marker.kind(), SketchInputKind::Relation(_)) {
                None
            } else {
                Some(ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT relation-line native reference",
                )?)
            };
            let geometry_ref = if matches!(marker.kind(), SketchInputKind::Relation(_)) {
                Some(ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT relation-line geometry reference",
                )?)
            } else {
                None
            };
            let endpoint_refs = vec![
                ctx.format_retained(
                    format_args!("{}", first_marker.id()),
                    "copy SLDPRT relation-line first endpoint reference",
                )?,
                ctx.format_retained(
                    format_args!("{}", second_marker.id()),
                    "copy SLDPRT relation-line second endpoint reference",
                )?,
            ];
            ctx.reserve_vec(entities, 1, "append SLDPRT relation line")?;
            entities.push(
                SketchEntity::new(id, sketch_id, geometry)
                    .with_construction(true)
                    .with_native_ref(native_ref)
                    .with_geometry_ref(geometry_ref)
                    .with_endpoint_refs(endpoint_refs),
            );
        }
    }
    Ok(())
}

fn indexed_geometry_ref_matches(
    ctx: &DecodeContext<'_>,
    value: &str,
    owner: &str,
    kind: &str,
    index: usize,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(rest) = ctx.strip_prefix(value, owner, "match SLDPRT indexed geometry owner")? else {
        return Ok(false);
    };
    let Some(suffix) = ctx.strip_prefix(rest, kind, "match SLDPRT indexed geometry kind")? else {
        return Ok(false);
    };
    if suffix.is_empty() || (suffix != "0" && suffix.starts_with('0')) {
        return Ok(false);
    }
    if !ctx.all_by(
        suffix.as_bytes(),
        |digit| Ok(digit.is_ascii_digit()),
        "scan SLDPRT indexed geometry reference digits",
    )? {
        return Ok(false);
    }
    Ok(ctx.parse_text::<usize>(suffix, "parse SLDPRT indexed geometry reference")? == Ok(index))
}

pub(super) fn solver_line_geometry_ref_matches(
    ctx: &DecodeContext<'_>,
    value: &str,
    feature: &str,
    index: u16,
) -> Result<bool, cadmpeg_core::CodecError> {
    indexed_geometry_ref_matches(ctx, value, feature, ":solver-line:", usize::from(index))
}

fn is_solver_line_operand(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::E1 | FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7)
    )
}

pub(super) fn relation_uses_solver_line_operand(
    relation: &FeatureInputRelationInstance,
    index: usize,
) -> bool {
    let Some(operand) = relation.operands.get(index) else {
        return false;
    };
    is_solver_line_operand(operand.kind)
        || (relation_uses_dynamic_operands(relation)
            && matches!(
                (relation.family, index),
                (
                    FeatureInputRelationFamily::LineLineDistance
                        | FeatureInputRelationFamily::Angle,
                    0 | 1
                ) | (FeatureInputRelationFamily::PointLineDistance, 1)
            ))
}

pub(crate) fn project_relation_solved_line_geometry(
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketch_records = std::cell::OnceCell::new();
    let mut sketches_by_feature_storage =
        ctx.reserve_scoped(0, "index SLDPRT solved-line sketches")?;
    let mut sketches_by_feature = HashMap::new();
    for feature in ctx.admit_iter(features, "index SLDPRT solved-line sketches")? {
        let operation = "index SLDPRT solved-line sketches";
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
        let sketch_id = sketch;
        sketches_by_feature_storage.with_storage(|| {
            ctx.insert_hash_map(&mut sketches_by_feature, native_ref, sketch_id, operation)
        })?;
    }
    let ownership_scope = ctx
        .with_scoped_storage("SLDPRT solved-line relation parameter ownership", || {
            owned_relation_parameters(ctx, features, parameters, lanes)
        })?;
    let mut parameters_by_id_storage = ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut parameters_by_id = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "index SLDPRT solved-line parameters")? {
        let operation = "index SLDPRT solved-line parameters";
        parameters_by_id_storage.with_storage(|| {
            ctx.insert_hash_map(&mut parameters_by_id, &parameter.id, parameter, operation)
        })?;
    }
    let (transforms, _transforms_storage) = ctx
        .with_scoped_storage("SLDPRT relation transform workspace", || {
            marker_transform_candidates_by_feature(ctx, features, sketches, entities, lanes)
        })?;
    let (markers, _markers_storage) = ctx
        .with_scoped_storage("index SLDPRT solved-line markers", || {
            RelationMarkers::new(ctx, lanes)
        })?;
    let markers_by_id = markers.by_id();

    for lane in ctx.admit_iter(lanes, "scan SLDPRT solved-line lanes")? {
        let ((marker_roster, lane_markers_by_id), _roster_storage) =
            ctx.with_scoped_storage("collect SLDPRT solved-line marker roster", || {
                const OPERATION: &str = "collect SLDPRT solved-line marker roster";
                let mut roster = Vec::new();
                let mut by_id = HashMap::new();
                for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                    ctx.push_vec(&mut roster, marker, OPERATION)?;
                    ctx.entry_hash_map(&mut by_id, marker.id(), OPERATION)?
                        .or_insert(marker);
                }
                Ok::<_, cadmpeg_core::CodecError>((roster, by_id))
            })?;
        let curve_markers = std::cell::OnceCell::new();
        let point_rosters = std::cell::OnceCell::new();
        let geometry =
            crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                ctx,
                &marker_roster,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    ctx,
                    &lane.native_payload,
                )?,
            )?;
        for relation in ctx.admit_iter(
            &lane.relation_instances,
            "scan SLDPRT solved-line relations",
        )? {
            let [first_operand, second_operand] = relation.operands.as_slice() else {
                continue;
            };
            let Some(sketch) = ctx
                .get_hash_map(
                    &sketches_by_feature,
                    relation.feature_ref.as_str(),
                    "resolve SLDPRT solved-line sketch",
                )?
                .copied()
            else {
                continue;
            };
            let direct_line_reference =
                |operand: &FeatureInputOperand| -> Result<bool, cadmpeg_core::CodecError> {
                    let Some(entity_ref) = operand.entity_ref.as_deref() else {
                        return Ok(false);
                    };
                    let mut remaining = entities.iter();
                    let mut next = || {
                        ctx.find_by(
                            &mut remaining,
                            |entity| {
                                Ok(ctx.equal(
                                    &entity.sketch,
                                    sketch,
                                    "match SLDPRT direct relation line sketch",
                                )? && ctx.equal(
                                    &entity.native_ref.as_deref(),
                                    &Some(entity_ref),
                                    "match SLDPRT direct relation line reference",
                                )? && matches!(
                                    *entity.geometry.definition(),
                                    SketchGeometryDefinition::Line { .. }
                                ))
                            },
                            "find direct SLDPRT relation line",
                        )
                    };
                    Ok(next()?.is_some() && next()?.is_none())
                };
            let line_operands = match relation.family {
                FeatureInputRelationFamily::LineLineDistance
                    if relation_uses_solver_line_operand(relation, 0)
                        && relation_uses_solver_line_operand(relation, 1)
                        && first_operand.entity_index != second_operand.entity_index =>
                {
                    let first_is_direct = direct_line_reference(first_operand)?;
                    let second_is_direct = direct_line_reference(second_operand)?;
                    [first_operand, second_operand]
                        .into_iter()
                        .enumerate()
                        .filter(|(index, _)| {
                            if *index == 0 {
                                !first_is_direct
                            } else {
                                !second_is_direct
                            }
                        })
                        .collect::<Vec<_>>()
                }
                FeatureInputRelationFamily::PointLineDistance
                    if relation_uses_solver_line_operand(relation, 1)
                        && !direct_line_reference(second_operand)? =>
                {
                    vec![(1, second_operand)]
                }
                FeatureInputRelationFamily::Angle
                    if relation_uses_solver_line_operand(relation, 0)
                        && relation_uses_solver_line_operand(relation, 1)
                        && first_operand.entity_index != second_operand.entity_index =>
                {
                    let first_is_direct = direct_line_reference(first_operand)?;
                    let second_is_direct = direct_line_reference(second_operand)?;
                    [first_operand, second_operand]
                        .into_iter()
                        .enumerate()
                        .filter(|(index, _)| {
                            if *index == 0 {
                                !first_is_direct
                            } else {
                                !second_is_direct
                            }
                        })
                        .collect::<Vec<_>>()
                }
                _ => continue,
            };
            if line_operands.is_empty() {
                continue;
            }
            let parameter_value = ctx
                .get_hash_map(
                    &ownership_scope.0,
                    &relation.id,
                    "resolve SLDPRT solved-line parameter ownership",
                )?
                .and_then(Option::as_ref)
                .map(|parameter| {
                    ctx.get_hash_map(
                        &parameters_by_id,
                        parameter,
                        "resolve SLDPRT solved-line parameter",
                    )
                })
                .transpose()?
                .flatten()
                .and_then(|parameter| parameter.value.as_ref());
            let Some(parameter_value) = parameter_value else {
                continue;
            };
            let expected = match (relation.family, parameter_value) {
                (
                    FeatureInputRelationFamily::LineLineDistance
                    | FeatureInputRelationFamily::PointLineDistance,
                    cadmpeg_ir::features::ParameterValue::Length(expected),
                ) => expected.get(),
                (
                    FeatureInputRelationFamily::Angle,
                    cadmpeg_ir::features::ParameterValue::Angle(expected),
                ) => expected.get(),
                _ => continue,
            };
            if !expected.is_finite() || expected < 0.0 {
                continue;
            }
            let (point_rosters, _) = match point_rosters.get() {
                Some(rosters) => rosters,
                None => {
                    const OPERATION: &str = "index SLDPRT solved-line point markers";
                    let built = ctx.with_scoped_storage(OPERATION, || {
                        let mut rosters = BTreeMap::new();
                        for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                            if marker.coordinates_m.is_some()
                                && matches!(
                                    marker.kind(),
                                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                                )
                            {
                                ctx.push_btree_group(
                                    &mut rosters,
                                    marker.feature_ref.as_deref(),
                                    marker,
                                    OPERATION,
                                    OPERATION,
                                )?;
                            }
                        }
                        for (_, points) in ctx.admit_iter(&mut rosters, OPERATION)? {
                            ctx.stable_sort_by_key(
                                points,
                                |marker| marker.offset(),
                                Ord::cmp,
                                OPERATION,
                            )?;
                        }
                        Ok::<_, cadmpeg_core::CodecError>(rosters)
                    })?;
                    point_rosters.get_or_init(|| built)
                }
            };
            let points = ctx
                .get_btree_map(
                    point_rosters,
                    &Some(relation.feature_ref.as_str()),
                    "resolve SLDPRT solved-line point markers",
                )?
                .map_or(&[][..], Vec::as_slice);
            let endpoint_line_markers = |operand_index: usize| -> Result<
                Option<[&SketchInputEntity; 2]>,
                cadmpeg_core::CodecError,
            > {
                let marker_id =
                    relation_operand_marker_in(ctx, relation, operand_index, sketch, &markers)?;
                let marker = if let Some(marker_id) = marker_id {
                    ctx.get_hash_map(
                        &lane_markers_by_id,
                        marker_id,
                        "resolve SLDPRT solved-line endpoint marker",
                    )?
                    .copied()
                } else {
                    None
                };
                let Some(marker) = marker else {
                    return Ok(None);
                };
                let (curve_index, _) = match curve_markers.get() {
                    Some(index) => index,
                    None => {
                        let built = CurveMarkers::new(ctx, &marker_roster)?;
                        curve_markers.get_or_init(|| built)
                    }
                };
                let (endpoints, _endpoint_storage) =
                    ctx.with_scoped_storage("SLDPRT solved-line endpoint workspace", || {
                        marker_curve_endpoint_markers_in(
                            ctx,
                            &lane.native_payload,
                            marker,
                            markers_by_id,
                            curve_index,
                            &geometry,
                        )
                    })?;
                let [first, second] = endpoints.as_slice() else {
                    return Ok(None);
                };
                Ok(Some([*first, *second]))
            };
            let fallback_line_markers = |index: u16| {
                let pair = usize::from(index).checked_mul(2)?;
                Some([*points.get(pair)?, *points.get(pair + 1)?])
            };
            let point_marker = if relation.family == FeatureInputRelationFamily::PointLineDistance {
                let explicit = if let Some(id) = first_operand.entity_ref.as_deref() {
                    ctx.get_hash_map(
                        &lane_markers_by_id,
                        id,
                        "resolve explicit SLDPRT point operand marker",
                    )?
                    .copied()
                } else {
                    None
                };
                let resolved = match explicit {
                    Some(marker) => Some(marker),
                    None => {
                        let marker_id =
                            relation_operand_marker_in(ctx, relation, 0, sketch, &markers)?;
                        if let Some(marker_id) = marker_id {
                            ctx.get_hash_map(
                                &lane_markers_by_id,
                                marker_id,
                                "resolve SLDPRT point operand marker",
                            )?
                            .copied()
                        } else {
                            None
                        }
                    }
                };
                resolved.or_else(|| {
                    (first_operand.kind
                        == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD))
                    .then(|| points.get(usize::from(first_operand.entity_index)).copied())
                    .flatten()
                })
            } else {
                None
            };
            let point_position = (|| -> Result<Option<Point2>, cadmpeg_core::CodecError> {
                let Some(marker) = point_marker else {
                    return Ok(None);
                };
                let mut resolved = None;
                let mut steps = entities.iter();
                while let Some(entity) =
                    ctx.next_charged(&mut steps, "find SLDPRT point operand geometry")?
                {
                    if !ctx.equal(
                        &entity.sketch,
                        sketch,
                        "match SLDPRT point operand geometry sketch",
                    )? || !ctx.equal(
                        &entity.native_ref.as_deref(),
                        &Some(marker.id()),
                        "match SLDPRT point operand native reference",
                    )? {
                        continue;
                    }
                    if let SketchGeometryDefinition::Point { position } =
                        entity.geometry.definition()
                    {
                        resolved = Some(position.get());
                    }
                    break;
                }
                if resolved.is_some() {
                    return Ok(resolved);
                }
                let Some([u, v]) = marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    return Ok(None);
                };
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let mut unique_position = None;
                let mut ambiguous = false;
                if let Some(transforms) = ctx.get_hash_map(
                    &transforms,
                    relation.feature_ref.as_str(),
                    "resolve SLDPRT solved-line point transforms",
                )? {
                    for transform in
                        ctx.admit_iter(transforms, "scan SLDPRT solved-line point transforms")?
                    {
                        let Some(position) = transform.apply(native) else {
                            continue;
                        };
                        if unique_position.is_some_and(|previous| previous != position) {
                            ambiguous = true;
                        } else if unique_position.is_none() {
                            unique_position = Some(position);
                        }
                    }
                }
                let position = if ambiguous || unique_position.is_none() {
                    let (records, _) = match sketch_records.get() {
                        Some(records) => records,
                        None => {
                            let built = relation_sketch_records(ctx, sketches)?;
                            sketch_records.get_or_init(|| built)
                        }
                    };
                    let selected_sketch = ctx
                        .get_hash_map(records, sketch, "find SLDPRT solved-line sketch frame")?
                        .copied();
                    selected_sketch
                        .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                        .and_then(|transform| transform.apply(native))
                } else {
                    unique_position
                };
                let Some(position) = position else {
                    return Ok(None);
                };
                let (Some(position_u), Some(position_v)) =
                    (f64_from_i64(position.0), f64_from_i64(position.1))
                else {
                    return Ok(None);
                };
                Ok(Some(Point2::new(
                    position_u * QUANTUM,
                    position_v * QUANTUM,
                )))
            })()?;
            let candidate = |start,
                             end|
             -> Result<Option<SketchEntity>, cadmpeg_core::CodecError> {
                let Ok(id) = SketchEntityId::mint("sldprt:model:sketch-entity#solver-line") else {
                    return Ok(None);
                };
                let Ok(geometry) =
                    SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
                else {
                    return Ok(None);
                };
                let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
                Ok(Some(
                    SketchEntity::new(id, sketch_id, geometry).with_construction(true),
                ))
            };
            let transformed_line = |markers: [&SketchInputEntity; 2]| -> Result<
                Option<(Point2, Point2)>,
                cadmpeg_core::CodecError,
            > {
                let native = markers.map(|marker| {
                    marker.coordinates_m.map(|coordinates| {
                        let [u, v] = coordinates.get();
                        quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM)
                    })
                });
                let [Some(first), Some(second)] = native else {
                    return Ok(None);
                };
                let transform_candidates = ctx
                    .get_hash_map(
                        &transforms,
                        relation.feature_ref.as_str(),
                        "resolve SLDPRT solved-line fallback transforms",
                    )?
                    .map_or(&[][..], Vec::as_slice);
                let fallback = if transform_candidates.is_empty() {
                    let (records, _) = match sketch_records.get() {
                        Some(records) => records,
                        None => {
                            let built = relation_sketch_records(ctx, sketches)?;
                            sketch_records.get_or_init(|| built)
                        }
                    };
                    let selected_sketch = ctx
                        .get_hash_map(records, sketch, "find SLDPRT solved-line fallback sketch")?
                        .copied();
                    selected_sketch
                        .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                } else {
                    None
                };
                let mut candidates_storage =
                    ctx.reserve_scoped(0, "SLDPRT solved-line transform candidates")?;
                let mut candidates = Vec::new();
                for transform in ctx
                    .admit_iter(transform_candidates, "scan SLDPRT solved-line transforms")?
                    .chain(fallback.iter())
                {
                    let (Some(start), Some(end)) =
                        (transform.apply(first), transform.apply(second))
                    else {
                        continue;
                    };
                    let pair = (start, end);
                    if start != end
                        && !ctx.contains(
                            &candidates,
                            &pair,
                            "match SLDPRT solved-line transform candidates",
                        )?
                    {
                        candidates_storage.with_storage(|| {
                            ctx.reserve_vec(
                                &mut candidates,
                                1,
                                "collect SLDPRT solved-line transform candidates",
                            )
                        })?;
                        candidates.push(pair);
                    }
                }
                let (candidates, _selected_storage) = if relation.family
                    == FeatureInputRelationFamily::PointLineDistance
                {
                    let mut filtered_storage =
                        ctx.reserve_scoped(0, "SLDPRT solved-line filtered candidates")?;
                    let mut filtered = Vec::new();
                    for (start, end) in
                        ctx.admit_iter(candidates, "filter SLDPRT solved-line candidates")?
                    {
                        let (Some(start_u), Some(start_v), Some(end_u), Some(end_v)) = (
                            f64_from_i64(start.0),
                            f64_from_i64(start.1),
                            f64_from_i64(end.0),
                            f64_from_i64(end.1),
                        ) else {
                            continue;
                        };
                        let (line, _line_storage) = ctx.with_scoped_storage(
                            "SLDPRT solved-line candidate geometry",
                            || {
                                candidate(
                                    Point2::new(start_u * QUANTUM, start_v * QUANTUM),
                                    Point2::new(end_u * QUANTUM, end_v * QUANTUM),
                                )
                            },
                        )?;
                        let Some(line) = line else {
                            continue;
                        };
                        if point_position.is_some_and(|point| {
                            point_line_distance_value(point, &line)
                                .is_some_and(|measured| same_dimension_length(measured, expected))
                        }) {
                            filtered_storage.with_storage(|| {
                                ctx.reserve_vec(
                                    &mut filtered,
                                    1,
                                    "filter SLDPRT solved-line candidates",
                                )
                            })?;
                            filtered.push((start, end));
                        }
                    }
                    let Some(&(first_start, first_end)) = filtered.first() else {
                        return Ok(None);
                    };
                    let orientation_is_ambiguous = ctx.any_by(
                        &filtered,
                        |(start, end)| Ok(*start == first_end && *end == first_start),
                        "check SLDPRT solved-line orientation ambiguity",
                    )?;
                    if ctx.all_by(
                        &filtered,
                        |(start, end)| {
                            Ok((*start == first_start && *end == first_end)
                                || (*start == first_end && *end == first_start))
                        },
                        "check SLDPRT solved-line orientation agreement",
                    )? {
                        let representative = if orientation_is_ambiguous {
                            if first_start <= first_end {
                                (first_start, first_end)
                            } else {
                                (first_end, first_start)
                            }
                        } else {
                            (first_start, first_end)
                        };
                        ctx.truncate_vec(&mut filtered, 1, "select SLDPRT solved-line candidate")?;
                        filtered[0] = representative;
                    } else {
                        return Ok(None);
                    }
                    drop(candidates_storage);
                    (filtered, filtered_storage)
                } else {
                    (candidates, candidates_storage)
                };
                let [(start, end)] = candidates.as_slice() else {
                    return Ok(None);
                };
                let (Some(start_u), Some(start_v), Some(end_u), Some(end_v)) = (
                    f64_from_i64(start.0),
                    f64_from_i64(start.1),
                    f64_from_i64(end.0),
                    f64_from_i64(end.1),
                ) else {
                    return Ok(None);
                };
                Ok(Some((
                    Point2::new(start_u * QUANTUM, start_v * QUANTUM),
                    Point2::new(end_u * QUANTUM, end_v * QUANTUM),
                )))
            };
            let build_lines = |prefer_marker_endpoints: bool| -> Result<
                (Vec<_>, ScopedReservation<'_>),
                cadmpeg_core::CodecError,
            > {
                let operation = "collect SLDPRT relation operand lines";
                let mut storage = ctx.reserve_scoped(0, operation)?;
                let mut lines = Vec::new();
                for &(operand_index, operand) in
                    ctx.admit_iter(&line_operands, "build SLDPRT relation operand lines")?
                {
                    let markers = if prefer_marker_endpoints {
                        endpoint_line_markers(operand_index)?
                            .or_else(|| fallback_line_markers(operand.entity_index))
                    } else {
                        fallback_line_markers(operand.entity_index)
                    };
                    let Some(markers) = markers else {
                        return Ok((Vec::new(), ctx.reserve_scoped(0, operation)?));
                    };
                    let Some((start, end)) = transformed_line(markers)? else {
                        return Ok((Vec::new(), ctx.reserve_scoped(0, operation)?));
                    };
                    let Some(line) = storage.with_storage(|| candidate(start, end))? else {
                        return Ok((Vec::new(), ctx.reserve_scoped(0, operation)?));
                    };
                    storage.with_storage(|| {
                        ctx.push_vec(&mut lines, (operand, markers, line), operation)
                    })?;
                }
                Ok((lines, storage))
            };
            let relation_lines_valid =
                |lines: &[(&FeatureInputOperand, [&SketchInputEntity; 2], SketchEntity)]| {
                    match relation.family {
                        FeatureInputRelationFamily::LineLineDistance => match lines {
                            [(_, _, first), (_, _, second)] => line_line_distance(first, second)
                                .is_some_and(|measured| same_dimension_length(measured, expected)),
                            _ => false,
                        },
                        FeatureInputRelationFamily::PointLineDistance => match lines {
                            [(_, _, line)] => point_position.is_some_and(|point| {
                                point_line_distance_value(point, line).is_some_and(|measured| {
                                    same_dimension_length(measured, expected)
                                })
                            }),
                            _ => false,
                        },
                        FeatureInputRelationFamily::Angle => match lines {
                            [(_, _, first), (_, _, second)] => {
                                let angle = if relation_uses_dynamic_operands(relation) {
                                    unoriented_line_line_angle(first, second)
                                } else {
                                    line_line_angle(first, second)
                                };
                                angle.is_some_and(|measured| {
                                    same_dimension_angle(measured, expected)
                                })
                            }
                            _ => false,
                        },
                        _ => false,
                    }
                };
            let mut lines = build_lines(true)?;
            let mut valid = relation_lines_valid(&lines.0);
            if !valid {
                let fallback_lines = build_lines(false)?;
                if !fallback_lines.0.is_empty() {
                    lines = fallback_lines;
                    valid = relation_lines_valid(&lines.0);
                }
            }
            if !valid {
                if relation.family == FeatureInputRelationFamily::LineLineDistance
                    && relation_uses_dynamic_operands(relation)
                {
                    let generated_storage =
                        ctx.with_scoped_storage("collect SLDPRT generated solver lines", || {
                            ctx.collect_vec(
                                lines.0.iter().map(|(_, _, line)| line),
                                "collect SLDPRT generated solver lines",
                            )
                        })?;
                    if let Some([first, second]) = unique_dynamic_line_pair(
                        ctx,
                        expected,
                        sketch,
                        entities,
                        &generated_storage.0,
                        QUANTUM,
                    )? {
                        let selected = [first, second];
                        let mut aliases_match = true;
                        'operands: for (operand, line) in ctx
                            .admit_iter(
                                &relation.operands,
                                "verify SLDPRT dynamic solver-line operands",
                            )?
                            .zip(selected.iter())
                        {
                            for entity in ctx
                                .admit_iter(&*entities, "find SLDPRT dynamic solver-line aliases")?
                            {
                                if !ctx.equal(
                                    &entity.sketch,
                                    sketch,
                                    "match SLDPRT dynamic solver-line sketch",
                                )? {
                                    continue;
                                }
                                let Some(geometry_ref) = entity.geometry_ref.as_deref() else {
                                    continue;
                                };
                                if !solver_line_geometry_ref_matches(
                                    ctx,
                                    geometry_ref,
                                    &relation.feature_ref,
                                    operand.entity_index,
                                )? {
                                    continue;
                                }
                                if !ctx.equal(
                                    &dynamic_line_geometry_key(ctx, entity, QUANTUM)?,
                                    &dynamic_line_geometry_key(ctx, line, QUANTUM)?,
                                    "compare SLDPRT dynamic solver-line geometry",
                                )? {
                                    aliases_match = false;
                                    break 'operands;
                                }
                            }
                        }
                        if !aliases_match {
                            continue;
                        }
                        let feature_key = ctx
                            .rsplit_once(
                                &relation.feature_ref,
                                "#",
                                "split SLDPRT dynamic solver-line feature",
                            )?
                            .map_or(relation.feature_ref.as_str(), |(_, key)| key);
                        for (operand, line) in ctx
                            .admit_iter(
                                &relation.operands,
                                "materialize SLDPRT dynamic solver-line operands",
                            )?
                            .zip(selected)
                        {
                            let geometry_ref = ctx.format_retained(
                                format_args!(
                                    "{}:solver-line:{}",
                                    relation.feature_ref, operand.entity_index
                                ),
                                "format SLDPRT dynamic solver-line reference",
                            )?;
                            let mut already_present = false;
                            let mut steps = entities.iter();
                            while let Some(entity) = ctx.next_charged(
                                &mut steps,
                                "check existing SLDPRT dynamic solver line",
                            )? {
                                if ctx.equal(
                                    &entity.sketch,
                                    sketch,
                                    "match SLDPRT dynamic solver-line sketch",
                                )? && ctx.equal(
                                    &entity.geometry_ref.as_deref(),
                                    &Some(geometry_ref.as_str()),
                                    "match SLDPRT dynamic solver-line reference",
                                )? {
                                    already_present = true;
                                    break;
                                }
                            }
                            if already_present {
                                continue;
                            }
                            let id_text = ctx.format_retained(
                                format_args!(
                                    "sldprt:model:sketch-entity#solver-line:{feature_key}:{}",
                                    operand.entity_index
                                ),
                                "format SLDPRT dynamic solver-line entity identity",
                            )?;
                            ctx.charge_work(
                                u64_from_index(id_text.len()),
                                "validate SLDPRT dynamic solver-line entity identity",
                            )?;
                            let Ok(id) = SketchEntityId::mint(id_text) else {
                                continue;
                            };
                            let sketch_id = copy_planar_sketch_id(ctx, &line.sketch)?;
                            let mut endpoint_refs = Vec::new();
                            for reference in ctx.admit_iter(
                                &line.endpoint_refs,
                                "copy SLDPRT dynamic solver-line endpoint references",
                            )? {
                                let reference = ctx.format_retained(
                                    format_args!("{reference}"),
                                    "copy SLDPRT dynamic solver-line endpoint reference",
                                )?;
                                ctx.reserve_vec(
                                    &mut endpoint_refs,
                                    1,
                                    "copy SLDPRT dynamic solver-line endpoints",
                                )?;
                                endpoint_refs.push(reference);
                            }
                            ctx.reserve_vec(entities, 1, "append SLDPRT dynamic solver line")?;
                            entities.push(
                                SketchEntity::new(
                                    id,
                                    sketch_id,
                                    line.geometry.try_clone_for_decode(
                                        ctx,
                                        "copy SLDPRT relation line geometry",
                                    )?,
                                )
                                .with_construction(true)
                                .with_geometry_ref(Some(geometry_ref))
                                .with_endpoint_refs(endpoint_refs),
                            );
                        }
                        continue;
                    }
                }
                continue;
            }
            let feature_key = ctx
                .rsplit_once(
                    &relation.feature_ref,
                    "#",
                    "split SLDPRT solver-line feature",
                )?
                .map_or(relation.feature_ref.as_str(), |(_, key)| key);
            for (operand, markers, line) in
                ctx.admit_iter(&lines.0, "append SLDPRT solved relation lines")?
            {
                let geometry_ref = ctx.format_retained(
                    format_args!(
                        "{}:solver-line:{}",
                        relation.feature_ref, operand.entity_index
                    ),
                    "format SLDPRT solver-line reference",
                )?;
                let mut already_present = false;
                let mut steps = entities.iter();
                while let Some(entity) =
                    ctx.next_charged(&mut steps, "check existing SLDPRT solved relation line")?
                {
                    if ctx.equal(
                        &entity.sketch,
                        sketch,
                        "match SLDPRT solved relation-line sketch",
                    )? && ctx.equal(
                        &entity.geometry_ref.as_deref(),
                        &Some(geometry_ref.as_str()),
                        "match SLDPRT solved relation-line reference",
                    )? {
                        already_present = true;
                        break;
                    }
                }
                if already_present {
                    continue;
                }
                let id_text = ctx.format_retained(
                    format_args!(
                        "sldprt:model:sketch-entity#solver-line:{feature_key}:{}",
                        operand.entity_index
                    ),
                    "format SLDPRT solver-line entity identity",
                )?;
                ctx.charge_work(
                    u64_from_index(id_text.len()),
                    "validate SLDPRT solver-line entity identity",
                )?;
                let Ok(id) = SketchEntityId::mint(id_text) else {
                    continue;
                };
                let sketch_id = copy_planar_sketch_id(ctx, &line.sketch)?;
                let native_ref = line
                    .native_ref
                    .as_deref()
                    .map(|reference| {
                        ctx.format_retained(
                            format_args!("{reference}"),
                            "copy SLDPRT solver-line native reference",
                        )
                    })
                    .transpose()?;
                let first_ref = ctx.format_retained(
                    format_args!("{}", markers[0].id()),
                    "copy SLDPRT solver-line first endpoint reference",
                )?;
                let second_ref = ctx.format_retained(
                    format_args!("{}", markers[1].id()),
                    "copy SLDPRT solver-line second endpoint reference",
                )?;
                let endpoint_refs = vec![first_ref, second_ref];
                ctx.reserve_vec(entities, 1, "append SLDPRT solver line")?;
                entities.push(
                    SketchEntity::new(
                        id,
                        sketch_id,
                        line.geometry
                            .try_clone_for_decode(ctx, "copy SLDPRT relation line geometry")?,
                    )
                    .with_construction(line.construction)
                    .with_native_ref(native_ref)
                    .with_geometry_ref(Some(geometry_ref))
                    .with_endpoint_refs(endpoint_refs),
                );
            }
        }
    }
    Ok(())
}

/// The point-to-point distance relation families that project a missing endpoint.
#[derive(Clone, Copy)]
enum PointPointDistanceFamily {
    /// Straight-line distance between the two points.
    Direct,
    /// Distance along one of the profile's axes.
    AxisAligned,
}

impl PointPointDistanceFamily {
    fn of(family: FeatureInputRelationFamily) -> Option<Self> {
        match family {
            FeatureInputRelationFamily::PointPointDistance => Some(Self::Direct),
            FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance => Some(Self::AxisAligned),
            _ => None,
        }
    }
}

fn unique_dynamic_line_pair<'a>(
    ctx: &DecodeContext<'_>,
    expected: f64,
    sketch: &cadmpeg_ir::sketches::SketchId,
    entities: &'a [SketchEntity],
    generated: &[&'a SketchEntity],
    quantum: f64,
) -> Result<Option<[SketchEntity; 2]>, cadmpeg_core::CodecError> {
    if generated.len() != 2 {
        return Ok(None);
    }
    let mut candidates_storage = ctx.reserve_scoped(0, "SLDPRT dynamic line candidates")?;
    let mut candidates = Vec::<([GridPoint; 2], &SketchEntity)>::new();
    for entity in ctx
        .admit_iter(generated, "scan SLDPRT dynamic line candidates")?
        .copied()
        .chain(ctx.admit_iter(entities, "scan SLDPRT dynamic line candidates")?)
    {
        if !ctx.equal(&entity.sketch, sketch, "match SLDPRT dynamic line sketch")?
            || !matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        let Some(key) = dynamic_line_geometry_key(ctx, entity, quantum)? else {
            continue;
        };
        if ctx.any_by(
            &candidates,
            |(candidate, _)| Ok(*candidate == key),
            "match SLDPRT dynamic line candidates",
        )? {
            continue;
        }
        candidates_storage.with_storage(|| {
            ctx.reserve_vec(&mut candidates, 1, "collect SLDPRT dynamic line candidates")
        })?;
        candidates.push((key, entity));
    }
    let mut match_pair = None;
    if !ctx.all_by(
        candidates.iter().enumerate(),
        |(first_index, (first_key, first))| {
            let next_index = first_index.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("compare SLDPRT dynamic line pairs", u64::MAX - 1, u64::MAX)
            })?;
            if !ctx.all_by(
                &candidates[next_index..],
                |(second_key, second)| {
                    if line_line_distance(first, second)
                        .is_some_and(|measured| same_dimension_length(measured, expected))
                    {
                        let mut pair_key = [*first_key, *second_key];
                        ctx.sort_unstable_by(
                            &mut pair_key,
                            |value| value,
                            Ord::cmp,
                            "sldprt dynamic line pair keys sort",
                        )?;
                        if let Some((previous, _)) = match_pair {
                            if previous != pair_key {
                                return Ok(false);
                            }
                        } else {
                            match_pair = Some((pair_key, [*first, *second]));
                        }
                    }

                    Ok(true)
                },
                "compare SLDPRT dynamic line pairs",
            )? {
                return Ok(false);
            }

            Ok(true)
        },
        "compare SLDPRT dynamic line pairs",
    )? {
        return Ok(None);
    }
    let Some((_, pair)) = match_pair else {
        return Ok(None);
    };
    let Some(first_key) = dynamic_line_geometry_key(ctx, generated[0], quantum)? else {
        return Ok(None);
    };
    let Some(second_key) = dynamic_line_geometry_key(ctx, generated[1], quantum)? else {
        return Ok(None);
    };
    let ordered = if dynamic_line_geometry_key(ctx, pair[0], quantum)? == Some(first_key) {
        pair
    } else if dynamic_line_geometry_key(ctx, pair[1], quantum)? == Some(first_key)
        || dynamic_line_geometry_key(ctx, pair[0], quantum)? == Some(second_key)
    {
        [pair[1], pair[0]]
    } else {
        pair
    };
    Ok(Some([
        copy_dynamic_line_entity(ctx, ordered[0])?,
        copy_dynamic_line_entity(ctx, ordered[1])?,
    ]))
}

fn copy_dynamic_line_entity(
    ctx: &DecodeContext<'_>,
    entity: &SketchEntity,
) -> Result<SketchEntity, cadmpeg_core::CodecError> {
    let id_text = ctx.format_retained(
        format_args!("{}", entity.id().as_str()),
        "copy SLDPRT dynamic line identity",
    )?;
    ctx.charge_work(
        u64_from_index(id_text.len()),
        "validate SLDPRT dynamic line identity",
    )?;
    let id = SketchEntityId::mint(id_text).map_err(|_| {
        cadmpeg_core::CodecError::Malformed("SolidWorks dynamic line identity is invalid".into())
    })?;
    let sketch = copy_planar_sketch_id(ctx, &entity.sketch)?;
    let native_ref = entity
        .native_ref
        .as_deref()
        .map(|reference| {
            ctx.format_retained(
                format_args!("{reference}"),
                "copy SLDPRT dynamic line native reference",
            )
        })
        .transpose()?;
    let geometry_ref = entity
        .geometry_ref
        .as_deref()
        .map(|reference| {
            ctx.format_retained(
                format_args!("{reference}"),
                "copy SLDPRT dynamic line geometry reference",
            )
        })
        .transpose()?;
    let mut endpoint_refs = Vec::new();
    for reference in ctx.admit_iter(
        &entity.endpoint_refs,
        "copy SLDPRT dynamic line endpoint references",
    )? {
        let reference = ctx.format_retained(
            format_args!("{reference}"),
            "copy SLDPRT dynamic line endpoint reference",
        )?;
        ctx.reserve_vec(&mut endpoint_refs, 1, "copy SLDPRT dynamic line endpoints")?;
        endpoint_refs.push(reference);
    }
    Ok(SketchEntity::new(
        id,
        sketch,
        entity
            .geometry
            .try_clone_for_decode(ctx, "copy SLDPRT dynamic line geometry")?,
    )
    .with_construction(entity.construction)
    .with_native_ref(native_ref)
    .with_geometry_ref(geometry_ref)
    .with_endpoint_refs(endpoint_refs))
}

fn dynamic_line_geometry_key(
    ctx: &DecodeContext<'_>,
    entity: &SketchEntity,
    quantum: f64,
) -> Result<Option<[GridPoint; 2]>, cadmpeg_core::CodecError> {
    let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
        return Ok(None);
    };
    let mut endpoints = [quantize(start.get(), quantum), quantize(end.get(), quantum)];
    ctx.sort_unstable_by(
        &mut endpoints,
        |value| value,
        Ord::cmp,
        "sldprt dynamic line endpoints sort",
    )?;
    Ok(Some(endpoints))
}

pub(crate) fn project_relation_solved_point_geometry(
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketch_records = std::cell::OnceCell::new();
    let mut sketches_by_feature_storage =
        ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut sketches_by_feature = HashMap::new();
    for feature in ctx.admit_iter(features, "index SLDPRT solved-point sketches")? {
        let operation = "index SLDPRT solved-point sketches";
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
        sketches_by_feature_storage.with_storage(|| {
            ctx.insert_hash_map(&mut sketches_by_feature, native_ref, sketch, operation)
        })?;
    }
    let (markers, _markers_storage) = ctx
        .with_scoped_storage("index SLDPRT solved-point markers", || {
            RelationMarkers::new(ctx, lanes)
        })?;
    let markers_by_id = markers.by_id();
    // The loci join the entities present before this projection appends points.
    let ((transforms, loci_by_marker), _loci_storage) =
        ctx.with_scoped_storage("build SLDPRT solved-point marker loci", || {
            let profile = ProfileEntities::new(ctx, entities)?;
            let transforms =
                marker_transform_candidates_in(ctx, features, sketches, &profile, lanes)?;
            let loci = profile_loci_in(ctx, features, &profile, &markers, lanes, &transforms)?;
            Ok::<_, cadmpeg_core::CodecError>((transforms, loci))
        })?;
    let ownership_scope = ctx
        .with_scoped_storage("SLDPRT solved-point relation parameter ownership", || {
            owned_relation_parameters(ctx, features, parameters, lanes)
        })?;
    let mut parameters_by_id_storage = ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut parameters_by_id = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "index SLDPRT solved-point parameters")? {
        let operation = "index SLDPRT solved-point parameters";
        parameters_by_id_storage.with_storage(|| {
            ctx.insert_hash_map(&mut parameters_by_id, &parameter.id, parameter, operation)
        })?;
    }

    for lane in ctx.admit_iter(lanes, "scan SLDPRT solved-point lanes")? {
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "split SLDPRT solved-point lane identity")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        // The omitted-point coordinates of each feature in this lane, solved
        // once for the first relation of the feature that needs them.
        let mut solved_storage = ctx.reserve_scoped(0, "solve SLDPRT omitted points")?;
        let mut solved_by_feature = HashMap::<&str, HashMap<u32, [f64; 2]>>::new();
        for relation in ctx.admit_iter(
            &lane.relation_instances,
            "scan SLDPRT solved-point relations",
        )? {
            let Some(family) = PointPointDistanceFamily::of(relation.family) else {
                continue;
            };
            if relation.operands.len() != 2 {
                continue;
            }
            let Some(sketch) = ctx.get_hash_map(
                &sketches_by_feature,
                relation.feature_ref.as_str(),
                "resolve SLDPRT solved-point sketch",
            )?
            else {
                continue;
            };
            let parameter = ctx
                .get_hash_map(
                    &ownership_scope.0,
                    &relation.id,
                    "resolve SLDPRT solved-point parameter ownership",
                )?
                .and_then(Option::as_ref)
                .map(|parameter| {
                    ctx.get_hash_map(
                        &parameters_by_id,
                        parameter,
                        "resolve SLDPRT solved-point parameter",
                    )
                })
                .transpose()?
                .flatten()
                .copied();
            let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) =
                parameter.and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            let profile_axis = profile_axis_for_relation(
                ctx,
                relation,
                ctx.get_hash_map(
                    &transforms,
                    relation.feature_ref.as_str(),
                    "resolve SLDPRT solved-point transforms",
                )?
                .map(Vec::as_slice),
            )?;
            if relation_uses_solver_points(relation) {
                let feature = relation.feature_ref.as_str();
                if !ctx.contains_key_hash_map(
                    &solved_by_feature,
                    feature,
                    "solve SLDPRT omitted points",
                )? {
                    let solved = solved_storage
                        .with_storage(|| inferred_point_coordinates_by_index(ctx, lane, feature))?;
                    solved_storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut solved_by_feature,
                            feature,
                            solved,
                            "solve SLDPRT omitted points",
                        )
                    })?;
                }
                let Some(coordinates_by_index) =
                    ctx.get_hash_map(&solved_by_feature, feature, "solve SLDPRT omitted points")?
                else {
                    continue;
                };
                let mut resolved_positions = [None; 2];
                let mut resolved = true;
                for (index, operand) in ctx
                    .admit_iter(&relation.operands, "scan SLDPRT solved-point operands")?
                    .enumerate()
                {
                    let geometry_ref = ctx.format_retained(
                        format_args!("{}:operand:{index}", relation.id),
                        "format SLDPRT solved-point operand reference",
                    )?;
                    let mut already_present = false;
                    let mut steps = entities.iter();
                    while let Some(entity) =
                        ctx.next_charged(&mut steps, "check existing SLDPRT solved-point geometry")?
                    {
                        if ctx.equal(
                            &entity.geometry_ref.as_deref(),
                            &Some(geometry_ref.as_str()),
                            "match existing SLDPRT solved-point geometry",
                        )? {
                            already_present = true;
                            break;
                        }
                    }
                    if already_present {
                        resolved_positions[index] = None;
                        continue;
                    }
                    let Some(coordinates) = ctx
                        .get_hash_map(
                            coordinates_by_index,
                            &u32::from(operand.entity_index),
                            "resolve SLDPRT solved-point coordinates",
                        )?
                        .copied()
                    else {
                        resolved = false;
                        break;
                    };
                    let native = quantize(
                        Point2::new(coordinates[0] * NATIVE_TO_IR, coordinates[1] * NATIVE_TO_IR),
                        QUANTUM,
                    );
                    let mut unique_position = None;
                    let mut ambiguous = false;
                    if let Some(transforms) = ctx.get_hash_map(
                        &transforms,
                        relation.feature_ref.as_str(),
                        "resolve SLDPRT solved-point transforms",
                    )? {
                        for transform in
                            ctx.admit_iter(transforms, "scan SLDPRT solved-point transforms")?
                        {
                            let Some(position) = transform.apply(native) else {
                                continue;
                            };
                            if unique_position.is_some_and(|previous| previous != position) {
                                ambiguous = true;
                            } else if unique_position.is_none() {
                                unique_position = Some(position);
                            }
                        }
                    }
                    let position = if ambiguous || unique_position.is_none() {
                        let (records, _) = match sketch_records.get() {
                            Some(records) => records,
                            None => {
                                let built = relation_sketch_records(ctx, sketches)?;
                                sketch_records.get_or_init(|| built)
                            }
                        };
                        let selected_sketch = ctx
                            .get_hash_map(records, sketch, "find SLDPRT solved-point sketch frame")?
                            .copied();
                        selected_sketch
                            .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                            .and_then(|transform| transform.apply(native))
                    } else {
                        unique_position
                    };
                    let Some(position) = position else {
                        resolved = false;
                        break;
                    };
                    resolved_positions[index] = Some(position);
                }
                if !resolved {
                    continue;
                }
                for (index, position) in ctx
                    .admit_iter(&resolved_positions, "materialize SLDPRT solved points")?
                    .enumerate()
                {
                    let Some(position) = *position else {
                        continue;
                    };
                    let geometry_ref = ctx.format_retained(
                        format_args!("{}:operand:{index}", relation.id),
                        "format SLDPRT solved-point operand reference",
                    )?;
                    let id_text = ctx.format_retained(
                        format_args!(
                            "sldprt:model:sketch-entity#solver-point:{lane_key}:{}:{index}",
                            relation.offset
                        ),
                        "format SLDPRT solved-point entity identity",
                    )?;
                    ctx.charge_work(
                        u64_from_index(id_text.len()),
                        "validate SLDPRT solved-point entity identity",
                    )?;
                    let Ok(id) = SketchEntityId::mint(id_text) else {
                        continue;
                    };
                    let (Some(position_u), Some(position_v)) =
                        (f64_from_i64(position.0), f64_from_i64(position.1))
                    else {
                        continue;
                    };
                    let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: Point2::new(position_u * QUANTUM, position_v * QUANTUM),
                    }) else {
                        continue;
                    };
                    let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
                    ctx.reserve_vec(entities, 1, "append SLDPRT solved point")?;
                    entities.push(
                        SketchEntity::new(id, sketch_id, geometry)
                            .with_construction(true)
                            .with_geometry_ref(Some(geometry_ref)),
                    );
                }
                continue;
            }
            let resolve = |index: usize| -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
                match relation.operands[index].entity_ref.as_deref() {
                    Some(marker) => marker_point_locus(ctx, marker, markers_by_id, &loci_by_marker),
                    None => Ok(None),
                }
            };
            let resolved = [resolve(0)?, resolve(1)?];
            let (known, missing_index) = match resolved {
                [Some(known), None] => (known, 1),
                [None, Some(known)] => (known, 0),
                _ => continue,
            };
            let Some(missing_marker_id) = relation.operands[missing_index].entity_ref.as_deref()
            else {
                continue;
            };
            let Some(missing_marker) = ctx
                .get_hash_map(
                    markers_by_id,
                    missing_marker_id,
                    "resolve SLDPRT missing dimension point marker",
                )?
                .copied()
            else {
                continue;
            };
            if missing_marker.coordinates_m.is_some()
                || !matches!(
                    missing_marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            {
                continue;
            }
            let Some(known_point) = find_profile_entity(
                ctx,
                entities,
                super::transforms::locus_entity(&known),
                "resolve SLDPRT profile locus",
            )?
            .and_then(|entity| entity_locus_point(entity, &known)) else {
                continue;
            };
            let mut selected_point = None;
            let mut ambiguous_point = false;
            for entity in ctx.admit_iter(&*entities, "find SLDPRT dimension-point candidates")? {
                if !ctx.equal(
                    &entity.sketch,
                    *sketch,
                    "match SLDPRT dimension-point sketch",
                )? {
                    continue;
                }
                for (point, _) in sketch_entity_locus_points(entity).into_iter().flatten() {
                    let measured = match family {
                        PointPointDistanceFamily::Direct => {
                            (point.u - known_point.u).hypot(point.v - known_point.v)
                        }
                        PointPointDistanceFamily::AxisAligned => match profile_axis {
                            Some(ProfileAxis::U) => (point.u - known_point.u).abs(),
                            Some(ProfileAxis::V) => (point.v - known_point.v).abs(),
                            None => continue,
                        },
                    };
                    if !same_dimension_length(measured, distance.get()) {
                        continue;
                    }
                    let point = quantize(point, QUANTUM);
                    if let Some(selected) = selected_point {
                        if selected != point {
                            ambiguous_point = true;
                        }
                    } else {
                        selected_point = Some(point);
                    }
                }
            }
            let Some(point) = selected_point else {
                continue;
            };
            if ambiguous_point {
                continue;
            }
            let geometry_ref = ctx.format_retained(
                format_args!("{}:operand:{missing_index}", relation.id),
                "format SLDPRT dimension-point operand reference",
            )?;
            let mut already_present = false;
            let mut steps = entities.iter();
            while let Some(entity) =
                ctx.next_charged(&mut steps, "check existing SLDPRT dimension-point geometry")?
            {
                if ctx.equal(
                    &entity.geometry_ref.as_deref(),
                    &Some(geometry_ref.as_str()),
                    "match existing SLDPRT dimension-point geometry",
                )? {
                    already_present = true;
                    break;
                }
            }
            if already_present {
                continue;
            }
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}:{missing_index}",
                    relation.offset
                ),
                "format SLDPRT dimension-point entity identity",
            )?;
            ctx.charge_work(
                u64_from_index(id_text.len()),
                "validate SLDPRT dimension-point entity identity",
            )?;
            let Ok(id) = SketchEntityId::mint(id_text) else {
                continue;
            };
            let Some(position) = point.point(QUANTUM) else {
                continue;
            };
            let Ok(geometry) =
                SketchGeometry::try_from(SketchGeometryDefinition::Point { position })
            else {
                continue;
            };
            let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
            ctx.reserve_vec(entities, 1, "append SLDPRT dimension point")?;
            entities.push(
                SketchEntity::new(id, sketch_id, geometry)
                    .with_construction(true)
                    .with_geometry_ref(Some(geometry_ref)),
            );
        }
    }
    Ok(())
}

const DIMENSIONED_HANDLE_OPERATION: &str = "resolve SLDPRT dimensioned handles";

fn collect_handle_markers<'source, 'entity: 'source, T: 'source>(
    ctx: &DecodeContext<'_>,
    markers: &'source [T],
    as_marker: impl Fn(&'source T) -> &'entity SketchInputEntity,
    feature: &str,
    keep: impl Fn(&SketchInputEntity) -> Result<bool, cadmpeg_core::CodecError>,
    result: &mut Vec<&'entity SketchInputEntity>,
) -> Result<(), cadmpeg_core::CodecError> {
    for value in ctx.admit_iter(markers, DIMENSIONED_HANDLE_OPERATION)? {
        let marker = as_marker(value);
        if !ctx.equal(
            &marker.feature_ref.as_deref(),
            &Some(feature),
            DIMENSIONED_HANDLE_OPERATION,
        )? || !keep(marker)?
        {
            continue;
        }
        ctx.reserve_vec(result, 1, DIMENSIONED_HANDLE_OPERATION)?;
        result.push(marker);
    }
    Ok(())
}

fn sort_handle_markers(
    ctx: &DecodeContext<'_>,
    markers: &mut [&SketchInputEntity],
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.sort_unstable_by_key(
        markers,
        |value| value.offset(),
        Ord::cmp,
        DIMENSIONED_HANDLE_OPERATION,
    )?;
    Ok(())
}

pub(super) fn implicit_circle_marker<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand_kind: FeatureInputOperandKind,
    index: u16,
    expected_radius: f64,
) -> Result<Option<(&'a SketchInputEntity, f64)>, cadmpeg_core::CodecError> {
    // CircleDiameter selects the semantic family; native operand tags are only
    // carrier kinds and must not narrow the geometric witness search.
    if !matches!(operand_kind, FeatureInputOperandKind::Native(_))
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return Ok(None);
    }
    let Some(relation_index) = u32::from(index).checked_add(1) else {
        return Ok(None);
    };
    let mut candidate: Option<(&SketchInputEntity, f64)> = None;
    let disagree = ctx.any_by(
        lanes,
        |lane| {
            let mut relation = None;
            let mut steps = lane.sketch_entities.iter();
            while let Some(marker) =
                ctx.next_charged(&mut steps, "find SLDPRT circle dimension relation marker")?
            {
                if !ctx.equal(
                    &marker.feature_ref.as_deref(),
                    &Some(feature),
                    DIMENSIONED_HANDLE_OPERATION,
                )? || marker.object_index() != Some(relation_index)
                    || marker.kind() != SketchInputKind::Relation(SketchRelationKind::Distance)
                {
                    continue;
                }
                let [first, second] = marker.links() else {
                    continue;
                };
                if !ctx.equal(
                    first.entity_ref.as_str(),
                    second.entity_ref.as_str(),
                    DIMENSIONED_HANDLE_OPERATION,
                )? || first.local_id != second.local_id
                {
                    continue;
                }
                relation = Some(marker);
                break;
            }
            let Some(relation) = relation else {
                return Ok(false);
            };
            let Some(link) = relation.links().first() else {
                return Ok(false);
            };
            let center_id = link.entity_ref.as_str();
            let mut center = None;
            let mut steps = lane.sketch_entities.iter();
            while let Some(marker) =
                ctx.next_charged(&mut steps, "find SLDPRT circle dimension center marker")?
            {
                if ctx.equal(marker.id(), center_id, DIMENSIONED_HANDLE_OPERATION)?
                    && marker.coordinates_m.is_some()
                {
                    center = Some(marker);
                    break;
                }
            }
            let resolved = (|| -> Result<_, cadmpeg_core::CodecError> {
                let Some(center) = center else {
                    return Ok(None);
                };
                let radial = ctx
                    .min_by_key(
                        &lane.sketch_entities,
                        |marker| {
                            let eligible = ctx.equal(
                                &marker.feature_ref.as_deref(),
                                &Some(feature),
                                DIMENSIONED_HANDLE_OPERATION,
                            )? && marker.offset() > center.offset()
                                && marker.coordinates_m.is_some();
                            Ok((eligible, marker.offset()))
                        },
                        |left, right| Ok(right.0.cmp(&left.0).then(left.1.cmp(&right.1))),
                        "select SLDPRT circle radial marker",
                    )?
                    .filter(|marker| {
                        marker.offset() > center.offset() && marker.coordinates_m.is_some()
                    });
                let radial = if let Some(radial) = radial {
                    if ctx.equal(
                        &radial.feature_ref.as_deref(),
                        &Some(feature),
                        DIMENSIONED_HANDLE_OPERATION,
                    )? {
                        radial
                    } else {
                        return Ok(None);
                    }
                } else {
                    return Ok(None);
                };
                let Some([cu, cv]) = center
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    return Ok(None);
                };
                let Some([ru, rv]) = radial
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    return Ok(None);
                };
                let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                Ok(same_dimension_length(radius, expected_radius).then_some((center, radius)))
            })();
            let Some(resolved) = resolved? else {
                return Ok(false);
            };
            if let Some(first) = candidate {
                if !ctx.equal(first.0.id(), resolved.0.id(), DIMENSIONED_HANDLE_OPERATION)?
                    || first.1.to_bits() != resolved.1.to_bits()
                {
                    return Ok(true);
                }
            } else {
                candidate = Some(resolved);
            }
            Ok(false)
        },
        "scan SLDPRT circle dimension lanes",
    )?;
    if !disagree && candidate.is_some() {
        return Ok(candidate);
    }
    let mut terminal_pair: Option<(&SketchInputEntity, f64)> = None;
    let mut terminal_ambiguous = false;
    for lane in ctx.admit_iter(lanes, "scan SLDPRT circle-dimension lanes")? {
        let mut feature_markers = Vec::new();
        let mut feature_markers_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
        feature_markers_storage.with_storage(|| {
            collect_handle_markers(
                ctx,
                &lane.sketch_entities,
                |marker| marker,
                feature,
                |marker| {
                    Ok(marker.coordinates_m.is_some()
                        && matches!(
                            marker.kind(),
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        ))
                },
                &mut feature_markers,
            )
        })?;
        for radial in ctx
            .admit_iter(&feature_markers, "scan SLDPRT circle radial markers")?
            .copied()
            .filter(|marker| marker.local_id().is_none())
        {
            for center in ctx
                .admit_iter(&feature_markers, DIMENSIONED_HANDLE_OPERATION)?
                .copied()
            {
                if center.local_id().is_none() || center.offset() >= radial.offset() {
                    continue;
                }
                let Some([cu, cv]) = center
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    return Ok(None);
                };
                let Some([ru, rv]) = radial
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    return Ok(None);
                };
                let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                if !same_dimension_length(radius, expected_radius) {
                    continue;
                }
                if let Some(first) = terminal_pair {
                    if !ctx.equal(first.0.id(), center.id(), DIMENSIONED_HANDLE_OPERATION)?
                        || first.1.to_bits() != radius.to_bits()
                    {
                        terminal_ambiguous = true;
                    }
                } else {
                    terminal_pair = Some((center, radius));
                }
            }
        }
    }
    if !terminal_ambiguous && terminal_pair.is_some() {
        return Ok(terminal_pair);
    }
    // Only 83fe defines an ordered center/radial point roster. Other native
    // carriers may use the relation-qualified witness tiers above, but their
    // point-marker order does not identify a circular-dimension pair.
    if operand_kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_83FE) {
        return Ok(None);
    }
    let mut markers = Vec::new();
    let mut markers_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
    for lane in ctx.admit_iter(lanes, "scan SLDPRT ordered circle-marker lanes")? {
        markers_storage.with_storage(|| {
            collect_handle_markers(
                ctx,
                &lane.sketch_entities,
                |marker| marker,
                feature,
                |marker| {
                    Ok(marker.local_id() != Some(0)
                        && marker.coordinates_m.is_some()
                        && matches!(
                            marker.kind(),
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        ))
                },
                &mut markers,
            )
        })?;
    }
    sort_handle_markers(ctx, &mut markers)?;

    let pair = if markers.len() % 2 == 0 {
        let pair_width = std::num::NonZeroUsize::new(2).ok_or_else(|| {
            ctx.refuse_codec_limit("select SLDPRT ordered circle-marker pair", 1, 0)
        })?;
        ctx.admit_iter(&markers, "select SLDPRT ordered circle-marker pair")?
            .chunks(pair_width)
            .nth(usize::from(index))
    } else {
        None
    };
    Ok((|| {
        let pair = pair?;
        let [center, radial] = pair else {
            return None;
        };
        let [cu, cv] = center.coordinates_m?.get();
        let [ru, rv] = radial.coordinates_m?.get();
        let radius = (ru - cu).hypot(rv - cv) * 1000.0;
        same_dimension_length(radius, expected_radius).then_some((*center, radius))
    })())
}

#[derive(Clone, Copy)]
pub(super) enum DeclaredEntityHandleOwner<'a> {
    Absent,
    Unique(&'a FeatureInputLane),
    Ambiguous,
}

pub(super) fn declared_entity_handle_owner<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    operand: &FeatureInputOperand,
) -> Result<DeclaredEntityHandleOwner<'a>, cadmpeg_core::CodecError> {
    let mut owner = None;
    let ambiguous = ctx.any_by(
        lanes,
        |lane| {
            let selected_reference = ctx.find_by(
                &lane.references,
                |reference| {
                    ctx.equal(
                        reference.id.as_str(),
                        operand.reference_ref.as_str(),
                        DIMENSIONED_HANDLE_OPERATION,
                    )
                },
                "find SLDPRT declared entity-handle reference",
            )?;
            let Some(reference) = selected_reference else {
                return Ok(false);
            };
            let Some(class_ref) = reference.class_ref.as_deref() else {
                return Ok(false);
            };
            let selected_class = ctx.find_by(
                &lane.classes,
                |class| ctx.equal(class.id.as_str(), class_ref, DIMENSIONED_HANDLE_OPERATION),
                "find SLDPRT declared entity-handle class",
            )?;
            let Some(class) = selected_class else {
                return Ok(false);
            };
            if class.name != "sgEntHandle" {
                return Ok(false);
            }
            if owner.is_some() {
                return Ok(true);
            }
            owner = Some(lane);
            Ok(false)
        },
        "scan SLDPRT declared entity-handle lanes",
    )?;
    if ambiguous {
        return Ok(DeclaredEntityHandleOwner::Ambiguous);
    }
    Ok(owner.map_or(
        DeclaredEntityHandleOwner::Absent,
        DeclaredEntityHandleOwner::Unique,
    ))
}

/// Resolve the circular-dimension center carried by a slot handle.
///
/// A slot is an aggregate boundary descriptor, not an independent circle. Its
/// radial dimension handle therefore identifies the slot marker first and a
/// selected center point second. The two exact `sgSlotHandle` reference cells
/// are required so a slot's center cannot be guessed from its radius or from
/// the slot's boundary roster alone.
pub(super) fn declared_slot_handle_dimension_center<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Result<Option<(&'a SketchInputEntity, &'a SketchInputEntity)>, cadmpeg_core::CodecError> {
    let Some(entity_ref) = operand.entity_ref.as_deref() else {
        return Ok(None);
    };
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(None);
    };
    let mut selected_marker = None;
    let mut steps = lane.sketch_entities.iter();
    while let Some(marker) =
        ctx.next_charged(&mut steps, "find SLDPRT declared slot-handle marker")?
    {
        if ctx.equal(marker.id(), entity_ref, DIMENSIONED_HANDLE_OPERATION)?
            && ctx.equal(
                &marker.feature_ref.as_deref(),
                &Some(feature),
                DIMENSIONED_HANDLE_OPERATION,
            )?
            && matches!(
                marker.kind(),
                SketchInputKind::Native(_) | SketchInputKind::NativeHandle(_)
            )
        {
            selected_marker = Some(marker);
            break;
        }
    }
    let Some(marker) = selected_marker else {
        return Ok(None);
    };
    let Ok(marker_offset) = usize::try_from(marker.offset()) else {
        return Ok(None);
    };
    let slots = SlotReferences::new(ctx, &lane.native_payload)?;
    let Some((_, center_indices)) = slot_curve_and_center_indices(ctx, &slots, marker_offset)?
    else {
        return Ok(None);
    };
    let class_ref = ctx
        .find_by(
            &lane.references,
            |reference| {
                ctx.equal(
                    reference.id.as_str(),
                    operand.reference_ref.as_str(),
                    DIMENSIONED_HANDLE_OPERATION,
                )
            },
            "find SLDPRT declared slot-handle reference",
        )?
        .and_then(|reference| reference.class_ref.as_deref());
    let Some(class_ref) = class_ref else {
        return Ok(None);
    };
    let entity_class = ctx.find_by(
        &lane.classes,
        |class| ctx.equal(class.id.as_str(), class_ref, DIMENSIONED_HANDLE_OPERATION),
        "find SLDPRT declared slot-handle class",
    )?;
    let Some(entity_class) = entity_class.filter(|class| class.name == "sgEntHandle") else {
        return Ok(None);
    };
    let bounds = (|| -> Result<_, cadmpeg_core::CodecError> {
        let mut classes = lane.classes.iter();
        let mut next_slot = || {
            ctx.find_by(
                &mut classes,
                |class| {
                    Ok(class.name == "sgSlotHandle"
                        && class.offset > entity_class.offset
                        && class.offset < marker.offset())
                },
                DIMENSIONED_HANDLE_OPERATION,
            )
        };
        let (Some(slot_class), None) = (next_slot()?, next_slot()?) else {
            return Ok(None);
        };
        let next_class = ctx.min_by_key(
            &lane.classes,
            |class| Ok((class.offset > slot_class.offset, class.offset)),
            |left, right| Ok(right.0.cmp(&left.0).then(left.1.cmp(&right.1))),
            "find SLDPRT slot-handle class end",
        )?;
        let class_end = next_class
            .filter(|class| class.offset > slot_class.offset)
            .map_or_else(
                || u64_from_index(lane.native_payload.len()),
                |class| class.offset,
            )
            .min(marker.offset());
        let Ok(class_start) = usize::try_from(slot_class.offset) else {
            return Ok(None);
        };
        let Ok(class_end) = usize::try_from(class_end) else {
            return Ok(None);
        };
        let Some(class_end) = super::DeclaredEnd::of(class_end, lane.native_payload.len()) else {
            return Ok(None);
        };
        Ok((class_start < class_end.get()).then_some((class_start, class_end.get())))
    })();
    let Some((class_start, class_end)) = bounds? else {
        return Ok(None);
    };
    let mut offsets = class_start..class_end;
    let mut next_reference = || {
        ctx.find_map(
            &mut offsets,
            |offset| {
                let Some(cell_end) = offset.checked_add(12) else {
                    return Ok(None);
                };
                if cell_end > class_end {
                    return Ok(None);
                }
                let Some(cell) = lane.native_payload.get(offset..cell_end) else {
                    return Ok(None);
                };
                if cell.get(..2) != Some(&[0xe7, 0x88])
                    || cell.get(4..8) != Some(&[0xff; 4])
                    || cell.get(8..12) != Some(&[0; 4])
                {
                    return Ok(None);
                }
                Ok(View::u16_le_at(&lane.native_payload, offset + 2).map(usize::from))
            },
            DIMENSIONED_HANDLE_OPERATION,
        )
    };
    let (Some(slot_index), Some(center_index), None) =
        (next_reference()?, next_reference()?, next_reference()?)
    else {
        return Ok(None);
    };
    let (Ok(slot_index), Ok(center_index)) =
        (u32::try_from(slot_index), u32::try_from(center_index))
    else {
        return Ok(None);
    };
    if slot_index != u32::from(operand.entity_index) || marker.local_id() != Some(slot_index) {
        return Ok(None);
    }
    let mut points = Vec::new();
    let mut points_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
    points_storage.with_storage(|| {
        collect_handle_markers(
            ctx,
            &lane.sketch_entities,
            |marker| marker,
            feature,
            |candidate| {
                Ok(candidate.coordinates_m.is_some()
                    && matches!(
                        candidate.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    ))
            },
            &mut points,
        )
    })?;
    sort_handle_markers(ctx, &mut points)?;

    let [first, second] = center_indices.map(|index| points.get(index).copied());
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(None);
    };
    let center = match (
        first.local_id() == Some(center_index),
        second.local_id() == Some(center_index),
    ) {
        (true, false) => first,
        (false, true) => second,
        _ => return Ok(None),
    };
    if center.coordinates_m.is_none() {
        return Ok(None);
    }
    Ok(Some((marker, center)))
}

/// Resolve the indexed point-pair form of a circular dimension.
///
/// The `6e 83` operand has no explicit sketch marker. Its `sgEntHandle`
/// reference scopes an ordered point roster, while the operand index selects
/// one adjacent center/radial pair. Every pair must carry the indexed
/// center-to-radial object/local join; a radius match cannot establish the
/// carrier on its own.
pub(super) fn declared_entity_handle_indexed_circle_dimension_center<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Result<Option<&'a SketchInputEntity>, cadmpeg_core::CodecError> {
    if operand.kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_836E)
        || operand.entity_ref.is_some()
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return Ok(None);
    }
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(None);
    };
    let mut markers = Vec::new();
    let mut markers_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
    markers_storage.with_storage(|| {
        collect_handle_markers(
            ctx,
            &lane.sketch_entities,
            |marker| marker,
            feature,
            |marker| {
                // Stored marker coordinates are finite.
                let coordinates_are_finite = marker.coordinates_m.is_some();
                Ok(coordinates_are_finite
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    ))
            },
            &mut markers,
        )
    })?;
    sort_handle_markers(ctx, &mut markers)?;
    if !markers.chunks_exact(2).remainder().is_empty() {
        return Ok(None);
    }
    if !ctx.all_by(
        markers.chunks_exact(2),
        |pair| {
            let [center, radial] = pair else {
                return Ok(false);
            };
            Ok(center
                .local_id()
                .is_some_and(|local_id| local_id != 0 && radial.object_index() == Some(local_id))
                && radial.local_id().is_some_and(|local_id| local_id != 0))
        },
        DIMENSIONED_HANDLE_OPERATION,
    )? {
        return Ok(None);
    }
    let pair_width = std::num::NonZeroUsize::new(2)
        .ok_or_else(|| ctx.refuse_codec_limit("select SLDPRT indexed circle-marker pair", 1, 0))?;
    let selected_pair = ctx
        .admit_iter(&markers, "select SLDPRT indexed circle-marker pair")?
        .chunks(pair_width)
        .nth(usize::from(operand.entity_index));
    Ok((|| {
        let pair = selected_pair?;
        let [center, radial] = pair else {
            return None;
        };
        let [cu, cv] = center.coordinates_m?.get();
        let [ru, rv] = radial.coordinates_m?.get();
        let radius = (ru - cu).hypot(rv - cv) * 1000.0;
        same_dimension_length(radius, expected_radius).then_some(*center)
    })())
}

fn point_dimension_marker_matches_operand(
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    feature: &str,
    operand: &FeatureInputOperand,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(entity_ref) = operand.entity_ref.as_deref() else {
        return Ok(false);
    };
    let address = u32::from(operand.entity_index);
    let identity_matches = match operand.kind {
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C) => {
            marker.object_index() == Some(address)
        }
        FeatureInputOperandKind::Native(_) => marker.local_id() == Some(address),
        _ => false,
    };
    Ok(
        ctx.equal(marker.id(), entity_ref, DIMENSIONED_HANDLE_OPERATION)?
            && ctx.equal(
                &marker.feature_ref.as_deref(),
                &Some(feature),
                DIMENSIONED_HANDLE_OPERATION,
            )?
            && identity_matches
            && matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ),
    )
}

/// Resolve the explicit point-center form of a circular dimension.
///
/// A native operand tag carries the point identity in the resolved operand
/// reference. The reference must resolve to a point marker in the unique
/// `sgEntHandle` lane. Native tag `4c 81` uses the marker's feature-local
/// object index as the identity; other native point identities use the local
/// identifier. No radius-based pair or marker-family fallback is valid for
/// this explicit identity form.
pub(super) fn declared_entity_handle_point_dimension_center<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Result<Option<&'a SketchInputEntity>, cadmpeg_core::CodecError> {
    if !matches!(operand.kind, FeatureInputOperandKind::Native(_)) {
        return Ok(None);
    }
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(None);
    };
    Ok(ctx
        .find_by(
            &lane.sketch_entities,
            |marker| point_dimension_marker_matches_operand(ctx, marker, feature, operand),
            "scan SLDPRT explicit point-dimension markers",
        )?
        .filter(|marker| marker.coordinates_m.is_some()))
}

/// Resolve a classless direct point identity for a circular dimension.
///
/// Some circular dimensions carry a reference cell and an explicit point
/// marker without an `sgEntHandle` class declaration. The reference kind,
/// feature, object index, marker identity, and marker-local address must all
/// agree. A point that is the radial member of an encoded center/radial pair
/// at the dimension's radius is not a circle center.
pub(super) fn direct_point_dimension_center<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Result<Option<&'a SketchInputEntity>, cadmpeg_core::CodecError> {
    if !matches!(operand.kind, FeatureInputOperandKind::Native(_))
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return Ok(None);
    }
    let mut unique = None;
    if !ctx.all_by(
        lanes,
        |lane| {
            let matched_reference = ctx.find_by(
                &lane.references,
                |reference| {
                    ctx.equal(
                        &reference.id,
                        &operand.reference_ref,
                        DIMENSIONED_HANDLE_OPERATION,
                    )
                },
                "find SLDPRT direct point-dimension reference",
            )?;
            let Some(reference) = matched_reference else {
                return Ok(true);
            };
            if !ctx.equal(
                &reference.feature_ref.as_deref(),
                &Some(feature),
                DIMENSIONED_HANDLE_OPERATION,
            )? || reference.kind != operand.kind
                || reference.object_index != operand.entity_index
                || reference.class_ref.is_some()
            {
                return Ok(true);
            }
            let found = ctx.find_by(
                &lane.sketch_entities,
                |marker| point_dimension_marker_matches_operand(ctx, marker, feature, operand),
                "scan SLDPRT direct point-dimension markers",
            )?;
            let Some(marker) = found else {
                return Ok(true);
            };
            if marker.coordinates_m.is_none() {
                return Ok(true);
            }
            if unique.is_some() {
                return Ok(false);
            }
            unique = Some((lane, marker));

            Ok(true)
        },
        "scan SLDPRT direct point-dimension lanes",
    )? {
        return Ok(None);
    }
    let Some((lane, marker)) = unique else {
        return Ok(None);
    };
    let pairs_scope = ctx.with_scoped_storage("SLDPRT declared point-dimension pairs", || {
        declared_entity_handle_pairs(ctx, lane, feature)
    })?;
    if !ctx.all_by(
        &pairs_scope.0,
        |[center, radial]| {
            if !ctx.equal(radial.id(), marker.id(), DIMENSIONED_HANDLE_OPERATION)? {
                return Ok(true);
            }

            let Some([cu, cv]) = center
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                return Ok(false);
            };
            let Some([ru, rv]) = radial
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                return Ok(false);
            };
            if same_dimension_length((ru - cu).hypot(rv - cv) * 1000.0, expected_radius) {
                return Ok(false);
            }

            Ok(true)
        },
        "check declared SLDPRT point-dimension pairs",
    )? {
        return Ok(None);
    }
    Ok(Some(marker))
}

pub(super) fn declared_entity_handle_circular_marker<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Result<Option<(&'a SketchInputEntity, f64)>, cadmpeg_core::CodecError> {
    if !expected_radius.is_finite() || expected_radius <= 0.0 {
        return Ok(None);
    }
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(None);
    };
    let child_pairs_scope = ctx
        .with_scoped_storage("SLDPRT declared circular-marker child pairs", || {
            declared_entity_handle_declared_child_pairs(ctx, lane, feature)
        })?;
    let pairs_scope = ctx.with_scoped_storage("SLDPRT declared circular-marker pairs", || {
        declared_entity_handle_pairs(ctx, lane, feature)
    })?;
    // An explicit radial identity is stronger than one feature-scoped child
    // declaration. Resolve it first because an unrelated line or arc child
    // can coexist with the circular-dimension point pair. Multiple child
    // declarations remain ambiguous, even when one point pair also matches.
    if child_pairs_scope.0.len() <= 1 {
        if let Some(entity_ref) = operand.entity_ref.as_deref() {
            let mut unique = None;
            if !ctx.all_by(
                &pairs_scope.0,
                |[center, radial]| {
                    if !ctx.equal(radial.id(), entity_ref, DIMENSIONED_HANDLE_OPERATION)? {
                        return Ok(true);
                    }
                    if unique.is_some() {
                        return Ok(false);
                    }
                    unique = Some([*center, *radial]);

                    Ok(true)
                },
                "resolve explicit SLDPRT circular-marker pairs",
            )? {
                return Ok(None);
            }
            if let Some([center, radial]) = unique {
                return Ok((|| {
                    let [cu, cv] = center.coordinates_m?.get();
                    let [ru, rv] = radial.coordinates_m?.get();
                    let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                    same_dimension_length(radius, expected_radius).then_some((center, radius))
                })());
            }
        }
    }
    if let [child_pair] = child_pairs_scope.0.as_slice() {
        // The relation operand identifies the radial child when present. Use
        // that identity to reject a mismatched child. The scoped child
        // declaration already identifies this pair, so unrelated linked
        // pairs in the same feature do not make it ambiguous.
        let operand_identifies_child = if let Some(entity_ref) = operand.entity_ref.as_deref() {
            ctx.equal(child_pair[1].id(), entity_ref, DIMENSIONED_HANDLE_OPERATION)?
        } else {
            false
        };
        if operand.entity_ref.is_some() && !operand_identifies_child {
            return Ok(None);
        }
        let [center, radial] = *child_pair;

        return Ok((|| {
            let [cu, cv] = center.coordinates_m?.get();
            let [ru, rv] = radial.coordinates_m?.get();
            let radius = (ru - cu).hypot(rv - cv) * 1000.0;
            same_dimension_length(radius, expected_radius).then_some((center, radius))
        })());
    }
    if !child_pairs_scope.0.is_empty() {
        return Ok(None);
    }
    let mut unique = None;
    if !ctx.all_by(
        &pairs_scope.0,
        |[center, radial]| {
            let Some([cu, cv]) = center
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                return Ok(true);
            };
            let Some([ru, rv]) = radial
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                return Ok(true);
            };
            let radius = (ru - cu).hypot(rv - cv) * 1000.0;
            if !same_dimension_length(radius, expected_radius) {
                return Ok(true);
            }
            if unique.is_some() {
                return Ok(false);
            }
            unique = Some((*center, radius));

            Ok(true)
        },
        "resolve SLDPRT circular-marker pairs",
    )? {
        return Ok(None);
    }
    Ok(unique)
}

pub(super) fn declared_entity_handle_has_resolved_pair(
    ctx: &DecodeContext<'_>,
    lanes: &[FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Result<bool, cadmpeg_core::CodecError> {
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(false);
    };
    let pairs_scope = ctx.with_scoped_storage("SLDPRT resolved entity-handle pairs", || {
        declared_entity_handle_pairs(ctx, lane, feature)
    })?;
    Ok(!pairs_scope.0.is_empty())
}

/// Test whether an explicit point reference is the radial member of a
/// declared entity-handle pair. Radial identity cannot be reinterpreted as a
/// center by the native point-identity fallback.
pub(super) fn declared_entity_handle_point_is_declared_radial(
    ctx: &DecodeContext<'_>,
    lanes: &[FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(entity_ref) = operand.entity_ref.as_deref() else {
        return Ok(false);
    };
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(false);
    };
    let pairs_scope = ctx.with_scoped_storage("SLDPRT declared radial identity pairs", || {
        declared_entity_handle_pairs(ctx, lane, feature)
    })?;
    ctx.any_by(
        &pairs_scope.0,
        |[_, radial]| ctx.equal(radial.id(), entity_ref, DIMENSIONED_HANDLE_OPERATION),
        "check SLDPRT declared radial identities",
    )
}

fn declared_entity_handle_pairs<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Result<Vec<[&'a SketchInputEntity; 2]>, cadmpeg_core::CodecError> {
    let mut pairs = declared_entity_handle_linked_pairs(ctx, lane, feature)?;
    let children = declared_entity_handle_declared_child_pairs(ctx, lane, feature)?;
    ctx.extend_vec(&mut pairs, children, DIMENSIONED_HANDLE_OPERATION)?;
    let indexed = declared_entity_handle_indexed_point_pairs(ctx, lane, feature)?;
    ctx.extend_vec(&mut pairs, indexed, DIMENSIONED_HANDLE_OPERATION)?;
    ctx.sort_unstable_by_key(
        &mut pairs,
        |value| {
            let [left_center, left_radial] = value;
            (left_center.offset(), left_radial.offset())
        },
        Ord::cmp,
        "sldprt declared entity handle pairs sort",
    )?;
    ctx.dedup_by(
        &mut pairs,
        |left, right| {
            Ok(
                ctx.equal(left[0].id(), right[0].id(), DIMENSIONED_HANDLE_OPERATION)?
                    && ctx.equal(left[1].id(), right[1].id(), DIMENSIONED_HANDLE_OPERATION)?,
            )
        },
        DIMENSIONED_HANDLE_OPERATION,
    )?;
    Ok(pairs)
}

/// Resolve the indexed point form used by a declared entity handle when the
/// radial point carries its own local identifier. The adjacent roster order
/// and the center-to-radial object/local join are both required; a radius
/// match alone is not a carrier identity.
fn declared_entity_handle_indexed_point_pairs<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Result<Vec<[&'a SketchInputEntity; 2]>, cadmpeg_core::CodecError> {
    let mut markers = Vec::new();
    let mut markers_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
    markers_storage.with_storage(|| {
        collect_handle_markers(
            ctx,
            &lane.sketch_entities,
            |marker| marker,
            feature,
            |marker| {
                Ok(marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    ))
            },
            &mut markers,
        )
    })?;
    sort_handle_markers(ctx, &mut markers)?;
    let mut pairs = Vec::new();
    for pair in ctx
        .admit_iter(&markers, DIMENSIONED_HANDLE_OPERATION)?
        .windows(
            std::num::NonZeroUsize::new(2)
                .ok_or_else(|| ctx.refuse_codec_limit(DIMENSIONED_HANDLE_OPERATION, 1, 0))?,
        )
    {
        let [center, radial] = pair else {
            continue;
        };
        let Some(center_local_id) = center.local_id() else {
            continue;
        };
        if center_local_id == 0
            || radial.object_index() != Some(center_local_id)
            || radial.local_id().is_none_or(|local_id| local_id == 0)
        {
            continue;
        }
        ctx.reserve_vec(&mut pairs, 1, DIMENSIONED_HANDLE_OPERATION)?;
        pairs.push([*center, *radial]);
    }
    Ok(pairs)
}

/// Resolve the wide child form where a curve marker is followed by its radial
/// point and the point interval declares the curve handle class. The class
/// declaration is scoped to the following marker interval; a radius match
/// alone is not sufficient because the same feature can contain repeated
/// circular construction carriers.
fn declared_entity_handle_declared_child_pairs<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Result<Vec<[&'a SketchInputEntity; 2]>, cadmpeg_core::CodecError> {
    let mut feature_markers = Vec::new();
    let mut feature_markers_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
    feature_markers_storage.with_storage(|| {
        collect_handle_markers(
            ctx,
            &lane.sketch_entities,
            |marker| marker,
            feature,
            |_| Ok(true),
            &mut feature_markers,
        )
    })?;
    sort_handle_markers(ctx, &mut feature_markers)?;
    let mut markers = Vec::new();
    let mut markers_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
    markers_storage.with_storage(|| {
        collect_handle_markers(
            ctx,
            &feature_markers,
            |marker| *marker,
            feature,
            |marker| {
                Ok(marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    ))
            },
            &mut markers,
        )
    })?;
    sort_handle_markers(ctx, &mut markers)?;
    let mut pairs = Vec::new();
    for pair in ctx
        .admit_iter(&markers, DIMENSIONED_HANDLE_OPERATION)?
        .windows(
            std::num::NonZeroUsize::new(2)
                .ok_or_else(|| ctx.refuse_codec_limit(DIMENSIONED_HANDLE_OPERATION, 1, 0))?,
        )
    {
        let [center, radial] = pair else {
            continue;
        };
        let class_name = match center.kind() {
            SketchInputKind::Arc => "sgArcHandle",
            SketchInputKind::LineOrCircle => "sgLineHandle",
            _ => continue,
        };
        if !matches!(
            radial.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        ) {
            continue;
        }
        let mut next_marker_offset = None;
        let mut steps = feature_markers.iter();
        while let Some(marker) =
            ctx.next_charged(&mut steps, "find next SLDPRT entity-handle child marker")?
        {
            if marker.offset() > radial.offset() {
                next_marker_offset = Some(marker.offset());
                break;
            }
        }
        let mut class_declares_child = false;
        let mut steps = lane.classes.iter();
        while let Some(class) =
            ctx.next_charged(&mut steps, "match SLDPRT entity-handle child class")?
        {
            if ctx.equal(
                class.name.as_str(),
                class_name,
                DIMENSIONED_HANDLE_OPERATION,
            )? && class.offset > radial.offset()
                && next_marker_offset.is_none_or(|end| class.offset < end)
            {
                class_declares_child = true;
                break;
            }
        }
        if !class_declares_child {
            continue;
        }
        ctx.reserve_vec(&mut pairs, 1, DIMENSIONED_HANDLE_OPERATION)?;
        pairs.push([*center, *radial]);
    }
    Ok(pairs)
}

fn declared_entity_handle_linked_pairs<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Result<Vec<[&'a SketchInputEntity; 2]>, cadmpeg_core::CodecError> {
    let mut markers = Vec::new();
    let mut markers_storage = ctx.reserve_scoped(0, DIMENSIONED_HANDLE_OPERATION)?;
    markers_storage.with_storage(|| {
        collect_handle_markers(
            ctx,
            &lane.sketch_entities,
            |marker| marker,
            feature,
            |marker| {
                Ok(marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    ))
            },
            &mut markers,
        )
    })?;
    sort_handle_markers(ctx, &mut markers)?;
    let mut pairs = Vec::new();
    for pair in ctx
        .admit_iter(&markers, DIMENSIONED_HANDLE_OPERATION)?
        .windows(
            std::num::NonZeroUsize::new(2)
                .ok_or_else(|| ctx.refuse_codec_limit(DIMENSIONED_HANDLE_OPERATION, 1, 0))?,
        )
    {
        let [center, radial] = pair else {
            continue;
        };
        if !matches!(
            radial.kind(),
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::LineOrCircle
        ) {
            continue;
        }
        let Some(center_local_id) = center.local_id() else {
            continue;
        };
        if center_local_id == 0
            || radial.object_index() != Some(center_local_id)
            || !matches!(radial.local_id(), None | Some(0))
        {
            continue;
        }
        ctx.reserve_vec(&mut pairs, 1, DIMENSIONED_HANDLE_OPERATION)?;
        pairs.push([*center, *radial]);
    }
    Ok(pairs)
}

pub(crate) fn project_relation_bindings(
    ctx: &DecodeContext<'_>,
    constraints: &mut Vec<SketchConstraint>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut sketches_by_feature_storage =
        ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut sketches_by_feature = HashMap::new();
    for feature in ctx.admit_iter(features, "index SLDPRT planar relation sketches")? {
        let operation = "index SLDPRT planar relation sketches";
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
        sketches_by_feature_storage.with_storage(|| {
            ctx.insert_hash_map(&mut sketches_by_feature, native_ref, sketch, operation)
        })?;
    }
    let (profile, _profile_storage) = ctx
        .with_scoped_storage("index SLDPRT planar relation entities", || {
            ProfileEntities::new(ctx, sketch_entities)
        })?;
    let (markers, _markers_storage) = ctx
        .with_scoped_storage("index SLDPRT planar relation markers", || {
            RelationMarkers::new(ctx, lanes)
        })?;
    let markers_by_id = markers.by_id();
    let ((transforms, loci_by_marker), _loci_storage) =
        ctx.with_scoped_storage("build SLDPRT planar relation marker loci", || {
            let transforms =
                marker_transform_candidates_in(ctx, features, sketches, &profile, lanes)?;
            let loci = profile_loci_in(ctx, features, &profile, &markers, lanes, &transforms)?;
            Ok::<_, cadmpeg_core::CodecError>((transforms, loci))
        })?;
    let index = RelationIndex {
        entities: &profile,
        markers: &markers,
        loci_by_marker: &loci_by_marker,
    };
    let relation_parameters_scope = ctx
        .with_scoped_storage("SLDPRT planar relation parameter ownership", || {
            owned_relation_parameters(ctx, features, parameters, lanes)
        })?;
    let mut parameters_by_id_storage = ctx.reserve_scoped(0, "SLDPRT relation geometry indexes")?;
    let mut parameters_by_id = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "index SLDPRT planar relation parameters")? {
        let operation = "index SLDPRT planar relation parameters";
        parameters_by_id_storage.with_storage(|| {
            ctx.insert_hash_map(&mut parameters_by_id, &parameter.id, parameter, operation)
        })?;
    }
    // The native-reference index keeps the earliest constraint for duplicate
    // references and is updated when this projection appends a constraint.
    let mut constraints_by_native_ref_storage =
        ctx.reserve_scoped(0, "index SLDPRT planar relation constraints")?;
    let mut constraints_by_native_ref = HashMap::<String, usize>::new();
    for (index, constraint) in ctx
        .admit_iter(&*constraints, "index SLDPRT planar relation constraints")?
        .enumerate()
    {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            let operation = "index SLDPRT planar relation constraints";
            if !ctx.contains_key_hash_map(&constraints_by_native_ref, native_ref, operation)? {
                let key = ctx.format_scoped_text(
                    &mut constraints_by_native_ref_storage,
                    format_args!("{native_ref}"),
                    "copy SLDPRT planar constraint reference",
                )?;
                constraints_by_native_ref_storage.with_storage(|| {
                    ctx.insert_hash_map(&mut constraints_by_native_ref, key, index, operation)
                })?;
            }
        }
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT planar relation lanes")? {
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "split SLDPRT planar relation lane identity")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in ctx.admit_iter(&lane.relation_instances, "scan SLDPRT planar relations")? {
            const ENTITY_SORT: &str = "sort SLDPRT planar relation entities";

            let existing = ctx
                .get_hash_map(
                    &constraints_by_native_ref,
                    relation.id.as_str(),
                    "resolve SLDPRT planar relation constraint",
                )?
                .copied();
            if existing.is_some_and(|index| {
                !matches!(
                    constraints[index].definition.kind(),
                    SketchConstraintDefinitionInput::Native { .. }
                )
            }) {
                continue;
            }
            let Some(parameter_id) = ctx.get_hash_map(
                &relation_parameters_scope.0,
                &relation.id,
                "resolve SLDPRT relation parameter ownership",
            )?
            else {
                continue;
            };
            let Some(sketch) = ctx.get_hash_map(
                &sketches_by_feature,
                relation.feature_ref.as_str(),
                "resolve SLDPRT planar relation sketch",
            )?
            else {
                continue;
            };
            let parameter = parameter_id
                .as_ref()
                .map(|parameter| {
                    ctx.get_hash_map(
                        &parameters_by_id,
                        parameter,
                        "resolve SLDPRT planar relation parameter",
                    )
                })
                .transpose()?
                .flatten()
                .copied();
            let reference_parameter = parameter
                .map(|parameter| is_reference_relation_parameter_in(ctx, parameter))
                .transpose()?
                .unwrap_or(false);
            let native_kind = relation_native_kind(relation.family);
            let mut entities = Vec::new();
            for marker in ctx
                .admit_iter(
                    &relation.operands,
                    "resolve SLDPRT planar relation operands",
                )?
                .filter_map(|operand| operand.entity_ref.as_deref())
            {
                ctx.extend_vec(
                    &mut entities,
                    marker_entities(
                        ctx,
                        marker,
                        markers_by_id,
                        &loci_by_marker,
                        MarkerEntityFilter::All,
                    )?,
                    "collect SLDPRT planar relation entities",
                )?;
            }
            ctx.sort_unstable_by(&mut entities, |value| value.as_str(), Ord::cmp, ENTITY_SORT)?;
            ctx.dedup_vec(&mut entities, ENTITY_SORT)?;
            let typed_definition = match relation.family {
                FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance => {
                    profile_axis_for_relation(
                        ctx,
                        relation,
                        ctx.get_hash_map(
                            &transforms,
                            relation.feature_ref.as_str(),
                            "resolve SLDPRT planar relation transforms",
                        )?
                        .map(Vec::as_slice),
                    )?
                    .map(|profile_axis| {
                        relation_definition(
                            ctx,
                            relation,
                            parameter,
                            sketch,
                            index,
                            Some(profile_axis),
                        )
                    })
                    .transpose()?
                    .flatten()
                }
                _ => relation_definition(ctx, relation, parameter, sketch, index, None)?,
            };
            let typed_definition = match typed_definition {
                Some(definition)
                    if reference_parameter
                        && relation_constraint_is_inactive_in(
                            ctx,
                            parameter,
                            &definition,
                            &profile,
                        )? =>
                {
                    None
                }
                definition => definition,
            };
            let definition = if let Some(definition) = typed_definition {
                definition
            } else {
                let mut operands = Vec::new();
                for operand in ctx.admit_iter(
                    &relation.operands,
                    "collect SLDPRT planar relation operands",
                )? {
                    let native_ref = operand
                        .entity_ref
                        .as_deref()
                        .map(|reference| {
                            ctx.format_retained(
                                format_args!("{reference}"),
                                "copy SLDPRT planar relation operand reference",
                            )
                        })
                        .transpose()?;
                    ctx.reserve_vec(&mut operands, 1, "collect SLDPRT planar relation operands")?;
                    operands.push(SketchNativeOperand {
                        native_kind: operand_kind_name(ctx, operand.kind)?,
                        field: None,
                        object_index: Some(u32::from(operand.entity_index)),
                        native_ref,
                    });
                }
                SketchConstraintDefinitionInput::Native {
                    native_kind,
                    native_state: None,
                    native_flags: None,
                    native_properties: std::collections::BTreeMap::new(),
                    entities,
                    parameter: parameter
                        .map(|parameter| copy_relation_parameter_id(ctx, &parameter.id))
                        .transpose()?,
                    operands,
                }
            };
            let active = relation_constraint_is_inactive_in(ctx, parameter, &definition, &profile)?
                .then_some(false);
            let has_display_scalar =
                relation_display_scalar_for_parameter(ctx, relation, lane)?.is_some();
            let Ok(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            else {
                continue;
            };
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                ),
                "format SLDPRT planar relation constraint identity",
            )?;
            ctx.charge_work(
                u64_from_index(id_text.len()),
                "validate SLDPRT planar relation constraint identity",
            )?;
            let projected = SketchConstraint {
                id: match SketchConstraintId::mint(id_text) {
                    Ok(id) => id,
                    Err(_) => continue,
                },
                sketch: copy_planar_sketch_id(ctx, sketch)?,
                definition,
                name: None,
                driving: relation
                    .parameter_scalar_ref()
                    .map(|_| true)
                    .or_else(|| has_display_scalar.then_some(false)),
                active,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(ctx.format_retained(
                    format_args!("{}", relation.id),
                    "copy SLDPRT planar relation reference",
                )?),
            };
            if let Some(index) = existing {
                if !matches!(
                    projected.definition.kind(),
                    SketchConstraintDefinitionInput::Native { .. }
                ) {
                    constraints[index] = projected;
                }
            } else {
                let operation = "index SLDPRT planar relation constraints";
                let key = ctx.format_scoped_text(
                    &mut constraints_by_native_ref_storage,
                    format_args!("{}", relation.id),
                    "copy SLDPRT planar constraint reference",
                )?;
                constraints_by_native_ref_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut constraints_by_native_ref,
                        key,
                        constraints.len(),
                        operation,
                    )
                })?;
                ctx.reserve_vec(constraints, 1, "append SLDPRT planar relation constraint")?;
                constraints.push(projected);
            }
        }
        for marker in ctx.admit_iter(
            &lane.sketch_entities,
            "project SLDPRT planar relation markers",
        )? {
            let existing = ctx
                .get_hash_map(
                    &constraints_by_native_ref,
                    marker.id(),
                    "resolve SLDPRT planar marker constraint",
                )?
                .copied();
            if existing.is_some_and(|index| {
                !matches!(
                    constraints[index].definition.kind(),
                    SketchConstraintDefinitionInput::Native { .. }
                )
            }) {
                continue;
            }
            let Some(feature_ref) = marker.feature_ref.as_deref() else {
                continue;
            };
            let Some(sketch) = ctx.get_hash_map(
                &sketches_by_feature,
                feature_ref,
                "resolve SLDPRT planar marker sketch",
            )?
            else {
                continue;
            };
            let Some(definition) = marker_relation_definition(ctx, marker, sketch, index)? else {
                continue;
            };
            let active = marker_relation_is_inactive_in(ctx, marker, &definition, &profile)?
                .then_some(false);
            let Ok(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            else {
                continue;
            };
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-constraint#marker:{lane_key}:{}",
                    marker.offset()
                ),
                "format SLDPRT planar marker constraint identity",
            )?;
            ctx.charge_work(
                u64_from_index(id_text.len()),
                "validate SLDPRT planar marker constraint identity",
            )?;
            let projected = SketchConstraint {
                id: match SketchConstraintId::mint(id_text) {
                    Ok(id) => id,
                    Err(_) => continue,
                },
                sketch: copy_planar_sketch_id(ctx, sketch)?,
                definition,
                name: None,
                driving: None,
                active,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT planar marker relation reference",
                )?),
            };
            if let Some(index) = existing {
                if !matches!(
                    projected.definition.kind(),
                    SketchConstraintDefinitionInput::Native { .. }
                ) {
                    constraints[index] = projected;
                }
            } else {
                let operation = "index SLDPRT planar relation constraints";
                let key = ctx.format_scoped_text(
                    &mut constraints_by_native_ref_storage,
                    format_args!("{}", marker.id()),
                    "copy SLDPRT planar constraint reference",
                )?;
                constraints_by_native_ref_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut constraints_by_native_ref,
                        key,
                        constraints.len(),
                        operation,
                    )
                })?;
                ctx.reserve_vec(constraints, 1, "append SLDPRT planar marker constraint")?;
                constraints.push(projected);
            }
        }
    }
    Ok(())
}

/// The first sketch record for each identity, in a scoped lookup table.
fn relation_sketch_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    sketches: &'a [cadmpeg_ir::sketches::Sketch],
) -> Result<
    (
        HashMap<&'a cadmpeg_ir::sketches::SketchId, &'a cadmpeg_ir::sketches::Sketch>,
        ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    const OPERATION: &str = "index SLDPRT relation sketch frames";
    ctx.with_scoped_storage(OPERATION, || {
        let mut records = HashMap::new();
        for record in ctx.admit_iter(sketches, OPERATION)? {
            ctx.entry_hash_map(&mut records, &record.id, OPERATION)?
                .or_insert(record);
        }
        Ok(records)
    })
}

fn copy_planar_sketch_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::sketches::SketchId,
) -> Result<cadmpeg_ir::sketches::SketchId, cadmpeg_core::CodecError> {
    id.try_clone_for_decode(ctx, "copy SLDPRT planar sketch identity")
}

fn copy_relation_parameter_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::features::ParameterId,
) -> Result<cadmpeg_ir::features::ParameterId, cadmpeg_core::CodecError> {
    id.try_clone_for_decode(ctx, "copy SLDPRT relation parameter identity")
}

fn claim_relation_parameter(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    claimed: &mut HashSet<cadmpeg_ir::features::ParameterId>,
    id: &cadmpeg_ir::features::ParameterId,
) -> Result<bool, cadmpeg_core::CodecError> {
    let operation = "claim SLDPRT relation parameter";
    if ctx.contains_hash_set(claimed, id, operation)? {
        return Ok(false);
    }
    storage.with_storage(|| {
        ctx.insert_hash_set(claimed, copy_relation_parameter_id(ctx, id)?, operation)
    })
}

fn record_relation_parameter(
    ctx: &DecodeContext<'_>,
    owned: &mut HashMap<String, Option<cadmpeg_ir::features::ParameterId>>,
    relation_id: &str,
    parameter: Option<&cadmpeg_ir::features::ParameterId>,
) -> Result<(), cadmpeg_core::CodecError> {
    let operation = "index SLDPRT relation parameter ownership";
    let relation_id = ctx.format_retained(
        format_args!("{relation_id}"),
        "copy SLDPRT relation identity",
    )?;
    let parameter = parameter
        .map(|id| copy_relation_parameter_id(ctx, id))
        .transpose()?;
    ctx.insert_hash_map(owned, relation_id, parameter, operation)?;
    Ok(())
}

/// Bind each relation instance to the design parameter it drives.
///
/// `lanes` holds the lanes themselves or references to them.
pub(crate) fn owned_relation_parameters<Lane>(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[Lane],
) -> Result<HashMap<String, Option<cadmpeg_ir::features::ParameterId>>, cadmpeg_core::CodecError>
where
    Lane: std::borrow::Borrow<FeatureInputLane>,
{
    let mut parameters_by_scalar_storage =
        ctx.reserve_scoped(0, "index SLDPRT relation scalars")?;
    let mut parameters_by_scalar = HashMap::<&str, _>::new();
    for parameter in ctx.admit_iter(parameters, "index SLDPRT relation scalars")? {
        let Some(native_ref) = parameter.native_ref.as_deref() else {
            continue;
        };
        let operation = "index SLDPRT relation scalars";
        parameters_by_scalar_storage.with_storage(|| {
            ctx.insert_hash_map(&mut parameters_by_scalar, native_ref, parameter, operation)
        })?;
    }
    let mut claimed_storage = ctx.reserve_scoped(0, "claim SLDPRT relation parameter")?;
    let mut claimed = HashSet::new();
    let mut owned = HashMap::new();
    for lane in ctx
        .admit_iter(lanes, "scan SLDPRT relation ownership lanes")?
        .map(|lane| -> &FeatureInputLane { std::borrow::Borrow::borrow(lane) })
    {
        for relation in
            ctx.admit_iter(&lane.relation_instances, "scan SLDPRT relation ownership")?
        {
            let Some(scalar) = relation.parameter_scalar_ref() else {
                continue;
            };
            let parameter = if let Some(parameter) = ctx.get_hash_map(
                &parameters_by_scalar,
                scalar,
                "scan SLDPRT relation ownership",
            )? {
                Some(&parameter.id)
            } else {
                relation_parameter_by_driving_name(ctx, relation, lane, features, parameters)?
                    .map(|parameter| &parameter.id)
            };
            if let Some(parameter) = parameter {
                claim_relation_parameter(ctx, &mut claimed_storage, &mut claimed, parameter)?;
            }
            record_relation_parameter(ctx, &mut owned, &relation.id, parameter)?;
        }
    }
    for lane in ctx
        .admit_iter(lanes, "scan SLDPRT relation ownership lanes")?
        .map(|lane| -> &FeatureInputLane { std::borrow::Borrow::borrow(lane) })
    {
        for relation in
            ctx.admit_iter(&lane.relation_instances, "scan SLDPRT relation ownership")?
        {
            if relation.parameter_scalar_ref().is_some() {
                continue;
            }
            let scalar_refs = relation.scalar_refs();
            let mut exact_match = None;
            let mut multiple_exact_matches = false;
            let mut steps = scalar_refs.iter();
            while let Some(scalar) =
                ctx.next_charged(&mut steps, "match SLDPRT relation scalar parameters")?
            {
                let Some(parameter) = ctx.get_hash_map(
                    &parameters_by_scalar,
                    scalar.as_str(),
                    "match SLDPRT relation scalar parameters",
                )?
                else {
                    continue;
                };
                if exact_match.is_some() {
                    multiple_exact_matches = true;
                    break;
                }
                exact_match = Some(*parameter);
            }
            if let Some(parameter) = exact_match.filter(|_| !multiple_exact_matches) {
                if claim_relation_parameter(ctx, &mut claimed_storage, &mut claimed, &parameter.id)?
                {
                    record_relation_parameter(ctx, &mut owned, &relation.id, Some(&parameter.id))?;
                }
                continue;
            }
            let mut parameter = relation_parameter_by_relation_id(ctx, relation, parameters)?;
            if parameter.is_none() {
                parameter =
                    relation_parameter_by_driving_name(ctx, relation, lane, features, parameters)?;
            }
            if parameter.is_none() {
                if let Some(scalar) = circle_dimension_handle_driver(ctx, relation, lane)? {
                    parameter = ctx
                        .get_hash_map(
                            &parameters_by_scalar,
                            scalar.id.as_str(),
                            "scan SLDPRT relation ownership",
                        )?
                        .copied();
                }
            }
            if parameter.is_none() {
                parameter =
                    relation_parameter_by_display_name(ctx, relation, lane, features, parameters)?;
            }
            let Some(parameter) = parameter else {
                continue;
            };
            if claim_relation_parameter(ctx, &mut claimed_storage, &mut claimed, &parameter.id)? {
                record_relation_parameter(ctx, &mut owned, &relation.id, Some(&parameter.id))?;
            }
        }
    }
    Ok(owned)
}

/// The first lane scalar with this identity, found by a search that stops at it.
fn lane_scalar<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    id: &str,
) -> Result<Option<&'a FeatureInputScalar>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT display relation scalar";
    ctx.find_by(
        &lane.scalars,
        |scalar| ctx.equal(scalar.id.as_str(), id, OPERATION),
        OPERATION,
    )
}

fn relation_display_scalar<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &'a FeatureInputLane,
) -> Result<Option<&'a FeatureInputScalar>, cadmpeg_core::CodecError> {
    if let Some(display_id) = relation.display_scalar_ref() {
        return Ok(lane_scalar(ctx, lane, display_id)?
            .filter(|scalar| scalar.role == FeatureInputScalarRole::Display));
    }
    let mut candidate = None;
    let scalar_refs = relation.scalar_refs();
    if !ctx.all_by(
        scalar_refs,
        |scalar_id| {
            let Some(scalar) = lane_scalar(ctx, lane, scalar_id)?
                .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
            else {
                return Ok(true);
            };
            if candidate.is_some() {
                return Ok(false);
            }
            candidate = Some(scalar);

            Ok(true)
        },
        "scan SLDPRT display relation scalar references",
    )? {
        return Ok(None);
    }
    Ok(candidate)
}

pub(super) fn relation_display_scalar_for_parameter<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &'a FeatureInputLane,
) -> Result<Option<&'a FeatureInputScalar>, cadmpeg_core::CodecError> {
    if let Some(scalar) = relation_display_scalar(ctx, relation, lane)? {
        return Ok(Some(scalar));
    }
    if relation.family != FeatureInputRelationFamily::CircleDiameter
        || relation.parameter_scalar_ref().is_some()
        || relation.display_scalar_ref().is_some()
        || relation.scalar_refs().len() < 2
        || relation.operands.len() != 1
    {
        return Ok(None);
    }
    let mut scalars_storage = ctx.reserve_scoped(0, "collect SLDPRT display relation scalars")?;
    let mut scalars = Vec::new();
    let scalar_refs = relation.scalar_refs();
    for scalar_id in ctx.admit_iter(
        scalar_refs,
        "scan SLDPRT display relation scalar references",
    )? {
        if let Some(scalar) = lane_scalar(ctx, lane, scalar_id)? {
            scalars_storage.with_storage(|| {
                ctx.push_vec(
                    &mut scalars,
                    scalar,
                    "collect SLDPRT display relation scalars",
                )
            })?;
        }
    }
    let Some(&first) = scalars.first() else {
        return Ok(None);
    };
    let adjacent_count = scalars.len().checked_sub(1).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "check SLDPRT display scalar ordinals",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let mut has_nonadjacent_ordinal = false;
    let mut steps = scalars[..adjacent_count].iter().enumerate();
    while let Some((index, first)) =
        ctx.next_charged(&mut steps, "check SLDPRT display scalar ordinals")?
    {
        let second = &scalars[index + 1];
        let next_ordinal = if first.ordinal == u32::MAX {
            first.ordinal
        } else {
            first.ordinal + 1
        };
        if second.ordinal != next_ordinal {
            has_nonadjacent_ordinal = true;
            break;
        }
    }
    if has_nonadjacent_ordinal {
        return Ok(None);
    }
    let first_name = ctx
        .find_by(
            &lane.names,
            |name| ctx.equal(&name.id, &first.name, "resolve SLDPRT display scalar name"),
            "resolve SLDPRT display scalar name",
        )?
        .map(|name| name.value.as_str());
    let Some(first_name) = first_name else {
        return Ok(None);
    };
    let Some(first_kind) = first.operands.first().map(|operand| operand.kind) else {
        return Ok(None);
    };
    let mut entity_indices = std::collections::BTreeSet::new();
    let mut indices_storage = ctx.reserve_scoped(0, "index SLDPRT display relation entities")?;
    if !ctx.all_by(
        &scalars,
        |scalar| {
            let [operand] = scalar.operands.as_slice() else {
                return Ok(false);
            };
            let name = ctx
                .find_by(
                    &lane.names,
                    |candidate| {
                        ctx.equal(
                            &candidate.id,
                            &scalar.name,
                            "resolve SLDPRT display scalar name",
                        )
                    },
                    "resolve SLDPRT display scalar name",
                )?
                .map(|candidate| candidate.value.as_str());
            let Some(name) = name else {
                return Ok(false);
            };
            if scalar.role != FeatureInputScalarRole::Display
                || operand.kind != first_kind
                || operand.kind != relation.operands[0].kind
                || !ctx.equal(name, first_name, "compare SLDPRT display scalar names")?
                || ctx.contains_btree_set(
                    &entity_indices,
                    &operand.entity_index,
                    "match SLDPRT display relation entities",
                )?
            {
                return Ok(false);
            }
            indices_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut entity_indices,
                    operand.entity_index,
                    "index SLDPRT display relation entities",
                )
            })?;

            Ok(true)
        },
        "scan SLDPRT display relation scalars",
    )? {
        return Ok(None);
    }
    Ok((scalars.len() == relation.scalar_refs().len()).then_some(first))
}

fn relation_parameter_by_relation_id<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Result<Option<&'a cadmpeg_ir::features::DesignParameter>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "scan SLDPRT relation ownership";
    let mut remaining = parameters.iter();
    let mut next = || {
        ctx.find_by(
            &mut remaining,
            |parameter| {
                let Some(value) = ctx.get_btree_map(
                    &parameter.properties,
                    RELATION_PARAMETER_ID_PROPERTY,
                    OPERATION,
                )?
                else {
                    return Ok(false);
                };
                Ok(ctx.equal(value, &relation.id, OPERATION)?
                    && is_reference_relation_parameter_in(ctx, parameter)?)
            },
            OPERATION,
        )
    };
    let first = next()?;
    if next()?.is_some() {
        return Ok(None);
    }
    Ok(first)
}

fn relation_parameter_matches_display_scalar(
    parameter: &cadmpeg_ir::features::DesignParameter,
    family: FeatureInputRelationFamily,
    scalar: &FeatureInputScalar,
) -> bool {
    match family {
        FeatureInputRelationFamily::Angle => match parameter.value.as_ref() {
            Some(cadmpeg_ir::features::ParameterValue::Angle(value)) => {
                same_dimension_angle(value.get(), scalar.value.get())
            }
            Some(cadmpeg_ir::features::ParameterValue::Real(value)) => {
                same_dimension_angle(value.get(), scalar.value.get())
            }
            _ => false,
        },
        FeatureInputRelationFamily::CircleDiameter
        | FeatureInputRelationFamily::LineLineDistance
        | FeatureInputRelationFamily::PointPointDistance
        | FeatureInputRelationFamily::PointLineDistance
        | FeatureInputRelationFamily::PointPointHorizontalDistance
        | FeatureInputRelationFamily::PointPointVerticalDistance => {
            match parameter.value.as_ref() {
                Some(cadmpeg_ir::features::ParameterValue::Length(value)) => {
                    same_dimension_length(value.get(), scalar.value.get() * 1000.0)
                }
                Some(cadmpeg_ir::features::ParameterValue::Integer(value)) => {
                    crate::history::parameters::eval::exact_integer_f64(*value).is_some_and(
                        |value| same_dimension_length(value, scalar.value.get() * 1000.0),
                    )
                }
                // An untyped native real is still in the source scalar's SI
                // units until relation typing applies the family unit.
                Some(cadmpeg_ir::features::ParameterValue::Real(value)) => {
                    same_dimension_length(value.get(), scalar.value.get())
                }
                _ => false,
            }
        }
    }
}

fn relation_parameter_by_driving_name<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Result<Option<&'a cadmpeg_ir::features::DesignParameter>, cadmpeg_core::CodecError> {
    let owner = ctx.find_by(
        features,
        |feature| {
            ctx.equal(
                &feature.native_ref.as_deref(),
                &Some(relation.feature_ref.as_str()),
                "find SLDPRT relation feature",
            )
        },
        "find SLDPRT relation feature",
    )?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    let owner = &owner.id;
    let mut scalars = HashMap::new();
    for scalar in ctx.admit_iter(&lane.scalars, "index SLDPRT relation driving scalars")? {
        let operation = "index SLDPRT relation driving scalars";
        ctx.insert_hash_map(&mut scalars, scalar.id.as_str(), scalar, operation)?;
    }
    let mut names = HashMap::new();
    for name in ctx.admit_iter(&lane.names, "index SLDPRT relation driving names")? {
        let operation = "index SLDPRT relation driving names";
        ctx.insert_hash_map(&mut names, name.id.as_str(), name.value.as_str(), operation)?;
    }
    let mut name = None;
    if let Some(scalar_id) = relation.parameter_scalar_ref() {
        if let Some(scalar) =
            ctx.get_hash_map(&scalars, scalar_id, "resolve SLDPRT driving scalar")?
        {
            if scalar.role == FeatureInputScalarRole::Driving {
                name = ctx
                    .get_hash_map(&names, scalar.name.as_str(), "resolve SLDPRT driving name")?
                    .copied();
            }
        }
    }
    let scalar_refs = relation.scalar_refs();
    if !ctx.all_by(
        scalar_refs,
        |scalar_id| {
            let Some(scalar) = ctx.get_hash_map(
                &scalars,
                scalar_id.as_str(),
                "scan SLDPRT relation driving scalar references",
            )?
            else {
                return Ok(true);
            };
            if scalar.role != FeatureInputScalarRole::Driving {
                return Ok(true);
            }
            let Some(candidate) = ctx
                .get_hash_map(
                    &names,
                    scalar.name.as_str(),
                    "scan SLDPRT relation driving scalar references",
                )?
                .copied()
            else {
                return Ok(true);
            };
            if let Some(current) = name {
                if !ctx.equal(candidate, current, "compare SLDPRT relation driving names")? {
                    return Ok(false);
                }
            } else {
                name = Some(candidate);
            }

            Ok(true)
        },
        "scan SLDPRT relation driving scalar references",
    )? {
        return Ok(None);
    }
    let Some(name) = name else {
        return Ok(None);
    };
    let mut candidates = parameters.iter();
    let mut first = None;
    while let Some(parameter) =
        ctx.next_charged(&mut candidates, "find SLDPRT driving relation parameter")?
    {
        let owner_matches = if let Some(parameter_owner) = parameter.owner.as_ref() {
            ctx.equal(
                parameter_owner,
                owner,
                "match SLDPRT driving relation owner",
            )?
        } else {
            false
        };
        if owner_matches
            && ctx.equal(
                parameter.name.as_str(),
                name,
                "match SLDPRT driving relation name",
            )?
        {
            first = Some(parameter);
            break;
        }
    }
    let Some(parameter) = first else {
        return Ok(None);
    };
    while let Some(candidate) =
        ctx.next_charged(&mut candidates, "find SLDPRT driving relation parameter")?
    {
        let owner_matches = if let Some(parameter_owner) = candidate.owner.as_ref() {
            ctx.equal(
                parameter_owner,
                owner,
                "match SLDPRT driving relation owner",
            )?
        } else {
            false
        };
        if owner_matches
            && ctx.equal(
                candidate.name.as_str(),
                name,
                "match SLDPRT driving relation name",
            )?
        {
            return Ok(None);
        }
    }
    Ok(Some(parameter))
}

pub(super) fn relation_parameter_by_display_name<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Result<Option<&'a cadmpeg_ir::features::DesignParameter>, cadmpeg_core::CodecError> {
    let owner = ctx.find_by(
        features,
        |feature| {
            ctx.equal(
                &feature.native_ref.as_deref(),
                &Some(relation.feature_ref.as_str()),
                "find SLDPRT relation feature",
            )
        },
        "find SLDPRT relation feature",
    )?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    let owner = &owner.id;
    let mut names = HashMap::new();
    for name in ctx.admit_iter(&lane.names, "index SLDPRT relation display names")? {
        let operation = "index SLDPRT relation display names";
        ctx.insert_hash_map(&mut names, name.id.as_str(), name.value.as_str(), operation)?;
    }
    let Some(display_scalar) = relation_display_scalar_for_parameter(ctx, relation, lane)? else {
        return Ok(None);
    };
    let Some(name) = ctx
        .get_hash_map(
            &names,
            display_scalar.name.as_str(),
            "resolve SLDPRT display relation name",
        )?
        .copied()
    else {
        return Ok(None);
    };
    let mut candidates = parameters.iter();
    let mut first = None;
    while let Some(parameter) =
        ctx.next_charged(&mut candidates, "find SLDPRT display relation parameter")?
    {
        let owner_matches = if let Some(parameter_owner) = parameter.owner.as_ref() {
            ctx.equal(
                parameter_owner,
                owner,
                "match SLDPRT display relation owner",
            )?
        } else {
            false
        };
        if owner_matches
            && ctx.equal(
                parameter.name.as_str(),
                name,
                "match SLDPRT display relation name",
            )?
        {
            first = Some(parameter);
            break;
        }
    }
    let Some(first) = first else {
        return Ok(None);
    };
    let mut all_ids_match = true;
    while let Some(parameter) =
        ctx.next_charged(&mut candidates, "find SLDPRT display relation parameter")?
    {
        let owner_matches = if let Some(parameter_owner) = parameter.owner.as_ref() {
            ctx.equal(
                parameter_owner,
                owner,
                "match SLDPRT display relation owner",
            )?
        } else {
            false
        };
        let name_matches = if owner_matches {
            ctx.equal(
                parameter.name.as_str(),
                name,
                "match SLDPRT display relation name",
            )?
        } else {
            false
        };
        if name_matches
            && !ctx.equal(
                &parameter.id,
                &first.id,
                "compare SLDPRT display relation parameter ids",
            )?
        {
            all_ids_match = false;
            break;
        }
    }
    Ok((all_ids_match
        && relation_parameter_matches_display_scalar(first, relation.family, display_scalar))
    .then_some(first))
}

#[cfg(test)]
mod relation_geometry_tests {
    use super::super::relation_loci::same_dimension_length;
    use super::{
        indexed_geometry_ref_matches, project_relation_bindings,
        project_relation_solved_line_geometry, project_relation_solved_point_geometry,
        project_spatial_relation_bindings, spatial_point_line_distance, unique_dynamic_line_pair,
    };
    use crate::records::operand_tag::NativeOperandTag;
    use crate::records::FeatureInputLane;
    use crate::records::FeatureInputOperand;
    use crate::records::FeatureInputOperandKind;
    use crate::records::FeatureInputRelationFamily;
    use crate::records::FeatureInputRelationInstance;
    use crate::records::FeatureInputScalar;
    use crate::records::FeatureInputScalarRole;
    use crate::records::SketchInputEntity;
    use crate::records::SketchInputKind;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
    use cadmpeg_ir::sketches::SketchEntity;
    use cadmpeg_ir::sketches::SketchEntityId;
    use cadmpeg_ir::sketches::SketchGeometry;
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
    use std::collections::HashMap;

    const TEST_LINE_GEOMETRY_QUANTUM: f64 = 1.0 / 100_000_000.0;

    #[test]
    fn indexed_geometry_refs_match_only_canonical_exact_ids() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"indexed geometry references",
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        assert!(indexed_geometry_ref_matches(
            &ctx,
            "feature:solver-line:12",
            "feature",
            ":solver-line:",
            12,
        )
        .unwrap());
        for value in [
            "feature-extra:solver-line:12",
            "feature:solver-line:012",
            "feature:solver-line:+12",
            "feature:solver-line:12-extra",
            "feature:operand:12",
        ] {
            assert!(
                !indexed_geometry_ref_matches(&ctx, value, "feature", ":solver-line:", 12,)
                    .unwrap()
            );
        }
    }

    #[test]
    fn solver_point_relation_projects_graph_resolved_operands() {
        use cadmpeg_ir::sketches::{Sketch, SketchLocus, SketchPlacement};
        use cadmpeg_ir::{
            features::{
                Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
                ParameterValue,
            },
            scalar::Length,
        };
        use std::collections::BTreeMap;

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"relation test",
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();

        let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
        let feature = Feature {
            id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                        sketch.clone(),
                    )),
                }),
            ),
            native_ref: Some("feature-native".into()),
        };
        let marker = |id: &str, ordinal: u32, offset: u64, coordinates_m: Option<[f64; 2]>| {
            let mut marker =
                SketchInputEntity::new(id, "lane#test", ordinal, offset, SketchInputKind::Point);
            marker.feature_ref = Some("feature-native".into());
            marker.coordinates_m = coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
            marker
        };
        let operand = |offset: u64, entity_index: u16| FeatureInputOperand {
            offset,
            reference_ref: format!("reference-{offset}"),
            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100),
            entity_index,
            entity_ref: None,
        };
        let scalar =
            |id: &str, offset: u64, value: f64, operands| crate::records::FeatureInputScalar {
                id: id.into(),
                parent: "lane#test".into(),
                feature_ref: Some("feature-native".into()),
                ordinal: 0,
                offset,
                object_id: 0,
                name: "distance".into(),
                value: cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite test scalar"),
                role: FeatureInputScalarRole::Driving,

                operands,
            };
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane#test".into(),
            ordinal: 0,
            offset: 30,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["terminal".into()],
                Some("terminal".into()),
                None,
            )
            .unwrap(),
            operands: vec![operand(40, 12), operand(52, 13)],
        };
        let lane = FeatureInputLane {
            id: "lane#test".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: vec![
                scalar("center-1", 10, 0.008, vec![operand(11, 13), operand(12, 3)]),
                scalar(
                    "center-2",
                    20,
                    0.0015,
                    vec![operand(21, 13), operand(22, 4)],
                ),
                scalar(
                    "terminal",
                    30,
                    0.007,
                    vec![operand(31, 12), operand(32, 13)],
                ),
            ],
            relation_bindings: Vec::new(),
            relation_instances: vec![relation.clone()],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![
                marker("origin", 0, 0, Some([0.0, 0.0])),
                marker("negative", 1, 1, Some([-0.007, 0.0])),
                marker("first-center", 2, 2, Some([0.008, 0.0])),
                marker("second-center", 3, 3, Some([0.0015, 0.0])),
            ],
        };
        let parameter = cadmpeg_ir::features::DesignParameter {
            id: ParameterId::mint("synthetic:test:id#distance").expect("identity grammar"),
            owner: Some(feature.id.clone()),
            ordinal: 0,
            name: "distance".into(),
            expression: "7mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length::new(7.0).unwrap())),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("terminal".into()),
        };
        let sketches = vec![Sketch {
            id: sketch.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some("lane#test".into()),
        }];
        let mut entities = Vec::new();

        project_relation_solved_point_geometry(
            &ctx,
            &mut entities,
            &sketches,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        )
        .unwrap();

        let mut positions = entities
            .iter()
            .filter_map(|entity| {
                let geometry_ref = entity.geometry_ref.as_deref()?;
                let SketchGeometryDefinition::Point { position } = *entity.geometry.definition()
                else {
                    return None;
                };
                Some((geometry_ref, position.get()))
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(
            positions.remove("relation:operand:0"),
            Some(Point2::new(-7.0, 0.0))
        );
        assert_eq!(
            positions.remove("relation:operand:1"),
            Some(Point2::new(0.0, 0.0))
        );
        assert!(positions.is_empty());

        let mut constraints = Vec::new();
        project_relation_bindings(
            &ctx,
            &mut constraints,
            &sketches,
            std::slice::from_ref(&feature),
            &entities,
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        )
        .unwrap();
        let [constraint] = constraints.as_slice() else {
            panic!("one solver-point constraint");
        };
        assert!(matches!(
            constraint.definition.kind(),
            SketchConstraintDefinitionInput::DistanceLoci { first, second, .. }
                if first == &SketchLocus::Entity(
                    SketchEntityId::mint("sldprt:model:sketch-entity#solver-point:test:30:0").unwrap()
                ) && second == &SketchLocus::Entity(
                    SketchEntityId::mint("sldprt:model:sketch-entity#solver-point:test:30:1").unwrap()
                )
        ));
    }

    #[test]
    fn solver_line_relation_prefers_marker_endpoint_join() {
        use cadmpeg_ir::math::{Point3, Vector3};
        use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};
        use cadmpeg_ir::{
            features::{
                Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
                ParameterValue,
            },
            scalar::Length,
        };
        use std::collections::BTreeMap;
        const FEATURE: &str = "feature-native";
        const LANE: &str = "lane#test";

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"relation test",
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();

        let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
        let feature = Feature {
            id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                        sketch_id.clone(),
                    )),
                }),
            ),
            native_ref: Some(FEATURE.into()),
        };
        let point = |id: &str, ordinal: u32, offset: u64, u: f64, v: f64| {
            let mut marker =
                SketchInputEntity::new(id, LANE, ordinal, offset, SketchInputKind::Point);
            marker.feature_ref = Some(FEATURE.into());
            marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([u / 1000.0, v / 1000.0]);
            marker
        };
        let mut first_start = point("first-start", 2, 10, 0.0, 0.0);
        first_start = first_start.with_test_identity(Some(2), first_start.local_id());
        let mut first_end = point("first-end", 3, 11, 10.0, 0.0);
        first_end = first_end.with_test_identity(Some(3), first_end.local_id());
        let mut second_start = point("second-start", 4, 12, 0.0, 5.0);
        second_start = second_start.with_test_identity(Some(4), second_start.local_id());
        let mut second_end = point("second-end", 5, 13, 10.0, 5.0);
        second_end = second_end.with_test_identity(Some(5), second_end.local_id());
        let mut first_line =
            SketchInputEntity::new("first-line", LANE, 6, 20, SketchInputKind::LineOrCircle);
        first_line.feature_ref = Some(FEATURE.into());
        first_line = first_line.with_test_identity(Some(0), first_line.local_id());
        first_line.links = crate::records::SketchInputLinks::new(
            0,
            vec![
                crate::records::SketchInputLink {
                    local_id: 0,
                    entity_ref: first_start.id().to_string(),
                },
                crate::records::SketchInputLink {
                    local_id: 1,
                    entity_ref: first_end.id().to_string(),
                },
            ],
        );
        let mut second_line =
            SketchInputEntity::new("second-line", LANE, 7, 21, SketchInputKind::LineOrCircle);
        second_line.feature_ref = Some(FEATURE.into());
        second_line = second_line.with_test_identity(Some(1), second_line.local_id());
        second_line.links = crate::records::SketchInputLinks::new(
            0,
            vec![
                crate::records::SketchInputLink {
                    local_id: 2,
                    entity_ref: second_start.id().to_string(),
                },
                crate::records::SketchInputLink {
                    local_id: 3,
                    entity_ref: second_end.id().to_string(),
                },
            ],
        );
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: LANE.into(),
            ordinal: 0,
            offset: 30,
            family: FeatureInputRelationFamily::LineLineDistance,
            class_ref: "class".into(),
            feature_ref: FEATURE.into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["scalar".into()],
                Some("scalar".into()),
                None,
            )
            .unwrap(),
            operands: [0_u16, 1]
                .into_iter()
                .enumerate()
                .map(|(index, entity_index)| FeatureInputOperand {
                    offset: 40 + cadmpeg_core::decode::u64_from_index(index),
                    reference_ref: format!("reference-{index}"),
                    kind: FeatureInputOperandKind::Native(
                        NativeOperandTag::try_from(0x812a).unwrap(),
                    ),
                    entity_index,
                    entity_ref: None,
                })
                .collect(),
        };
        let lane = FeatureInputLane {
            id: LANE.into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: vec![FeatureInputScalar {
                id: "scalar".into(),
                parent: LANE.into(),
                feature_ref: Some(FEATURE.into()),
                ordinal: 0,
                offset: 30,
                object_id: 0,
                name: "distance".into(),
                value: cadmpeg_ir::scalar::FiniteReal::new(0.005).expect("finite test scalar"),
                role: FeatureInputScalarRole::Driving,

                operands: relation.operands.clone(),
            }],
            relation_bindings: Vec::new(),
            relation_instances: vec![relation],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![
                point("distractor-start", 0, 0, 0.0, 50.0),
                point("distractor-end", 1, 1, 10.0, 50.0),
                first_start,
                first_end,
                second_start,
                second_end,
                first_line,
                second_line,
            ],
        };
        let parameter = cadmpeg_ir::features::DesignParameter {
            id: ParameterId::mint("synthetic:test:id#distance").expect("identity grammar"),
            owner: Some(feature.id.clone()),
            ordinal: 0,
            name: "distance".into(),
            expression: "5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length::new(5.0).unwrap())),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("scalar".into()),
        };
        let mut fallback_lane = lane.clone();
        let fallback_relation = fallback_lane
            .relation_instances
            .first_mut()
            .expect("synthetic relation");
        fallback_relation.id = "fallback-relation".into();
        fallback_relation.offset = 31;
        fallback_relation.scalars = crate::records::relation_scalars::RelationScalars::from_refs(
            vec!["fallback-scalar".into()],
            Some("fallback-scalar".into()),
            None,
        )
        .unwrap();
        for (operand, entity_index) in fallback_relation.operands.iter_mut().zip([1_u16, 2]) {
            operand.entity_index = entity_index;
        }
        let fallback_operands = fallback_relation.operands.clone();
        let fallback_scalar = fallback_lane.scalars.first_mut().expect("synthetic scalar");
        fallback_scalar.id = "fallback-scalar".into();
        fallback_scalar.offset = 31;

        fallback_scalar.operands = fallback_operands;
        for marker in &mut fallback_lane.sketch_entities {
            match marker.id() {
                "first-line" => {
                    *marker = marker.with_test_identity(Some(1), marker.local_id());
                    marker.links = crate::records::SketchInputLinks::new(
                        0,
                        vec![
                            crate::records::SketchInputLink {
                                local_id: 4,
                                entity_ref: "distractor-start".into(),
                            },
                            crate::records::SketchInputLink {
                                local_id: 5,
                                entity_ref: "distractor-end".into(),
                            },
                        ],
                    );
                }
                "second-line" => {
                    *marker = marker.with_test_identity(Some(2), marker.local_id());
                    marker.links = crate::records::SketchInputLinks::new(
                        0,
                        vec![
                            crate::records::SketchInputLink {
                                local_id: 6,
                                entity_ref: "distractor-start".into(),
                            },
                            crate::records::SketchInputLink {
                                local_id: 7,
                                entity_ref: "distractor-end".into(),
                            },
                        ],
                    );
                }
                _ => {}
            }
        }
        let fallback_parameter = cadmpeg_ir::features::DesignParameter {
            id: ParameterId::mint("synthetic:test:id#fallback-distance").expect("identity grammar"),
            native_ref: Some("fallback-scalar".into()),
            ..parameter.clone()
        };
        let sketches = vec![Sketch {
            id: sketch_id,
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some(LANE.into()),
        }];
        let mut entities = Vec::new();

        project_relation_solved_line_geometry(
            &ctx,
            &mut entities,
            &sketches,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        )
        .unwrap();

        let solver_lines = entities
            .iter()
            .filter(|entity| entity.geometry_ref.is_some())
            .filter_map(|entity| match *entity.geometry.definition() {
                SketchGeometryDefinition::Line { start, end } => Some((start, end)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(solver_lines.len(), 2);
        assert!(solver_lines.iter().any(|(start, end)| {
            *start == Point2::new(0.0, 0.0) && *end == Point2::new(10.0, 0.0)
        }));
        assert!(solver_lines.iter().any(|(start, end)| {
            *start == Point2::new(0.0, 5.0) && *end == Point2::new(10.0, 5.0)
        }));

        let mut fallback_entities = Vec::new();
        project_relation_solved_line_geometry(
            &ctx,
            &mut fallback_entities,
            &sketches,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&fallback_parameter),
            std::slice::from_ref(&fallback_lane),
        )
        .unwrap();
        let fallback_lines = fallback_entities
            .iter()
            .filter(|entity| entity.geometry_ref.is_some())
            .filter_map(|entity| match *entity.geometry.definition() {
                SketchGeometryDefinition::Line { start, end } => Some((start, end)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(fallback_lines.len(), 2);
        assert!(fallback_lines.iter().any(|(start, end)| {
            *start == Point2::new(0.0, 0.0) && *end == Point2::new(10.0, 0.0)
        }));
        assert!(fallback_lines.iter().any(|(start, end)| {
            *start == Point2::new(0.0, 5.0) && *end == Point2::new(10.0, 5.0)
        }));
    }

    #[test]
    fn dynamic_line_pair_fallback_preserves_the_existing_solver_slot() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"dynamic line pair",
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
        let line = |id: &str, start: Point2, end: Point2| {
            SketchEntity::new(
                SketchEntityId::mint(id).unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
            )
            .with_construction(true)
        };
        let generated = [
            line(
                "synthetic:test:id#roster-4",
                Point2::new(-13.0, 3.0),
                Point2::new(0.0, 3.0),
            ),
            line(
                "synthetic:test:id#roster-0",
                Point2::new(0.0, 0.0),
                Point2::new(0.0, 13.0),
            ),
        ];
        let existing = vec![line(
            "synthetic:test:id#profile-line",
            Point2::new(-16.0, 3.0),
            Point2::new(-16.0, 7.0),
        )];

        let generated_refs = [&generated[0], &generated[1]];
        let [first, second] = unique_dynamic_line_pair(
            &ctx,
            16.0,
            &sketch,
            &existing,
            &generated_refs,
            TEST_LINE_GEOMETRY_QUANTUM,
        )
        .unwrap()
        .expect("one existing line pairs with the roster solver line");
        assert!(matches!(*first.geometry.definition(),
            SketchGeometryDefinition::Line { start, end }
                if start == Point2::new(-16.0, 3.0) && end == Point2::new(-16.0, 7.0)
        ));
        assert!(matches!(*second.geometry.definition(),
            SketchGeometryDefinition::Line { start, end }
                if start == Point2::new(0.0, 0.0) && end == Point2::new(0.0, 13.0)
        ));
    }

    #[test]
    fn spatial_point_line_relation_uses_unique_tagged_marker_roster() {
        use cadmpeg_ir::sketches::{
            SpatialSketch, SpatialSketchConstraintDefinitionInput, SpatialSketchGeometryDefinition,
            SpatialSketchId,
        };
        use cadmpeg_ir::{
            features::{
                Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
                ParameterValue,
            },
            scalar::Length,
        };
        use std::collections::BTreeMap;
        fn marker(
            payload: &mut [u8],
            offset: usize,
            code: u32,
            object_index: u32,
            locus: [u8; 4],
            position: Point3,
        ) -> SketchInputEntity {
            payload[offset - 4..offset].copy_from_slice(&object_index.to_le_bytes());
            payload[offset..offset + 5].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x01]);
            payload[offset + 5..offset + 13].fill(0xff);
            payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
            payload[offset + 17..offset + 21].copy_from_slice(&code.to_le_bytes());
            payload[offset + 21..offset + 23].fill(0);
            payload[offset + 23..offset + 27].copy_from_slice(&locus);
            payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
            payload[offset + 29..offset + 31].fill(0);
            payload[offset + 56..offset + 58].copy_from_slice(&[0x0e, 0x00]);
            for (index, value) in [position.x, position.y, position.z].into_iter().enumerate() {
                payload[offset + 58 + index * 8..offset + 66 + index * 8]
                    .copy_from_slice(&(value / 1000.0).to_le_bytes());
            }
            let mut marker = SketchInputEntity::new(
                format!("marker-{offset}"),
                LANE,
                u32::try_from(offset).unwrap(),
                cadmpeg_core::decode::u64_from_index(offset),
                SketchInputKind::Point,
            );
            marker.feature_ref = Some(FEATURE.into());
            marker = marker.with_test_identity(Some(object_index), marker.local_id());
            marker
        }
        const FEATURE: &str = "synthetic:test:id#feature";
        const LANE: &str = "lane";

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"relation test",
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();

        let source_position = Point3::new(0.0, 16.0, 12.0);

        let mut payload = vec![0u8; 800];
        let source = marker(
            &mut payload,
            4,
            2,
            1,
            [0x05, 0x00, 0x01, 0x00],
            source_position,
        );
        let first = marker(
            &mut payload,
            200,
            0,
            4,
            [0x04, 0x00, 0x02, 0x00],
            Point3::new(22.5, 22.5, 12.0),
        );
        let second = marker(
            &mut payload,
            400,
            0,
            5,
            [0x04, 0x00, 0x02, 0x00],
            Point3::new(-22.5, 22.5, 12.0),
        );
        let scalar = FeatureInputScalar {
            id: "scalar".into(),
            parent: LANE.into(),
            feature_ref: Some(FEATURE.into()),
            ordinal: 0,
            offset: 600,
            object_id: 0,
            name: "distance".into(),
            value: cadmpeg_ir::scalar::FiniteReal::new(0.0065).expect("finite test scalar"),
            role: FeatureInputScalarRole::Driving,

            operands: Vec::new(),
        };
        let operand = |entity_index| FeatureInputOperand {
            offset: 700 + u64::from(entity_index),
            reference_ref: format!("reference-{entity_index}"),
            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100),
            entity_index,
            entity_ref: None,
        };
        let relation = FeatureInputRelationInstance {
            id: "synthetic:test:relation#point-line".into(),
            parent: LANE.into(),
            ordinal: 0,
            offset: 650,
            family: FeatureInputRelationFamily::PointLineDistance,
            class_ref: "class".into(),
            feature_ref: FEATURE.into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["scalar".into()],
                Some("scalar".into()),
                None,
            )
            .unwrap(),
            operands: vec![operand(0), operand(1)],
        };
        let lane = FeatureInputLane {
            id: LANE.into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: vec![scalar],
            relation_bindings: Vec::new(),
            relation_instances: vec![relation.clone()],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![source, first, second],
        };
        let sketch = SpatialSketch {
            id: SpatialSketchId::mint("synthetic:test:id#spatial-sketch").unwrap(),
            name: None,
            configuration: None,
            visible: None,
            profiles: Vec::new(),
            native_ref: Some(LANE.into()),
        };
        let feature = Feature {
            id: FeatureId::mint(FEATURE).expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(sketch.id.clone()),
                }),
            ),
            native_ref: Some(FEATURE.into()),
        };
        let parameter = cadmpeg_ir::features::DesignParameter {
            id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
            owner: Some(feature.id.clone()),
            ordinal: 0,
            name: "distance".into(),
            expression: "6.5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length::new(6.5).unwrap())),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("scalar".into()),
        };

        let mut entities = Vec::new();
        let mut constraints = Vec::new();
        project_spatial_relation_bindings(
            &ctx,
            &mut constraints,
            &mut entities,
            std::slice::from_ref(&sketch),
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        )
        .unwrap();

        let [constraint] = constraints.as_slice() else {
            panic!("one spatial relation constraint");
        };
        let SpatialSketchConstraintDefinitionInput::PointLineDistance {
            point,
            line,
            parameter,
        } = constraint.definition.kind()
        else {
            panic!("tagged marker roster has a unique point-line witness");
        };
        assert_eq!(
            parameter,
            &ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar")
        );
        let point_entity = entities
            .iter()
            .find(|entity| entity.id().clone() == *point)
            .unwrap();
        assert!(matches!(*point_entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Point { position } if position.get() == source_position
        ));
        let line_entity = entities
            .iter()
            .find(|entity| entity.id().clone() == *line)
            .unwrap();
        let SpatialSketchGeometryDefinition::Line { start, end } =
            *line_entity.geometry.definition()
        else {
            panic!("point-line witness is a line");
        };
        assert_eq!(line_entity.endpoint_refs.len(), 2);
        assert!(same_dimension_length(
            spatial_point_line_distance(source_position, start.get(), end.get()).unwrap(),
            6.5
        ));
    }
}

#[cfg(test)]
mod point_point_distance_family_tests {
    use super::{FeatureInputRelationFamily, PointPointDistanceFamily};

    #[test]
    fn only_the_three_point_point_distance_families_are_admitted() {
        assert!(matches!(
            PointPointDistanceFamily::of(FeatureInputRelationFamily::PointPointDistance),
            Some(PointPointDistanceFamily::Direct)
        ));
        assert!(matches!(
            PointPointDistanceFamily::of(FeatureInputRelationFamily::PointPointHorizontalDistance),
            Some(PointPointDistanceFamily::AxisAligned)
        ));
        assert!(matches!(
            PointPointDistanceFamily::of(FeatureInputRelationFamily::PointPointVerticalDistance),
            Some(PointPointDistanceFamily::AxisAligned)
        ));
        for family in [
            FeatureInputRelationFamily::CircleDiameter,
            FeatureInputRelationFamily::LineLineDistance,
            FeatureInputRelationFamily::PointLineDistance,
            FeatureInputRelationFamily::Angle,
        ] {
            assert!(PointPointDistanceFamily::of(family).is_none());
        }
    }
}

#[cfg(test)]
mod ownership_tests;

#[cfg(test)]
mod planar_budget_tests;

#[cfg(test)]
mod spatial_budget_tests;
