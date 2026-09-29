//! Relation point and solved geometry projection.

use super::curves::slot_curve_and_center_indices;
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
    line_line_angle, line_line_distance, marker_point_locus,
    marker_transform_candidates_by_feature, point_line_distance_value, profile_axis_for_relation,
    profile_loci_by_marker, profile_locus_point, relation_constraint_is_inactive,
    relation_operand_marker, same_dimension_angle, same_dimension_length,
    typed_relation_definition, typed_relation_definition_with_profile_axis,
    unoriented_line_line_angle,
};
use super::relation_records::{
    circle_dimension_handle_driver, relation_uses_dynamic_operands, relation_uses_solver_points,
};
use super::transforms::{
    marker_entities, sketch_entity_loci,
    sketch_frame_marker_transform, ProfileAxis,
};
use super::typed_relations::{
    current_undetailed_bounded_curve_is_line, marker_curve_endpoint_markers,
    marker_relation_is_inactive, typed_marker_relation_definition_in_sketch,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity,
    SketchInputKind, SketchRelationKind,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchNativeOperand, SpatialSketch,
    SpatialSketchConstraint, SpatialSketchConstraintDefinitionInput, SpatialSketchEntity,
    SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
};
use std::collections::{HashMap, HashSet};

pub(super) const RELATION_PARAMETER_ID_PROPERTY: &str = "sldprt_relation_id";
pub(super) const RELATION_PARAMETER_ROLE_PROPERTY: &str = "sldprt_relation_parameter_role";
pub(super) const RELATION_PARAMETER_ROLE_REFERENCE: &str = "reference";
pub(super) const RELATION_DISPLAY_SCALAR_ID_PROPERTY: &str = "sldprt_display_scalar_id";

pub(crate) fn is_reference_relation_parameter(
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> bool {
    parameter
        .properties
        .get(RELATION_PARAMETER_ROLE_PROPERTY)
        .map(String::as_str)
        == Some(RELATION_PARAMETER_ROLE_REFERENCE)
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
    let mut matches = entities
        .iter()
        .filter(|entity| {
            entity.sketch == *sketch && entity.native_ref.as_deref() == Some(marker.id())
        });
    let first = matches.next();
    if matches.next().is_some() || first.is_some_and(|entity| {
        !matches!(*entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Point { position: candidate } if candidate == position
        )
    }) {
        return Ok(None);
    }
    if let Some(entity) = first {
        return copy_spatial_entity_id(ctx, entity.id()).map(Some);
    }
    let id_text = ctx.format_retained(format_args!(
        "{}:relation-point:{}",
        sketch.as_str(),
        marker.offset()
    ), "format SLDPRT spatial relation point identity")?;
    let Ok(id) = SpatialSketchEntityId::mint(id_text) else {
        return Ok(None);
    };
    let entity_id = copy_spatial_entity_id(ctx, &id)?;
    let sketch_id = copy_spatial_sketch_id(ctx, sketch)?;
    let native_ref = ctx.format_retained(
        format_args!("{}", marker.id()),
        "copy SLDPRT spatial relation marker identity",
    )?;
    let Some(geometry) = SpatialSketchGeometry::try_from(
        SpatialSketchGeometryDefinition::Point { position },
    ).ok() else {
        return Ok(None);
    };
    ctx.reserve_collection_vec(entities, 1, "append SLDPRT spatial relation point")?;
    entities.push(
        SpatialSketchEntity::new(
            entity_id,
            sketch_id,
            geometry,
        )
        .with_construction(true)
        .with_native_ref(Some(native_ref)),
    );
    Ok(Some(id))
}

fn copy_spatial_entity_id(
    ctx: &DecodeContext<'_>,
    id: &SpatialSketchEntityId,
) -> Result<SpatialSketchEntityId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(
        format_args!("{}", id.as_str()),
        "copy SLDPRT spatial entity identity",
    )?;
    SpatialSketchEntityId::mint(text).map_err(|_| {
        cadmpeg_core::CodecError::Malformed("SolidWorks spatial entity identity is invalid".into())
    })
}

fn copy_spatial_sketch_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::sketches::SpatialSketchId,
) -> Result<cadmpeg_ir::sketches::SpatialSketchId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(
        format_args!("{}", id.as_str()),
        "copy SLDPRT spatial sketch identity",
    )?;
    cadmpeg_ir::sketches::SpatialSketchId::mint(text).map_err(|_| {
        cadmpeg_core::CodecError::Malformed("SolidWorks spatial sketch identity is invalid".into())
    })
}

fn spatial_relation_point_line_entities(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    lane: &FeatureInputLane,
    entities: &mut Vec<SpatialSketchEntity>,
) -> Result<Option<(SpatialSketchEntityId, SpatialSketchEntityId)>, cadmpeg_core::CodecError> {
    let Some(value) = parameter.value.as_ref() else {
        return Ok(None);
    };
    let expected = match value {
        cadmpeg_ir::features::ParameterValue::Length(length) => length.get().abs(),
        _ => return Ok(None),
    };
    let point_candidates = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(relation.feature_ref.as_str()))
        .filter_map(|marker| {
            let offset = usize::try_from(marker.offset()).ok()?;
            let code = marker_native_code(&lane.native_payload, offset)?;
            matches!(code, 2..=5).then_some(())?;
            Some((
                marker,
                spatial_relation_marker_coordinates(&lane.native_payload, offset)?,
            ))
        });
    let mut point_markers = Vec::new();
    for candidate in point_candidates {
        ctx.reserve_collection_vec(&mut point_markers, 1, "collect SLDPRT spatial point markers")?;
        point_markers.push(candidate);
    }
    point_markers.sort_unstable_by_key(|(marker, _)| marker.offset());
    let Some(point_operand) = relation.operands.first() else {
        return Ok(None);
    };
    let Some(point_marker) = point_operand
        .entity_ref
        .as_deref()
        .and_then(|entity_ref| {
            point_markers
                .iter()
                .find(|(marker, _)| marker.id() == entity_ref)
        })
        .or_else(|| point_markers.get(usize::from(point_operand.entity_index))) else {
        return Ok(None);
    };
    let (point_marker, point) = *point_marker;

    let line_candidates = lane
        .sketch_entities
        .iter()
        .filter(|marker| {
            marker.feature_ref.as_deref() == Some(relation.feature_ref.as_str())
                && marker.object_index().is_some()
        })
        .filter_map(|marker| {
            let offset = usize::try_from(marker.offset()).ok()?;
            (marker_native_code(&lane.native_payload, offset) == Some(0)).then_some(())?;
            Some((
                marker,
                spatial_relation_marker_coordinates(&lane.native_payload, offset)?,
            ))
        });
    let mut line_markers = Vec::new();
    for candidate in line_candidates {
        ctx.reserve_collection_vec(&mut line_markers, 1, "collect SLDPRT spatial line markers")?;
        line_markers.push(candidate);
    }
    line_markers.sort_unstable_by_key(|(marker, _)| marker.offset());
    let mut line_matches = line_markers
        .chunks_exact(2)
        .filter_map(|pair| {
            let ((first_marker, first), (second_marker, second)) = (pair[0], pair[1]);
            (first != second
                && spatial_point_line_distance(point, first, second)
                    .is_some_and(|distance| same_dimension_length(distance, expected)))
            .then_some((first_marker, first, second_marker, second))
        });
    let (Some((start_marker, start, end_marker, end)), None) =
        (line_matches.next(), line_matches.next()) else {
        return Ok(None);
    };

    let Some(start_id) = ensure_spatial_relation_point(ctx, entities, sketch, start_marker, start)? else {
        return Ok(None);
    };
    let Some(end_id) = ensure_spatial_relation_point(ctx, entities, sketch, end_marker, end)? else {
        return Ok(None);
    };
    let Some(point_id) = ensure_spatial_relation_point(ctx, entities, sketch, point_marker, point)? else {
        return Ok(None);
    };
    let existing_line_id = entities
        .iter()
        .find(|entity| {
            entity.sketch == *sketch
                && matches!(
                    *entity.geometry.definition(),
                    SpatialSketchGeometryDefinition::Line { .. }
                )
                && matches!(entity.endpoint_refs.as_slice(), [first, second]
                    if (first == start_id.as_str() && second == end_id.as_str())
                        || (first == end_id.as_str() && second == start_id.as_str()))
        })
        .map(SpatialSketchEntity::id);
    let line_id = if let Some(line_id) = existing_line_id {
        copy_spatial_entity_id(ctx, line_id)?
    } else {
        let id_text = ctx.format_retained(
            format_args!("{}:relation-line", relation.id),
            "format SLDPRT spatial relation line identity",
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
        let Some(geometry) = SpatialSketchGeometry::try_from(
            SpatialSketchGeometryDefinition::Line { start, end },
        ).ok() else {
            return Ok(None);
        };
        ctx.reserve_collection_vec(entities, 1, "append SLDPRT spatial relation line")?;
        entities.push(
            SpatialSketchEntity::new(
                entity_id,
                sketch_id,
                geometry,
            )
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
    let mut spatial_sketch_ids = HashSet::new();
    for sketch in sketches {
        let operation = "index SLDPRT spatial sketch identities";
        ctx.charge_work(1, operation)?;
        if !spatial_sketch_ids.contains(&sketch.id) {
            ctx.charge_collection_items(1, operation)?;
            spatial_sketch_ids.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        spatial_sketch_ids.insert(&sketch.id);
    }
    let mut sketches_by_feature = HashMap::new();
    for feature in features {
        let operation = "index SLDPRT spatial sketches by feature";
        ctx.charge_work(1, operation)?;
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::SpatialSketch {
                sketch: Some(sketch),
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        if !spatial_sketch_ids.contains(sketch) {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        if !sketches_by_feature.contains_key(native_ref) {
            ctx.charge_collection_items(1, operation)?;
            sketches_by_feature.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        sketches_by_feature.insert(native_ref, sketch);
    }
    let relation_parameters = owned_relation_parameters(ctx, features, parameters, lanes)?;
    let mut parameters_by_id = HashMap::new();
    for parameter in parameters {
        let operation = "index SLDPRT spatial relation parameters";
        ctx.charge_work(1, operation)?;
        if !parameters_by_id.contains_key(&parameter.id) {
            ctx.charge_collection_items(1, operation)?;
            parameters_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        parameters_by_id.insert(&parameter.id, parameter);
    }
    let mut constraints_by_native_ref = HashMap::<String, usize>::new();
    for (index, constraint) in constraints.iter().enumerate() {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            let operation = "index SLDPRT spatial relation constraints";
            ctx.charge_work(1, operation)?;
            if !constraints_by_native_ref.contains_key(native_ref) {
                ctx.charge_collection_items(1, operation)?;
                constraints_by_native_ref.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                let key = ctx.format_retained(
                    format_args!("{native_ref}"),
                    "copy SLDPRT spatial constraint reference",
                )?;
                constraints_by_native_ref.insert(key, index);
            }
        }
    }
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let parameter_id = relation_parameters.get(&relation.id).and_then(Option::as_ref);
            let parameter = parameter_id
                .and_then(|parameter| parameters_by_id.get(parameter))
                .copied();
            let typed_definition = if relation.family == FeatureInputRelationFamily::PointLineDistance {
                if let (Some(parameter), Some(parameter_id)) = (parameter, parameter_id) {
                    spatial_relation_point_line_entities(ctx, relation, sketch, parameter, lane, entities)?
                        .map(|(point, line)| (point, line, parameter_id))
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
                for operand in &relation.operands {
                    let native_ref = operand.entity_ref.as_deref().map(|reference| {
                        ctx.format_retained(
                            format_args!("{reference}"),
                            "copy SLDPRT spatial relation operand reference",
                        )
                    }).transpose()?;
                    ctx.reserve_collection_vec(
                        &mut operands, 1, "collect SLDPRT spatial relation operands",
                    )?;
                    operands.push(SketchNativeOperand {
                        native_kind: operand_kind_name(operand.kind),
                        field: None,
                        object_index: Some(u32::from(operand.entity_index)),
                        native_ref,
                    });
                }
                SpatialSketchConstraintDefinitionInput::Native {
                    native_kind,
                    native_state: None,
                    parameter: parameter_id.map(|id| copy_relation_parameter_id(ctx, id)).transpose()?,
                    operands,
                }
            };
            let Ok(definition) =
                cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(definition)
            else {
                continue;
            };
            let id_text = ctx.format_retained(format_args!(
                    "sldprt:model:spatial-sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                ), "format SLDPRT spatial relation constraint identity")?;
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
            if let Some(index) = constraints_by_native_ref.get(relation.id.as_str()).copied() {
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
                ctx.charge_collection_items(1, operation)?;
                constraints_by_native_ref.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                let key = ctx.format_retained(
                    format_args!("{}", relation.id),
                    "copy SLDPRT spatial constraint reference",
                )?;
                constraints_by_native_ref.insert(key, constraints.len());
                ctx.reserve_collection_vec(constraints, 1, "append SLDPRT spatial relation constraint")?;
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

    let mut sketches_by_feature = HashMap::new();
    for feature in features {
        let operation = "index SLDPRT relation-point sketches";
        ctx.charge_work(1, operation)?;
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
        if !sketches_by_feature.contains_key(native_ref) {
            ctx.charge_collection_items(1, operation)?;
            sketches_by_feature.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
        sketches_by_feature.insert(native_ref, sketch_id);
    }
    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let mut markers_by_id = HashMap::new();
    for marker in lanes.iter().flat_map(|lane| &lane.sketch_entities) {
        let operation = "index SLDPRT relation-point markers";
        ctx.charge_work(1, operation)?;
        if !markers_by_id.contains_key(marker.id()) {
            ctx.charge_collection_items(1, operation)?;
            markers_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        markers_by_id.insert(marker.id(), marker);
    }
    let mut point_operands = HashSet::new();
    let mut curve_operands = HashSet::new();
    let mut referenced = HashSet::new();
    for lane in lanes {
        for relation in &lane.relation_instances {
            ctx.charge_work(1, "scan SLDPRT relation-point operands")?;
            let count = match relation.family {
                FeatureInputRelationFamily::PointPointDistance
                | FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance => 2,
                FeatureInputRelationFamily::PointLineDistance => 1,
                _ => 0,
            };
            for operand in relation.operands.iter().take(count) {
                let Some(id) = operand.entity_ref.as_deref() else {
                    continue;
                };
                if !point_operands.contains(id) {
                    let operation = "index SLDPRT relation-point operands";
                    ctx.charge_collection_items(1, operation)?;
                    point_operands.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                    })?;
                    point_operands.insert(id);
                }
            }
            let first = match relation.family {
                FeatureInputRelationFamily::LineLineDistance
                | FeatureInputRelationFamily::Angle => 0,
                FeatureInputRelationFamily::PointLineDistance => 1,
                _ => relation.operands.len(),
            };
            for operand in relation.operands.iter().skip(first) {
                let Some(id) = operand.entity_ref.as_deref() else {
                    continue;
                };
                if !curve_operands.contains(id) {
                    let operation = "index SLDPRT relation-curve operands";
                    ctx.charge_collection_items(1, operation)?;
                    curve_operands.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                    })?;
                    curve_operands.insert(id);
                }
            }
            for operand in &relation.operands {
                let Some(id) = operand.entity_ref.as_deref() else {
                    continue;
                };
                if !referenced.contains(id) {
                    let operation = "index SLDPRT referenced relation markers";
                    ctx.charge_collection_items(1, operation)?;
                    referenced.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                    })?;
                    referenced.insert(id);
                }
            }
        }
        for marker in &lane.sketch_entities {
            if !matches!(marker.kind(), SketchInputKind::Relation(_)) {
                continue;
            }
            let id = marker.id();
            if !referenced.contains(id) {
                let operation = "index SLDPRT referenced relation markers";
                ctx.charge_collection_items(1, operation)?;
                referenced.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                referenced.insert(id);
            }
        }
    }
    loop {
        let mut linked = Vec::new();
        for marker in markers_by_id.values() {
            ctx.charge_work(1, "scan SLDPRT relation marker links")?;
            let marker_referenced = referenced.contains(marker.id());
            for link in marker.links() {
                ctx.charge_work(1, "scan SLDPRT relation marker links")?;
                let adjacent = if marker_referenced {
                    Some(link.entity_ref.as_str())
                } else if referenced.contains(link.entity_ref.as_str()) {
                    Some(marker.id())
                } else {
                    None
                };
                if let Some(id) = adjacent.filter(|id| !referenced.contains(id)) {
                    ctx.reserve_collection_vec(
                        &mut linked,
                        1,
                        "collect SLDPRT adjacent relation markers",
                    )?;
                    linked.push(id);
                }
            }
        }
        if linked.is_empty() {
            break;
        }
        for id in linked {
            if !referenced.contains(id) {
                let operation = "index SLDPRT referenced relation markers";
                ctx.charge_collection_items(1, operation)?;
                referenced.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                referenced.insert(id);
            }
        }
    }
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for marker in &lane.sketch_entities {
            let qualified_point = point_operands.contains(marker.id());
            let has_existing_point = entities.iter().any(|entity| {
                (entity.native_ref.as_deref() == Some(marker.id())
                    || entity.geometry_ref.as_deref() == Some(marker.id()))
                    && matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    )
            });
            if !referenced.contains(marker.id())
                || !(qualified_point
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
                || entities.iter().any(|entity| {
                    entity
                        .endpoint_refs
                        .iter()
                        .any(|reference| reference == marker.id())
                })
            {
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
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            if sketch.as_str().contains("sketch#compact:")
                && !marker_is_geometry_locus(&lane.native_payload, marker.offset() as usize)
                && !entities.iter().any(|entity| {
                    entity
                        .endpoint_refs
                        .iter()
                        .any(|reference| reference == marker.id())
                })
            {
                continue;
            }
            let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
            let mut unique_position = None;
            let mut ambiguous = false;
            for transform in transforms
                .get(feature)
                .into_iter()
                .flatten()
            {
                ctx.charge_work(1, "scan SLDPRT relation-point transforms")?;
                let Some(position) = transform.apply(native) else {
                    continue;
                };
                if unique_position.is_some_and(|previous| previous != position) {
                    ambiguous = true;
                } else if unique_position.is_none() {
                    unique_position = Some(position);
                }
            }
            let position = if ambiguous || unique_position.is_none() {
                sketches
                    .iter()
                    .find(|candidate| candidate.id == *sketch)
                    .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                    .and_then(|transform| transform.apply(native))
            } else {
                unique_position
            };
            let Some(position) = position else {
                continue;
            };
            let position = Point2::new(position.0 as f64 * QUANTUM, position.1 as f64 * QUANTUM);
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-entity#relation-point:{lane_key}:{}",
                    marker.offset()
                ),
                "format SLDPRT relation-point entity identity",
            )?;
            let Ok(id) = SketchEntityId::mint(id_text) else {
                continue;
            };
            let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point { position })
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
                && matches!(marker.kind(), SketchInputKind::LineOrCircle | SketchInputKind::Arc)
            {
                Some(ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT relation-point geometry reference",
                )?)
            } else {
                None
            };
            ctx.reserve_collection_vec(entities, 1, "append SLDPRT relation point")?;
            entities.push(
                SketchEntity::new(id, sketch_id, geometry)
                    .with_construction(true)
                    .with_native_ref(native_ref)
                    .with_geometry_ref(geometry_ref),
            );
        }
        let mut markers_by_id = HashMap::new();
        let mut marker_roster = Vec::new();
        ctx.reserve_collection_vec(
            &mut marker_roster,
            lane.sketch_entities.len(),
            "collect SLDPRT relation-line marker roster",
        )?;
        for marker in &lane.sketch_entities {
            let operation = "index SLDPRT relation-line markers";
            ctx.charge_work(1, operation)?;
            if !markers_by_id.contains_key(marker.id()) {
                ctx.charge_collection_items(1, operation)?;
                markers_by_id.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
            }
            markers_by_id.insert(marker.id(), marker);
            marker_roster.push(marker);
        }
        for marker in &lane.sketch_entities {
            let marker_offset = usize::try_from(marker.offset()).ok();
            let undetailed_arc_line = marker.kind() == SketchInputKind::Arc
                && marker_offset.is_some_and(|offset| {
                    current_undetailed_bounded_curve_is_line(&lane.native_payload, offset)
                        || legacy_undetailed_profile_line(&lane.native_payload, offset)
                });
            let self_linked_curve_handle = curve_operands.contains(marker.id())
                && marker.coordinates_m.is_some()
                && marker
                    .links()
                    .iter()
                    .any(|link| link.entity_ref == marker.id())
                && marker
                    .links()
                    .iter()
                    .filter(|link| link.entity_ref != marker.id())
                    .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()))
                    .filter(|linked| linked.coordinates_m.is_some())
                    .count()
                    == 1;
            let linked_curve_handle = curve_operands.contains(marker.id())
                && !marker
                    .links()
                    .iter()
                    .any(|link| link.entity_ref == marker.id())
                && (linked_coordinate_line_endpoints(marker, &markers_by_id).is_some()
                    || coordinate_line_endpoints_with_linked_point(marker, &markers_by_id)
                        .is_some());
            if !referenced.contains(marker.id())
                || !(marker.kind() == SketchInputKind::LineOrCircle
                    || undetailed_arc_line
                    || self_linked_curve_handle
                    || linked_curve_handle)
                || entities
                    .iter()
                    .any(|entity| entity.native_ref.as_deref() == Some(marker.id()))
            {
                continue;
            }
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            let mut endpoints = marker_curve_endpoint_markers(
                &lane.native_payload,
                marker,
                &markers_by_id,
                &marker_roster,
            );
            if endpoints.len() != 2 && linked_curve_handle {
                endpoints = linked_coordinate_line_endpoints(marker, &markers_by_id)
                    .or_else(|| coordinate_line_endpoints_with_linked_point(marker, &markers_by_id))
                    .into_iter()
                    .flatten()
                    .collect();
            }
            if endpoints.len() != 2 {
                endpoints.clear();
                if self_linked_curve_handle {
                    ctx.reserve_collection_vec(
                        &mut endpoints,
                        1,
                        "collect SLDPRT relation-line fallback endpoints",
                    )?;
                    endpoints.push(marker);
                }
                for endpoint in marker
                    .links()
                    .iter()
                    .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()).copied())
                    .filter(|endpoint| endpoint.id() != marker.id())
                    .filter(|endpoint| {
                        endpoint.feature_ref == marker.feature_ref
                            && endpoint.coordinates_m.is_some()
                            && entities.iter().any(|entity| {
                                entity.sketch == *sketch
                                    && matches!(
                                        *entity.geometry.definition(),
                                        SketchGeometryDefinition::Point { .. }
                                    )
                                    && (entity.native_ref.as_deref() == Some(endpoint.id())
                                        || entity.geometry_ref.as_deref() == Some(endpoint.id()))
                            })
                    })
                {
                    ctx.reserve_collection_vec(
                        &mut endpoints,
                        1,
                        "collect SLDPRT relation-line fallback endpoints",
                    )?;
                    endpoints.push(endpoint);
                }
                endpoints.sort_unstable_by_key(|endpoint| endpoint.offset());
                endpoints.dedup_by_key(|endpoint| endpoint.id());
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
            for transform in transforms
                .get(feature)
                .into_iter()
                .flatten()
            {
                ctx.charge_work(1, "scan SLDPRT relation-line transforms")?;
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
            let Some((start, end)) = unique_candidate.filter(|_| !ambiguous) else {
                continue;
            };
            if start == end {
                continue;
            }
            let start = Point2::new(start.0 as f64 * QUANTUM, start.1 as f64 * QUANTUM);
            let end = Point2::new(end.0 as f64 * QUANTUM, end.1 as f64 * QUANTUM);
            let already_present = entities.iter().any(|entity| {
                entity.sketch == *sketch
                    && matches!(entity.geometry.definition(), SketchGeometryDefinition::Line { start: existing_start, end: existing_end }
                        if (quantize(existing_start.get(), QUANTUM) == quantize(start, QUANTUM)
                            && quantize(existing_end.get(), QUANTUM) == quantize(end, QUANTUM))
                            || (quantize(existing_start.get(), QUANTUM) == quantize(end, QUANTUM)
                                && quantize(existing_end.get(), QUANTUM) == quantize(start, QUANTUM)))
            });
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
            let Ok(id) = SketchEntityId::mint(id_text) else {
                continue;
            };
            let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
            else {
                continue;
            };
            let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
            let native_ref = if !matches!(marker.kind(), SketchInputKind::Relation(_)) {
                Some(ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT relation-line native reference",
                )?)
            } else {
                None
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
            ctx.reserve_collection_vec(entities, 1, "append SLDPRT relation line")?;
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

pub(super) fn relation_operand_geometry_ref(
    relation: &FeatureInputRelationInstance,
    operand_index: usize,
) -> String {
    format!("{}:operand:{operand_index}", relation.id)
}

pub(super) fn solver_line_geometry_ref(feature: &str, index: u16) -> String {
    format!("{feature}:solver-line:{index}")
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

    let mut sketches_by_feature = HashMap::new();
    for feature in features {
        let operation = "index SLDPRT solved-line sketches";
        ctx.charge_work(1, operation)?;
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
        if !sketches_by_feature.contains_key(native_ref) {
            ctx.charge_collection_items(1, operation)?;
            sketches_by_feature.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
        sketches_by_feature.insert(native_ref, sketch_id);
    }
    let ownership = owned_relation_parameters(ctx, features, parameters, lanes)?;
    let mut parameters_by_id = HashMap::new();
    for parameter in parameters {
        let operation = "index SLDPRT solved-line parameters";
        ctx.charge_work(1, operation)?;
        if !parameters_by_id.contains_key(&parameter.id) {
            ctx.charge_collection_items(1, operation)?;
            parameters_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        parameters_by_id.insert(&parameter.id, parameter);
    }
    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let mut markers_by_id = HashMap::new();
    for marker in lanes.iter().flat_map(|lane| &lane.sketch_entities) {
        let operation = "index SLDPRT solved-line markers";
        ctx.charge_work(1, operation)?;
        if !markers_by_id.contains_key(marker.id()) {
            ctx.charge_collection_items(1, operation)?;
            markers_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        markers_by_id.insert(marker.id(), marker);
    }

    for lane in lanes {
        let mut marker_roster = Vec::new();
        ctx.reserve_collection_vec(
            &mut marker_roster,
            lane.sketch_entities.len(),
            "collect SLDPRT solved-line marker roster",
        )?;
        marker_roster.extend(lane.sketch_entities.iter());
        for relation in &lane.relation_instances {
            let [first_operand, second_operand] = relation.operands.as_slice() else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let direct_line_reference = |operand: &FeatureInputOperand| {
                let Some(entity_ref) = operand.entity_ref.as_deref() else {
                    return false;
                };
                let mut matches = entities.iter().filter(|entity| {
                    entity.sketch == *sketch
                        && entity.native_ref.as_deref() == Some(entity_ref)
                        && matches!(
                            *entity.geometry.definition(),
                            SketchGeometryDefinition::Line { .. }
                        )
                });
                matches.next().is_some() && matches.next().is_none()
            };
            let line_operands = match relation.family {
                FeatureInputRelationFamily::LineLineDistance
                    if relation_uses_solver_line_operand(relation, 0)
                        && relation_uses_solver_line_operand(relation, 1)
                        && first_operand.entity_index != second_operand.entity_index =>
                {
                    [first_operand, second_operand]
                        .into_iter()
                        .enumerate()
                        .filter(|(_, operand)| !direct_line_reference(operand))
                        .collect::<Vec<_>>()
                }
                FeatureInputRelationFamily::PointLineDistance
                    if relation_uses_solver_line_operand(relation, 1)
                        && !direct_line_reference(second_operand) =>
                {
                    vec![(1, second_operand)]
                }
                FeatureInputRelationFamily::Angle
                    if relation_uses_solver_line_operand(relation, 0)
                        && relation_uses_solver_line_operand(relation, 1)
                        && first_operand.entity_index != second_operand.entity_index =>
                {
                    [first_operand, second_operand]
                        .into_iter()
                        .enumerate()
                        .filter(|(_, operand)| !direct_line_reference(operand))
                        .collect::<Vec<_>>()
                }
                _ => continue,
            };
            if line_operands.is_empty() {
                continue;
            }
            let Some(parameter_value) = ownership
                .get(&relation.id)
                .and_then(Option::as_ref)
                .and_then(|parameter| parameters_by_id.get(parameter))
                .and_then(|parameter| parameter.value.as_ref())
            else {
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
            let mut points = Vec::new();
            for marker in &lane.sketch_entities {
                ctx.charge_work(1, "scan SLDPRT solved-line point markers")?;
                if marker.feature_ref.as_deref() == Some(relation.feature_ref.as_str())
                    && marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                {
                    ctx.reserve_collection_vec(
                        &mut points,
                        1,
                        "collect SLDPRT solved-line point markers",
                    )?;
                    points.push(marker);
                }
            }
            points.sort_by_key(|marker| marker.offset());
            let endpoint_line_markers = |operand_index: usize| {
                relation_operand_marker(relation, operand_index, sketch, &markers_by_id)
                    .and_then(|marker_id| {
                        lane.sketch_entities
                            .iter()
                            .find(|marker| marker.id() == marker_id)
                    })
                    .map(|marker| {
                        marker_curve_endpoint_markers(
                            &lane.native_payload,
                            marker,
                            &markers_by_id,
                            &marker_roster,
                        )
                    })
                    .and_then(|endpoints| {
                        let [first, second] = endpoints.as_slice() else {
                            return None;
                        };
                        Some([*first, *second])
                    })
            };
            let fallback_line_markers = |index: u16| {
                let pair = usize::from(index).checked_mul(2)?;
                Some([*points.get(pair)?, *points.get(pair + 1)?])
            };
            let point_marker = (relation.family == FeatureInputRelationFamily::PointLineDistance)
                .then(|| {
                    first_operand
                        .entity_ref
                        .as_deref()
                        .and_then(|id| lane.sketch_entities.iter().find(|marker| marker.id() == id))
                        .or_else(|| {
                            relation_operand_marker(relation, 0, sketch, &markers_by_id).and_then(
                                |id| lane.sketch_entities.iter().find(|marker| marker.id() == id),
                            )
                        })
                        .or_else(|| {
                            (first_operand.kind
                                == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD))
                            .then(|| points.get(usize::from(first_operand.entity_index)).copied())
                            .flatten()
                        })
                })
                .flatten();
            let point_position = (|| -> Result<Option<Point2>, cadmpeg_core::CodecError> {
                let Some(marker) = point_marker else {
                    return Ok(None);
                };
                let resolved = entities
                    .iter()
                    .find(|entity| {
                        entity.sketch == *sketch
                            && entity.native_ref.as_deref() == Some(marker.id())
                    })
                    .and_then(|entity| match entity.geometry.definition() {
                        SketchGeometryDefinition::Point { position } => Some(position.get()),
                        _ => None,
                    });
                if resolved.is_some() {
                    return Ok(resolved);
                }
                let Some([u, v]) = marker.coordinates_m.map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    return Ok(None);
                };
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let mut unique_position = None;
                let mut ambiguous = false;
                for transform in transforms
                    .get(relation.feature_ref.as_str())
                    .into_iter()
                    .flatten()
                {
                    ctx.charge_work(1, "scan SLDPRT solved-line point transforms")?;
                    let Some(position) = transform.apply(native) else {
                        continue;
                    };
                    if unique_position.is_some_and(|previous| previous != position) {
                        ambiguous = true;
                    } else if unique_position.is_none() {
                        unique_position = Some(position);
                    }
                }
                let position = if ambiguous || unique_position.is_none() {
                    sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch)
                        .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                        .and_then(|transform| transform.apply(native))
                } else {
                    unique_position
                };
                let Some(position) = position else {
                    return Ok(None);
                };
                Ok(Some(Point2::new(
                    position.0 as f64 * QUANTUM,
                    position.1 as f64 * QUANTUM,
                )))
            })()?;
            let candidate = |start, end| -> Result<Option<SketchEntity>, cadmpeg_core::CodecError> {
                let Ok(id) = SketchEntityId::mint("sldprt:model:sketch-entity#solver-line") else {
                    return Ok(None);
                };
                let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
                else {
                    return Ok(None);
                };
                let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
                Ok(Some(SketchEntity::new(id, sketch_id, geometry).with_construction(true)))
            };
            let transformed_line = |markers: [&SketchInputEntity; 2]| -> Result<Option<(Point2, Point2)>, cadmpeg_core::CodecError> {
                let native = markers.map(|marker| {
                    marker.coordinates_m.map(|coordinates| {
                        let [u, v] = coordinates.get();
                        quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM)
                    })
                });
                let [Some(first), Some(second)] = native else {
                    return Ok(None);
                };
                let transform_candidates = transforms
                    .get(relation.feature_ref.as_str())
                    .map_or(&[][..], Vec::as_slice);
                let fallback = transform_candidates
                    .is_empty()
                    .then(|| {
                        sketches
                            .iter()
                            .find(|candidate| candidate.id == *sketch)
                            .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                    })
                    .flatten();
                let mut candidates = Vec::new();
                for transform in transform_candidates.iter().chain(fallback.iter()) {
                    ctx.charge_work(1, "scan SLDPRT solved-line transforms")?;
                    let (Some(start), Some(end)) =
                        (transform.apply(first), transform.apply(second))
                    else {
                        continue;
                    };
                    let pair = (start, end);
                    if start != end && !candidates.contains(&pair) {
                        ctx.reserve_collection_vec(
                            &mut candidates,
                            1,
                            "collect SLDPRT solved-line transform candidates",
                        )?;
                        candidates.push(pair);
                    }
                }
                let candidates = if relation.family == FeatureInputRelationFamily::PointLineDistance
                {
                    let mut filtered = Vec::new();
                    for (start, end) in candidates {
                        ctx.charge_work(1, "filter SLDPRT solved-line candidates")?;
                        let Some(line) = candidate(
                                Point2::new(start.0 as f64 * QUANTUM, start.1 as f64 * QUANTUM),
                                Point2::new(end.0 as f64 * QUANTUM, end.1 as f64 * QUANTUM),
                        )? else {
                            continue;
                        };
                        if point_position.is_some_and(|point| {
                            point_line_distance_value(point, &line).is_some_and(|measured| {
                                same_dimension_length(measured, expected)
                            })
                        }) {
                            ctx.reserve_collection_vec(
                                &mut filtered,
                                1,
                                "filter SLDPRT solved-line candidates",
                            )?;
                            filtered.push((start, end));
                        }
                    }
                    let Some(&(first_start, first_end)) = filtered.first() else {
                        return Ok(None);
                    };
                    let orientation_is_ambiguous = filtered
                        .iter()
                        .any(|(start, end)| *start == first_end && *end == first_start);
                    if filtered.iter().all(|(start, end)| {
                        (*start == first_start && *end == first_end)
                            || (*start == first_end && *end == first_start)
                    }) {
                        let representative = if orientation_is_ambiguous {
                            if first_start <= first_end {
                                (first_start, first_end)
                            } else {
                                (first_end, first_start)
                            }
                        } else {
                            (first_start, first_end)
                        };
                        filtered.truncate(1);
                        filtered[0] = representative;
                    } else {
                        return Ok(None);
                    }
                    filtered
                } else {
                    candidates
                };
                let [(start, end)] = candidates.as_slice() else {
                    return Ok(None);
                };
                Ok(Some((
                    Point2::new(start.0 as f64 * QUANTUM, start.1 as f64 * QUANTUM),
                    Point2::new(end.0 as f64 * QUANTUM, end.1 as f64 * QUANTUM),
                )))
            };
            let build_lines = |prefer_marker_endpoints: bool| -> Result<Vec<_>, cadmpeg_core::CodecError> {
                let mut lines = Vec::with_capacity(line_operands.len());
                for &(operand_index, operand) in &line_operands {
                    let markers = if prefer_marker_endpoints {
                        endpoint_line_markers(operand_index)
                            .or_else(|| fallback_line_markers(operand.entity_index))
                    } else {
                        fallback_line_markers(operand.entity_index)
                    };
                    let Some(markers) = markers else {
                        return Ok(Vec::new());
                    };
                    let Some((start, end)) = transformed_line(markers)? else {
                        return Ok(Vec::new());
                    };
                    let Some(line) = candidate(start, end)? else {
                        return Ok(Vec::new());
                    };
                    lines.push((operand, markers, line));
                }
                Ok(lines)
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
            let mut valid = relation_lines_valid(&lines);
            if !valid {
                let fallback_lines = build_lines(false)?;
                if !fallback_lines.is_empty() {
                    lines = fallback_lines;
                    valid = relation_lines_valid(&lines);
                }
            }
            if !valid {
                if relation.family == FeatureInputRelationFamily::LineLineDistance
                    && relation_uses_dynamic_operands(relation)
                {
                    let generated = lines
                        .iter()
                        .map(|(_, _, line)| line.clone())
                        .collect::<Vec<_>>();
                    if let Some([first, second]) =
                        unique_dynamic_line_pair(expected, sketch, entities, &generated, QUANTUM)
                    {
                        let selected = [first, second];
                        let aliases_match =
                            relation
                                .operands
                                .iter()
                                .zip(selected.iter())
                                .all(|(operand, line)| {
                                    let geometry_ref = solver_line_geometry_ref(
                                        &relation.feature_ref,
                                        operand.entity_index,
                                    );
                                    entities
                                        .iter()
                                        .filter(|entity| {
                                            entity.sketch == *sketch
                                                && entity.geometry_ref.as_deref()
                                                    == Some(geometry_ref.as_str())
                                        })
                                        .all(|entity| {
                                            dynamic_line_geometry_key(entity, QUANTUM)
                                                == dynamic_line_geometry_key(line, QUANTUM)
                                        })
                                });
                        if !aliases_match {
                            continue;
                        }
                        let feature_key = relation
                            .feature_ref
                            .rsplit_once('#')
                            .map_or(relation.feature_ref.as_str(), |(_, key)| key);
                        for (operand, line) in relation.operands.iter().zip(selected) {
                            let geometry_ref = ctx.format_retained(
                                format_args!(
                                    "{}:solver-line:{}",
                                    relation.feature_ref,
                                    operand.entity_index
                                ),
                                "format SLDPRT dynamic solver-line reference",
                            )?;
                            if entities.iter().any(|entity| {
                                entity.sketch == *sketch
                                    && entity.geometry_ref.as_deref() == Some(geometry_ref.as_str())
                            }) {
                                continue;
                            }
                            let id_text = ctx.format_retained(
                                format_args!(
                                    "sldprt:model:sketch-entity#solver-line:{feature_key}:{}",
                                    operand.entity_index
                                ),
                                "format SLDPRT dynamic solver-line entity identity",
                            )?;
                            let Ok(id) = SketchEntityId::mint(id_text) else {
                                continue;
                            };
                            let sketch_id = copy_planar_sketch_id(ctx, &line.sketch)?;
                            let mut endpoint_refs = Vec::new();
                            for reference in &line.endpoint_refs {
                                let reference = ctx.format_retained(
                                    format_args!("{reference}"),
                                    "copy SLDPRT dynamic solver-line endpoint reference",
                                )?;
                                ctx.reserve_collection_vec(
                                    &mut endpoint_refs,
                                    1,
                                    "copy SLDPRT dynamic solver-line endpoints",
                                )?;
                                endpoint_refs.push(reference);
                            }
                            ctx.reserve_collection_vec(entities, 1, "append SLDPRT dynamic solver line")?;
                            entities.push(
                                SketchEntity::new(id, sketch_id, line.geometry.clone())
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
            let feature_key = relation
                .feature_ref
                .rsplit_once('#')
                .map_or(relation.feature_ref.as_str(), |(_, key)| key);
            for (operand, markers, line) in lines {
                let geometry_ref = ctx.format_retained(
                    format_args!(
                        "{}:solver-line:{}",
                        relation.feature_ref,
                        operand.entity_index
                    ),
                    "format SLDPRT solver-line reference",
                )?;
                if entities.iter().any(|entity| {
                    entity.sketch == *sketch
                        && entity.geometry_ref.as_deref() == Some(geometry_ref.as_str())
                }) {
                    continue;
                }
                let id_text = ctx.format_retained(
                    format_args!(
                        "sldprt:model:sketch-entity#solver-line:{feature_key}:{}",
                        operand.entity_index
                    ),
                    "format SLDPRT solver-line entity identity",
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
                ctx.reserve_collection_vec(entities, 1, "append SLDPRT solver line")?;
                entities.push(
                    SketchEntity::new(id, sketch_id, line.geometry.clone())
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

fn unique_dynamic_line_pair(
    expected: f64,
    sketch: &cadmpeg_ir::sketches::SketchId,
    entities: &[SketchEntity],
    generated: &[SketchEntity],
    quantum: f64,
) -> Option<[SketchEntity; 2]> {
    if generated.len() != 2 {
        return None;
    }
    let mut candidates = Vec::<([GridPoint; 2], SketchEntity)>::new();
    for entity in generated.iter().chain(entities.iter()) {
        if entity.sketch != *sketch
            || !matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        let Some(key) = dynamic_line_geometry_key(entity, quantum) else {
            continue;
        };
        if candidates.iter().any(|(candidate, _)| *candidate == key) {
            continue;
        }
        candidates.push((key, entity.clone()));
    }
    let mut matches = Vec::new();
    for (first_index, (first_key, first)) in candidates.iter().enumerate() {
        for (second_key, second) in candidates.iter().skip(first_index + 1) {
            if line_line_distance(first, second)
                .is_some_and(|measured| same_dimension_length(measured, expected))
            {
                let mut pair_key = [*first_key, *second_key];
                pair_key.sort_unstable();
                matches.push((pair_key, [first.clone(), second.clone()]));
            }
        }
    }
    matches.sort_by_key(|(key, _)| *key);
    matches.dedup_by(|(left, _), (right, _)| left == right);
    let [(_, pair)] = matches.as_slice() else {
        return None;
    };
    let first_key = dynamic_line_geometry_key(&generated[0], quantum)?;
    let second_key = dynamic_line_geometry_key(&generated[1], quantum)?;
    if dynamic_line_geometry_key(&pair[0], quantum) == Some(first_key) {
        return Some(pair.clone());
    }
    if dynamic_line_geometry_key(&pair[1], quantum) == Some(first_key) {
        return Some([pair[1].clone(), pair[0].clone()]);
    }
    if dynamic_line_geometry_key(&pair[0], quantum) == Some(second_key) {
        return Some([pair[1].clone(), pair[0].clone()]);
    }
    if dynamic_line_geometry_key(&pair[1], quantum) == Some(second_key) {
        return Some(pair.clone());
    }
    Some(pair.clone())
}

fn dynamic_line_geometry_key(entity: &SketchEntity, quantum: f64) -> Option<[GridPoint; 2]> {
    let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
        return None;
    };
    let mut endpoints = [quantize(start.get(), quantum), quantize(end.get(), quantum)];
    endpoints.sort_unstable();
    Some(endpoints)
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

    let mut sketches_by_feature = HashMap::new();
    for feature in features {
        let operation = "index SLDPRT solved-point sketches";
        ctx.charge_work(1, operation)?;
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
        if !sketches_by_feature.contains_key(native_ref) {
            ctx.charge_collection_items(1, operation)?;
            sketches_by_feature.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        sketches_by_feature.insert(native_ref, sketch);
    }
    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let ownership = owned_relation_parameters(ctx, features, parameters, lanes)?;
    let mut parameters_by_id = HashMap::new();
    for parameter in parameters {
        let operation = "index SLDPRT solved-point parameters";
        ctx.charge_work(1, operation)?;
        if !parameters_by_id.contains_key(&parameter.id) {
            ctx.charge_collection_items(1, operation)?;
            parameters_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        parameters_by_id.insert(&parameter.id, parameter);
    }
    let mut markers_by_id = HashMap::new();
    for marker in lanes.iter().flat_map(|lane| &lane.sketch_entities) {
        let operation = "index SLDPRT solved-point markers";
        ctx.charge_work(1, operation)?;
        if !markers_by_id.contains_key(marker.id()) {
            ctx.charge_collection_items(1, operation)?;
            markers_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        markers_by_id.insert(marker.id(), marker);
    }
    let loci_by_marker = profile_loci_by_marker(features, sketches, entities, lanes);

    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            let Some(family) = PointPointDistanceFamily::of(relation.family) else {
                continue;
            };
            if relation.operands.len() != 2 {
                continue;
            }
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let parameter = ownership
                .get(&relation.id)
                .and_then(Option::as_ref)
                .and_then(|parameter| parameters_by_id.get(parameter))
                .copied();
            let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) =
                parameter.and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            let profile_axis = profile_axis_for_relation(
                relation,
                transforms
                    .get(relation.feature_ref.as_str())
                    .map(Vec::as_slice),
            );
            if relation_uses_solver_points(relation) {
                let coordinates_by_index =
                    inferred_point_coordinates_by_index(lane, relation.feature_ref.as_str());
                let mut resolved_positions = Vec::with_capacity(relation.operands.len());
                for (index, operand) in relation.operands.iter().enumerate() {
                    let geometry_ref = ctx.format_retained(
                        format_args!("{}:operand:{index}", relation.id),
                        "format SLDPRT solved-point operand reference",
                    )?;
                    if entities
                        .iter()
                        .any(|entity| entity.geometry_ref.as_deref() == Some(geometry_ref.as_str()))
                    {
                        resolved_positions.push(None);
                        continue;
                    }
                    let Some(coordinates) = coordinates_by_index
                        .get(&u32::from(operand.entity_index))
                        .copied()
                    else {
                        resolved_positions.clear();
                        break;
                    };
                    let native = quantize(
                        Point2::new(coordinates[0] * NATIVE_TO_IR, coordinates[1] * NATIVE_TO_IR),
                        QUANTUM,
                    );
                    let mut unique_position = None;
                    let mut ambiguous = false;
                    for transform in transforms
                        .get(relation.feature_ref.as_str())
                        .into_iter()
                        .flatten()
                    {
                        ctx.charge_work(1, "scan SLDPRT solved-point transforms")?;
                        let Some(position) = transform.apply(native) else {
                            continue;
                        };
                        if unique_position.is_some_and(|previous| previous != position) {
                            ambiguous = true;
                        } else if unique_position.is_none() {
                            unique_position = Some(position);
                        }
                    }
                    let position = if ambiguous || unique_position.is_none() {
                        sketches
                            .iter()
                            .find(|candidate| candidate.id == **sketch)
                            .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                            .and_then(|transform| transform.apply(native))
                    } else {
                        unique_position
                    };
                    let Some(position) = position else {
                        resolved_positions.clear();
                        break;
                    };
                    resolved_positions.push(Some(position));
                }
                if resolved_positions.len() != relation.operands.len() {
                    continue;
                }
                for (index, position) in resolved_positions
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, position)| position.map(|position| (index, position)))
                {
                    let geometry_ref = ctx.format_retained(
                        format_args!("{}:operand:{index}", relation.id),
                        "format SLDPRT solved-point operand reference",
                    )?;
                    let id_text = ctx.format_retained(format_args!(
                                "sldprt:model:sketch-entity#solver-point:{lane_key}:{}:{index}",
                                relation.offset
                            ), "format SLDPRT solved-point entity identity")?;
                    let Ok(id) = SketchEntityId::mint(id_text) else {
                        continue;
                    };
                    let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: Point2::new(
                            position.0 as f64 * QUANTUM,
                            position.1 as f64 * QUANTUM,
                        ),
                    }) else {
                        continue;
                    };
                    let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
                    ctx.reserve_collection_vec(entities, 1, "append SLDPRT solved point")?;
                    entities.push(
                        SketchEntity::new(id, sketch_id, geometry)
                            .with_construction(true)
                            .with_geometry_ref(Some(geometry_ref)),
                    );
                }
                continue;
            }
            let resolved = [0, 1].map(|index| {
                relation.operands[index]
                    .entity_ref
                    .as_deref()
                    .and_then(|marker| marker_point_locus(marker, &markers_by_id, &loci_by_marker))
            });
            let (known, missing_index) = match resolved {
                [Some(known), None] => (known, 1),
                [None, Some(known)] => (known, 0),
                _ => continue,
            };
            let Some(missing_marker) = relation.operands[missing_index]
                .entity_ref
                .as_deref()
                .and_then(|marker| markers_by_id.get(marker).copied())
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
            let Some(known_point) = profile_locus_point(&known, entities) else {
                continue;
            };
            let mut candidates = entities
                .iter()
                .filter(|entity| entity.sketch == **sketch)
                .flat_map(sketch_entity_loci)
                .filter_map(|(point, _)| {
                    let measured = match family {
                        PointPointDistanceFamily::Direct => {
                            (point.u - known_point.u).hypot(point.v - known_point.v)
                        }
                        PointPointDistanceFamily::AxisAligned => match profile_axis? {
                            ProfileAxis::U => (point.u - known_point.u).abs(),
                            ProfileAxis::V => (point.v - known_point.v).abs(),
                        },
                    };
                    same_dimension_length(measured, distance.get())
                        .then_some(quantize(point, QUANTUM))
                });
            let Some(point) = candidates.next() else {
                continue;
            };
            if candidates.any(|candidate| candidate != point) {
                continue;
            }
            let geometry_ref = ctx.format_retained(
                format_args!("{}:operand:{missing_index}", relation.id),
                "format SLDPRT dimension-point operand reference",
            )?;
            if entities
                .iter()
                .any(|entity| entity.geometry_ref.as_deref() == Some(geometry_ref.as_str()))
            {
                continue;
            }
            let id_text = ctx.format_retained(format_args!(
                        "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}:{missing_index}",
                        relation.offset
                    ), "format SLDPRT dimension-point entity identity")?;
            let Ok(id) = SketchEntityId::mint(id_text) else {
                continue;
            };
            let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: point.point(QUANTUM),
            }) else {
                continue;
            };
            let sketch_id = copy_planar_sketch_id(ctx, sketch)?;
            ctx.reserve_collection_vec(entities, 1, "append SLDPRT dimension point")?;
            entities.push(
                SketchEntity::new(id, sketch_id, geometry)
                    .with_construction(true)
                    .with_geometry_ref(Some(geometry_ref)),
            );
        }
    }
    Ok(())
}

pub(super) fn implicit_circle_marker<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand_kind: FeatureInputOperandKind,
    index: u16,
    expected_radius: f64,
) -> Option<(&'a SketchInputEntity, f64)> {
    // CircleDiameter selects the semantic family; native operand tags are only
    // carrier kinds and must not narrow the geometric witness search.
    if !matches!(operand_kind, FeatureInputOperandKind::Native(_))
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return None;
    }
    let relation_index = u32::from(index).checked_add(1)?;
    let mut candidates = lanes
        .iter()
        .filter_map(|lane| {
            let relation = lane.sketch_entities.iter().find(|marker| {
                marker.feature_ref.as_deref() == Some(feature)
                    && marker.object_index() == Some(relation_index)
                    && marker.kind() == SketchInputKind::Relation(SketchRelationKind::Distance)
                    && matches!(marker.links(), [first, second]
                        if first.entity_ref == second.entity_ref
                            && first.local_id == second.local_id)
            })?;
            let center_id = relation.links().first()?.entity_ref.as_str();
            let center = lane
                .sketch_entities
                .iter()
                .find(|marker| marker.id() == center_id && marker.coordinates_m.is_some())?;
            let radial = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    marker.feature_ref.as_deref() == Some(feature)
                        && marker.offset() > center.offset()
                        && marker.coordinates_m.is_some()
                })
                .min_by_key(|marker| marker.offset())?;
            let [cu, cv] = center.coordinates_m?.get();
            let [ru, rv] = radial.coordinates_m?.get();
            let radius = (ru - cu).hypot(rv - cv) * 1000.0;
            same_dimension_length(radius, expected_radius).then_some((center, radius))
        });
    if let Some(candidate) = candidates.next() {
        if candidates.all(|other| {
            other.0.id() == candidate.0.id() && other.1.to_bits() == candidate.1.to_bits()
        }) {
            return Some(candidate);
        }
    }

    let mut terminal_pair = None;
    let mut terminal_ambiguous = false;
    for lane in lanes {
        let feature_markers = lane
            .sketch_entities
            .iter()
            .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
            .filter(|marker| marker.coordinates_m.is_some())
            .filter(|marker| {
                matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            })
            .collect::<Vec<_>>();
        for radial in feature_markers
            .iter()
            .copied()
            .filter(|marker| marker.local_id().is_none())
        {
            for center in feature_markers
                .iter()
                .copied()
                .filter(|marker| marker.local_id().is_some() && marker.offset() < radial.offset())
            {
                let [cu, cv] = center.coordinates_m?.get();
                let [ru, rv] = radial.coordinates_m?.get();
                let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                if same_dimension_length(radius, expected_radius) {
                    if terminal_pair.is_some_and(|(first, first_radius): (&SketchInputEntity, f64)| {
                        first.id() != center.id() || first_radius.to_bits() != radius.to_bits()
                    }) {
                        terminal_ambiguous = true;
                    } else if terminal_pair.is_none() {
                        terminal_pair = Some((center, radius));
                    }
                }
            }
        }
    }
    if !terminal_ambiguous {
        if let Some(candidate) = terminal_pair {
            return Some(candidate);
        }
    }

    // Only 83fe defines an ordered center/radial point roster. Other native
    // carriers may use the relation-qualified witness tiers above, but their
    // point-marker order does not identify a circular-dimension pair.
    if operand_kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_83FE) {
        return None;
    }

    let mut markers = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.local_id() != Some(0))
        .filter(|marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset());
    let pair = (markers.len() % 2 == 0)
        .then(|| markers.chunks_exact(2).nth(usize::from(index)))
        .flatten()?;
    let [center, radial] = pair else {
        return None;
    };
    let [cu, cv] = center.coordinates_m?.get();
    let [ru, rv] = radial.coordinates_m?.get();
    let radius = (ru - cu).hypot(rv - cv) * 1000.0;
    same_dimension_length(radius, expected_radius).then_some((*center, radius))
}

#[derive(Clone, Copy)]
pub(super) enum DeclaredEntityHandleOwner<'a> {
    Absent,
    Unique(&'a FeatureInputLane),
    Ambiguous,
}

pub(super) fn declared_entity_handle_owner<'a>(
    lanes: &'a [FeatureInputLane],
    operand: &FeatureInputOperand,
) -> DeclaredEntityHandleOwner<'a> {
    let mut owners = lanes.iter().filter_map(|lane| {
        let reference = lane
            .references
            .iter()
            .find(|reference| reference.id == operand.reference_ref)?;
        let class = reference
            .class_ref
            .as_deref()
            .and_then(|id| lane.classes.iter().find(|class| class.id == id))?;
        (class.name == "sgEntHandle").then_some(lane)
    });
    let Some(lane) = owners.next() else {
        return DeclaredEntityHandleOwner::Absent;
    };
    if owners.next().is_some() {
        DeclaredEntityHandleOwner::Ambiguous
    } else {
        DeclaredEntityHandleOwner::Unique(lane)
    }
}

/// Resolve the circular-dimension center carried by a slot handle.
///
/// A slot is an aggregate boundary descriptor, not an independent circle. Its
/// radial dimension handle therefore identifies the slot marker first and a
/// selected center point second. The two exact `sgSlotHandle` reference cells
/// are required so a slot's center cannot be guessed from its radius or from
/// the slot's boundary roster alone.
pub(super) fn declared_slot_handle_dimension_center<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Option<(&'a SketchInputEntity, &'a SketchInputEntity)> {
    let entity_ref = operand.entity_ref.as_deref()?;
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let marker = lane.sketch_entities.iter().find(|marker| {
        marker.id() == entity_ref
            && marker.feature_ref.as_deref() == Some(feature)
            && matches!(
                marker.kind(),
                SketchInputKind::Native(_) | SketchInputKind::NativeHandle(_)
            )
    })?;
    let marker_offset = usize::try_from(marker.offset()).ok()?;
    let (_, center_indices) = slot_curve_and_center_indices(&lane.native_payload, marker_offset)?;

    let entity_class = lane
        .references
        .iter()
        .find(|reference| reference.id == operand.reference_ref)
        .and_then(|reference| reference.class_ref.as_deref())
        .and_then(|class_ref| lane.classes.iter().find(|class| class.id == class_ref))
        .filter(|class| class.name == "sgEntHandle")?;
    let mut slot_classes = lane.classes.iter().filter(|class| {
        class.name == "sgSlotHandle"
            && class.offset > entity_class.offset
            && class.offset < marker.offset()
    });
    let (Some(slot_class), None) = (slot_classes.next(), slot_classes.next()) else {
        return None;
    };
    let class_end = lane
        .classes
        .iter()
        .filter(|class| class.offset > slot_class.offset)
        .map(|class| class.offset)
        .min()
        .unwrap_or_else(|| u64_from_index(lane.native_payload.len()))
        .min(marker.offset());
    let class_start = usize::try_from(slot_class.offset).ok()?;
    let class_end =
        super::DeclaredEnd::of(usize::try_from(class_end).ok()?, lane.native_payload.len())?.get();
    if class_start >= class_end {
        return None;
    }
    let reference_indices = (class_start..class_end)
        .filter_map(|offset| {
            let cell_end = offset.checked_add(12)?;
            if cell_end > class_end {
                return None;
            }
            let cell = lane.native_payload.get(offset..cell_end)?;
            if cell.get(..2) != Some(&[0xe7, 0x88])
                || cell.get(4..8) != Some(&[0xff; 4])
                || cell.get(8..12) != Some(&[0; 4])
            {
                return None;
            }
            Some(usize::from(View::u16_le_at(
                &lane.native_payload,
                offset + 2,
            )?))
        })
        .collect::<Vec<_>>();
    let [slot_index, center_index] = reference_indices.as_slice() else {
        return None;
    };
    let slot_index = u32::try_from(*slot_index).ok()?;
    let center_index = u32::try_from(*center_index).ok()?;
    if slot_index != u32::from(operand.entity_index) || marker.local_id() != Some(slot_index) {
        return None;
    }

    let mut points = lane
        .sketch_entities
        .iter()
        .filter(|candidate| candidate.feature_ref.as_deref() == Some(feature))
        .filter(|candidate| candidate.coordinates_m.is_some())
        .filter(|candidate| {
            matches!(
                candidate.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|candidate| candidate.offset());
    let [first, second] = center_indices.map(|index| points.get(index).copied());
    let (first, second) = (first?, second?);
    let center = match (
        first.local_id() == Some(center_index),
        second.local_id() == Some(center_index),
    ) {
        (true, false) => first,
        (false, true) => second,
        _ => return None,
    };
    center.coordinates_m?;
    Some((marker, center))
}

/// Resolve the indexed point-pair form of a circular dimension.
///
/// The `6e 83` operand has no explicit sketch marker. Its `sgEntHandle`
/// reference scopes an ordered point roster, while the operand index selects
/// one adjacent center/radial pair. Every pair must carry the indexed
/// center-to-radial object/local join; a radius match cannot establish the
/// carrier on its own.
pub(super) fn declared_entity_handle_indexed_circle_dimension_center<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<&'a SketchInputEntity> {
    if operand.kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_836E)
        || operand.entity_ref.is_some()
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let mut markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| {
            marker
                .coordinates_m
                .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
        })
        .filter(|marker| {
            matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset());
    let pairs = markers
        .chunks_exact(2)
        .map(|pair| [pair[0], pair[1]])
        .collect::<Vec<_>>();
    if !markers.chunks_exact(2).remainder().is_empty()
        || pairs.iter().any(|[center, radial]| {
            let Some(center_local_id) = center.local_id() else {
                return true;
            };
            center_local_id == 0
                || radial.object_index() != Some(center_local_id)
                || radial.local_id().is_none_or(|local_id| local_id == 0)
        })
    {
        return None;
    }
    let [center, radial] = *pairs.get(usize::from(operand.entity_index))?;
    let [cu, cv] = center.coordinates_m?.get();
    let [ru, rv] = radial.coordinates_m?.get();
    let radius = (ru - cu).hypot(rv - cv) * 1000.0;
    same_dimension_length(radius, expected_radius).then_some(center)
}

fn point_dimension_marker_matches_operand(
    marker: &SketchInputEntity,
    feature: &str,
    operand: &FeatureInputOperand,
) -> bool {
    let Some(entity_ref) = operand.entity_ref.as_deref() else {
        return false;
    };
    let address = u32::from(operand.entity_index);
    let identity_matches = match operand.kind {
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C) => {
            marker.object_index() == Some(address)
        }
        FeatureInputOperandKind::Native(_) => marker.local_id() == Some(address),
        _ => false,
    };
    marker.id() == entity_ref
        && marker.feature_ref.as_deref() == Some(feature)
        && identity_matches
        && matches!(
            marker.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
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
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Option<&'a SketchInputEntity> {
    if !matches!(operand.kind, FeatureInputOperandKind::Native(_)) {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let marker = lane
        .sketch_entities
        .iter()
        .find(|marker| point_dimension_marker_matches_operand(marker, feature, operand))?;
    marker
        .coordinates_m
        .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
        .then_some(marker)
}

/// Resolve a classless direct point identity for a circular dimension.
///
/// Some circular dimensions carry a reference cell and an explicit point
/// marker without an `sgEntHandle` class declaration. The reference kind,
/// feature, object index, marker identity, and marker-local address must all
/// agree. A point that is the radial member of an encoded center/radial pair
/// at the dimension's radius is not a circle center.
pub(super) fn direct_point_dimension_center<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<&'a SketchInputEntity> {
    if !matches!(operand.kind, FeatureInputOperandKind::Native(_))
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return None;
    }
    let mut candidates = lanes.iter().filter_map(|lane| {
        let reference = lane
            .references
            .iter()
            .find(|reference| reference.id == operand.reference_ref)?;
        if reference.feature_ref.as_deref() != Some(feature)
            || reference.kind != operand.kind
            || reference.object_index != operand.entity_index
            || reference.class_ref.is_some()
        {
            return None;
        }
        let marker = lane
            .sketch_entities
            .iter()
            .find(|marker| point_dimension_marker_matches_operand(marker, feature, operand))?;
        marker
            .coordinates_m
            .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
            .then_some((lane, marker))
    });
    let (lane, marker) = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    for [center, radial] in declared_entity_handle_pairs(lane, feature)
        .into_iter()
        .filter(|[_, radial]| radial.id() == marker.id())
    {
        let [cu, cv] = center.coordinates_m?.get();
        let [ru, rv] = radial.coordinates_m?.get();
        if same_dimension_length((ru - cu).hypot(rv - cv) * 1000.0, expected_radius) {
            return None;
        }
    }
    Some(marker)
}

pub(super) fn declared_entity_handle_circular_marker<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<(&'a SketchInputEntity, f64)> {
    if !expected_radius.is_finite() || expected_radius <= 0.0 {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let child_pairs = declared_entity_handle_declared_child_pairs(lane, feature);
    let pairs = declared_entity_handle_pairs(lane, feature);
    // An explicit radial identity is stronger than one feature-scoped child
    // declaration. Resolve it first because an unrelated line or arc child
    // can coexist with the circular-dimension point pair. Multiple child
    // declarations remain ambiguous, even when one point pair also matches.
    if child_pairs.len() <= 1 {
        if let Some(entity_ref) = operand.entity_ref.as_deref() {
            let mut candidates = pairs
                .iter()
                .copied()
                .filter(|[_, radial]| radial.id() == entity_ref);
            let candidate = candidates.next();
            if candidates.next().is_some() {
                return None;
            }
            if let Some([center, radial]) = candidate {
                let [cu, cv] = center.coordinates_m?.get();
                let [ru, rv] = radial.coordinates_m?.get();
                let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                return same_dimension_length(radius, expected_radius).then_some((center, radius));
            }
        }
    }
    if let [child_pair] = child_pairs.as_slice() {
        // The relation operand identifies the radial child when present. Use
        // that identity to reject a mismatched child. The scoped child
        // declaration already identifies this pair, so unrelated linked
        // pairs in the same feature do not make it ambiguous.
        let operand_identifies_child = operand
            .entity_ref
            .as_deref()
            .is_some_and(|entity_ref| child_pair[1].id() == entity_ref);
        if operand.entity_ref.is_some() && !operand_identifies_child {
            return None;
        }
        let [center, radial] = *child_pair;
        let [cu, cv] = center.coordinates_m?.get();
        let [ru, rv] = radial.coordinates_m?.get();
        let radius = (ru - cu).hypot(rv - cv) * 1000.0;
        return same_dimension_length(radius, expected_radius).then_some((center, radius));
    }
    if !child_pairs.is_empty() {
        return None;
    }
    let mut candidates = pairs.into_iter().filter_map(|[center, radial]| {
        let [cu, cv] = center.coordinates_m?.get();
        let [ru, rv] = radial.coordinates_m?.get();
        let radius = (ru - cu).hypot(rv - cv) * 1000.0;
        same_dimension_length(radius, expected_radius).then_some((center, radius))
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(super) fn declared_entity_handle_has_resolved_pair(
    lanes: &[FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> bool {
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return false;
    };
    !declared_entity_handle_pairs(lane, feature).is_empty()
}

/// Test whether an explicit point reference is the radial member of a
/// declared entity-handle pair. Radial identity cannot be reinterpreted as a
/// center by the native point-identity fallback.
pub(super) fn declared_entity_handle_point_is_declared_radial(
    lanes: &[FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> bool {
    let Some(entity_ref) = operand.entity_ref.as_deref() else {
        return false;
    };
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return false;
    };
    declared_entity_handle_pairs(lane, feature)
        .iter()
        .any(|[_, radial]| radial.id() == entity_ref)
}

fn declared_entity_handle_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut pairs = declared_entity_handle_linked_pairs(lane, feature);
    pairs.extend(declared_entity_handle_declared_child_pairs(lane, feature));
    pairs.extend(declared_entity_handle_indexed_point_pairs(lane, feature));
    pairs.sort_unstable_by_key(|[center, radial]| (center.offset(), radial.offset()));
    pairs.dedup_by(|left, right| left[0].id() == right[0].id() && left[1].id() == right[1].id());
    pairs
}

/// Resolve the indexed point form used by a declared entity handle when the
/// radial point carries its own local identifier. The adjacent roster order
/// and the center-to-radial object/local join are both required; a radius
/// match alone is not a carrier identity.
fn declared_entity_handle_indexed_point_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.coordinates_m.is_some())
        .filter(|marker| {
            matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset());
    markers
        .iter()
        .zip(markers.iter().skip(1))
        .filter_map(|(center, radial)| {
            let center_local_id = center.local_id()?;
            if center_local_id == 0
                || radial.object_index() != Some(center_local_id)
                || radial.local_id().is_none_or(|local_id| local_id == 0)
            {
                return None;
            }
            Some([*center, *radial])
        })
        .collect()
}

/// Resolve the wide child form where a curve marker is followed by its radial
/// point and the point interval declares the curve handle class. The class
/// declaration is scoped to the following marker interval; a radius match
/// alone is not sufficient because the same feature can contain repeated
/// circular construction carriers.
fn declared_entity_handle_declared_child_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut feature_markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .collect::<Vec<_>>();
    feature_markers.sort_unstable_by_key(|marker| marker.offset());
    let mut markers = feature_markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind(),
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset());
    markers
        .iter()
        .zip(markers.iter().skip(1))
        .filter_map(|(center, radial)| {
            let class_name = match center.kind() {
                SketchInputKind::Arc => "sgArcHandle",
                SketchInputKind::LineOrCircle => "sgLineHandle",
                _ => return None,
            };
            if !matches!(
                radial.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                return None;
            }
            let next_marker_offset = feature_markers
                .iter()
                .find(|marker| marker.offset() > radial.offset())
                .map_or(u64::MAX, |marker| marker.offset());
            let declared = lane.classes.iter().any(|class| {
                class.name == class_name
                    && class.offset > radial.offset()
                    && class.offset < next_marker_offset
            });
            if !declared {
                return None;
            }
            Some([*center, *radial])
        })
        .collect()
}

fn declared_entity_handle_linked_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.coordinates_m.is_some())
        .filter(|marker| {
            matches!(
                marker.kind(),
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
                    | SketchInputKind::Arc
            )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset());
    markers
        .iter()
        .zip(markers.iter().skip(1))
        .filter_map(|(center, radial)| {
            if !matches!(
                radial.kind(),
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
            ) {
                return None;
            }
            let center_local_id = center.local_id()?;
            if center_local_id == 0
                || radial.object_index() != Some(center_local_id)
                || !matches!(radial.local_id(), None | Some(0))
            {
                return None;
            }
            Some([*center, *radial])
        })
        .collect()
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
    let mut sketches_by_feature = HashMap::new();
    for feature in features {
        let operation = "index SLDPRT planar relation sketches";
        ctx.charge_work(1, operation)?;
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
        if !sketches_by_feature.contains_key(native_ref) {
            ctx.charge_collection_items(1, operation)?;
            sketches_by_feature.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        sketches_by_feature.insert(native_ref, sketch);
    }
    let transforms =
        marker_transform_candidates_by_feature(features, sketches, sketch_entities, lanes);
    let loci_by_marker = profile_loci_by_marker(features, sketches, sketch_entities, lanes);
    let mut markers_by_id = HashMap::new();
    for marker in lanes.iter().flat_map(|lane| &lane.sketch_entities) {
        let operation = "index SLDPRT planar relation markers";
        ctx.charge_work(1, operation)?;
        if !markers_by_id.contains_key(marker.id()) {
            ctx.charge_collection_items(1, operation)?;
            markers_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        markers_by_id.insert(marker.id(), marker);
    }
    let relation_parameters = owned_relation_parameters(ctx, features, parameters, lanes)?;
    let mut parameters_by_id = HashMap::new();
    for parameter in parameters {
        let operation = "index SLDPRT planar relation parameters";
        ctx.charge_work(1, operation)?;
        if !parameters_by_id.contains_key(&parameter.id) {
            ctx.charge_collection_items(1, operation)?;
            parameters_by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        parameters_by_id.insert(&parameter.id, parameter);
    }
    // The native-reference index keeps the earliest constraint for duplicate
    // references and is updated when this projection appends a constraint.
    let mut constraints_by_native_ref = HashMap::<String, usize>::new();
    for (index, constraint) in constraints.iter().enumerate() {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            let operation = "index SLDPRT planar relation constraints";
            ctx.charge_work(1, operation)?;
            if !constraints_by_native_ref.contains_key(native_ref) {
                ctx.charge_collection_items(1, operation)?;
                constraints_by_native_ref.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                let key = ctx.format_retained(
                    format_args!("{native_ref}"),
                    "copy SLDPRT planar constraint reference",
                )?;
                constraints_by_native_ref.insert(key, index);
            }
        }
    }
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            let existing = constraints_by_native_ref.get(relation.id.as_str()).copied();
            if existing.is_some_and(|index| {
                !matches!(
                    constraints[index].definition.kind(),
                    SketchConstraintDefinitionInput::Native { .. }
                )
            }) {
                continue;
            }
            let Some(parameter_id) = relation_parameters.get(&relation.id) else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let parameter = parameter_id
                .as_ref()
                .and_then(|parameter| parameters_by_id.get(parameter))
                .copied();
            let reference_parameter = parameter.is_some_and(is_reference_relation_parameter);
            let native_kind = relation_native_kind(relation.family);
            let mut entities = Vec::new();
            for marker in relation.operands.iter().filter_map(|operand| operand.entity_ref.as_deref()) {
                for entity in marker_entities(marker, &markers_by_id, &loci_by_marker) {
                    ctx.reserve_collection_vec(
                        &mut entities, 1, "collect SLDPRT planar relation entities",
                    )?;
                    entities.push(entity);
                }
            }
            entities.sort_by(|left, right| left.as_str().cmp(right.as_str()));
            entities.dedup();
            let typed_definition = match relation.family {
                FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance => {
                    profile_axis_for_relation(
                        relation,
                        transforms
                            .get(relation.feature_ref.as_str())
                            .map(Vec::as_slice),
                    )
                    .and_then(|profile_axis| {
                        typed_relation_definition_with_profile_axis(
                            relation,
                            parameter,
                            sketch,
                            sketch_entities,
                            &markers_by_id,
                            &loci_by_marker,
                            Some(profile_axis),
                        )
                    })
                }
                _ => typed_relation_definition(
                    relation,
                    parameter,
                    sketch,
                    sketch_entities,
                    &markers_by_id,
                    &loci_by_marker,
                ),
            };
            let typed_definition = typed_definition.filter(|definition| {
                !(reference_parameter
                    && relation_constraint_is_inactive(parameter, definition, sketch_entities))
            });
            let definition = if let Some(definition) = typed_definition {
                definition
            } else {
                let mut operands = Vec::new();
                for operand in &relation.operands {
                    let native_ref = operand.entity_ref.as_deref().map(|reference| {
                        ctx.format_retained(
                            format_args!("{reference}"),
                            "copy SLDPRT planar relation operand reference",
                        )
                    }).transpose()?;
                    ctx.reserve_collection_vec(
                        &mut operands, 1, "collect SLDPRT planar relation operands",
                    )?;
                    operands.push(SketchNativeOperand {
                        native_kind: operand_kind_name(operand.kind),
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
                    parameter: parameter.map(|parameter| copy_relation_parameter_id(ctx, &parameter.id)).transpose()?,
                    operands,
                }
            };
            let active = relation_constraint_is_inactive(parameter, &definition, sketch_entities)
                .then_some(false);
            let has_display_scalar =
                relation_display_scalar_for_parameter(ctx, relation, lane)?.is_some();
            let Ok(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            else {
                continue;
            };
            let id_text = ctx.format_retained(format_args!(
                    "sldprt:model:sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                ), "format SLDPRT planar relation constraint identity")?;
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
                ctx.charge_collection_items(1, operation)?;
                constraints_by_native_ref.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                let key = ctx.format_retained(
                    format_args!("{}", relation.id),
                    "copy SLDPRT planar constraint reference",
                )?;
                constraints_by_native_ref.insert(key, constraints.len());
                ctx.reserve_collection_vec(constraints, 1, "append SLDPRT planar relation constraint")?;
                constraints.push(projected);
            }
        }
        for marker in &lane.sketch_entities {
            let existing = constraints_by_native_ref.get(marker.id()).copied();
            if existing.is_some_and(|index| {
                !matches!(
                    constraints[index].definition.kind(),
                    SketchConstraintDefinitionInput::Native { .. }
                )
            }) {
                continue;
            }
            let Some(sketch) = marker
                .feature_ref
                .as_deref()
                .and_then(|feature| sketches_by_feature.get(feature))
            else {
                continue;
            };
            let Some(definition) = typed_marker_relation_definition_in_sketch(
                marker,
                sketch,
                sketch_entities,
                &markers_by_id,
                &loci_by_marker,
            ) else {
                continue;
            };
            let active =
                marker_relation_is_inactive(marker, &definition, sketch_entities).then_some(false);
            let Ok(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            else {
                continue;
            };
            let id_text = ctx.format_retained(format_args!(
                    "sldprt:model:sketch-constraint#marker:{lane_key}:{}",
                    marker.offset()
                ), "format SLDPRT planar marker constraint identity")?;
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
                ctx.charge_collection_items(1, operation)?;
                constraints_by_native_ref.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                let key = ctx.format_retained(
                    format_args!("{}", marker.id()),
                    "copy SLDPRT planar constraint reference",
                )?;
                constraints_by_native_ref.insert(key, constraints.len());
                ctx.reserve_collection_vec(constraints, 1, "append SLDPRT planar marker constraint")?;
                constraints.push(projected);
            }
        }
    }
    Ok(())
}

fn copy_planar_sketch_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::sketches::SketchId,
) -> Result<cadmpeg_ir::sketches::SketchId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(
        format_args!("{}", id.as_str()),
        "copy SLDPRT planar sketch identity",
    )?;
    cadmpeg_ir::sketches::SketchId::mint(text).map_err(|_| {
        cadmpeg_core::CodecError::Malformed("SolidWorks planar sketch identity is invalid".into())
    })
}

fn copy_relation_parameter_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::features::ParameterId,
) -> Result<cadmpeg_ir::features::ParameterId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(
        format_args!("{}", id.as_str()),
        "copy SLDPRT relation parameter identity",
    )?;
    cadmpeg_ir::features::ParameterId::mint(text).map_err(|_| {
        cadmpeg_core::CodecError::Malformed("SolidWorks relation parameter identity is invalid".into())
    })
}

fn claim_relation_parameter(
    ctx: &DecodeContext<'_>,
    claimed: &mut HashSet<cadmpeg_ir::features::ParameterId>,
    id: &cadmpeg_ir::features::ParameterId,
) -> Result<bool, cadmpeg_core::CodecError> {
    if claimed.contains(id) {
        return Ok(false);
    }
    let operation = "claim SLDPRT relation parameter";
    ctx.charge_collection_items(1, operation)?;
    claimed.try_reserve(1).map_err(|_| {
        ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
    })?;
    Ok(claimed.insert(copy_relation_parameter_id(ctx, id)?))
}

fn record_relation_parameter(
    ctx: &DecodeContext<'_>,
    owned: &mut HashMap<String, Option<cadmpeg_ir::features::ParameterId>>,
    relation_id: &str,
    parameter: Option<&cadmpeg_ir::features::ParameterId>,
) -> Result<(), cadmpeg_core::CodecError> {
    let operation = "index SLDPRT relation parameter ownership";
    if !owned.contains_key(relation_id) {
        ctx.charge_collection_items(1, operation)?;
        owned.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
        })?;
    }
    let relation_id = ctx.format_retained(
        format_args!("{relation_id}"),
        "copy SLDPRT relation identity",
    )?;
    let parameter = parameter.map(|id| copy_relation_parameter_id(ctx, id)).transpose()?;
    owned.insert(relation_id, parameter);
    Ok(())
}

pub(crate) fn owned_relation_parameters<'a>(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
) -> Result<HashMap<String, Option<cadmpeg_ir::features::ParameterId>>, cadmpeg_core::CodecError> {
    let mut lane_refs = Vec::new();
    for lane in lanes {
        ctx.reserve_collection_vec(&mut lane_refs, 1, "collect SLDPRT relation lanes")?;
        lane_refs.push(lane);
    }
    let mut parameters_by_scalar = HashMap::new();
    for parameter in parameters {
        let Some(native_ref) = parameter.native_ref.as_deref() else {
            continue;
        };
        let operation = "index SLDPRT relation scalars";
        ctx.charge_work(1, operation)?;
        if !parameters_by_scalar.contains_key(native_ref) {
            ctx.charge_collection_items(1, operation)?;
            parameters_by_scalar.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        parameters_by_scalar.insert(native_ref, parameter);
    }
    let mut claimed = HashSet::new();
    let mut owned = HashMap::new();
    for lane in &lane_refs {
        for relation in &lane.relation_instances {
            ctx.charge_work(1, "scan SLDPRT relation ownership")?;
            let Some(scalar) = relation.parameter_scalar_ref() else {
                continue;
            };
            let parameter = if let Some(parameter) = parameters_by_scalar.get(scalar) {
                Some(&parameter.id)
            } else {
                relation_parameter_by_driving_name(ctx, relation, lane, features, parameters)?
                    .map(|parameter| &parameter.id)
            };
            if let Some(parameter) = parameter {
                claim_relation_parameter(ctx, &mut claimed, parameter)?;
            }
            record_relation_parameter(ctx, &mut owned, &relation.id, parameter)?;
        }
    }
    for lane in &lane_refs {
        for relation in &lane.relation_instances {
            ctx.charge_work(1, "scan SLDPRT relation ownership")?;
            if relation.parameter_scalar_ref().is_some() {
                continue;
            }
            let mut exact_matches = relation
                .scalar_refs()
                .iter()
                .filter_map(|scalar| parameters_by_scalar.get(scalar.as_str()).copied());
            if let (Some(parameter), None) = (exact_matches.next(), exact_matches.next()) {
                if claim_relation_parameter(ctx, &mut claimed, &parameter.id)? {
                    record_relation_parameter(ctx, &mut owned, &relation.id, Some(&parameter.id))?;
                }
                continue;
            }
            let mut parameter = relation_parameter_by_relation_id(relation, parameters);
            if parameter.is_none() {
                parameter = relation_parameter_by_driving_name(ctx, relation, lane, features, parameters)?;
            }
            if parameter.is_none() {
                parameter = circle_dimension_handle_driver(relation, lane)
                    .and_then(|scalar| parameters_by_scalar.get(scalar.id.as_str()).copied());
            }
            if parameter.is_none() {
                parameter = relation_parameter_by_display_name(ctx, relation, lane, features, parameters)?;
            }
            let Some(parameter) = parameter else {
                continue;
            };
            if claim_relation_parameter(ctx, &mut claimed, &parameter.id)? {
                record_relation_parameter(ctx, &mut owned, &relation.id, Some(&parameter.id))?;
            }
        }
    }
    Ok(owned)
}

fn relation_display_scalar<'a>(
    relation: &FeatureInputRelationInstance,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputScalar> {
    if let Some(display_id) = relation.display_scalar_ref() {
        return lane
            .scalars
            .iter()
            .find(|scalar| scalar.id == display_id)
            .filter(|scalar| scalar.role == FeatureInputScalarRole::Display);
    }
    let mut candidates = relation
        .scalar_refs()
        .iter()
        .filter_map(|scalar_id| lane.scalars.iter().find(|scalar| scalar.id == *scalar_id))
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Display);
    let (Some(scalar), None) = (candidates.next(), candidates.next()) else {
        return None;
    };
    Some(scalar)
}

pub(super) fn relation_display_scalar_for_parameter<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &'a FeatureInputLane,
) -> Result<Option<&'a FeatureInputScalar>, cadmpeg_core::CodecError> {
    if let Some(scalar) = relation_display_scalar(relation, lane) {
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
    let mut scalars = Vec::new();
    for scalar_id in relation.scalar_refs() {
        if let Some(scalar) = lane.scalars.iter().find(|scalar| scalar.id == *scalar_id) {
            ctx.reserve_collection_vec(&mut scalars, 1, "collect SLDPRT display relation scalars")?;
            scalars.push(scalar);
        }
    }
    let Some(&first) = scalars.first() else {
        return Ok(None);
    };
    if scalars.windows(2).any(|pair| {
        pair[1].ordinal != pair[0].ordinal.checked_add(1).unwrap_or(u32::MAX)
    }) {
        return Ok(None);
    }
    let Some(first_name) = lane
        .names
        .iter()
        .find(|name| name.id == first.name)
        .map(|name| name.value.as_str())
    else {
        return Ok(None);
    };
    let Some(first_kind) = first.operands.first().map(|operand| operand.kind) else {
        return Ok(None);
    };
    let mut entity_indices = Vec::new();
    for scalar in &scalars {
        let [operand] = scalar.operands.as_slice() else {
            return Ok(None);
        };
        let Some(name) = lane
            .names
            .iter()
            .find(|name| name.id == scalar.name)
            .map(|name| name.value.as_str())
        else {
            return Ok(None);
        };
        if scalar.role != FeatureInputScalarRole::Display
            || operand.kind != first_kind
            || operand.kind != relation.operands[0].kind
            || name != first_name
            || entity_indices.contains(&operand.entity_index)
        {
            return Ok(None);
        }
        ctx.reserve_collection_vec(&mut entity_indices, 1, "collect SLDPRT display relation entities")?;
        entity_indices.push(operand.entity_index);
    }
    Ok((scalars.len() == relation.scalar_refs().len()).then_some(first))
}

fn relation_parameter_by_relation_id<'a>(
    relation: &FeatureInputRelationInstance,
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Option<&'a cadmpeg_ir::features::DesignParameter> {
    let mut matches = parameters
        .iter()
        .filter(|parameter| {
            parameter.properties.get(RELATION_PARAMETER_ID_PROPERTY) == Some(&relation.id)
                && is_reference_relation_parameter(parameter)
        });
    let (Some(parameter), None) = (matches.next(), matches.next()) else {
        return None;
    };
    Some(parameter)
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
    let Some(owner) = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(relation.feature_ref.as_str()))
    else {
        return Ok(None);
    };
    let owner = &owner.id;
    let mut scalars = HashMap::new();
    for scalar in &lane.scalars {
        let operation = "index SLDPRT relation driving scalars";
        ctx.charge_work(1, operation)?;
        if !scalars.contains_key(scalar.id.as_str()) {
            ctx.charge_collection_items(1, operation)?;
            scalars.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        scalars.insert(scalar.id.as_str(), scalar);
    }
    let mut names = HashMap::new();
    for name in &lane.names {
        let operation = "index SLDPRT relation driving names";
        ctx.charge_work(1, operation)?;
        if !names.contains_key(name.id.as_str()) {
            ctx.charge_collection_items(1, operation)?;
            names.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        names.insert(name.id.as_str(), name.value.as_str());
    }
    let mut driving_names = relation
        .parameter_scalar_ref()
        .into_iter()
        .chain(relation.scalar_refs().iter().map(String::as_str))
        .filter_map(|scalar| scalars.get(scalar))
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
        .filter_map(|scalar| names.get(scalar.name.as_str()).copied());
    let Some(name) = driving_names.next() else {
        return Ok(None);
    };
    if driving_names.any(|candidate| candidate != name) {
        return Ok(None);
    }
    let mut matches = parameters.iter().filter(|parameter| {
        parameter.owner.as_ref() == Some(owner) && parameter.name.as_str() == name
    });
    let Some(parameter) = matches.next() else {
        return Ok(None);
    };
    Ok(matches.next().is_none().then_some(parameter))
}

pub(super) fn relation_parameter_by_display_name<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Result<Option<&'a cadmpeg_ir::features::DesignParameter>, cadmpeg_core::CodecError> {
    let Some(owner) = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(relation.feature_ref.as_str()))
    else {
        return Ok(None);
    };
    let owner = &owner.id;
    let mut names = HashMap::new();
    for name in &lane.names {
        let operation = "index SLDPRT relation display names";
        ctx.charge_work(1, operation)?;
        if !names.contains_key(name.id.as_str()) {
            ctx.charge_collection_items(1, operation)?;
            names.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        names.insert(name.id.as_str(), name.value.as_str());
    }
    let Some(display_scalar) = relation_display_scalar_for_parameter(ctx, relation, lane)? else {
        return Ok(None);
    };
    let Some(name) = names.get(display_scalar.name.as_str()).copied() else {
        return Ok(None);
    };
    let mut matches = parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(owner) && parameter.name == name);
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    Ok((matches.all(|parameter| parameter.id == first.id)
        && relation_parameter_matches_display_scalar(first, relation.family, display_scalar))
    .then_some(first))
}

#[cfg(test)]
mod relation_geometry_tests {
    use super::super::relation_loci::same_dimension_length;
    use super::{
        project_relation_bindings, project_relation_solved_line_geometry,
        project_relation_solved_point_geometry, project_spatial_relation_bindings,
        spatial_point_line_distance, unique_dynamic_line_pair,
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
    fn solver_point_relation_projects_graph_resolved_operands() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"relation test", &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).unwrap();
        use cadmpeg_ir::sketches::{Sketch, SketchLocus, SketchPlacement};
        use cadmpeg_ir::{
            features::{
                Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
                ParameterValue,
            },
            scalar::Length,
        };
        use std::collections::BTreeMap;

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
        ).unwrap();

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
        ).unwrap();
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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"relation test", &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).unwrap();
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
                    offset: 40 + index as u64,
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
        ).unwrap();

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
        ).unwrap();
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
        let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
        let line = |id: &str, start: Point2, end: Point2| {
            SketchEntity::new(
                SketchEntityId::mint(id).unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
            )
            .with_construction(true)
        };
        let generated = vec![
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

        let [first, second] = unique_dynamic_line_pair(
            16.0,
            &sketch,
            &existing,
            &generated,
            TEST_LINE_GEOMETRY_QUANTUM,
        )
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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            b"relation test", &arena, &cadmpeg_core::decode::DecodePolicy::service(),
        ).unwrap();
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
                offset as u32,
                offset as u64,
                SketchInputKind::Point,
            );
            marker.feature_ref = Some(FEATURE.into());
            marker = marker.with_test_identity(Some(object_index), marker.local_id());
            marker
        }

        const FEATURE: &str = "synthetic:test:id#feature";
        const LANE: &str = "lane";
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
        ).unwrap();

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
