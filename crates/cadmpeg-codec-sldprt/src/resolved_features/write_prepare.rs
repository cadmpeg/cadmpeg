//! Sketch write preparation and write-back validation.

use super::bindings::bind_scalar_operands;
use super::hashes::{constraint_hash, lane_hash, sketch_hash};
use super::markers::{
    marker_spatial_coordinate_offset, reference_cells, relation_bindings, sketch_input_entities,
    spatial_sketches, spatial_vertex_offsets,
};
use super::names::{class_declarations, object_names};
use super::scalars::{feature_object_name, named_scalars};
use super::sketch_write::{patch_line_profiles, same_sketch_point, sketch_brep};
use super::transforms::{locus_entity, sketch_entity_loci};
use super::typed_relations::{sketch_entity_contains_point, symmetric_loci_match_axis};
use super::write_generate::{
    append_generated_object_name, append_generated_sketch_markers, generated_locus_is_point,
};
use super::{SKETCH_MARKER, SKETCH_POINT_TOLERANCE, SPATIAL_VERTEX_PREFIX};
use crate::records::{FeatureInputLane, SketchRelationKind};
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry,
    SketchId, SketchLocus, SpatialSketchGeometry, SpatialSketchId,
};

#[cfg(test)]
use super::selections::{coordinate_marker_local_links, marker_local_links};
#[cfg(test)]
use super::write_generate::{
    append_coordinate_marker, append_coordinate_marker_link, append_reference_marker,
    generated_marker_relations, GeneratedMarkerRelation,
};

/// Reject unsupported neutral sketch edits before native lane replay.
///
/// Bitwise comparison against the machine-local document baseline; see
/// [`cadmpeg_ir::hash::document_local_sha256`]. Absent baseline: sync lanes from
/// the neutral side.
pub fn prepare_sketches_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), cadmpeg_core::CodecError> {
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_sketch_local_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_sketch_sha256"));
    let baseline_constraints = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_neutral_sketch_constraint_local_sha256")
    });
    let current_neutral = sketch_hash(ir);
    let current_native = native.as_ref().map(lane_hash);
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.sketches.is_empty()
            && ir.model.sketch_entities.is_empty()
            && ir.model.sketch_constraints.is_empty()
            && ir.model.spatial_sketches.is_empty()
            && ir.model.spatial_sketch_entities.is_empty()
        {
            return Ok(());
        }
        validate_source_less_constraints(ir)?;
        let native = native.get_or_insert_with(crate::native::SldprtNative::default);
        let generated = source_less_lanes(ir, native)?;
        native.feature_input_lanes.extend(generated);
        return Ok(());
    }
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &current_neutral);
    if !neutral_changed {
        return Ok(());
    }
    let current_constraints = constraint_hash(ir);
    if baseline_constraints.is_none_or(|hash| hash != &current_constraints) {
        return Err(cadmpeg_core::CodecError::NotImplemented(
            "SLDPRT native sketch relation editing is not implemented".into(),
        ));
    }
    let native_changed = match (&current_native, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if native_changed {
        return Err(cadmpeg_core::CodecError::Malformed(
            "conflicting neutral and native SLDPRT sketch edits".into(),
        ));
    }
    let retained = native.as_mut().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(
            "SLDPRT sketch write-back requires retained feature-input lanes".into(),
        )
    })?;
    patch_spatial_sketches(ir, retained)?;
    patch_line_profiles(ir, retained)
}

fn patch_spatial_sketches(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), cadmpeg_core::CodecError> {
    for sketch in &ir.model.spatial_sketches {
        let owners = ir
            .model
            .features
            .iter()
            .filter(|feature| {
                matches!(
                    &feature.definition,
                    FeatureDefinition::SpatialSketch {
                        sketch: Some(candidate),
                    } if candidate == &sketch.id
                )
            })
            .collect::<Vec<_>>();
        let [owner] = owners.as_slice() else {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "SLDPRT spatial sketch {} requires one owning feature",
                sketch.id.0
            )));
        };
        let native_ref = owner.native_ref.as_deref().ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} requires a retained feature object",
                sketch.id.0
            ))
        })?;
        let record = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|record| record.id == native_ref)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "SLDPRT spatial sketch {} references missing feature object {native_ref}",
                    sketch.id.0
                ))
            })?;
        let entities = ir
            .model
            .spatial_sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect::<Vec<_>>();
        if entities.is_empty() {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} requires at least one retained line",
                sketch.id.0
            )));
        }
        let point_entities = entities
            .iter()
            .copied()
            .filter(|entity| matches!(entity.geometry, SpatialSketchGeometry::Point { .. }))
            .collect::<Vec<_>>();
        for entity in &point_entities {
            let SpatialSketchGeometry::Point { position } = entity.geometry else {
                unreachable!("spatial point filter establishes the geometry family");
            };
            let native_ref = entity.native_ref.as_deref().ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT spatial sketch point {} requires a retained native marker",
                    entity.id.0
                ))
            })?;
            let candidates = native
                .feature_input_lanes
                .iter()
                .enumerate()
                .filter_map(|(lane_index, lane)| {
                    let marker = lane
                        .sketch_entities
                        .iter()
                        .find(|marker| marker.id == native_ref)?;
                    let offset = usize::try_from(marker.offset).ok()?;
                    marker_spatial_coordinate_offset(&lane.native_payload, offset)?;
                    Some((lane_index, offset))
                })
                .collect::<Vec<_>>();
            let [(lane_index, offset)] = candidates.as_slice() else {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT spatial sketch point {} does not resolve to one native marker",
                    entity.id.0
                )));
            };
            patch_spatial_marker_point(
                &mut native.feature_input_lanes[*lane_index].native_payload,
                *offset,
                position,
            )?;
        }
        let line_entities = entities
            .iter()
            .copied()
            .filter(|entity| matches!(entity.geometry, SpatialSketchGeometry::Line { .. }))
            .collect::<Vec<_>>();
        if entities.len() != line_entities.len() + point_entities.len() {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} supports retained point and line geometry only",
                sketch.id.0
            )));
        }
        if line_entities.is_empty() {
            continue;
        }
        let candidates = native
            .feature_input_lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| sketch.native_ref.as_deref().is_none_or(|id| id == lane.id))
            .filter_map(|(lane_index, lane)| {
                let name = feature_object_name(record, lane)?;
                let object_start = usize::try_from(name.offset).ok()?;
                let object_end = native
                    .feature_histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .filter_map(|candidate| feature_object_name(candidate, lane))
                    .filter(|candidate| candidate.offset > name.offset)
                    .map(|candidate| candidate.offset)
                    .min()
                    .and_then(|offset| usize::try_from(offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let offsets =
                    spatial_vertex_offsets(lane.native_payload.get(object_start..object_end)?);
                if line_entities
                    .len()
                    .checked_mul(2)
                    .is_none_or(|expected| offsets.len() != expected)
                {
                    return None;
                }
                Some((
                    lane_index,
                    offsets
                        .into_iter()
                        .map(|offset| object_start + offset)
                        .collect::<Vec<_>>(),
                ))
            })
            .collect::<Vec<_>>();
        let [(lane_index, offsets)] = candidates.as_slice() else {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} does not resolve to one feature object with two vertices per line",
                sketch.id.0
            )));
        };
        let payload = &mut native.feature_input_lanes[*lane_index].native_payload;
        for (entity, offsets) in line_entities.iter().zip(offsets.chunks_exact(2)) {
            let SpatialSketchGeometry::Line { start, end } = entity.geometry else {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT spatial sketch {} supports retained line geometry only",
                    sketch.id.0
                )));
            };
            if start == end {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "SLDPRT spatial sketch {} has a zero-length line",
                    sketch.id.0
                )));
            }
            patch_spatial_vertex(payload, offsets[0], start)?;
            patch_spatial_vertex(payload, offsets[1], end)?;
        }
    }

    let mut features = crate::history::project_features(&native.feature_histories);
    let (projected_sketches, projected_entities) = spatial_sketches(
        &mut features,
        &native.feature_histories,
        &native.feature_input_lanes,
    );
    if ir.model.spatial_sketches != projected_sketches
        || ir.model.spatial_sketch_entities != projected_entities
    {
        return Err(cadmpeg_core::CodecError::NotImplemented(
            "SLDPRT spatial sketch edit has no complete native lane encoding".into(),
        ));
    }
    Ok(())
}

fn patch_spatial_marker_point(
    payload: &mut [u8],
    offset: usize,
    point: Point3,
) -> Result<(), cadmpeg_core::CodecError> {
    let native = spatial_point_native_coordinates(point)?;
    let coordinate_offset = marker_spatial_coordinate_offset(payload, offset).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed(
            "SLDPRT spatial point marker changed native storage shape".into(),
        )
    })?;
    let coordinates = payload
        .get_mut(coordinate_offset..coordinate_offset + 24)
        .ok_or_else(|| {
            cadmpeg_core::CodecError::Malformed(
                "SLDPRT spatial point marker coordinates lie outside its feature-input lane".into(),
            )
        })?;
    coordinates[0..8].copy_from_slice(&native[0].to_le_bytes());
    coordinates[8..16].copy_from_slice(&native[1].to_le_bytes());
    coordinates[16..24].copy_from_slice(&native[2].to_le_bytes());
    Ok(())
}

pub(super) fn patch_spatial_vertex(
    payload: &mut [u8],
    offset: usize,
    point: Point3,
) -> Result<(), cadmpeg_core::CodecError> {
    let bytes = payload.get_mut(offset..offset + 69).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed(
            "SLDPRT spatial vertex record lies outside its feature-input lane".into(),
        )
    })?;
    if bytes.get(..SPATIAL_VERTEX_PREFIX.len()) != Some(SPATIAL_VERTEX_PREFIX)
        || bytes.get(43..45) != Some(&[0x0e, 0x00])
    {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT spatial vertex record changed shape".into(),
        ));
    }
    bytes[45..53].copy_from_slice(&point.x.to_le_bytes());
    bytes[53..61].copy_from_slice(&point.y.to_le_bytes());
    bytes[61..69].copy_from_slice(&point.z.to_le_bytes());
    Ok(())
}

fn validate_source_less_constraints(
    ir: &cadmpeg_ir::CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    for constraint in &ir.model.sketch_constraints {
        let SketchConstraintDefinition::CoincidentLoci { loci } = &constraint.definition else {
            validate_generated_marker_constraint(ir, constraint)?;
            continue;
        };
        if loci.len() < 2 {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "sketch constraint {} requires at least two loci",
                constraint.id.0
            )));
        }
        let mut expected = None;
        for locus in loci {
            let point = constraint_locus_point(ir, constraint, locus)?;
            if expected.is_some_and(|expected| !same_sketch_point(expected, point)) {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT sketch constraint {} has noncoincident locus coordinates",
                    constraint.id.0
                )));
            }
            expected = Some(point);
        }
    }
    Ok(())
}

fn validate_generated_marker_constraint(
    ir: &cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
) -> Result<(), cadmpeg_core::CodecError> {
    if !ir.model.features.iter().any(|feature| {
        matches!(
            &feature.definition,
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } if sketch == &constraint.sketch
        )
    }) {
        return Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT marker relation {} requires an owning sketch feature",
            constraint.id.0
        )));
    }
    match &constraint.definition {
        SketchConstraintDefinition::HorizontalPoints { first, second }
        | SketchConstraintDefinition::VerticalPoints { first, second } => {
            let first_point = constraint_locus_point(ir, constraint, first)?;
            let second_point = constraint_locus_point(ir, constraint, second)?;
            let delta = if matches!(
                &constraint.definition,
                SketchConstraintDefinition::HorizontalPoints { .. }
            ) {
                (first_point.v - second_point.v).abs()
            } else {
                (first_point.u - second_point.u).abs()
            };
            if constraint.active != Some(false) && delta > SKETCH_POINT_TOLERANCE {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT sketch constraint {} is not satisfied by its locus coordinates",
                    constraint.id.0
                )));
            }
            return Ok(());
        }
        SketchConstraintDefinition::Midpoint { point, entity } => {
            let point = constraint_locus_point(ir, constraint, point)?;
            let entity = sketch_constraint_entity(ir, constraint, entity)?;
            let (start, end) = sketch_line(&entity.geometry).ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(format!(
                    "source-less SLDPRT midpoint constraint {} requires a line entity",
                    constraint.id.0
                ))
            })?;
            if !same_point2(
                point,
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
            ) {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT sketch constraint {} is not satisfied by its midpoint coordinates",
                    constraint.id.0
                )));
            }
            return Ok(());
        }
        SketchConstraintDefinition::AtIntersection {
            point,
            first,
            second,
        } => {
            if first == second {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT at-intersection constraint {} repeats one entity",
                    constraint.id.0
                )));
            }
            let point = constraint_locus_point(ir, constraint, point)?;
            for entity in [first, second] {
                let entity = sketch_constraint_entity(ir, constraint, entity)?;
                if !sketch_entity_contains_point(entity, point) {
                    return Err(cadmpeg_core::CodecError::malformed(format_args!(
                        "source-less SLDPRT at-intersection constraint {} is not satisfied by its entity geometry",
                        constraint.id.0
                    )));
                }
            }
            return Ok(());
        }
        SketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } => {
            if first == second {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT symmetric constraint {} repeats one locus",
                    constraint.id.0
                )));
            }
            let first = constraint_locus_point(ir, constraint, first)?;
            let second = constraint_locus_point(ir, constraint, second)?;
            let axis = sketch_constraint_entity(ir, constraint, axis)?;
            let Some(solved) = symmetric_loci_match_axis(first, second, axis) else {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT symmetric constraint {} requires a nondegenerate line axis",
                    constraint.id.0
                )));
            };
            if !solved {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT symmetric constraint {} is not satisfied by its locus coordinates",
                    constraint.id.0
                )));
            }
            return Ok(());
        }
        _ => {}
    }
    let dimension_parameter = match &constraint.definition {
        SketchConstraintDefinition::Distance { parameter, .. }
        | SketchConstraintDefinition::DistanceLoci { parameter, .. }
        | SketchConstraintDefinition::HorizontalDistance { parameter, .. }
        | SketchConstraintDefinition::VerticalDistance { parameter, .. }
        | SketchConstraintDefinition::Angle { parameter, .. }
        | SketchConstraintDefinition::Radius { parameter, .. }
        | SketchConstraintDefinition::Diameter { parameter, .. } => Some(parameter),
        _ => None,
    };
    if let Some(parameter_id) = dimension_parameter {
        let parameter = ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == *parameter_id)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT dimension {} references missing parameter {}",
                    constraint.id.0, parameter_id.0
                ))
            })?;
        let compatible = match &constraint.definition {
            SketchConstraintDefinition::Angle { .. } => {
                matches!(
                    parameter.value,
                    Some(cadmpeg_ir::features::ParameterValue::Angle(_))
                )
            }
            _ => matches!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Length(_))
            ),
        };
        if !compatible {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "source-less SLDPRT dimension parameter {} has no compatible evaluated value",
                parameter.id.0
            )));
        }
        let expected_display = match &constraint.definition {
            SketchConstraintDefinition::Radius { .. } => {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius)
            }
            SketchConstraintDefinition::Diameter { .. } => {
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter)
            }
            _ => None,
        };
        if parameter.display != expected_display {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "source-less SLDPRT dimension parameter {} has incompatible display semantics",
                parameter.id.0
            )));
        }
        let owner = ir.model.features.iter().find(|feature| {
            matches!(
                &feature.definition,
                FeatureDefinition::Sketch { sketch: Some(sketch), .. }
                    if sketch == &constraint.sketch
            )
        });
        if owner.is_none_or(|owner| parameter.owner.as_ref() != Some(&owner.id)) {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "source-less SLDPRT dimension parameter {} is not owned by its sketch feature",
                parameter.id.0
            )));
        }
        if constraint.active != Some(false) {
            validate_solved_dimension(ir, constraint, parameter)?;
        }
        return Ok(());
    }
    if let Some((kind, first, second)) = binary_marker_relation(&constraint.definition) {
        let first = sketch_constraint_entity(ir, constraint, first)?;
        let second = sketch_constraint_entity(ir, constraint, second)?;
        validate_solved_binary_relation(constraint, kind, first, second)?;
        return Ok(());
    }
    let (entity_id, axis) = match &constraint.definition {
        SketchConstraintDefinition::Horizontal { entity } => (entity, Some(false)),
        SketchConstraintDefinition::Vertical { entity } => (entity, Some(true)),
        SketchConstraintDefinition::Fixed { entity } => (entity, None),
        SketchConstraintDefinition::ArcAngle { entity, angle } => {
            if arc_angle_relation_kind(angle.0).is_none() {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "source-less SLDPRT arc-angle constraint {} is not 90, 180, or 270 degrees",
                    constraint.id.0
                )));
            }
            (entity, None)
        }
        SketchConstraintDefinition::EllipseAngle { entity, angle } => {
            if ellipse_angle_relation_kind(angle.0).is_none() {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "source-less SLDPRT ellipse-angle constraint {} is not 90, 180, or 270 degrees",
                    constraint.id.0
                )));
            }
            (entity, None)
        }
        _ => {
            return Err(cadmpeg_core::CodecError::NotImplemented(
                "source-less SLDPRT sketch constraints support solved endpoint coincidences and horizontal, vertical, or fixed marker relations"
                    .into(),
            ));
        }
    };
    let entity = ir
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.id == *entity_id && entity.sketch == constraint.sketch)
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "sketch constraint {} references entity {} outside sketch {}",
                constraint.id.0, entity_id.0, constraint.sketch.0
            ))
        })?;
    if matches!(
        &constraint.definition,
        SketchConstraintDefinition::ArcAngle { .. }
    ) && !matches!(&entity.geometry, SketchGeometry::Arc { .. })
    {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "sketch constraint {} applies an arc-angle relation to a non-arc entity",
            constraint.id.0
        )));
    }
    if matches!(
        &constraint.definition,
        SketchConstraintDefinition::EllipseAngle { .. }
    ) && !matches!(&entity.geometry, SketchGeometry::Ellipse { .. })
    {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "sketch constraint {} applies an ellipse-angle relation to a non-ellipse entity",
            constraint.id.0
        )));
    }
    let Some(axis) = axis else {
        return Ok(());
    };
    let SketchGeometry::Line { start, end } = entity.geometry else {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "sketch constraint {} applies an axis relation to a non-line entity",
            constraint.id.0
        )));
    };
    let delta = if axis {
        (end.u - start.u).abs()
    } else {
        (end.v - start.v).abs()
    };
    if constraint.active != Some(false) && delta > SKETCH_POINT_TOLERANCE {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT sketch constraint {} is not satisfied by its line coordinates",
            constraint.id.0
        )));
    }
    Ok(())
}

fn validate_solved_dimension(
    ir: &cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<(), cadmpeg_core::CodecError> {
    let expected = match parameter.value {
        Some(cadmpeg_ir::features::ParameterValue::Length(value)) => value.0,
        Some(cadmpeg_ir::features::ParameterValue::Angle(value)) => value.0,
        _ => unreachable!("dimension parameter compatibility was checked by the caller"),
    };
    let mut measured = match &constraint.definition {
        SketchConstraintDefinition::DistanceLoci { first, second, .. } => match (first, second) {
            (SketchLocus::Entity(first), SketchLocus::Entity(second))
                if !generated_locus_is_point(ir, first)
                    && !generated_locus_is_point(ir, second) =>
            {
                line_line_dimension(
                    constraint,
                    sketch_constraint_entity(ir, constraint, first)?,
                    sketch_constraint_entity(ir, constraint, second)?,
                )?
            }
            (SketchLocus::Entity(line), point) if !generated_locus_is_point(ir, line) => {
                point_line_dimension(
                    constraint_locus_point(ir, constraint, point)?,
                    sketch_constraint_entity(ir, constraint, line)?,
                    constraint,
                )?
            }
            (point, SketchLocus::Entity(line)) if !generated_locus_is_point(ir, line) => {
                point_line_dimension(
                    constraint_locus_point(ir, constraint, point)?,
                    sketch_constraint_entity(ir, constraint, line)?,
                    constraint,
                )?
            }
            _ => {
                let first = constraint_locus_point(ir, constraint, first)?;
                let second = constraint_locus_point(ir, constraint, second)?;
                vector2_length([second.u - first.u, second.v - first.v])
            }
        },
        SketchConstraintDefinition::Distance { entities, .. } => {
            let [first, second] = entities.as_slice() else {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "source-less SLDPRT distance dimension {} requires exactly two lines",
                    constraint.id.0
                )));
            };
            line_line_dimension(
                constraint,
                sketch_constraint_entity(ir, constraint, first)?,
                sketch_constraint_entity(ir, constraint, second)?,
            )?
        }
        SketchConstraintDefinition::HorizontalDistance { first, second, .. } => {
            let first = constraint_locus_point(ir, constraint, first)?;
            let second = constraint_locus_point(ir, constraint, second)?;
            (second.u - first.u).abs()
        }
        SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            let first = constraint_locus_point(ir, constraint, first)?;
            let second = constraint_locus_point(ir, constraint, second)?;
            (second.v - first.v).abs()
        }
        SketchConstraintDefinition::Angle { first, second, .. } => {
            let first = sketch_constraint_entity(ir, constraint, first)?;
            let second = sketch_constraint_entity(ir, constraint, second)?;
            let (first_start, first_end) = sketch_line(&first.geometry).ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(format!(
                    "source-less SLDPRT angular dimension {} requires two lines",
                    constraint.id.0
                ))
            })?;
            let (second_start, second_end) = sketch_line(&second.geometry).ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(format!(
                    "source-less SLDPRT angular dimension {} requires two lines",
                    constraint.id.0
                ))
            })?;
            let first = [first_end.u - first_start.u, first_end.v - first_start.v];
            let second = [second_end.u - second_start.u, second_end.v - second_start.v];
            let denominator = vector2_length(first) * vector2_length(second);
            if denominator <= SKETCH_POINT_TOLERANCE {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT angular dimension {} has a degenerate line",
                    constraint.id.0
                )));
            }
            ((first[0] * second[0] + first[1] * second[1]) / denominator)
                .clamp(-1.0, 1.0)
                .acos()
        }
        SketchConstraintDefinition::Radius { entity, .. }
        | SketchConstraintDefinition::Diameter { entity, .. } => {
            let entity = sketch_constraint_entity(ir, constraint, entity)?;
            let radius = match &entity.geometry {
                SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
                    radius.0
                }
                _ => {
                    return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                        "source-less SLDPRT radial dimension {} requires circular geometry",
                        constraint.id.0
                    )));
                }
            };
            if matches!(
                constraint.definition,
                SketchConstraintDefinition::Diameter { .. }
            ) {
                radius * 2.0
            } else {
                radius
            }
        }
        _ => unreachable!("only dimension definitions are passed"),
    };
    if matches!(
        &constraint.definition,
        SketchConstraintDefinition::Angle { .. }
    ) {
        let supplement = std::f64::consts::PI - measured;
        if (supplement - expected).abs() < (measured - expected).abs() {
            measured = supplement;
        }
    }
    let tolerance = SKETCH_POINT_TOLERANCE * (1.0 + measured.abs().max(expected.abs()));
    if !measured.is_finite() || (measured - expected).abs() > tolerance {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT dimension {} value {} is not satisfied by measured geometry {}",
            constraint.id.0, expected, measured
        )));
    }
    Ok(())
}

fn point_line_dimension(
    point: Point2,
    line: &SketchEntity,
    constraint: &SketchConstraint,
) -> Result<f64, cadmpeg_core::CodecError> {
    let (start, end) = sketch_line(&line.geometry).ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT point-line dimension {} requires a line",
            constraint.id.0
        ))
    })?;
    let direction = [end.u - start.u, end.v - start.v];
    let length = vector2_length(direction);
    if length <= SKETCH_POINT_TOLERANCE {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT point-line dimension {} has a degenerate line",
            constraint.id.0
        )));
    }
    Ok(cross2([point.u - start.u, point.v - start.v], direction).abs() / length)
}

fn line_line_dimension(
    constraint: &SketchConstraint,
    first: &SketchEntity,
    second: &SketchEntity,
) -> Result<f64, cadmpeg_core::CodecError> {
    let (first_start, first_end) = sketch_line(&first.geometry).ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT line-line dimension {} requires two lines",
            constraint.id.0
        ))
    })?;
    let (second_start, second_end) = sketch_line(&second.geometry).ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT line-line dimension {} requires two lines",
            constraint.id.0
        ))
    })?;
    let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
    let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
    let first_length = vector2_length(first_direction);
    let second_length = vector2_length(second_direction);
    if first_length <= SKETCH_POINT_TOLERANCE || second_length <= SKETCH_POINT_TOLERANCE {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT line-line dimension {} has a degenerate line",
            constraint.id.0
        )));
    }
    if cross2(first_direction, second_direction).abs()
        > SKETCH_POINT_TOLERANCE * first_length * second_length
    {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT line-line dimension {} requires parallel solved lines",
            constraint.id.0
        )));
    }
    Ok(cross2(
        [
            second_start.u - first_start.u,
            second_start.v - first_start.v,
        ],
        first_direction,
    )
    .abs()
        / first_length)
}

fn constraint_locus_point(
    ir: &cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
    locus: &SketchLocus,
) -> Result<Point2, cadmpeg_core::CodecError> {
    let entity = sketch_constraint_entity(ir, constraint, &locus_entity(locus))?;
    sketch_entity_loci(entity)
        .into_iter()
        .find_map(|(point, candidate)| (candidate == *locus).then_some(point))
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "sketch constraint {} references unavailable locus {:?}",
                constraint.id.0, locus
            ))
        })
}

pub(super) fn binary_marker_relation(
    definition: &SketchConstraintDefinition,
) -> Option<(SketchRelationKind, &SketchEntityId, &SketchEntityId)> {
    Some(match definition {
        SketchConstraintDefinition::Parallel { first, second } => {
            (SketchRelationKind::Parallel, first, second)
        }
        SketchConstraintDefinition::Perpendicular { first, second } => {
            (SketchRelationKind::Perpendicular, first, second)
        }
        SketchConstraintDefinition::Equal { first, second } => {
            (SketchRelationKind::Equal, first, second)
        }
        SketchConstraintDefinition::Collinear { first, second } => {
            (SketchRelationKind::Collinear, first, second)
        }
        SketchConstraintDefinition::Concentric { first, second } => {
            (SketchRelationKind::Concentric, first, second)
        }
        SketchConstraintDefinition::Coradial { first, second } => {
            (SketchRelationKind::Coradial, first, second)
        }
        SketchConstraintDefinition::Tangent { first, second } => {
            (SketchRelationKind::Tangent, first, second)
        }
        _ => return None,
    })
}

fn sketch_constraint_entity<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
    entity: &SketchEntityId,
) -> Result<&'a SketchEntity, cadmpeg_core::CodecError> {
    ir.model
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id == *entity && candidate.sketch == constraint.sketch)
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "sketch constraint {} references entity {} outside sketch {}",
                constraint.id.0, entity.0, constraint.sketch.0
            ))
        })
}

fn validate_solved_binary_relation(
    constraint: &SketchConstraint,
    kind: SketchRelationKind,
    first: &SketchEntity,
    second: &SketchEntity,
) -> Result<(), cadmpeg_core::CodecError> {
    use SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    let solved = match kind {
        Parallel | Perpendicular | Collinear => {
            let (first_start, first_end) = sketch_line(&first.geometry).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch constraint {} requires two line entities",
                    constraint.id.0
                ))
            })?;
            let (second_start, second_end) = sketch_line(&second.geometry).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch constraint {} requires two line entities",
                    constraint.id.0
                ))
            })?;
            let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
            let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
            let scale = vector2_length(first_direction) * vector2_length(second_direction);
            if scale <= SKETCH_POINT_TOLERANCE {
                false
            } else if kind == Perpendicular {
                (first_direction[0] * second_direction[0]
                    + first_direction[1] * second_direction[1])
                    .abs()
                    <= SKETCH_POINT_TOLERANCE * scale
            } else {
                let directions_parallel = cross2(first_direction, second_direction).abs()
                    <= SKETCH_POINT_TOLERANCE * scale;
                kind == Parallel && directions_parallel
                    || kind == Collinear
                        && directions_parallel
                        && cross2(
                            [
                                second_start.u - first_start.u,
                                second_start.v - first_start.v,
                            ],
                            first_direction,
                        )
                        .abs()
                            <= SKETCH_POINT_TOLERANCE
                                * vector2_length(first_direction)
                                * (1.0
                                    + vector2_length([
                                        second_start.u - first_start.u,
                                        second_start.v - first_start.v,
                                    ]))
            }
        }
        Concentric => match (
            sketch_center(&first.geometry),
            sketch_center(&second.geometry),
        ) {
            (Some(first), Some(second)) => same_point2(first, second),
            _ => {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch constraint {} requires two centered entities",
                    constraint.id.0
                )));
            }
        },
        Coradial => match (
            circular_center_radius(&first.geometry),
            circular_center_radius(&second.geometry),
        ) {
            (Some((first_center, first_radius)), Some((second_center, second_radius))) => {
                same_point2(first_center, second_center)
                    && (first_radius - second_radius).abs()
                        <= SKETCH_POINT_TOLERANCE
                            * (1.0 + first_radius.abs().max(second_radius.abs()))
            }
            _ => {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch constraint {} requires two circular entities",
                    constraint.id.0
                )));
            }
        },
        Equal => equal_sketch_size(&first.geometry, &second.geometry).ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented(format!(
                "source-less SLDPRT equal constraint {} uses unsupported entity families",
                constraint.id.0
            ))
        })?,
        Tangent => solved_tangent(&first.geometry, &second.geometry).ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented(format!(
                "source-less SLDPRT tangent constraint {} uses unsupported entity families",
                constraint.id.0
            ))
        })?,
        _ => unreachable!("only generated binary relation kinds are passed"),
    };
    if !solved {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT sketch constraint {} is not satisfied by its entity geometry",
            constraint.id.0
        )));
    }
    Ok(())
}

pub(super) fn solved_tangent(first: &SketchGeometry, second: &SketchGeometry) -> Option<bool> {
    match (first, second) {
        (SketchGeometry::Line { start, end }, circular)
        | (circular, SketchGeometry::Line { start, end }) => {
            let (center, radius) = circular_center_radius(circular)?;
            let direction = [end.u - start.u, end.v - start.v];
            let length = vector2_length(direction);
            if length <= SKETCH_POINT_TOLERANCE {
                return Some(false);
            }
            let distance =
                cross2([center.u - start.u, center.v - start.v], direction).abs() / length;
            Some((distance - radius).abs() <= SKETCH_POINT_TOLERANCE * (1.0 + radius.abs()))
        }
        (first, second) => {
            let (first_center, first_radius) = circular_center_radius(first)?;
            let (second_center, second_radius) = circular_center_radius(second)?;
            let distance = vector2_length([
                second_center.u - first_center.u,
                second_center.v - first_center.v,
            ]);
            let external = first_radius + second_radius;
            let internal = (first_radius - second_radius).abs();
            let tolerance = SKETCH_POINT_TOLERANCE * (1.0 + distance.max(external).max(internal));
            Some(
                (distance - external).abs() <= tolerance
                    || (distance - internal).abs() <= tolerance,
            )
        }
    }
}

fn circular_center_radius(geometry: &SketchGeometry) -> Option<(Point2, f64)> {
    match geometry {
        SketchGeometry::Circle { center, radius } | SketchGeometry::Arc { center, radius, .. } => {
            Some((*center, radius.0))
        }
        _ => None,
    }
}

fn sketch_line(geometry: &SketchGeometry) -> Option<(Point2, Point2)> {
    match geometry {
        SketchGeometry::Line { start, end } => Some((*start, *end)),
        _ => None,
    }
}

fn sketch_center(geometry: &SketchGeometry) -> Option<Point2> {
    match geometry {
        SketchGeometry::Circle { center, .. }
        | SketchGeometry::Arc { center, .. }
        | SketchGeometry::Ellipse { center, .. } => Some(*center),
        _ => None,
    }
}

fn equal_sketch_size(first: &SketchGeometry, second: &SketchGeometry) -> Option<bool> {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= SKETCH_POINT_TOLERANCE * (1.0 + left.abs().max(right.abs()))
    };
    Some(match (first, second) {
        (
            SketchGeometry::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => close(
            vector2_length([first_end.u - first_start.u, first_end.v - first_start.v]),
            vector2_length([second_end.u - second_start.u, second_end.v - second_start.v]),
        ),
        (
            SketchGeometry::Circle { radius: first, .. }
            | SketchGeometry::Arc { radius: first, .. },
            SketchGeometry::Circle { radius: second, .. }
            | SketchGeometry::Arc { radius: second, .. },
        ) => close(first.0, second.0),
        (
            SketchGeometry::Ellipse {
                major_radius: first_major,
                minor_radius: first_minor,
                ..
            },
            SketchGeometry::Ellipse {
                major_radius: second_major,
                minor_radius: second_minor,
                ..
            },
        ) => close(first_major.0, second_major.0) && close(first_minor.0, second_minor.0),
        _ => return None,
    })
}

fn vector2_length(vector: [f64; 2]) -> f64 {
    vector[0].hypot(vector[1])
}

fn cross2(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0] * second[1] - first[1] * second[0]
}

pub(super) fn same_point2(first: Point2, second: Point2) -> bool {
    (first.u - second.u).abs() <= SKETCH_POINT_TOLERANCE
        && (first.v - second.v).abs() <= SKETCH_POINT_TOLERANCE
}

pub(super) fn arc_angle_relation_kind(angle: f64) -> Option<SketchRelationKind> {
    const TOLERANCE: f64 = 1.0e-9;
    [
        (std::f64::consts::FRAC_PI_2, SketchRelationKind::ArcAngle90),
        (std::f64::consts::PI, SketchRelationKind::ArcAngle180),
        (
            3.0 * std::f64::consts::FRAC_PI_2,
            SketchRelationKind::ArcAngle270,
        ),
    ]
    .into_iter()
    .find_map(|(expected, kind)| ((angle - expected).abs() <= TOLERANCE).then_some(kind))
}

pub(super) fn ellipse_angle_relation_kind(angle: f64) -> Option<SketchRelationKind> {
    const TOLERANCE: f64 = 1.0e-9;
    [
        (
            std::f64::consts::FRAC_PI_2,
            SketchRelationKind::EllipseAngle90,
        ),
        (std::f64::consts::PI, SketchRelationKind::EllipseAngle180),
        (
            3.0 * std::f64::consts::FRAC_PI_2,
            SketchRelationKind::EllipseAngle270,
        ),
    ]
    .into_iter()
    .find_map(|(expected, kind)| ((angle - expected).abs() <= TOLERANCE).then_some(kind))
}

fn unique_planar_sketch_owner<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    sketch: &SketchId,
) -> Result<&'a cadmpeg_ir::features::Feature, cadmpeg_core::CodecError> {
    unique_sketch_owner(ir, &sketch.0, |feature| {
        matches!(
            &feature.definition,
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(candidate),
                ..
            } if candidate == sketch
        )
    })
}

fn unique_spatial_sketch_owner<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    sketch: &SpatialSketchId,
) -> Result<&'a cadmpeg_ir::features::Feature, cadmpeg_core::CodecError> {
    unique_sketch_owner(ir, &sketch.0, |feature| {
        matches!(
            &feature.definition,
            FeatureDefinition::SpatialSketch {
                sketch: Some(candidate),
            } if candidate == sketch
        )
    })
}

fn unique_sketch_owner<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    sketch: &str,
    owns: impl Fn(&cadmpeg_ir::features::Feature) -> bool,
) -> Result<&'a cadmpeg_ir::features::Feature, cadmpeg_core::CodecError> {
    let mut owners = ir.model.features.iter().filter(|feature| owns(feature));
    let owner = owners.next().ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT sketch {sketch} has no owning feature"
        ))
    })?;
    if owners.next().is_some() {
        return Err(cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT sketch {sketch} has multiple owning features"
        )));
    }
    Ok(owner)
}

fn generated_sketch_owner_record<'a>(
    native: &'a crate::native::SldprtNative,
    owner: &cadmpeg_ir::features::Feature,
    sketch: &str,
) -> Result<&'a crate::records::Feature, cadmpeg_core::CodecError> {
    let owner_record_id = owner
        .native_ref
        .clone()
        .unwrap_or_else(|| format!("sldprt:generated:feature#{}", owner.id.0));
    native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.id == owner_record_id)
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "source-less SLDPRT sketch {sketch} has no native feature record"
            ))
        })
}

fn generated_sketch_owner_id(
    owner: &crate::records::Feature,
    sketch: &str,
) -> Result<u32, cadmpeg_core::CodecError> {
    owner
        .source_id
        .as_deref()
        .and_then(|source_id| source_id.parse::<u32>().ok())
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "source-less SLDPRT sketch {sketch} has no numeric feature source id"
            ))
        })
}

fn source_less_lanes(
    ir: &cadmpeg_ir::CadIr,
    native: &crate::native::SldprtNative,
) -> Result<Vec<FeatureInputLane>, cadmpeg_core::CodecError> {
    let mut objects = Vec::<(String, u64, Vec<u8>)>::new();
    for sketch in &ir.model.sketches {
        let configuration = sketch.configuration.clone().unwrap_or_else(|| "0".into());
        let owner = unique_planar_sketch_owner(ir, &sketch.id)?;
        let owner_record = generated_sketch_owner_record(native, owner, &sketch.id.0)?;
        let object_id = generated_sketch_owner_id(owner_record, &sketch.id.0)?;
        let mut payload = Vec::new();
        append_generated_object_name(
            &mut payload,
            if owner_record.name.is_empty() {
                sketch.name.as_deref().unwrap_or(&sketch.id.0)
            } else {
                owner_record.name.as_str()
            },
            object_id,
        )?;
        append_generated_sketch_markers(ir, sketch, &mut payload)?;
        let sketch_ir = sketch_brep(ir, sketch)?;
        let body = crate::writer::brep_body(&sketch_ir, 0.001, false)?;
        payload.extend(crate::writer::parasolid_stream_named(
            &body,
            "SCH_SW_33103_11000",
            sketch.name.as_deref().unwrap_or(&sketch.id.0),
        ));
        objects.push((configuration, owner.ordinal, payload));
    }
    for sketch in &ir.model.spatial_sketches {
        let configuration = sketch.configuration.clone().unwrap_or_else(|| "0".into());
        let owner = unique_spatial_sketch_owner(ir, &sketch.id)?;
        let owner_record = generated_sketch_owner_record(native, owner, &sketch.id.0)?;
        let object_id = generated_sketch_owner_id(owner_record, &sketch.id.0)?;
        let mut payload = Vec::new();
        append_generated_object_name(
            &mut payload,
            if owner_record.name.is_empty() {
                sketch.name.as_deref().unwrap_or(&sketch.id.0)
            } else {
                owner_record.name.as_str()
            },
            object_id,
        )?;
        let entities = ir
            .model
            .spatial_sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect::<Vec<_>>();
        if entities.is_empty() {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "source-less SLDPRT spatial sketch {} requires at least one line",
                sketch.id.0
            )));
        }
        for entity in entities {
            match entity.geometry {
                SpatialSketchGeometry::Point { position } => {
                    append_spatial_point_marker(&mut payload, position, object_id)?;
                }
                SpatialSketchGeometry::Line { start, end } => {
                    if start == end {
                        return Err(cadmpeg_core::CodecError::malformed(format_args!(
                            "source-less SLDPRT spatial sketch {} has a zero-length line",
                            sketch.id.0
                        )));
                    }
                    append_spatial_vertex(&mut payload, start);
                    append_spatial_vertex(&mut payload, end);
                }
                _ => {
                    return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                        "source-less SLDPRT spatial sketch {} supports point and line geometry only",
                        sketch.id.0
                    )));
                }
            }
        }
        objects.push((configuration, owner.ordinal, payload));
    }
    let mut lanes = assemble_source_less_lanes(objects);
    for lane in &mut lanes {
        lane.classes = class_declarations(&lane.native_payload, &lane.id);
        lane.names = object_names(&lane.native_payload, &lane.id);
        lane.scalars = named_scalars(&lane.native_payload, &lane.id, &lane.names);
        lane.relation_bindings = relation_bindings(&lane.id, &lane.classes, &lane.scalars);
        lane.references = reference_cells(&lane.scalars, &lane.classes);
        lane.sketch_entities = sketch_input_entities(&lane.native_payload, &lane.id);
    }
    bind_scalar_operands(&native.feature_histories, &mut lanes);
    Ok(lanes)
}

fn assemble_source_less_lanes(mut objects: Vec<(String, u64, Vec<u8>)>) -> Vec<FeatureInputLane> {
    objects.sort_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
    let mut lanes = Vec::new();
    for (configuration, _, payload) in objects {
        source_less_lane(&mut lanes, &configuration)
            .native_payload
            .extend(payload);
    }
    lanes
}

fn source_less_lane<'a>(
    lanes: &'a mut Vec<FeatureInputLane>,
    configuration: &str,
) -> &'a mut FeatureInputLane {
    if let Some(position) = lanes
        .iter()
        .position(|lane| lane.configuration.as_deref() == Some(configuration))
    {
        return &mut lanes[position];
    }
    lanes.push(FeatureInputLane {
        id: format!("Contents/Config-{configuration}-ResolvedFeatures"),
        configuration: Some(configuration.into()),
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    });
    lanes.last_mut().expect("lane was inserted")
}

pub(super) fn append_spatial_vertex(payload: &mut Vec<u8>, point: Point3) {
    let start = payload.len();
    payload.resize(start + 69, 0);
    payload[start..start + SPATIAL_VERTEX_PREFIX.len()].copy_from_slice(SPATIAL_VERTEX_PREFIX);
    payload[start + 43..start + 45].copy_from_slice(&[0x0e, 0x00]);
    payload[start + 45..start + 53].copy_from_slice(&point.x.to_le_bytes());
    payload[start + 53..start + 61].copy_from_slice(&point.y.to_le_bytes());
    payload[start + 61..start + 69].copy_from_slice(&point.z.to_le_bytes());
}

fn append_spatial_point_marker(
    payload: &mut Vec<u8>,
    point: Point3,
    object_id: u32,
) -> Result<(), cadmpeg_core::CodecError> {
    let native = spatial_point_native_coordinates(point)?;
    payload.extend_from_slice(&object_id.to_le_bytes());
    let start = payload.len();
    payload.resize(start + 90, 0);
    payload[start..start + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[start + 5..start + 13].fill(0xff);
    payload[start + 13..start + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[start + 23..start + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[start + 27..start + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[start + 48..start + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[start + 64..start + 66].copy_from_slice(&[0x0e, 0x00]);
    payload[start + 66..start + 74].copy_from_slice(&native[0].to_le_bytes());
    payload[start + 74..start + 82].copy_from_slice(&native[1].to_le_bytes());
    payload[start + 82..start + 90].copy_from_slice(&native[2].to_le_bytes());
    Ok(())
}

fn spatial_point_native_coordinates(point: Point3) -> Result<[f64; 3], cadmpeg_core::CodecError> {
    let native = [point.x * 0.001, point.y * 0.001, point.z * 0.001];
    if native
        .iter()
        .any(|value| *value != 0.0 && !value.is_normal())
    {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT spatial point coordinates must be zero or normal finite native f64 values"
                .into(),
        ));
    }
    Ok(native)
}

#[cfg(test)]
mod source_less_lane_tests {
    use std::collections::BTreeMap;

    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId, SketchLocus,
    };
    use cadmpeg_ir::units::Units;

    use super::*;

    fn generated_sketch() -> Sketch {
        Sketch {
            id: SketchId("sketch".into()),
            name: Some("Sketch".into()),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: None,
        }
    }

    fn generated_entity(id: &str, geometry: SketchGeometry) -> SketchEntity {
        SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("sketch".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        }
    }

    fn add_sketch_owner(ir: &mut cadmpeg_ir::CadIr, sketch: &Sketch) {
        ir.model.features.push(cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId("sketch-feature".into()),
            ordinal: 0,
            name: Some("Sketch".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch.id.clone()),
            },
            native_ref: None,
        });
    }

    #[test]
    fn objects_follow_feature_ordinals_within_each_configuration() {
        let lanes = assemble_source_less_lanes(vec![
            ("1".into(), 2, vec![2]),
            ("0".into(), 9, vec![9]),
            ("1".into(), 1, vec![1]),
        ]);

        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].configuration.as_deref(), Some("0"));
        assert_eq!(lanes[0].native_payload, [9]);
        assert_eq!(lanes[1].configuration.as_deref(), Some("1"));
        assert_eq!(lanes[1].native_payload, [1, 2]);
    }

    #[test]
    fn non_endpoint_coincidences_form_pairwise_native_relations() {
        let point = SketchLocus::Entity(SketchEntityId("point".into()));
        let center = SketchLocus::Center(SketchEntityId("circle".into()));
        let endpoint = SketchLocus::Start(SketchEntityId("line".into()));
        let definition = SketchConstraintDefinition::CoincidentLoci {
            loci: vec![point.clone(), center.clone(), endpoint.clone()],
        };

        let relations = generated_marker_relations(&definition);

        assert_eq!(relations.len(), 2);
        assert!(matches!(
            relations[0],
            GeneratedMarkerRelation::Loci(
                crate::records::SketchRelationKind::Coincident,
                first,
                second,
            ) if first == &point && second == &center
        ));
        assert!(matches!(
            relations[1],
            GeneratedMarkerRelation::Loci(
                crate::records::SketchRelationKind::Coincident,
                first,
                second,
            ) if first == &point && second == &endpoint
        ));
    }

    #[test]
    fn endpoint_only_coincidences_remain_topology_derived() {
        let definition = SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::End(SketchEntityId("first".into())),
                SketchLocus::Start(SketchEntityId("second".into())),
            ],
        };

        assert!(generated_marker_relations(&definition).is_empty());
    }

    #[test]
    fn generated_coincident_relation_uses_parseable_local_links() {
        let mut payload = Vec::new();
        append_coordinate_marker(
            &mut payload,
            crate::records::SketchInputKind::Point,
            [0.0, 0.0],
            1,
        );
        append_coordinate_marker(
            &mut payload,
            crate::records::SketchInputKind::Point,
            [0.0, 0.0],
            2,
        );
        append_reference_marker(
            &mut payload,
            crate::records::SketchRelationKind::Coincident,
            [1, 2],
            3,
        );

        assert_eq!(marker_local_links(&payload, 284), Some(([1, 2], 0)));
    }

    #[test]
    fn generated_coordinate_marker_carries_two_reverse_relations() {
        let mut payload = Vec::new();
        append_coordinate_marker(
            &mut payload,
            crate::records::SketchInputKind::Point,
            [0.0, 0.0],
            1,
        );

        append_coordinate_marker_link(&mut payload, 1, 2).expect("required invariant");
        append_coordinate_marker_link(&mut payload, 1, 3).expect("required invariant");

        assert_eq!(
            coordinate_marker_local_links(&payload, 0),
            Some((vec![2, 3], 0x8386))
        );
        assert!(append_coordinate_marker_link(&mut payload, 1, 4)
            .expect_err("expected error")
            .to_string()
            .contains("exceeds two reverse relations"));
    }

    #[test]
    fn ternary_relations_retain_their_reverse_owner() {
        let point = SketchLocus::Entity(SketchEntityId("point".into()));
        let first = SketchEntityId("first".into());
        let second = SketchEntityId("second".into());
        let axis = SketchEntityId("axis".into());

        let at_intersection_definition = SketchConstraintDefinition::AtIntersection {
            point: point.clone(),
            first: first.clone(),
            second: second.clone(),
        };
        let at_intersection = generated_marker_relations(&at_intersection_definition);
        assert!(matches!(
            at_intersection.as_slice(),
            [GeneratedMarkerRelation::AtIntersection(owner, left, right)]
                if *owner == &point && *left == &first && *right == &second
        ));

        let symmetric_definition = SketchConstraintDefinition::Symmetric {
            first: point.clone(),
            second: SketchLocus::Center(second.clone()),
            axis: axis.clone(),
        };
        let symmetric = generated_marker_relations(&symmetric_definition);
        assert!(matches!(
            symmetric.as_slice(),
            [GeneratedMarkerRelation::Symmetric(left, _, owner)]
                if *left == &point && *owner == &axis
        ));
    }

    #[test]
    fn inactive_axis_relation_retains_structure_without_requiring_solved_geometry() {
        let sketch = generated_sketch();
        let mut ir = cadmpeg_ir::CadIr::empty(Units::default());
        add_sketch_owner(&mut ir, &sketch);
        ir.model.sketch_entities = vec![
            generated_entity(
                "first",
                SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            ),
            generated_entity(
                "second",
                SketchGeometry::Point {
                    position: Point2::new(1.0, 1.0),
                },
            ),
        ];
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId("horizontal".into()),
            sketch: sketch.id,
            definition: SketchConstraintDefinition::HorizontalPoints {
                first: SketchLocus::Entity(SketchEntityId("first".into())),
                second: SketchLocus::Entity(SketchEntityId("second".into())),
            },
            name: None,
            driving: None,
            active: Some(false),
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });

        validate_source_less_constraints(&ir).expect("inactive relation remains representable");
        ir.model.sketch_constraints[0].active = None;
        assert!(validate_source_less_constraints(&ir)
            .expect_err("active relation must be solved")
            .to_string()
            .contains("not satisfied"));
    }

    #[test]
    fn generated_at_intersection_carries_point_reverse_incidence() {
        let sketch = generated_sketch();
        let mut ir = cadmpeg_ir::CadIr::empty(Units::default());
        add_sketch_owner(&mut ir, &sketch);
        ir.model.sketch_entities = vec![
            generated_entity(
                "point",
                SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            ),
            generated_entity(
                "horizontal",
                SketchGeometry::Line {
                    start: Point2::new(-1.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                },
            ),
            generated_entity(
                "vertical",
                SketchGeometry::Line {
                    start: Point2::new(0.0, -1.0),
                    end: Point2::new(0.0, 1.0),
                },
            ),
        ];
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId("intersection".into()),
            sketch: sketch.id.clone(),
            definition: SketchConstraintDefinition::AtIntersection {
                point: SketchLocus::Entity(SketchEntityId("point".into())),
                first: SketchEntityId("horizontal".into()),
                second: SketchEntityId("vertical".into()),
            },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
        let mut payload = Vec::new();

        validate_source_less_constraints(&ir).expect("required invariant");
        append_generated_sketch_markers(&ir, &sketch, &mut payload).expect("required invariant");

        assert_eq!(
            coordinate_marker_local_links(&payload, 0),
            Some((vec![6], 0x8386))
        );
        assert_eq!(marker_local_links(&payload, 710), Some(([2, 4], 0)));
    }

    #[test]
    fn generated_symmetry_carries_axis_reverse_incidence() {
        let sketch = generated_sketch();
        let mut ir = cadmpeg_ir::CadIr::empty(Units::default());
        add_sketch_owner(&mut ir, &sketch);
        ir.model.sketch_entities = vec![
            generated_entity(
                "first",
                SketchGeometry::Point {
                    position: Point2::new(-1.0, 0.0),
                },
            ),
            generated_entity(
                "second",
                SketchGeometry::Point {
                    position: Point2::new(1.0, 0.0),
                },
            ),
            generated_entity(
                "axis",
                SketchGeometry::Line {
                    start: Point2::new(0.0, -1.0),
                    end: Point2::new(0.0, 1.0),
                },
            ),
        ];
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId("symmetric".into()),
            sketch: sketch.id.clone(),
            definition: SketchConstraintDefinition::Symmetric {
                first: SketchLocus::Entity(SketchEntityId("first".into())),
                second: SketchLocus::Entity(SketchEntityId("second".into())),
                axis: SketchEntityId("axis".into()),
            },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
        let mut payload = Vec::new();

        validate_source_less_constraints(&ir).expect("required invariant");
        append_generated_sketch_markers(&ir, &sketch, &mut payload).expect("required invariant");

        assert_eq!(
            coordinate_marker_local_links(&payload, 284),
            Some((vec![5], 0x8386))
        );
        assert_eq!(marker_local_links(&payload, 568), Some(([1, 2], 0)));

        ir.model.sketch_constraints[0].definition = SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Entity(SketchEntityId("first".into())),
            second: SketchLocus::Entity(SketchEntityId("first".into())),
            axis: SketchEntityId("axis".into()),
        };
        assert!(validate_source_less_constraints(&ir)
            .expect_err("expected error")
            .to_string()
            .contains("repeats one locus"));
    }
}

#[cfg(test)]
mod write_prepare_tests;
