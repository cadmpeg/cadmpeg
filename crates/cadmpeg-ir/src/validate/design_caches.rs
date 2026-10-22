// SPDX-License-Identifier: Apache-2.0
//! Agreement of resolved design frames with their neutral reference geometry.

use cadmpeg_core::{decode::DecodeContext, CodecError};

use crate::features::{
    DatumPlaneReference, DatumPointConstruction, EdgeSelection, ExtrudeDirection,
    ExtrusionDirectionSource, FaceSelection, FeatureOperation, PathRef, PlanarProfileRef,
    SketchPointSelection,
};
use crate::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::math::{Point2, Point3, Vector3};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::transform::Transform;
use crate::CadIr;

pub(super) fn check(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for feature in &ir.model.features {
        if feature.needs_recompute {
            continue;
        }
        let mismatch = match feature.definition().operation() {
            FeatureOperation::Revolve { construction, .. } => {
                if let Some((axis, reference)) = construction
                    .axis()
                    .and_then(|axis| axis.reference.as_ref().map(|reference| (axis, reference)))
                {
                    path_line(ctx, ir, reference)?.and_then(|(origin, direction)| {
                        (!parallel(ir, axis.direction.get(), direction)
                            || !on_line(ir, axis.origin.get(), origin, direction))
                        .then_some(
                            "resolved revolution axis disagrees with its canonical path reference",
                        )
                    })
                } else {
                    None
                }
            }
            FeatureOperation::Extrude {
                profile,
                direction:
                    ExtrudeDirection::Explicit {
                        vector,
                        source: Some(source),
                    },
                ..
            } => {
                let expected = match source {
                    ExtrusionDirectionSource::Custom {} => None,
                    ExtrusionDirectionSource::Edge { reference } => {
                        path_line(ctx, ir, reference)?.map(|(_, direction)| (direction, false))
                    }
                    ExtrusionDirectionSource::ProfileNormal {} => {
                        profile_normal(ir, profile).map(|normal| (normal, true))
                    }
                };
                expected.and_then(|(expected, oriented)| {
                    (!(if oriented {
                        same_direction(ir, vector.get(), expected)
                    } else {
                        parallel(ir, vector.get(), expected)
                    }))
                    .then_some(
                        "resolved extrusion direction disagrees with its canonical reference",
                    )
                })
            }
            FeatureOperation::Draft { anchor, .. } => {
                let expected = anchor.pull().and_then(|pull| {
                    pull.plane
                        .as_ref()
                        .and_then(|plane| datum_feature_plane(ir, plane))
                        .map(|(_, normal)| (pull.direction.get(), normal))
                });
                expected.and_then(|(direction, normal)| {
                    (!parallel(ir, direction, normal))
                        .then_some("resolved draft pull disagrees with its canonical datum plane")
                })
            }
            FeatureOperation::MirrorShape {
                plane_origin,
                plane_normal,
                plane_reference: Some(reference),
                ..
            } => face_plane(ir, reference).and_then(|(origin, normal)| {
                (!parallel(ir, *plane_normal.as_raw(), normal)
                    || !on_plane(ir, plane_origin.get(), origin, normal))
                .then_some("resolved mirror plane disagrees with its canonical face reference")
            }),
            FeatureOperation::DatumPoint {
                position,
                construction: Some(construction),
            } => datum_point_agreement(ctx, ir, position.get(), construction)?.and_then(|agrees| {
                (!agrees).then_some(
                    "resolved datum-point position disagrees with its canonical construction",
                )
            }),
            FeatureOperation::Hole {
                face,
                direction,
                placements,
                ..
            } => {
                let plane = face.as_ref().and_then(|face| face_plane(ir, face));
                let mismatch = placements
                    .iter()
                    .flatten()
                    .any(|placement| match placement {
                        crate::features::holes::HolePlacement::Directed {
                            position,
                            direction: placement_direction,
                        } => {
                            plane.is_some_and(|(origin, normal)| {
                                !on_plane(ir, position.get(), origin, normal)
                            }) || direction.as_ref().is_some_and(|direction| {
                                !same_direction(ir, placement_direction.get(), direction.get())
                            })
                        }
                        crate::features::holes::HolePlacement::Axis { axis, .. } => direction
                            .as_ref()
                            .is_some_and(|direction| !parallel(ir, axis.get(), direction.get())),
                    });
                mismatch.then_some("resolved hole placement disagrees with its canonical support or drilling direction")
            }
            _ => None,
        };
        if let Some(message) = mismatch {
            super::record_finding(
                findings,
                Check::GeometricConsistency,
                Severity::Error,
                Some(feature.id.as_str()),
                format_args!("{message}"),
            );
        }
    }
    Ok(())
}

fn point_close(ir: &CadIr, first: Point3, second: Point3) -> bool {
    first.distance(second) <= ir.tolerances.linear.get()
}
fn parallel(ir: &CadIr, first: Vector3, second: Vector3) -> bool {
    first
        .unit()
        .zip(second.unit())
        .is_some_and(|(first, second)| first.cross(second).norm() <= ir.tolerances.angular.get())
}
fn same_direction(ir: &CadIr, first: Vector3, second: Vector3) -> bool {
    parallel(ir, first, second) && first.dot(second) > 0.0
}
fn on_line(ir: &CadIr, point: Point3, origin: Point3, direction: Vector3) -> bool {
    direction.unit().is_some_and(|direction| {
        point.vector_from(origin).cross(direction).norm() <= ir.tolerances.linear.get()
    })
}
fn on_plane(ir: &CadIr, point: Point3, origin: Point3, normal: Vector3) -> bool {
    normal.unit().is_some_and(|normal| {
        point.vector_from(origin).dot(normal).abs() <= ir.tolerances.linear.get()
    })
}

fn shell_transform(ir: &CadIr, shell: &crate::ids::ShellId) -> Option<Transform> {
    let shell = ir
        .model
        .shells
        .iter()
        .find(|candidate| candidate.id == *shell)?;
    let region = ir
        .model
        .regions
        .iter()
        .find(|candidate| candidate.id == shell.region)?;
    let body = ir
        .model
        .bodies
        .iter()
        .find(|candidate| candidate.id == region.body)?;
    Some(body.transform.unwrap_or_default())
}
fn edge_transform(ir: &CadIr, edge: &crate::ids::EdgeId) -> Option<Transform> {
    let mut transforms = Vec::new();
    for shell in &ir.model.shells {
        if shell.wire_edges().contains(edge) {
            transforms.push(shell_transform(ir, &shell.id)?);
        }
    }
    for coedge in ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == *edge)
    {
        let loop_ = ir
            .model
            .loops
            .iter()
            .find(|candidate| candidate.id == coedge.owner_loop)?;
        let face = ir
            .model
            .faces
            .iter()
            .find(|candidate| candidate.id == loop_.face)?;
        transforms.push(shell_transform(ir, &face.shell)?);
    }
    let transform = transforms.first().copied().unwrap_or_default();
    transforms
        .iter()
        .all(|candidate| *candidate == transform)
        .then_some(transform)
}
fn curve_line(
    ctx: &DecodeContext<'_>,
    curve: &SolvedCurveGeometry,
) -> Result<Option<(Point3, Vector3)>, CodecError> {
    let _depth = ctx.enter_nested("resolved design line reference")?;
    Ok(match curve {
        SolvedCurveGeometry::Line(line) => Some((line.origin().get(), *line.direction().as_raw())),
        SolvedCurveGeometry::Transformed(placed) => {
            let Some((origin, direction)) = curve_line(ctx, placed.basis())? else {
                return Ok(None);
            };
            placed
                .transform()
                .apply_point(origin)
                .zip(placed.transform().apply_vector(direction))
                .map(|(origin, direction)| (origin.get(), direction.get()))
        }
        _ => None,
    })
}
fn edge_line(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    edge: &crate::ids::EdgeId,
) -> Result<Option<(Point3, Vector3)>, CodecError> {
    let Some(record) = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == *edge)
    else {
        return Ok(None);
    };
    let Some(curve) = record
        .curve()
        .and_then(|curve| {
            ir.model
                .curves
                .iter()
                .find(|candidate| candidate.id == *curve)
        })
        .and_then(|curve| curve.geometry.solved())
    else {
        return Ok(None);
    };
    let Some((origin, direction)) = curve_line(ctx, curve)? else {
        return Ok(None);
    };
    let Some(transform) = edge_transform(ir, edge) else {
        return Ok(None);
    };
    Ok(transform
        .apply_point(origin)
        .zip(transform.apply_vector(direction))
        .map(|(origin, direction)| (origin.get(), direction.get())))
}
fn path_line(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    path: &PathRef,
) -> Result<Option<(Point3, Vector3)>, CodecError> {
    match path {
        PathRef::SketchCurves { sketch, curves } => match curves.as_slice() {
            [entity] => Ok(planar_line(ir, sketch, entity)),
            _ => Ok(None),
        },
        PathRef::Sketch(sketch) => {
            let Some(record) = ir.model.sketches.iter().find(|record| record.id == *sketch) else {
                return Ok(None);
            };
            let [profile] = record.profiles.as_slice() else {
                return Ok(None);
            };
            let [usage] = profile.as_slice() else {
                return Ok(None);
            };
            Ok(planar_line(ir, sketch, &usage.entity))
        }
        PathRef::SpatialSketchCurves { sketch, curves } => {
            let [id] = curves.as_slice() else {
                return Ok(None);
            };
            Ok(ir
                .model
                .spatial_sketch_entities
                .iter()
                .find(|entity| entity.id() == id && entity.sketch == *sketch)
                .and_then(|entity| match entity.geometry.definition() {
                    crate::sketches::SpatialSketchGeometryDefinition::Line { start, end } => {
                        Some((start.get(), end.get().vector_from(start.get())))
                    }
                    _ => None,
                }))
        }
        PathRef::Edges(edges) => match edges.as_slice() {
            [edge] => edge_line(ctx, ir, edge),
            _ => Ok(None),
        },
        PathRef::Curves(curves) => {
            let [id] = curves.as_slice() else {
                return Ok(None);
            };
            // A curve id alone carries no instance selection. Only evaluate an
            // uninstanced carrier, rather than guessing an owning placement.
            if ir.model.edges.iter().any(|edge| {
                edge.curve() == Some(id)
                    && edge_transform(ir, &edge.id) != Some(Transform::default())
            }) {
                return Ok(None);
            }
            let Some(curve) = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *id)
                .and_then(|curve| curve.geometry.solved())
            else {
                return Ok(None);
            };
            curve_line(ctx, curve)
        }
        _ => Ok(None),
    }
}
fn planar_line(
    ir: &CadIr,
    sketch: &crate::sketches::SketchId,
    id: &crate::sketches::SketchEntityId,
) -> Option<(Point3, Vector3)> {
    let entity = ir
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.id() == id && entity.sketch == *sketch)?;
    let crate::sketches::SketchGeometryDefinition::Line { start, end } =
        entity.geometry.definition()
    else {
        return None;
    };
    let start = planar_point(ir, sketch, start.get())?;
    let end = planar_point(ir, sketch, end.get())?;
    Some((start, end.vector_from(start)))
}
fn face_plane(ir: &CadIr, selection: &FaceSelection) -> Option<(Point3, Vector3)> {
    let (FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. }) = selection else {
        return None;
    };
    let [id] = faces.as_slice() else {
        return None;
    };
    let face = ir.model.faces.iter().find(|face| face.id == *id)?;
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)?;
    let SolvedSurfaceGeometry::Plane(plane) = surface.geometry.solved()? else {
        return None;
    };
    let transform = shell_transform(ir, &face.shell)?;
    Some((
        transform.apply_point(plane.origin().get())?.get(),
        *transform
            .apply_normal(*plane.frame().axis().as_raw())?
            .as_raw(),
    ))
}
fn datum_feature_plane(ir: &CadIr, id: &crate::features::FeatureId) -> Option<(Point3, Vector3)> {
    match ir
        .model
        .features
        .iter()
        .find(|feature| feature.id == *id)?
        .definition()
        .operation()
    {
        FeatureOperation::DatumPlane { frame }
        | FeatureOperation::DatumThreePointPlane { frame, .. } => {
            Some((frame.origin().get(), frame.normal().get()))
        }
        _ => None,
    }
}
fn datum_plane(ir: &CadIr, reference: &DatumPlaneReference) -> Option<(Point3, Vector3)> {
    match reference {
        DatumPlaneReference::Feature { feature } => datum_feature_plane(ir, feature),
        DatumPlaneReference::Face { face } => face_plane(ir, face),
        DatumPlaneReference::ResolvedPlane { frame } => {
            Some((frame.origin().get(), frame.normal().get()))
        }
    }
}
fn planar_point(ir: &CadIr, sketch: &crate::sketches::SketchId, point: Point2) -> Option<Point3> {
    let (origin, normal, u) = ir
        .model
        .sketches
        .iter()
        .find(|candidate| candidate.id == *sketch)?
        .resolved_placement()?;
    let u = u.unit_nonzero()?;
    let v = normal.unit_nonzero()?.cross(u);
    Some(origin.get().translated(u, point.u).translated(v, point.v))
}
fn profile_normal(ir: &CadIr, profile: &crate::features::ProfileRef) -> Option<Vector3> {
    let profile = profile.planar()?;
    let (PlanarProfileRef::Sketch(sketch)
    | PlanarProfileRef::SketchProfiles { sketch, .. }
    | PlanarProfileRef::SketchRegions { sketch, .. }
    | PlanarProfileRef::SketchEntities { sketch, .. }
    | PlanarProfileRef::SketchSelection { sketch, .. }) = profile
    else {
        return None;
    };
    Some(
        ir.model
            .sketches
            .iter()
            .find(|candidate| candidate.id == *sketch)?
            .resolved_placement()?
            .1
            .get(),
    )
}
fn selected_edge<'a>(
    ir: &'a CadIr,
    selection: &EdgeSelection,
) -> Option<&'a crate::topology::Edge> {
    let (EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. }) = selection else {
        return None;
    };
    let [id] = edges.as_slice() else {
        return None;
    };
    ir.model.edges.iter().find(|edge| edge.id == *id)
}
fn circle_center(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedCurveGeometry,
) -> Result<Option<Point3>, CodecError> {
    let _depth = ctx.enter_nested("resolved datum circle-center reference")?;
    Ok(match geometry {
        SolvedCurveGeometry::Circle(circle) => Some(circle.center().get()),
        SolvedCurveGeometry::Transformed(placed) => match circle_center(ctx, placed.basis())? {
            Some(center) => placed
                .transform()
                .apply_point(center)
                .map(crate::features::FinitePoint3::get),
            None => None,
        },
        _ => None,
    })
}
fn datum_point_agreement(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    position: Point3,
    construction: &DatumPointConstruction,
) -> Result<Option<bool>, CodecError> {
    match construction {
        DatumPointConstruction::TwoEdgeIntersection { edges } => {
            let Some(first) = selected_edge(ir, &edges[0]) else {
                return Ok(None);
            };
            let Some(second) = selected_edge(ir, &edges[1]) else {
                return Ok(None);
            };
            let Some((first_origin, first_direction)) = edge_line(ctx, ir, &first.id)? else {
                return Ok(None);
            };
            let Some((second_origin, second_direction)) = edge_line(ctx, ir, &second.id)? else {
                return Ok(None);
            };
            return Ok(Some(
                on_line(ir, position, first_origin, first_direction)
                    && on_line(ir, position, second_origin, second_direction),
            ));
        }
        DatumPointConstruction::ThreePlaneIntersection { planes } => {
            let resolved: Option<Vec<_>> =
                planes.iter().map(|plane| datum_plane(ir, plane)).collect();
            return Ok(resolved.map(|planes| {
                planes
                    .into_iter()
                    .all(|(origin, normal)| on_plane(ir, position, origin, normal))
            }));
        }
        DatumPointConstruction::EdgePlaneIntersection { edge, plane } => {
            let Some(edge) = selected_edge(ir, edge) else {
                return Ok(None);
            };
            let Some((origin, direction)) = edge_line(ctx, ir, &edge.id)? else {
                return Ok(None);
            };
            return Ok(datum_plane(ir, plane).map(|(plane_origin, normal)| {
                on_line(ir, position, origin, direction)
                    && on_plane(ir, position, plane_origin, normal)
            }));
        }
        _ => {}
    }
    let expected = match construction {
        DatumPointConstruction::SketchPoint {
            point: SketchPointSelection::Planar { sketch, point, .. },
        } => {
            let Some(entity) = ir
                .model
                .sketch_entities
                .iter()
                .find(|entity| entity.id() == point && entity.sketch == *sketch)
            else {
                return Ok(None);
            };
            let crate::sketches::SketchGeometryDefinition::Point { position } =
                entity.geometry.definition()
            else {
                return Ok(None);
            };
            planar_point(ir, sketch, position.get())
        }
        DatumPointConstruction::SketchPoint {
            point: SketchPointSelection::Spatial { sketch, point, .. },
        } => ir
            .model
            .spatial_sketch_entities
            .iter()
            .find(|entity| entity.id() == point && entity.sketch == *sketch)
            .and_then(|entity| match entity.geometry.definition() {
                crate::sketches::SpatialSketchGeometryDefinition::Point { position } => {
                    Some(position.get())
                }
                _ => None,
            }),
        DatumPointConstruction::CircleCenter { edge } => {
            let Some(edge) = selected_edge(ir, edge) else {
                return Ok(None);
            };
            let Some(curve) = edge
                .curve()
                .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
                .and_then(|curve| curve.geometry.solved())
            else {
                return Ok(None);
            };
            let center = circle_center(ctx, curve)?;
            center
                .zip(edge_transform(ir, &edge.id))
                .and_then(|(center, transform)| transform.apply_point(center))
                .map(crate::features::FinitePoint3::get)
        }
        DatumPointConstruction::DistanceOnEdge { edge, fraction } => {
            let Some(edge) = selected_edge(ir, edge) else {
                return Ok(None);
            };
            let Some(curve) = edge
                .curve()
                .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
            else {
                return Ok(None);
            };
            let Some([start, end]) = edge.param_range().map(crate::units::FiniteVector::get) else {
                return Ok(None);
            };
            let Some(fraction) = ir.dimension_value(fraction) else {
                return Ok(None);
            };
            let parameter = start * (1.0 - fraction.get()) + end * fraction.get();
            let point = crate::eval::finite_or_refusal(crate::eval::decode::curve_point(
                crate::eval::admission::EvaluationAdmission::Decode(ctx),
                &curve.geometry,
                parameter,
            ))?;
            point
                .zip(edge_transform(ir, &edge.id))
                .and_then(|(point, transform)| transform.apply_point(point.get()))
                .map(crate::features::FinitePoint3::get)
        }
        _ => None,
    };
    Ok(expected.map(|expected| point_close(ir, position, expected)))
}

#[cfg(test)]
mod tests;
