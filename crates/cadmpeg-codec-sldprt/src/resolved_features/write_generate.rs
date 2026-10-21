//! Generated sketch marker, relation and scalar emission.

use super::markers::marker_coordinates;
use super::relation_loci::marker_accepts_locus;
use super::selections::{operand_accepts_marker, operand_uses_compatible_ordinal};
use super::transforms::{
    locus_entity, locus_key, sketch_entity_loci, sketch_entity_locus_points,
    sketch_entity_marker_loci,
};
use super::write_prepare::{
    arc_angle_relation_kind, binary_marker_relation, ellipse_angle_relation_kind, same_point2,
};
use super::{CLASS_MARKER, NAME_MARKER, SCALAR_HEADER, SKETCH_MARKER};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{FeatureInputOperandKind, SketchInputKind, SketchRelationKind};
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId,
    SketchGeometryDefinition, SketchLocus,
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

pub(super) fn generated_marker_relations<'a>(
    ir: &cadmpeg_ir::CadIr,
    definition: &'a SketchConstraintDefinitionInput,
) -> Vec<GeneratedMarkerRelation<'a>> {
    match definition {
        SketchConstraintDefinitionInput::Horizontal { entity } => {
            vec![GeneratedMarkerRelation::Unary(
                SketchRelationKind::Horizontal,
                entity,
            )]
        }
        SketchConstraintDefinitionInput::Vertical { entity } => {
            vec![GeneratedMarkerRelation::Unary(
                SketchRelationKind::Vertical,
                entity,
            )]
        }
        SketchConstraintDefinitionInput::Fixed { entity } => vec![GeneratedMarkerRelation::Unary(
            SketchRelationKind::Fixed,
            entity,
        )],
        SketchConstraintDefinitionInput::ArcAngle { entity, angle } => ir
            .dimension_value(angle)
            .and_then(|angle| arc_angle_relation_kind(angle.get()))
            .map(|kind| vec![GeneratedMarkerRelation::Unary(kind, entity)])
            .unwrap_or_default(),
        SketchConstraintDefinitionInput::EllipseAngle { entity, angle } => ir
            .dimension_value(angle)
            .and_then(|angle| ellipse_angle_relation_kind(angle.get()))
            .map(|kind| vec![GeneratedMarkerRelation::Unary(kind, entity)])
            .unwrap_or_default(),
        SketchConstraintDefinitionInput::SameCoordinate { relation } => {
            let first = relation.first();
            let second = relation.second();
            let axis = relation.axis();
            vec![GeneratedMarkerRelation::Loci(
                match axis {
                    SketchCoordinateAxis::U => SketchRelationKind::VerticalPoints,
                    SketchCoordinateAxis::V => SketchRelationKind::HorizontalPoints,
                },
                first,
                second,
            )]
        }
        SketchConstraintDefinitionInput::Midpoint { point, entity } => {
            vec![GeneratedMarkerRelation::Midpoint(point, entity)]
        }
        SketchConstraintDefinitionInput::AtIntersection {
            point,
            first,
            second,
        } => vec![GeneratedMarkerRelation::AtIntersection(
            point, first, second,
        )],
        SketchConstraintDefinitionInput::Symmetric {
            first,
            second,
            axis,
        } => vec![GeneratedMarkerRelation::Symmetric(first, second, axis)],
        SketchConstraintDefinitionInput::CoincidentLoci { loci }
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
            .map(|(kind, first, second)| {
                vec![GeneratedMarkerRelation::Binary(
                    kind.relation(),
                    first,
                    second,
                )]
            })
            .unwrap_or_default(),
    }
}

/// A borrowed canonical value rendered only for this target scalar.
#[derive(Clone, Copy)]
enum GeneratedValue<'a> {
    Length(&'a cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::Length>),
    Angle(&'a cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::Angle>),
    Radius(&'a cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::PositiveLength>),
}

impl GeneratedValue<'_> {
    fn render(
        self,
        ir: &cadmpeg_ir::CadIr,
        ordinal: u32,
    ) -> Result<(String, f64), cadmpeg_core::CodecError> {
        let missing = || {
            cadmpeg_core::CodecError::UnsupportedContent(
                "SLDPRT sketch dimension has no compatible evaluated canonical value".into(),
            )
        };
        let (id, value) = match self {
            Self::Length(dimension) => (
                dimension.parameter(),
                ir.dimension_value(dimension).ok_or_else(missing)?.get() * 0.001,
            ),
            Self::Angle(dimension) => (
                dimension.parameter(),
                ir.dimension_value(dimension).ok_or_else(missing)?.get(),
            ),
            Self::Radius(dimension) => (
                dimension.parameter(),
                ir.dimension_value(dimension).ok_or_else(missing)?.get() * 0.001,
            ),
        };
        let name = match id {
            Some(id) => ir
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *id)
                .map(|parameter| parameter.name.clone())
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "SLDPRT sketch dimension references missing parameter {id}"
                    ))
                })?,
            None => format!("D{ordinal}"),
        };
        Ok((name, value))
    }
}

enum GeneratedDimension<'a> {
    PointPoint(&'a SketchLocus, &'a SketchLocus, GeneratedValue<'a>),
    PointLine(&'a SketchLocus, &'a SketchEntityId, GeneratedValue<'a>),
    LineLine(&'a SketchEntityId, &'a SketchEntityId, GeneratedValue<'a>),
    Horizontal(&'a SketchLocus, &'a SketchLocus, GeneratedValue<'a>),
    Vertical(&'a SketchLocus, &'a SketchLocus, GeneratedValue<'a>),
    Angle(&'a SketchEntityId, &'a SketchEntityId, GeneratedValue<'a>),
    Circle(&'a SketchEntityId, GeneratedValue<'a>),
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
        .flat_map(|constraint| generated_marker_relations(ir, constraint.definition.kind()))
        .collect::<Vec<_>>();
    let dimensions = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == sketch.id)
        .filter_map(|constraint| generated_dimension(ir, constraint.definition.kind()))
        .collect::<Result<Vec<_>, _>>()?;
    if relations.is_empty() && dimensions.is_empty() {
        return Ok(());
    }

    let mut marker_ids = HashMap::<SketchEntityId, (u16, Option<u16>)>::new();
    let marker_loci = generated_marker_layout(ir, sketch)?;
    let mut next_id = u32::try_from(marker_loci.len())
        .ok()
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("SLDPRT marker-local id space is exhausted")
        })?;
    for (locus, point, kind, local_id) in &marker_loci {
        append_coordinate_marker(
            payload,
            *kind,
            [point.u * 0.001, point.v * 0.001],
            u32::from(*local_id),
        );
        marker_ids
            .entry(locus_entity(locus).clone())
            .and_modify(|(_, second)| {
                second.get_or_insert(*local_id);
            })
            .or_insert((*local_id, None));
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
                    cadmpeg_core::CodecError::UnsupportedContent(format!(
                        "source-less SLDPRT relation on {} has no coordinate-bearing marker loci",
                        entity.as_str()
                    ))
                })?;
                let links = match unique_generated_entity_marker(ir, sketch, &marker_loci, entity) {
                    Ok(unique) => [unique, unique],
                    Err(_) => [ids.0, ids.1.unwrap_or(ids.0)],
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
                    sketch.id.as_str()
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
                        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Vertical(first, second, parameter) => (
                "sgPntPntVertDist",
                vec![
                    (
                        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DCB),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Angle(first, second, parameter) => (
                "sgAnglDim",
                vec![
                    (
                        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Circle(entity, parameter) => (
                "sgCircleDim",
                vec![(
                    FeatureInputOperandKind::Native(NativeOperandTag::TAG_83FE),
                    generated_entity_operand(
                        ir,
                        sketch,
                        &marker_loci,
                        entity,
                        FeatureInputOperandKind::Native(NativeOperandTag::TAG_83FE),
                    )?,
                )],
                parameter,
            ),
        };
        let (name, value) = parameter.render(ir, next_id)?;
        append_generated_scalar(payload, class, &name, value, next_id, &operands)?;
        next_id = next_id.checked_add(1).ok_or_else(|| {
            cadmpeg_core::CodecError::Malformed(
                "source-less SLDPRT marker-local id space is exhausted".into(),
            )
        })?;
    }
    Ok(())
}

/// Deterministic target marker identities for the current sketch geometry.
/// The result is write-local layout, never persisted as another geometry home.
pub(super) fn generated_marker_layout(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
) -> Result<Vec<(SketchLocus, Point2, SketchInputKind, u16)>, cadmpeg_core::CodecError> {
    let mut layout = Vec::new();
    for entity in ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        let Some(markers) = sketch_entity_marker_loci(entity) else {
            continue;
        };
        for (point, locus) in markers.loci() {
            let id = layout
                .len()
                .checked_add(1)
                .and_then(|id| u16::try_from(id).ok())
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "SLDPRT sketch exceeds its u16 marker-local id space",
                    )
                })?;
            layout.push((locus.clone(), *point, markers.kind(), id));
        }
    }
    Ok(layout)
}

fn generated_dimension<'a>(
    ir: &cadmpeg_ir::CadIr,
    definition: &'a SketchConstraintDefinitionInput,
) -> Option<Result<GeneratedDimension<'a>, cadmpeg_core::CodecError>> {
    let unsupported = || {
        cadmpeg_core::CodecError::UnsupportedContent(
            "source-less SLDPRT distance dimensions require two entities or point/entity loci"
                .into(),
        )
    };
    match definition {
        SketchConstraintDefinitionInput::DistanceLoci {
            first,
            second,
            parameter,
        } => Some(match (first, second) {
            (SketchLocus::Entity(first), SketchLocus::Entity(second))
                if !generated_locus_is_point(ir, first)
                    && !generated_locus_is_point(ir, second) =>
            {
                Ok(GeneratedDimension::LineLine(
                    first,
                    second,
                    GeneratedValue::Length(parameter),
                ))
            }
            (SketchLocus::Entity(line), point) if !generated_locus_is_point(ir, line) => Ok(
                GeneratedDimension::PointLine(point, line, GeneratedValue::Length(parameter)),
            ),
            (point, SketchLocus::Entity(line)) if !generated_locus_is_point(ir, line) => Ok(
                GeneratedDimension::PointLine(point, line, GeneratedValue::Length(parameter)),
            ),
            (first, second) => Ok(GeneratedDimension::PointPoint(
                first,
                second,
                GeneratedValue::Length(parameter),
            )),
        }),
        SketchConstraintDefinitionInput::Distance {
            entities,
            parameter,
        } => Some(match entities.as_slice() {
            [first, second] => Ok(GeneratedDimension::LineLine(
                first,
                second,
                GeneratedValue::Length(parameter),
            )),
            _ => Err(unsupported()),
        }),
        SketchConstraintDefinitionInput::HorizontalDistance {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Horizontal(
            first,
            second,
            GeneratedValue::Length(parameter),
        ))),
        SketchConstraintDefinitionInput::VerticalDistance {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Vertical(
            first,
            second,
            GeneratedValue::Length(parameter),
        ))),
        SketchConstraintDefinitionInput::Angle {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Angle(
            first,
            second,
            GeneratedValue::Angle(parameter),
        ))),
        SketchConstraintDefinitionInput::Radius { entity, parameter }
        | SketchConstraintDefinitionInput::Diameter { entity, parameter } => Some(Ok(
            GeneratedDimension::Circle(entity, GeneratedValue::Radius(parameter)),
        )),
        _ => None,
    }
}

pub(super) fn generated_locus_is_point(ir: &cadmpeg_ir::CadIr, entity: &SketchEntityId) -> bool {
    ir.model
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id() == entity)
        .is_some_and(|candidate| {
            matches!(
                *candidate.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
        })
}

pub(crate) fn append_generated_scalar(
    payload: &mut Vec<u8>,
    class: &str,
    name: &str,
    value: f64,
    object_id: u32,
    operands: &[(FeatureInputOperandKind, u16)],
) -> Result<(), cadmpeg_core::CodecError> {
    let class_length = u16::try_from(class.len()).map_err(|_| {
        cadmpeg_core::CodecError::Malformed(
            "SLDPRT generated parameter class name exceeds 65535 bytes".into(),
        )
    })?;
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&class_length.to_le_bytes());
    payload.extend_from_slice(class.as_bytes());
    append_scalar_fields(payload, name, value, object_id, operands)
}

/// Author a named scalar and operand cells without inventing a class family.
pub(super) fn append_scalar_fields(
    payload: &mut Vec<u8>,
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
            FeatureInputOperandKind::Native(tag) => tag.value(),
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
        .filter(|(candidate, ..)| locus_entity(candidate) == entity)
    {
        let mut candidates = ir
            .model
            .sketch_entities
            .iter()
            .filter(|candidate| candidate.sketch == sketch.id)
            .filter(|candidate| marker_accepts_locus(*kind, &candidate.geometry))
            .filter(|candidate| {
                sketch_entity_locus_points(candidate)
                    .into_iter()
                    .flatten()
                    .any(|(candidate, _)| same_point2(*point, candidate))
            })
            .map(cadmpeg_ir::SketchEntity::id);
        if candidates.next() == Some(entity) && candidates.next().is_none() {
            return Ok(*local_id);
        }
    }
    Err(cadmpeg_core::CodecError::UnsupportedContent(format!(
        "source-less SLDPRT binary relation cannot identify entity {} with one unambiguous marker locus",
        entity.as_str()
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
            cadmpeg_core::CodecError::UnsupportedContent(format!(
                "source-less SLDPRT dimension operand cannot address marker {local_id} with tag {kind:?}"
            ))
        })?;
    u16::try_from(ordinal).map_err(|_| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "source-less SLDPRT sketch {} exceeds the dimension operand space",
            sketch.id.as_str()
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
    Err(cadmpeg_core::CodecError::UnsupportedContent(format!(
        "source-less SLDPRT locus relation cannot identify {locus:?} with one unambiguous marker"
    )))
}

pub(crate) fn append_coordinate_marker(
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
        return Err(cadmpeg_core::CodecError::UnsupportedContent(format!(
            "source-less SLDPRT reverse relation cannot identify coordinate marker {owner_local_id}"
        )));
    };
    let count = usize::from(View::u16_le_at(payload, *offset + 84).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed(
            "source-less SLDPRT coordinate marker is truncated".into(),
        )
    })?);
    if count >= 2 {
        // (O-capacity): the target field cannot encode a value outside its width.
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
    let relation_count = u16::try_from(count + 1).map_err(|_| {
        cadmpeg_core::CodecError::Malformed(
            "source-less SLDPRT coordinate marker reverse relation count exceeds 65535".into(),
        )
    })?;
    payload[*offset + 84..*offset + 86].copy_from_slice(&relation_count.to_le_bytes());
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

pub(crate) fn append_generated_object_name(
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
