// SPDX-License-Identifier: Apache-2.0
//! Native constraint codes with neutral operands and current parameter values.
use super::super::super::xml;
use cadmpeg_ir::{
    document::CadIr,
    features::ParameterValue,
    sketches::{
        SketchAxis, SketchConstraint, SketchConstraintDefinitionInput as C, SketchEntityId,
        SketchInternalAlignment as A, SketchLocus,
    },
};
use std::collections::HashMap;
pub(super) type Indices = HashMap<String, (i64, u8)>;
fn entity(id: &SketchEntityId, indices: &Indices) -> Option<(i64, u8)> {
    indices.get(id.as_str()).copied()
}
fn locus(value: &SketchLocus, indices: &Indices) -> Option<(i64, u8)> {
    let (id, pos) = match value {
        SketchLocus::Entity(id) => (id, 0),
        SketchLocus::Start(id) => (id, 1),
        SketchLocus::End(id) => (id, 2),
        SketchLocus::Center(id) => (id, 3),
    };
    let (index, special) = entity(id, indices)?;
    Some((index, if special == 0 { pos } else { special }))
}
fn scalar<Q: cadmpeg_ir::features::DimensionValue>(
    ir: &CadIr,
    dimension: &cadmpeg_ir::features::Dimension<Q>,
) -> Option<f64> {
    match ir.dimension_value(dimension)?.to_parameter_value()? {
        ParameterValue::Length(value) => Some(value.get()),
        ParameterValue::Angle(value) => Some(value.get()),
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => cadmpeg_core::convert::f64_from_i64(value),
        _ => None,
    }
}
pub(super) fn row(ir: &CadIr, constraint: &SketchConstraint, indices: &Indices) -> Option<String> {
    let mut extra: Vec<(&str, String)> = Vec::new();
    let mut dimension_value = None;
    let (kind, operands) = match constraint.definition.kind() {
        C::Disabled {} => (0, vec![]),
        C::Coincident { entities } if entities.len() == 2 => (
            1,
            entities
                .iter()
                .map(|id| entity(id, indices))
                .collect::<Option<Vec<_>>>()?,
        ),
        C::CoincidentLoci { loci } if loci.len() == 2 => (
            1,
            loci.iter()
                .map(|id| locus(id, indices))
                .collect::<Option<Vec<_>>>()?,
        ),
        C::Midpoint { point, entity: id } => {
            let (index, _) = entity(id, indices)?;
            (1, vec![locus(point, indices)?, (index, 3)])
        }
        C::Horizontal { entity: id } => (2, vec![entity(id, indices)?]),
        C::Vertical { entity: id } => (3, vec![entity(id, indices)?]),
        C::Parallel { first, second } => {
            (4, vec![entity(first, indices)?, entity(second, indices)?])
        }
        C::Tangent { first, second } => {
            (5, vec![entity(first, indices)?, entity(second, indices)?])
        }
        C::TangentLoci { first, second } => {
            (5, vec![locus(first, indices)?, locus(second, indices)?])
        }
        C::Distance {
            entities,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (
                6,
                entities
                    .iter()
                    .map(|id| entity(id, indices))
                    .collect::<Option<Vec<_>>>()?,
            )
        }
        C::DistanceLoci {
            first,
            second,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (6, vec![locus(first, indices)?, locus(second, indices)?])
        }
        C::DistanceLociValue {
            first,
            second,
            distance,
        } => {
            dimension_value = Some(scalar(ir, distance)?);
            (6, vec![locus(first, indices)?, locus(second, indices)?])
        }
        C::HorizontalDistance {
            first,
            second,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (7, vec![locus(first, indices)?, locus(second, indices)?])
        }
        C::VerticalDistance {
            first,
            second,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (8, vec![locus(first, indices)?, locus(second, indices)?])
        }
        C::Angle {
            first,
            second,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (9, vec![entity(first, indices)?, entity(second, indices)?])
        }
        C::AngleToAxis {
            entity: id,
            axis,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (
                9,
                vec![
                    entity(id, indices)?,
                    (
                        match axis {
                            SketchAxis::Horizontal => -1,
                            SketchAxis::Vertical => -2,
                        },
                        0,
                    ),
                ],
            )
        }
        C::Perpendicular { first, second } => {
            (10, vec![entity(first, indices)?, entity(second, indices)?])
        }
        C::Radius {
            entity: id,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (11, vec![entity(id, indices)?])
        }
        C::Equal { first, second } => (12, vec![entity(first, indices)?, entity(second, indices)?]),
        C::PointOnObject { point, entity: id } => {
            (13, vec![locus(point, indices)?, entity(id, indices)?])
        }
        C::Symmetric {
            first,
            second,
            axis,
        } => (
            14,
            vec![
                locus(first, indices)?,
                locus(second, indices)?,
                entity(axis, indices)?,
            ],
        ),
        C::InternalAlignment {
            helper,
            parent,
            alignment,
        } => {
            let (code, index) = match alignment {
                A::EllipseMajorDiameter => (1, None),
                A::EllipseMinorDiameter => (2, None),
                A::EllipseFocus1 => (3, None),
                A::EllipseFocus2 => (4, None),
                A::HyperbolaMajor => (5, None),
                A::HyperbolaMinor => (6, None),
                A::HyperbolaFocus => (7, None),
                A::ParabolaFocus => (8, None),
                A::BsplineControlPoint(i) => (9, Some(*i)),
                A::BsplineKnotPoint(i) => (10, Some(*i)),
                A::ParabolaFocalAxis => (11, None),
            };
            extra.push(("InternalAlignmentType", code.to_string()));
            if let Some(index) = index {
                extra.push(("InternalAlignmentIndex", index.to_string()));
            }
            (15, vec![entity(helper, indices)?, entity(parent, indices)?])
        }
        C::SnellsLaw {
            incident,
            refracted,
            interface,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (
                16,
                vec![
                    locus(incident, indices)?,
                    locus(refracted, indices)?,
                    entity(interface, indices)?,
                ],
            )
        }
        C::Fixed { entity: id } => (17, vec![entity(id, indices)?]),
        C::Diameter {
            entity: id,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (18, vec![entity(id, indices)?])
        }
        C::Weight {
            entity: id,
            parameter: p,
        } => {
            dimension_value = Some(scalar(ir, p)?);
            (19, vec![entity(id, indices)?])
        }
        C::Group { elements } => (
            20,
            elements
                .iter()
                .map(|id| locus(id, indices))
                .collect::<Option<Vec<_>>>()?,
        ),
        C::Text {
            elements,
            text,
            font,
            is_text_height,
        } => {
            extra.push((
                "MetaData",
                serde_json::json!({"text":text,"font":font,"isTextHeight":is_text_height})
                    .to_string(),
            ));
            (
                21,
                elements
                    .iter()
                    .map(|id| locus(id, indices))
                    .collect::<Option<Vec<_>>>()?,
            )
        }
        _ => return None,
    };
    if let Some(value) = dimension_value {
        extra.retain(|(name, _)| *name != "Value");
        extra.push(("Value", value.to_string()));
    }
    let mut attributes = vec![("Type", kind.to_string())];
    if operands.len() <= 3 {
        for (index, (entity, position)) in operands.iter().enumerate() {
            let (key, pos) = [
                ("First", "FirstPos"),
                ("Second", "SecondPos"),
                ("Third", "ThirdPos"),
            ][index];
            attributes.push((key, entity.to_string()));
            attributes.push((pos, position.to_string()));
        }
    } else {
        attributes.push((
            "ElementIds",
            operands
                .iter()
                .map(|(i, _)| i.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ));
        attributes.push((
            "ElementPositions",
            operands
                .iter()
                .map(|(_, p)| p.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    attributes.extend(extra);
    common(ir, constraint, &mut attributes);
    Some(xml::element("Constrain", &attributes))
}
pub(super) fn common(
    ir: &CadIr,
    constraint: &SketchConstraint,
    attributes: &mut Vec<(&'static str, String)>,
) {
    attributes.extend([
        (
            "IsDriving",
            u8::from(
                constraint
                    .definition
                    .kind()
                    .dimension_parameters()
                    .ok()
                    .and_then(|ids| {
                        ids.into_iter().find_map(|id| {
                            ir.model
                                .parameters
                                .iter()
                                .find(|parameter| parameter.id == id)
                                .map(|parameter| parameter.driving)
                        })
                    })
                    .unwrap_or_else(|| constraint.driving.unwrap_or(true)),
            )
            .to_string(),
        ),
        (
            "IsActive",
            u8::from(constraint.active.unwrap_or(true)).to_string(),
        ),
    ]);
    for (key, value) in [
        ("IsInVirtualSpace", constraint.virtual_space),
        ("IsVisible", constraint.visible),
    ] {
        if let Some(value) = value {
            attributes.push((key, u8::from(value).to_string()));
        }
    }
    attributes.push(("Name", constraint.name.clone().unwrap_or_default()));
    if let Some(orientation) = constraint.orientation {
        attributes.push(("Orientation", orientation.to_string()));
    }
    for (key, value) in [
        ("LabelDistance", constraint.label_distance),
        ("LabelPosition", constraint.label_position),
    ] {
        if let Some(value) = value {
            attributes.push((key, value.get().to_string()));
        }
    }
    if !attributes.iter().any(|(key, _)| *key == "MetaData") {
        if let Some(metadata) = &constraint.metadata {
            attributes.push(("MetaData", metadata.clone()));
        }
    }
}
