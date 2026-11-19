// SPDX-License-Identifier: Apache-2.0
//! Datum constructions from decoded Inventor reference geometry.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{
    kinds, AnyConstruction, AxisValue, Construction, ConstructionInput, ConstructionKind,
    FrameValue, InputReference, InputRole, PlaneValue, Relation, ScalarDimension, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::selection::{PickTarget, PrincipalEntity};
use cadmpeg_ir::features::{
    DatumPlaneReference, DatumPointConstruction, FeatureDatumPlaneFrame,
    FeatureOperation as LegacyOperation, PrincipalPlane,
};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;

mod selections;

pub(super) fn project(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    for feature in &ir.model.features {
        let Some(target) = ir
            .design
            .features
            .iter_mut()
            .find(|item| item.id == feature.id)
        else {
            continue;
        };
        let operation = feature.evaluation.definition().operation();
        if let Some(datum) = datum(
            operation,
            &ir.design.sketch_entities,
            &ir.design.spatial_sketch_entities,
        ) {
            ctx.charge_entities(1, "project Inventor v11 datum")?;
            target.definition.operation = FeatureOperation::Datum(Box::new(datum));
        } else if matches!(operation, LegacyOperation::Native { kind, .. } if kind.as_str() == crate::feature::work_plane::NATIVE_KIND)
        {
            target.definition.operation = FeatureOperation::Unresolved {
                family: OperationFamily::Datum,
            };
        }
    }
    Ok(())
}

fn construction<K: ConstructionKind>(resolved: Option<K::Value>) -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::default(),
    }
}

fn input(reference: InputReference, relation: Relation) -> ConstructionInput {
    ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Whole,
            relation,
        }),
        reference: Some(reference),
        value: None,
        reversed: None,
    }
}

fn plane_value(frame: FeatureDatumPlaneFrame) -> Option<PlaneValue> {
    Some(PlaneValue {
        origin: frame.origin(),
        normal: UnitVector3::normalized_nonzero(frame.normal().into())?,
        x_axis: UnitVector3::normalized_nonzero(frame.u_axis().into())?,
    })
}

fn plane_reference(reference: &DatumPlaneReference) -> Option<InputReference> {
    Some(match reference {
        DatumPlaneReference::Feature { feature } => {
            InputReference::Picks(selections::target(PickTarget::Datum {
                feature: feature.clone(),
                part: None,
            })?)
        }
        DatumPlaneReference::Face { face } => InputReference::Picks(selections::face(face)?),
        DatumPlaneReference::ResolvedPlane { frame } => InputReference::Built(Box::new(
            AnyConstruction::Plane(construction(Some(PlaneValue {
                origin: frame.origin(),
                normal: UnitVector3::normalized_nonzero(frame.normal().into())?,
                x_axis: UnitVector3::normalized_nonzero(frame.u_axis().into())?,
            }))),
        )),
    })
}

fn point_inputs(
    rule: &DatumPointConstruction,
    planar: &[cadmpeg_ir::design::ops::sketch::SketchEntity<
        cadmpeg_ir::design::ops::sketch::Planar,
    >],
    spatial: &[cadmpeg_ir::design::ops::sketch::SketchEntity<
        cadmpeg_ir::design::ops::sketch::Spatial,
    >],
) -> Option<Vec<ConstructionInput>> {
    let edge_input = |edge, relation| {
        Some(input(
            InputReference::Picks(selections::edge(edge)?),
            relation,
        ))
    };
    Some(match rule {
        DatumPointConstruction::CircleCenter { edge } => vec![edge_input(edge, Relation::Center)?],
        DatumPointConstruction::TwoEdgeIntersection { edges } => vec![
            edge_input(&edges[0], Relation::Coincident)?,
            edge_input(&edges[1], Relation::Coincident)?,
        ],
        DatumPointConstruction::ThreePlaneIntersection { planes } => {
            let mut inputs = Vec::new();
            for plane in planes.iter() {
                inputs.push(input(plane_reference(plane)?, Relation::Coincident));
            }
            inputs
        }
        DatumPointConstruction::Vertex { vertex } => vec![input(
            InputReference::Picks(selections::vertex(vertex)?),
            Relation::Coincident,
        )],
        DatumPointConstruction::EdgePlaneIntersection { edge, plane } => vec![
            edge_input(edge, Relation::Coincident)?,
            input(plane_reference(plane)?, Relation::Coincident),
        ],
        DatumPointConstruction::DistanceOnEdge { edge, fraction } => {
            let mut at = edge_input(edge, Relation::AtParameter)?;
            at.value = Some(ScalarDimension::Ratio(Dimension::Literal(
                cadmpeg_ir::scalar::FiniteReal::new(fraction.get())?,
            )));
            vec![at]
        }
        DatumPointConstruction::SketchPoint { point } => {
            use cadmpeg_ir::features::SketchPointSelection;
            let target = match point {
                SketchPointSelection::Planar { sketch, point, .. }
                    if planar
                        .iter()
                        .any(|entity| &entity.sketch == sketch && &entity.id == point) =>
                {
                    PickTarget::SketchEntity(point.clone())
                }
                SketchPointSelection::Spatial { sketch, point, .. }
                    if spatial
                        .iter()
                        .any(|entity| &entity.sketch == sketch && &entity.id == point) =>
                {
                    PickTarget::SpatialSketchEntity(point.clone())
                }
                _ => return None,
            };
            vec![input(
                InputReference::Picks(selections::target(target)?),
                Relation::Coincident,
            )]
        }
    })
}

fn datum(
    operation: &LegacyOperation,
    planar: &[cadmpeg_ir::design::ops::sketch::SketchEntity<
        cadmpeg_ir::design::ops::sketch::Planar,
    >],
    spatial: &[cadmpeg_ir::design::ops::sketch::SketchEntity<
        cadmpeg_ir::design::ops::sketch::Spatial,
    >],
) -> Option<Datum> {
    let element = match operation {
        LegacyOperation::DatumPrincipalPlane { plane } => {
            let entity = match plane {
                PrincipalPlane::Front => PrincipalEntity::XyPlane,
                PrincipalPlane::Top => PrincipalEntity::XzPlane,
                PrincipalPlane::Right => PrincipalEntity::YzPlane,
            };
            let mut value = construction::<kinds::Plane>(None);
            value.inputs.push(input(
                InputReference::Picks(selections::target(PickTarget::Principal {
                    entity,
                    container: None,
                })?),
                Relation::Coincident,
            ));
            DatumElement::Plane(value)
        }
        LegacyOperation::DatumPlane { frame } => {
            DatumElement::Plane(construction(Some(plane_value(*frame)?)))
        }
        LegacyOperation::DatumThreePointPlane { frame, points } => {
            let mut value = construction(Some(plane_value(*frame)?));
            for point in points.iter() {
                value.inputs.push(input(
                    InputReference::Picks(selections::vertex(point)?),
                    Relation::Coincident,
                ));
            }
            DatumElement::Plane(value)
        }
        LegacyOperation::DatumOffsetPlane {
            reference: Some(reference),
            distance,
        } => {
            let mut value = construction::<kinds::Plane>(None);
            let mut offset = input(plane_reference(reference)?, Relation::Offset);
            offset.value = Some(ScalarDimension::Length(Dimension::Literal(*distance)));
            value.inputs.push(offset);
            DatumElement::Plane(value)
        }
        LegacyOperation::DatumAxis { axis } => {
            if axis.has_controls() {
                return None;
            }
            let (origin, direction) = axis.geometry()?;
            DatumElement::Axis(construction(Some(AxisValue {
                origin: *origin,
                direction: UnitVector3::normalized_nonzero((*direction).into())?,
            })))
        }
        LegacyOperation::DatumPoint {
            position,
            construction: rule,
        } => {
            let mut value = construction(Some(*position));
            if let Some(rule) = rule {
                value.inputs = point_inputs(rule, planar, spatial)?;
            }
            DatumElement::Points(vec![value])
        }
        LegacyOperation::DatumCoordinateSystem { frame } => {
            DatumElement::Frame(construction(Some(FrameValue {
                origin: frame.origin(),
                x_axis: frame.x_axis(),
                y_axis: frame.y_axis(),
                z_axis: frame.z_axis(),
            })))
        }
        _ => return None,
    };
    Some(Datum {
        element,
        link: None,
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    })
}

#[cfg(test)]
mod tests;
