//! Generated sketch marker, relation and scalar emission.

use super::markers::marker_coordinates;
use super::relation_geometry::is_reference_relation_parameter;
use super::relation_loci::marker_accepts_locus;
use super::selections::{operand_accepts_marker, operand_uses_compatible_ordinal};
use super::transforms::{locus_entity, locus_key, sketch_entity_loci};
use super::write_prepare::{
    arc_angle_relation_kind, binary_marker_relation, ellipse_angle_relation_kind, same_point2,
};
use super::{CLASS_MARKER, NAME_MARKER, SCALAR_HEADER, SKETCH_MARKER};
use crate::records::{FeatureInputOperandKind, SketchInputKind, SketchRelationKind};
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraintDefinition, SketchCoordinateAxis, SketchEntityId, SketchGeometry,
    SketchLocus,
};
use std::collections::HashMap;

pub(super) enum GeneratedMarkerRelation<'a> {
    Unary(SketchRelationKind, &'a SketchEntityId),
    Binary(SketchRelationKind, &'a SketchEntityId, &'a SketchEntityId),
    Loci(SketchRelationKind, &'a SketchLocus, &'a SketchLocus),
    Midpoint(&'a SketchLocus, &'a SketchEntityId),
    AtIntersection(&'a SketchLocus, &'a SketchEntityId, &'a SketchEntityId),
    Symmetric(&'a SketchLocus, &'a SketchLocus, &'a SketchEntityId),
}

pub(super) fn generated_marker_relations(
    definition: &SketchConstraintDefinition,
) -> Vec<GeneratedMarkerRelation<'_>> {
    match definition {
        SketchConstraintDefinition::Horizontal { entity } => vec![GeneratedMarkerRelation::Unary(
            SketchRelationKind::Horizontal,
            entity,
        )],
        SketchConstraintDefinition::Vertical { entity } => vec![GeneratedMarkerRelation::Unary(
            SketchRelationKind::Vertical,
            entity,
        )],
        SketchConstraintDefinition::Fixed { entity } => vec![GeneratedMarkerRelation::Unary(
            SketchRelationKind::Fixed,
            entity,
        )],
        SketchConstraintDefinition::ArcAngle { entity, angle } => arc_angle_relation_kind(angle.0)
            .map(|kind| vec![GeneratedMarkerRelation::Unary(kind, entity)])
            .unwrap_or_default(),
        SketchConstraintDefinition::EllipseAngle { entity, angle } => {
            ellipse_angle_relation_kind(angle.0)
                .map(|kind| vec![GeneratedMarkerRelation::Unary(kind, entity)])
                .unwrap_or_default()
        }
        SketchConstraintDefinition::SameCoordinate {
            first,
            second,
            axis,
        } => {
            vec![GeneratedMarkerRelation::Loci(
                match axis {
                    SketchCoordinateAxis::U => SketchRelationKind::VerticalPoints,
                    SketchCoordinateAxis::V => SketchRelationKind::HorizontalPoints,
                },
                first,
                second,
            )]
        }
        SketchConstraintDefinition::Midpoint { point, entity } => {
            vec![GeneratedMarkerRelation::Midpoint(point, entity)]
        }
        SketchConstraintDefinition::AtIntersection {
            point,
            first,
            second,
        } => vec![GeneratedMarkerRelation::AtIntersection(
            point, first, second,
        )],
        SketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } => vec![GeneratedMarkerRelation::Symmetric(first, second, axis)],
        SketchConstraintDefinition::CoincidentLoci { loci }
            if !loci
                .iter()
                .all(|locus| matches!(locus, SketchLocus::Start(_) | SketchLocus::End(_))) =>
        {
            loci.first()
                .map(|first| {
                    loci.iter()
                        .skip(1)
                        .map(|locus| {
                            GeneratedMarkerRelation::Loci(
                                SketchRelationKind::Coincident,
                                first,
                                locus,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        definition => binary_marker_relation(definition)
            .map(|(kind, first, second)| vec![GeneratedMarkerRelation::Binary(kind, first, second)])
            .unwrap_or_default(),
    }
}

enum GeneratedDimension<'a> {
    PointPoint(
        &'a SketchLocus,
        &'a SketchLocus,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    PointLine(
        &'a SketchLocus,
        &'a SketchEntityId,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    LineLine(
        &'a SketchEntityId,
        &'a SketchEntityId,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Horizontal(
        &'a SketchLocus,
        &'a SketchLocus,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Vertical(
        &'a SketchLocus,
        &'a SketchLocus,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Angle(
        &'a SketchEntityId,
        &'a SketchEntityId,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Circle(&'a SketchEntityId, &'a cadmpeg_ir::features::ParameterId),
}

pub(super) fn append_generated_sketch_markers(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    payload: &mut Vec<u8>,
) -> Result<(), cadmpeg_core::CodecError> {
    let relations = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == sketch.id)
        .flat_map(|constraint| generated_marker_relations(&constraint.definition))
        .collect::<Vec<_>>();
    let dimensions = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == sketch.id)
        .filter_map(|constraint| generated_dimension(ir, &constraint.definition))
        .collect::<Result<Vec<_>, _>>()?;
    if relations.is_empty() && dimensions.is_empty() {
        return Ok(());
    }

    let mut marker_ids = HashMap::<SketchEntityId, Vec<u16>>::new();
    let mut marker_loci = Vec::<(SketchLocus, Point2, SketchInputKind, u16)>::new();
    let mut next_id = 1u32;
    for entity in ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        for (point, locus) in sketch_entity_loci(entity) {
            let local_id = u16::try_from(next_id).map_err(|_| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT sketch {} exceeds the marker-local id space",
                    sketch.id.0
                ))
            })?;
            append_coordinate_marker(
                payload,
                generated_marker_kind(&entity.geometry),
                [point.u * 0.001, point.v * 0.001],
                next_id,
            );
            marker_ids
                .entry(entity.id().clone())
                .or_default()
                .push(local_id);
            marker_loci.push((
                locus,
                point,
                generated_marker_kind(&entity.geometry),
                local_id,
            ));
            next_id += 1;
        }
    }
    for relation in relations {
        let reverse_owner =
            match &relation {
                GeneratedMarkerRelation::AtIntersection(point, ..) => Some(
                    unique_generated_locus_marker(ir, sketch, &marker_loci, point)?,
                ),
                GeneratedMarkerRelation::Symmetric(_, _, axis) => Some(
                    unique_generated_entity_marker(ir, sketch, &marker_loci, axis)?,
                ),
                _ => None,
            };
        let (kind, links) = match relation {
            GeneratedMarkerRelation::Unary(kind, entity) => {
                let ids = marker_ids.get(entity).ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(format!(
                        "source-less SLDPRT relation on {} has no coordinate-bearing marker loci",
                        entity.0
                    ))
                })?;
                let links = match unique_generated_entity_marker(ir, sketch, &marker_loci, entity) {
                    Ok(unique) => [unique, unique],
                    Err(_) => match ids.as_slice() {
                        [only] => [*only, *only],
                        [first, second, ..] => [*first, *second],
                        [] => unreachable!("empty marker-id vectors are never inserted"),
                    },
                };
                (kind, links)
            }
            GeneratedMarkerRelation::Binary(kind, first, second) => (
                kind,
                [
                    unique_generated_entity_marker(ir, sketch, &marker_loci, first)?,
                    unique_generated_entity_marker(ir, sketch, &marker_loci, second)?,
                ],
            ),
            GeneratedMarkerRelation::Loci(kind, first, second) => (
                kind,
                [
                    unique_generated_locus_marker(ir, sketch, &marker_loci, first)?,
                    unique_generated_locus_marker(ir, sketch, &marker_loci, second)?,
                ],
            ),
            GeneratedMarkerRelation::Midpoint(point, entity) => (
                SketchRelationKind::Midpoint,
                [
                    unique_generated_locus_marker(ir, sketch, &marker_loci, point)?,
                    unique_generated_entity_marker(ir, sketch, &marker_loci, entity)?,
                ],
            ),
            GeneratedMarkerRelation::AtIntersection(_, first, second) => (
                SketchRelationKind::AtIntersection,
                [
                    unique_generated_entity_marker(ir, sketch, &marker_loci, first)?,
                    unique_generated_entity_marker(ir, sketch, &marker_loci, second)?,
                ],
            ),
            GeneratedMarkerRelation::Symmetric(first, second, _) => (
                SketchRelationKind::Symmetric,
                [
                    unique_generated_locus_marker(ir, sketch, &marker_loci, first)?,
                    unique_generated_locus_marker(ir, sketch, &marker_loci, second)?,
                ],
            ),
        };
        if let Some(owner) = reverse_owner {
            let relation_id = u16::try_from(next_id).map_err(|_| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT sketch {} exceeds the marker-local id space",
                    sketch.id.0
                ))
            })?;
            append_coordinate_marker_link(payload, owner, relation_id)?;
        }
        append_reference_marker(payload, kind, links, next_id);
        next_id = next_id.checked_add(1).ok_or_else(|| {
            cadmpeg_core::CodecError::Malformed(
                "source-less SLDPRT marker-local id space is exhausted".into(),
            )
        })?;
    }
    for dimension in dimensions {
        let (class, operands, parameter) = match dimension {
            GeneratedDimension::PointPoint(first, second, parameter) => (
                "sgPntPntDist",
                vec![
                    (
                        FeatureInputOperandKind::D6,
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::D6,
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::D6,
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::D6,
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::PointLine(point, line, parameter) => (
                "sgPntLineDist",
                vec![
                    (
                        FeatureInputOperandKind::D6,
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            point,
                            FeatureInputOperandKind::D6,
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::E1,
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            line,
                            FeatureInputOperandKind::E1,
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::LineLine(first, second, parameter) => (
                "sgLLDist",
                vec![
                    (
                        FeatureInputOperandKind::E1,
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::E1,
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::E1,
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::E1,
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Horizontal(first, second, parameter) => (
                "sgPntPntHorDist",
                vec![
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Vertical(first, second, parameter) => (
                "sgPntPntVertDist",
                vec![
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Angle(first, second, parameter) => (
                "sgAnglDim",
                vec![
                    (
                        FeatureInputOperandKind::Native(0x8dda),
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(0x8dda),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(0x8dda),
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(0x8dda),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Circle(entity, parameter) => (
                "sgCircleDim",
                vec![(
                    FeatureInputOperandKind::Native(0x83fe),
                    generated_entity_operand(
                        ir,
                        sketch,
                        &marker_loci,
                        entity,
                        FeatureInputOperandKind::Native(0x83fe),
                    )?,
                )],
                parameter,
            ),
        };
        let parameter = ir
            .model
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT dimension references missing parameter {}",
                    parameter.0
                ))
            })?;
        let value = match (&parameter.value, class) {
            (Some(cadmpeg_ir::features::ParameterValue::Length(_)), "sgAnglDim") => {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT angular dimension {} has a length value",
                    parameter.id.0
                )));
            }
            (Some(cadmpeg_ir::features::ParameterValue::Angle(value)), "sgAnglDim") => value.0,
            (Some(cadmpeg_ir::features::ParameterValue::Length(value)), _) => value.0 * 0.001,
            _ => {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "source-less SLDPRT dimension parameter {} has no compatible evaluated value",
                    parameter.id.0
                )));
            }
        };
        append_generated_scalar(payload, class, &parameter.name, value, next_id, &operands)?;
        next_id = next_id.checked_add(1).ok_or_else(|| {
            cadmpeg_core::CodecError::Malformed(
                "source-less SLDPRT marker-local id space is exhausted".into(),
            )
        })?;
    }
    Ok(())
}

fn generated_dimension<'a>(
    ir: &cadmpeg_ir::CadIr,
    definition: &'a SketchConstraintDefinition,
) -> Option<Result<GeneratedDimension<'a>, cadmpeg_core::CodecError>> {
    let parameter_id = match definition {
        SketchConstraintDefinition::DistanceLoci { parameter, .. }
        | SketchConstraintDefinition::Distance { parameter, .. }
        | SketchConstraintDefinition::HorizontalDistance { parameter, .. }
        | SketchConstraintDefinition::VerticalDistance { parameter, .. }
        | SketchConstraintDefinition::Angle { parameter, .. }
        | SketchConstraintDefinition::Radius { parameter, .. }
        | SketchConstraintDefinition::Diameter { parameter, .. } => Some(parameter),
        _ => None,
    }?;
    if let Some(parameter) = ir
        .model
        .parameters
        .iter()
        .find(|candidate| candidate.id == *parameter_id)
        .filter(|parameter| is_reference_relation_parameter(parameter))
    {
        return Some(Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT display-only relation parameter {} has no native scalar encoding",
            parameter.id.0
        ))));
    }
    let unsupported = || {
        cadmpeg_core::CodecError::NotImplemented(
            "source-less SLDPRT distance dimensions require two entities or point/entity loci"
                .into(),
        )
    };
    match definition {
        SketchConstraintDefinition::DistanceLoci {
            first,
            second,
            parameter,
        } => Some(match (first, second) {
            (SketchLocus::Entity(first), SketchLocus::Entity(second))
                if !generated_locus_is_point(ir, first)
                    && !generated_locus_is_point(ir, second) =>
            {
                Ok(GeneratedDimension::LineLine(first, second, parameter))
            }
            (SketchLocus::Entity(line), point) if !generated_locus_is_point(ir, line) => {
                Ok(GeneratedDimension::PointLine(point, line, parameter))
            }
            (point, SketchLocus::Entity(line)) if !generated_locus_is_point(ir, line) => {
                Ok(GeneratedDimension::PointLine(point, line, parameter))
            }
            (first, second) => Ok(GeneratedDimension::PointPoint(first, second, parameter)),
        }),
        SketchConstraintDefinition::Distance {
            entities,
            parameter,
        } => Some(match entities.as_slice() {
            [first, second] => Ok(GeneratedDimension::LineLine(first, second, parameter)),
            _ => Err(unsupported()),
        }),
        SketchConstraintDefinition::HorizontalDistance {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Horizontal(first, second, parameter))),
        SketchConstraintDefinition::VerticalDistance {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Vertical(first, second, parameter))),
        SketchConstraintDefinition::Angle {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Angle(first, second, parameter))),
        SketchConstraintDefinition::Radius { entity, parameter }
        | SketchConstraintDefinition::Diameter { entity, parameter } => {
            Some(Ok(GeneratedDimension::Circle(entity, parameter)))
        }
        _ => None,
    }
}

pub(super) fn generated_locus_is_point(ir: &cadmpeg_ir::CadIr, entity: &SketchEntityId) -> bool {
    ir.model
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id() == entity)
        .is_some_and(|candidate| matches!(candidate.geometry, SketchGeometry::Point { .. }))
}

fn append_generated_scalar(
    payload: &mut Vec<u8>,
    class: &str,
    name: &str,
    value: f64,
    object_id: u32,
    operands: &[(FeatureInputOperandKind, u16)],
) -> Result<(), cadmpeg_core::CodecError> {
    let units = name.encode_utf16().collect::<Vec<_>>();
    let length = u8::try_from(units.len()).map_err(|_| {
        cadmpeg_core::CodecError::Malformed(
            "SLDPRT generated parameter name exceeds 255 UTF-16 code units".into(),
        )
    })?;
    if length == 0 || length > 128 {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT generated parameter name must contain 1 to 128 UTF-16 code units".into(),
        ));
    }
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
    payload.extend_from_slice(class.as_bytes());
    payload.extend_from_slice(NAME_MARKER);
    payload.push(length);
    for unit in units {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&value.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 35 + operands.len() * 12, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&object_id.to_le_bytes());
    payload[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
    for (index, (kind, entity)) in operands.iter().enumerate() {
        let offset = trailer + 35 + index * 12;
        let tag = match kind {
            FeatureInputOperandKind::D6 => 0x80d6,
            FeatureInputOperandKind::E1 => 0x80e1,
            FeatureInputOperandKind::Native(tag) => *tag,
        };
        payload[offset..offset + 2].copy_from_slice(&tag.to_le_bytes());
        payload[offset + 2..offset + 4].copy_from_slice(&entity.to_le_bytes());
        payload[offset + 4..offset + 8].fill(0xff);
    }
    Ok(())
}

fn unique_generated_entity_marker(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    entity: &SketchEntityId,
) -> Result<u16, cadmpeg_core::CodecError> {
    for (_, point, kind, local_id) in markers
        .iter()
        .filter(|(candidate, ..)| locus_entity(candidate) == *entity)
    {
        let mut candidates = ir
            .model
            .sketch_entities
            .iter()
            .filter(|candidate| candidate.sketch == sketch.id)
            .filter(|candidate| marker_accepts_locus(*kind, &candidate.geometry))
            .filter(|candidate| {
                sketch_entity_loci(candidate)
                    .iter()
                    .any(|(candidate, _)| same_point2(*point, *candidate))
            })
            .map(|candidate| candidate.id());
        if candidates.next() == Some(entity) && candidates.next().is_none() {
            return Ok(*local_id);
        }
    }
    Err(cadmpeg_core::CodecError::NotImplemented(format!(
        "source-less SLDPRT binary relation cannot identify entity {} with one unambiguous marker locus",
        entity.0
    )))
}

fn generated_entity_operand(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    entity: &SketchEntityId,
    kind: FeatureInputOperandKind,
) -> Result<u16, cadmpeg_core::CodecError> {
    let local_id = unique_generated_entity_marker(ir, sketch, markers, entity)?;
    generated_operand_address(markers, local_id, kind, sketch)
}

fn generated_locus_operand(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    locus: &SketchLocus,
    kind: FeatureInputOperandKind,
) -> Result<u16, cadmpeg_core::CodecError> {
    let local_id = unique_generated_locus_marker(ir, sketch, markers, locus)?;
    generated_operand_address(markers, local_id, kind, sketch)
}

fn generated_operand_address(
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    local_id: u16,
    kind: FeatureInputOperandKind,
    sketch: &Sketch,
) -> Result<u16, cadmpeg_core::CodecError> {
    if !operand_uses_compatible_ordinal(kind) {
        return Ok(local_id);
    }
    let ordinal = markers
        .iter()
        .filter(|(_, _, marker_kind, _)| operand_accepts_marker(kind, *marker_kind))
        .position(|(_, _, _, candidate)| *candidate == local_id)
        .ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented(format!(
                "source-less SLDPRT dimension operand cannot address marker {local_id} with tag {kind:?}"
            ))
        })?;
    u16::try_from(ordinal).map_err(|_| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT sketch {} exceeds the dimension operand space",
            sketch.id.0
        ))
    })
}

fn unique_generated_locus_marker(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    locus: &SketchLocus,
) -> Result<u16, cadmpeg_core::CodecError> {
    for (_, point, kind, local_id) in markers.iter().filter(|(candidate, ..)| candidate == locus) {
        let mut candidates = ir
            .model
            .sketch_entities
            .iter()
            .filter(|candidate| candidate.sketch == sketch.id)
            .filter(|candidate| marker_accepts_locus(*kind, &candidate.geometry))
            .flat_map(sketch_entity_loci)
            .filter_map(|(candidate_point, candidate_locus)| {
                same_point2(*point, candidate_point).then_some(candidate_locus)
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
        candidates.dedup();
        if candidates.as_slice() == [locus.clone()] {
            return Ok(*local_id);
        }
    }
    Err(cadmpeg_core::CodecError::NotImplemented(format!(
        "source-less SLDPRT locus relation cannot identify {locus:?} with one unambiguous marker"
    )))
}

fn generated_marker_kind(geometry: &SketchGeometry) -> SketchInputKind {
    match geometry {
        SketchGeometry::Point { .. } => SketchInputKind::Point,
        SketchGeometry::Arc { .. } => SketchInputKind::Arc,
        SketchGeometry::Line { .. }
        | SketchGeometry::ReferenceLine { .. }
        | SketchGeometry::Circle { .. }
        | SketchGeometry::Ellipse { .. }
        | SketchGeometry::Hyperbola { .. }
        | SketchGeometry::Parabola { .. }
        | SketchGeometry::Nurbs { .. }
        | SketchGeometry::ExternalReference { .. }
        | SketchGeometry::Native { .. } => SketchInputKind::LineOrCircle,
        SketchGeometry::Text { .. } => unreachable!("sketch text has no marker loci"),
    }
}

pub(super) fn append_coordinate_marker(
    payload: &mut Vec<u8>,
    kind: SketchInputKind,
    coordinates_m: [f64; 2],
    local_id: u32,
) {
    let start = payload.len();
    payload.resize(start + 142, 0);
    payload[start..start + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[start + 5..start + 13].fill(0xff);
    payload[start + 13..start + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[start + 17..start + 21].copy_from_slice(&kind.native_code().to_le_bytes());
    payload[start + 23..start + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[start + 48..start + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[start + 64..start + 66].copy_from_slice(&[0x1e, 0x00]);
    payload[start + 66..start + 74].copy_from_slice(&coordinates_m[0].to_le_bytes());
    payload[start + 74..start + 82].copy_from_slice(&coordinates_m[1].to_le_bytes());
    payload[start + 138..start + 142].copy_from_slice(&local_id.to_le_bytes());
}

pub(super) fn append_coordinate_marker_link(
    payload: &mut [u8],
    owner_local_id: u16,
    relation_local_id: u16,
) -> Result<(), cadmpeg_core::CodecError> {
    const SELECTOR: u16 = 0x8386;
    let offsets = payload
        .windows(SKETCH_MARKER.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == SKETCH_MARKER).then_some(offset))
        .filter(|offset| marker_coordinates(payload, *offset).is_some())
        .filter(|offset| View::u32_le_at(payload, *offset + 138) == Some(u32::from(owner_local_id)))
        .collect::<Vec<_>>();
    let [offset] = offsets.as_slice() else {
        return Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT reverse relation cannot identify coordinate marker {owner_local_id}"
        )));
    };
    let count = usize::from(View::u16_le_at(payload, *offset + 84).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed(
            "source-less SLDPRT coordinate marker is truncated".into(),
        )
    })?);
    if count >= 2 {
        return Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT coordinate marker {owner_local_id} exceeds two reverse relations"
        )));
    }
    if count > 0 && payload.get(*offset + 86..*offset + 88) != Some(&SELECTOR.to_le_bytes()) {
        return Err(cadmpeg_core::CodecError::Malformed(
            "source-less SLDPRT coordinate marker changed reverse-relation selector".into(),
        ));
    }
    let cell = offset + 86 + count * 12;
    let end = cell + 18;
    let bytes = payload.get_mut(cell..end).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed(
            "source-less SLDPRT coordinate marker reverse relation is truncated".into(),
        )
    })?;
    bytes.fill(0);
    bytes[0..2].copy_from_slice(&SELECTOR.to_le_bytes());
    bytes[2..4].copy_from_slice(&relation_local_id.to_le_bytes());
    bytes[4..8].fill(0xff);
    bytes[14..18].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[*offset + 84..*offset + 86].copy_from_slice(&((count + 1) as u16).to_le_bytes());
    Ok(())
}

pub(super) fn append_reference_marker(
    payload: &mut Vec<u8>,
    kind: SketchRelationKind,
    links: [u16; 2],
    local_id: u32,
) {
    let start = payload.len();
    payload.resize(start + 92, 0);
    payload[start..start + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[start + 5..start + 13].fill(0xff);
    payload[start + 13..start + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[start + 17..start + 21].copy_from_slice(&kind.native_code().to_le_bytes());
    payload[start + 48..start + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[start + 64..start + 66].copy_from_slice(&links[0].to_le_bytes());
    payload[start + 66..start + 68].copy_from_slice(&links[1].to_le_bytes());
    payload[start + 72..start + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[start + 88..start + 92].copy_from_slice(&local_id.to_le_bytes());
}

pub(super) fn append_generated_object_name(
    payload: &mut Vec<u8>,
    name: &str,
    object_id: u32,
) -> Result<(), cadmpeg_core::CodecError> {
    let units = name.encode_utf16().collect::<Vec<_>>();
    let length = u8::try_from(units.len()).map_err(|_| {
        cadmpeg_core::CodecError::Malformed(
            "SLDPRT generated feature name exceeds 255 UTF-16 code units".into(),
        )
    })?;
    if length == 0 || length > 128 {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT generated feature name must contain 1 to 128 UTF-16 code units".into(),
        ));
    }
    payload.extend_from_slice(NAME_MARKER);
    payload.push(length);
    for unit in units {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&object_id.to_le_bytes());
    Ok(())
}
