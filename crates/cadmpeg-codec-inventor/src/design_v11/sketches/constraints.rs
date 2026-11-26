// SPDX-License-Identifier: Apache-2.0
//! Established sketch relations in the v11 operand vocabulary.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::ScalarDimension;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::sketch::{
    DistanceAxis, Operand, Planar, RadialMeasure, SketchConstraint, SketchRelation,
};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::sketches::{
    SketchConstraint as LegacyConstraint, SketchConstraintDefinitionInput as Kind,
    SketchCoordinateAxis, SketchEntityId, SketchLocus,
};

fn entity(id: &SketchEntityId) -> Operand<Planar> {
    Operand::Entity(id.clone())
}

fn locus(value: &SketchLocus) -> Option<Operand<Planar>> {
    match value {
        SketchLocus::Entity(id) => Some(entity(id)),
        _ => None,
    }
}

pub(super) fn project(
    source: &LegacyConstraint,
    kinds: &BTreeMap<cadmpeg_ir::features::ParameterId, cadmpeg_ir::design::units::ValueKind>,
) -> Result<SketchConstraint<Planar>, cadmpeg_core::CodecError> {
    let mut native_options = BTreeMap::new();
    if let Some(value) = source.orientation {
        native_options.insert(
            cadmpeg_ir::codec_name!("inventor:orientation"),
            NativeOption::Integer(Dimension::Literal(i64::from(value))),
        );
    }
    if let Kind::Native {
        native_state,
        native_flags,
        native_properties,
        operands,
        ..
    } = source.definition.kind()
    {
        for (name, value) in [
            (cadmpeg_ir::codec_name!("inventor:state"), native_state),
            (cadmpeg_ir::codec_name!("inventor:flags"), native_flags),
        ] {
            if let Some(value) = value {
                let text = cadmpeg_core::text::NonBlankString::new(value.to_string()).ok_or_else(
                    || cadmpeg_core::CodecError::malformed("Inventor integer text is blank"),
                )?;
                native_options.insert(name, NativeOption::Text(Dimension::Literal(text)));
            }
        }
        for (name, text) in [
            (
                cadmpeg_ir::codec_name!("inventor:properties"),
                cadmpeg_core::serde_erase::json::to_string(native_properties),
            ),
            (
                cadmpeg_ir::codec_name!("inventor:operands"),
                cadmpeg_core::serde_erase::json::to_string(operands),
            ),
        ] {
            let text =
                text.map_err(|error| cadmpeg_core::CodecError::malformed(error.to_string()))?;
            let text = cadmpeg_core::text::NonBlankString::new(text).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Inventor native relation text is blank")
            })?;
            native_options.insert(name, NativeOption::Text(Dimension::Literal(text)));
        }
    }
    if relation(source.definition.kind(), kinds).is_none() {
        let text = cadmpeg_ir::hash::finite_json::to_canonical_json_string(&source.definition)
            .map_err(|error| cadmpeg_core::CodecError::malformed(error.to_string()))?;
        native_options.insert(
            cadmpeg_ir::codec_name!("inventor:relation"),
            NativeOption::Text(Dimension::Literal(
                cadmpeg_core::text::NonBlankString::new(text).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Inventor relation text is blank")
                })?,
            )),
        );
    }
    Ok(SketchConstraint {
        id: source.id.clone(),
        sketch: source.sketch.clone(),
        relation: relation(source.definition.kind(), kinds).unwrap_or_else(|| {
            SketchRelation::Native {
                kind: cadmpeg_core::nonblank_literal!("inventor:sketch_constraint"),
                operands: Vec::new(),
                values: Vec::new(),
            }
        }),
        name: source.name.clone(),
        active: source.active.map(Dimension::Literal),
        native_options,
        source_settings: BTreeMap::default(),
        native_ref: source.native_ref.clone(),
    })
}

pub(super) fn loss(
    source: &LegacyConstraint,
    kinds: &BTreeMap<cadmpeg_ir::features::ParameterId, cadmpeg_ir::design::units::ValueKind>,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    relation(source.definition.kind(), kinds)
        .is_none()
        .then(|| {
            crate::loss::InventorLossCode::SketchDesignConstraintNative.note(format!(
                "Sketch constraint {} retains its relation operands in native options.",
                source.id
            ))
        })
}

fn relation(
    kind: &Kind,
    kinds: &BTreeMap<cadmpeg_ir::features::ParameterId, cadmpeg_ir::design::units::ValueKind>,
) -> Option<SketchRelation<Planar>> {
    Some(match kind {
        Kind::Coincident { entities } if entities.len() == 2 => SketchRelation::Coincident {
            a: entity(&entities[0]),
            b: entity(&entities[1]),
        },
        Kind::CoincidentLoci { loci } if loci.len() == 2 => SketchRelation::Coincident {
            a: locus(&loci[0])?,
            b: locus(&loci[1])?,
        },
        Kind::PointOnObject { point, entity: id } => SketchRelation::Coincident {
            a: locus(point)?,
            b: entity(id),
        },
        Kind::Concentric { first, second } => SketchRelation::Concentric {
            a: entity(first),
            b: entity(second),
        },
        Kind::Collinear { first, second } => SketchRelation::Collinear {
            a: entity(first),
            b: entity(second),
        },
        Kind::Parallel { first, second } => SketchRelation::Parallel {
            a: entity(first),
            b: entity(second),
        },
        Kind::Perpendicular { first, second } => SketchRelation::Perpendicular {
            a: entity(first),
            b: entity(second),
        },
        Kind::Tangent { first, second } => SketchRelation::Tangent {
            a: entity(first),
            b: entity(second),
            contact: None,
        },
        Kind::Horizontal { entity: id } => SketchRelation::Horizontal {
            a: entity(id),
            b: None,
        },
        Kind::Vertical { entity: id } => SketchRelation::Vertical {
            a: entity(id),
            b: None,
        },
        Kind::Fixed { entity: id } => SketchRelation::Fixed { of: entity(id) },
        Kind::Equal { first, second } => SketchRelation::Equal {
            members: vec![entity(first), entity(second)],
        },
        Kind::Midpoint {
            point: SketchLocus::Entity(point),
            entity: id,
        } => SketchRelation::Midpoint {
            point: point.clone(),
            of: entity(id),
        },
        Kind::SameCoordinate { relation } => match relation.axis() {
            SketchCoordinateAxis::U => SketchRelation::Vertical {
                a: locus(relation.first())?,
                b: Some(locus(relation.second())?),
            },
            SketchCoordinateAxis::V => SketchRelation::Horizontal {
                a: locus(relation.first())?,
                b: Some(locus(relation.second())?),
            },
        },
        Kind::Symmetric {
            first,
            second,
            axis,
        } => SketchRelation::Symmetric {
            a: locus(first)?,
            b: locus(second)?,
            about: entity(axis),
        },
        Kind::EqualDistance { first, second } => SketchRelation::EqualDistance {
            first: [locus(&first.first)?, locus(&first.second)?],
            second: [locus(&second.first)?, locus(&second.second)?],
        },
        Kind::DistanceLoci {
            first,
            second,
            parameter,
        } => SketchRelation::Distance {
            a: locus(first)?,
            b: locus(second)?,
            along: Some(DistanceAxis::Aligned),
            doubled: None,
            value: Dimension::Parameter(parameter.clone()),
        },
        Kind::HorizontalDistance {
            first,
            second,
            parameter,
        } => SketchRelation::Distance {
            a: locus(first)?,
            b: locus(second)?,
            along: Some(DistanceAxis::Horizontal),
            doubled: None,
            value: Dimension::Parameter(parameter.clone()),
        },
        Kind::VerticalDistance {
            first,
            second,
            parameter,
        } => SketchRelation::Distance {
            a: locus(first)?,
            b: locus(second)?,
            along: Some(DistanceAxis::Vertical),
            doubled: None,
            value: Dimension::Parameter(parameter.clone()),
        },
        Kind::Radius { entity, parameter } => SketchRelation::Radial {
            of: entity.clone(),
            measure: RadialMeasure::Radius,
            value: Dimension::Parameter(parameter.clone()),
        },
        Kind::Diameter { entity, parameter } => SketchRelation::Radial {
            of: entity.clone(),
            measure: RadialMeasure::Diameter,
            value: Dimension::Parameter(parameter.clone()),
        },
        Kind::Native {
            native_kind,
            entities,
            parameter,
            ..
        } => SketchRelation::Native {
            kind: native_kind.clone(),
            operands: entities.iter().map(entity).collect(),
            values: match parameter {
                None => Vec::new(),
                Some(id) => {
                    use cadmpeg_ir::design::units::{Quantity, ValueKind};
                    let value = match kinds.get(id)? {
                        ValueKind::Quantity(quantity) if *quantity == Quantity::LENGTH => {
                            ScalarDimension::Length(Dimension::Parameter(id.clone()))
                        }
                        ValueKind::Quantity(quantity) if *quantity == Quantity::ANGLE => {
                            ScalarDimension::Angle(Dimension::Parameter(id.clone()))
                        }
                        ValueKind::Quantity(quantity) if *quantity == Quantity::REAL => {
                            ScalarDimension::Ratio(Dimension::Parameter(id.clone()))
                        }
                        _ => return None,
                    };
                    vec![value]
                }
            },
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
