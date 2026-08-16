// SPDX-License-Identifier: Apache-2.0
//! Intersection pcurve completion, edge incidence, and boundary transfer.

use super::blend::{
    blend_boundary_parameter_from_contact_pcurve_with_geometry,
    blend_boundary_parameter_from_support_pcurve, blend_surface_definition,
    blend_surface_definition_with_index, blend_surface_parameters_for_fit_with_grid,
    blend_surface_point_inner_with_index, closest_spine_parameter_with_index,
    decoded_surface_point_inner, decoded_surface_point_with_geometry, model_curve_point_with_index,
    model_curve_tangent_with_index, spine_contact_pcurve, BlendParameterGrid,
    BoundaryInverseTarget,
};
use super::offset::{
    lift_periodic_parameter, offset_surface_parameters_with_tolerance_with_index, point_distance,
    surface_parameter_domain, surface_parameter_periods,
};
use super::support_uv::{
    blend_spine_cache_fit_tolerance_with_index, linear_knots, parameterization_equivalent_surfaces,
    pcurve_requires_completion,
};
use crate::native::vector::{dot_vector, unit_vector};
use crate::topology::{Graph, Node};
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, curve_point, curve_second_derivative, curve_tangent,
    model_surface_partials_by_id, nurbs_curve_speed_bound, nurbs_surface_isocurve,
    nurbs_surface_parameter_within_tolerance, pcurve_tangent, pcurve_uv, surface_second_partials,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition,
    SurfaceGeometry, SurfaceParameterAxis, TolerantIntersectionParameterization,
};
use cadmpeg_ir::ids::{
    CoedgeId, CurveId, EdgeId, PcurveId, ProceduralCurveId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn pcurve_parameter_range(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    let PcurveGeometry::Nurbs { knots, .. } = geometry else {
        return None;
    };
    ordered_parameter_range([*knots.first()?, *knots.last()?])
}

pub(crate) fn ordered_parameter_range(mut range: [f64; 2]) -> Option<[f64; 2]> {
    if !range.iter().all(|value| value.is_finite()) || range[0] == range[1] {
        return None;
    }
    if range[0] > range[1] {
        range.swap(0, 1);
    }
    Some(range)
}

pub(crate) fn complete_intersection_supports_from_edge_incidence(ir: &mut CadIr) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_.face.clone()))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((edge.id.clone(), edge.curve.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let mut incident_surfaces = BTreeMap::<CurveId, Vec<SurfaceId>>::new();
    for coedge in &ir.model.coedges {
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        let surfaces = incident_surfaces.entry(curve.clone()).or_default();
        if !surfaces.contains(surface) {
            surfaces.push(surface.clone());
        }
    }

    for procedural in &mut ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        let missing = context
            .sides
            .iter()
            .enumerate()
            .filter_map(|(index, side)| side.surface.is_none().then_some(index))
            .collect::<Vec<_>>();
        if missing.len() != 1 {
            continue;
        }
        let Some(incident) = incident_surfaces.get(&procedural.curve) else {
            continue;
        };
        let candidates = incident
            .iter()
            .filter(|surface| {
                !context
                    .sides
                    .iter()
                    .any(|side| side.surface.as_ref() == Some(surface))
            })
            .collect::<Vec<_>>();
        let [surface] = candidates.as_slice() else {
            continue;
        };
        context.sides[missing[0]].surface = Some((*surface).clone());
    }
}

pub(crate) fn complete_intersection_pcurves_from_coedge_incidence(ir: &mut CadIr) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_.face.clone()))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((edge.id.clone(), edge.curve.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let mut incident_pcurves = BTreeMap::<(CurveId, SurfaceId), Vec<PcurveId>>::new();
    for coedge in &ir.model.coedges {
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        let pcurves = incident_pcurves
            .entry((curve.clone(), surface.clone()))
            .or_default();
        for pcurve in &coedge.pcurves {
            if !pcurves.contains(&pcurve.pcurve) {
                pcurves.push(pcurve.pcurve.clone());
            }
        }
    }

    for procedural in &mut ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        for side in &mut context.sides {
            if side.pcurve.is_some() {
                continue;
            }
            let Some(surface) = &side.surface else {
                continue;
            };
            let Some([pcurve]) = incident_pcurves
                .get(&(procedural.curve.clone(), surface.clone()))
                .map(Vec::as_slice)
            else {
                continue;
            };
            let Some(carrier) = ir
                .model
                .pcurves
                .iter()
                .find(|carrier| &carrier.id == pcurve)
            else {
                continue;
            };
            side.pcurve = Some(carrier.geometry.clone());
        }
    }
}

pub(crate) fn complete_tolerant_intersection_pcurves_from_serialized_branches(
    ir: &mut CadIr,
    serialized: &BTreeSet<(CurveId, SurfaceId, PcurveId)>,
    annotations: &mut AnnotationBuilder,
) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_.face.clone()))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((edge.id.clone(), edge.curve.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let mut incident = BTreeMap::<(CurveId, SurfaceId), Vec<(PcurveId, Option<[f64; 2]>)>>::new();
    for coedge in &ir.model.coedges {
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        for use_ in &coedge.pcurves {
            if !serialized.contains(&(curve.clone(), surface.clone(), use_.pcurve.clone())) {
                continue;
            }
            let candidates = incident
                .entry((curve.clone(), surface.clone()))
                .or_default();
            let candidate = (use_.pcurve.clone(), use_.parameter_range);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    let vertex_points = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| {
            let point = ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)?;
            Some((vertex.id.clone(), point.position))
        })
        .collect::<BTreeMap<_, _>>();
    let replacements = {
        let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
        let mut replacements = Vec::new();
        for procedural in &ir.model.procedural_curves {
            let ProceduralCurveDefinition::TolerantIntersection {
                supports,
                endpoints,
                tolerance: _,
                parameterization: None,
            } = &procedural.definition
            else {
                continue;
            };
            let edges = ir
                .model
                .edges
                .iter()
                .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
                .collect::<Vec<_>>();
            let [edge] = edges.as_slice() else {
                continue;
            };
            let Some(endpoint_tolerance) = edge
                .tolerance
                .filter(|value| value.is_finite() && *value >= 0.0)
            else {
                continue;
            };
            let edge_reversed = match (vertex_points.get(&edge.start), vertex_points.get(&edge.end))
            {
                (Some(start), Some(end)) => {
                    let forward = point_distance(*start, endpoints[0]) <= endpoint_tolerance
                        && point_distance(*end, endpoints[1]) <= endpoint_tolerance;
                    let reversed = point_distance(*start, endpoints[1]) <= endpoint_tolerance
                        && point_distance(*end, endpoints[0]) <= endpoint_tolerance;
                    match (forward, reversed) {
                        (true, false) => false,
                        (false, true) => true,
                        (true, true) if edge.start == edge.end => false,
                        _ => continue,
                    }
                }
                _ => continue,
            };
            let candidates = supports.each_ref().map(|support| {
                incident
                    .get(&(procedural.curve.clone(), support.clone()))
                    .map(Vec::as_slice)
            });
            let [Some([(first_id, first_use_range)]), Some([(second_id, second_use_range)])] =
                candidates
            else {
                continue;
            };
            let carriers = [first_id, second_id].map(|id| {
                ir.model
                    .pcurves
                    .iter()
                    .find(|candidate| &candidate.id == id)
            });
            let [Some(first), Some(second)] = carriers else {
                continue;
            };
            let ranges = [
                first_use_range
                    .or(first.parameter_range)
                    .or_else(|| pcurve_parameter_range(&first.geometry)),
                second_use_range
                    .or(second.parameter_range)
                    .or_else(|| pcurve_parameter_range(&second.geometry)),
            ];
            let [Some(first_range), Some(second_range)] = ranges else {
                continue;
            };
            if !first_range
                .iter()
                .zip(second_range)
                .all(|(first, second)| first.to_bits() == second.to_bits())
                || !first_range[0].is_finite()
                || !first_range[1].is_finite()
                || first_range[0] >= first_range[1]
            {
                continue;
            }
            if edge.param_range.is_some_and(|range| {
                !range
                    .iter()
                    .zip(first_range)
                    .all(|(existing, branch)| existing.to_bits() == branch.to_bits())
            }) {
                continue;
            }
            let Some(()) = first
                .fit_tolerance
                .zip(second.fit_tolerance)
                .map(|(first, second)| first + second)
                .filter(|bound| bound.is_finite() && *bound <= endpoint_tolerance)
                .map(|_| ())
            else {
                continue;
            };
            let carriers = [first, second];
            let pcurves: [Option<PcurveGeometry>; 2] = std::array::from_fn(|side| {
                orient_tolerant_intersection_pcurve_with_index(
                    &model_index,
                    &procedural.curve,
                    &supports[side],
                    &carriers[side].geometry,
                    first_range,
                    *endpoints,
                    endpoint_tolerance,
                )
            });
            if let [Some(first), Some(second)] = pcurves {
                replacements.push((
                    procedural.id.clone(),
                    edge.id.clone(),
                    edge_reversed,
                    TolerantIntersectionParameterization {
                        pcurves: [first, second],
                        parameter_range: first_range,
                    },
                ));
            }
        }
        replacements
    };

    for (procedural_id, edge_id, edge_reversed, parameterization) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::TolerantIntersection {
            parameterization: slot,
            ..
        } = &mut procedural.definition
        else {
            continue;
        };
        if slot.is_some() {
            continue;
        }
        let range = parameterization.parameter_range;
        *slot = Some(parameterization);
        if let Some(edge) = ir.model.edges.iter_mut().find(|edge| edge.id == edge_id) {
            if edge_reversed {
                std::mem::swap(&mut edge.start, &mut edge.end);
            }
            edge.param_range = Some(range);
            annotations.derived(&edge.id, "param_range");
        }
    }
}

#[cfg(test)]
pub(crate) fn orient_tolerant_intersection_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    support: &SurfaceId,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
    endpoints: [Point3; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    orient_tolerant_intersection_pcurve_with_index(
        &index, curve, support, pcurve, range, endpoints, tolerance,
    )
}

#[allow(clippy::too_many_arguments)]
fn orient_tolerant_intersection_pcurve_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    curve: &CurveId,
    support: &SurfaceId,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
    endpoints: [Point3; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    let points = range.map(|parameter| {
        let uv = pcurve_uv(pcurve, parameter)?;
        decoded_surface_point_inner(index, support, uv.u, uv.v, 0)
    });
    let [Some(first), Some(second)] = points else {
        return None;
    };
    let forward = point_distance(first, endpoints[0]) <= tolerance
        && point_distance(second, endpoints[1]) <= tolerance;
    let reversed = point_distance(first, endpoints[1]) <= tolerance
        && point_distance(second, endpoints[0]) <= tolerance;
    match (forward, reversed) {
        (true, false) => Some(pcurve.clone()),
        (false, true) => reverse_pcurve_over_range(pcurve, range),
        (true, true) => {
            let reversed = reverse_pcurve_over_range(pcurve, range)?;
            let curve_tangent = model_curve_tangent_with_index(index, curve, range[0])?;
            let alignment = |candidate: &PcurveGeometry| {
                let uv = pcurve_uv(candidate, range[0])?;
                let uv_tangent = pcurve_tangent(candidate, range[0])?;
                let partials = model_surface_partials_by_id(index, support, uv.u, uv.v)?;
                let tangent = unit_vector(Vector3::new(
                    uv_tangent.u * partials.du.x + uv_tangent.v * partials.dv.x,
                    uv_tangent.u * partials.du.y + uv_tangent.v * partials.dv.y,
                    uv_tangent.u * partials.du.z + uv_tangent.v * partials.dv.z,
                ))?;
                Some(dot_vector(curve_tangent, tangent))
            };
            match (alignment(pcurve)?, alignment(&reversed)?) {
                (forward_alignment, reversed_alignment)
                    if forward_alignment > 0.0 && reversed_alignment <= 0.0 =>
                {
                    Some(pcurve.clone())
                }
                (forward_alignment, reversed_alignment)
                    if reversed_alignment > 0.0 && forward_alignment <= 0.0 =>
                {
                    Some(reversed)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn reverse_pcurve_over_range(
    pcurve: &PcurveGeometry,
    [start, end]: [f64; 2],
) -> Option<PcurveGeometry> {
    let reflection = start + end;
    if !reflection.is_finite() {
        return None;
    }
    let combine = |first: Point2, first_scale: f64, second: Point2, second_scale: f64| {
        let value = Point2::new(
            first_scale * first.u + second_scale * second.u,
            first_scale * first.v + second_scale * second.v,
        );
        (value.u.is_finite() && value.v.is_finite()).then_some(value)
    };
    match pcurve {
        PcurveGeometry::Line { origin, direction } => Some(PcurveGeometry::Line {
            origin: Point2::new(
                origin.u + reflection * direction.u,
                origin.v + reflection * direction.v,
            ),
            direction: Point2::new(-direction.u, -direction.v),
        }),
        PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            Some(PcurveGeometry::PolarHarmonic {
                radial_center: *radial_center,
                radial_cos: Point2::new(
                    cosine * radial_cos.u + sine * radial_sin.u,
                    cosine * radial_cos.v + sine * radial_sin.v,
                ),
                radial_sin: Point2::new(
                    sine * radial_cos.u - cosine * radial_sin.u,
                    sine * radial_cos.v - cosine * radial_sin.v,
                ),
                axial_origin: *axial_origin,
                axial_cos: cosine * axial_cos + sine * axial_sin,
                axial_sin: sine * axial_cos - cosine * axial_sin,
            })
        }
        PcurveGeometry::Harmonic {
            center,
            cosine: source_cosine,
            sine: source_sine,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            Some(PcurveGeometry::Harmonic {
                center: *center,
                cosine: combine(*source_cosine, cosine, *source_sine, sine)?,
                sine: combine(*source_cosine, sine, *source_sine, -cosine)?,
            })
        }
        PcurveGeometry::Hyperbolic {
            center,
            cosine: source_cosine,
            sine: source_sine,
        } => {
            let cosine = reflection.cosh();
            let sine = reflection.sinh();
            Some(PcurveGeometry::Hyperbolic {
                center: *center,
                cosine: combine(*source_cosine, cosine, *source_sine, sine)?,
                sine: combine(*source_cosine, -sine, *source_sine, -cosine)?,
            })
        }
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            axial_control_points,
            weights,
            periodic,
        } => {
            let reversed_knots = knots
                .iter()
                .rev()
                .map(|knot| reflection - knot)
                .collect::<Vec<_>>();
            let mut radial_control_points = radial_control_points.clone();
            radial_control_points.reverse();
            let mut axial_control_points = axial_control_points.clone();
            axial_control_points.reverse();
            let mut weights = weights.clone();
            if let Some(weights) = &mut weights {
                weights.reverse();
            }
            let finite = reversed_knots
                .iter()
                .chain(
                    radial_control_points
                        .iter()
                        .flat_map(|point| [&point.u, &point.v]),
                )
                .chain(&axial_control_points)
                .all(|value| value.is_finite());
            finite.then_some(PcurveGeometry::PolarNurbs {
                degree: *degree,
                knots: reversed_knots,
                radial_control_points,
                axial_control_points,
                weights,
                periodic: *periodic,
            })
        }
        PcurveGeometry::SphericalGreatCircle {
            azimuth_origin,
            azimuth_rate,
            plane_phase,
            plane_slope,
        } => {
            let reversed_origin = azimuth_origin + azimuth_rate * reflection;
            let reversed_rate = -*azimuth_rate;
            [reversed_origin, reversed_rate, *plane_phase, *plane_slope]
                .into_iter()
                .all(f64::is_finite)
                .then_some(PcurveGeometry::SphericalGreatCircle {
                    azimuth_origin: reversed_origin,
                    azimuth_rate: reversed_rate,
                    plane_phase: *plane_phase,
                    plane_slope: *plane_slope,
                })
        }
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            let reversed_x = Point2::new(
                cosine * x_axis.u + sine * y_axis.u,
                cosine * x_axis.v + sine * y_axis.v,
            );
            let reversed_y = Point2::new(
                sine * x_axis.u - cosine * y_axis.u,
                sine * x_axis.v - cosine * y_axis.v,
            );
            [reversed_x.u, reversed_x.v, reversed_y.u, reversed_y.v]
                .into_iter()
                .all(f64::is_finite)
                .then_some(PcurveGeometry::Circle {
                    center: *center,
                    x_axis: reversed_x,
                    y_axis: reversed_y,
                    radius: *radius,
                })
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            let reversed_knots = knots
                .iter()
                .rev()
                .map(|knot| reflection - knot)
                .collect::<Vec<_>>();
            let mut control_points = control_points.clone();
            control_points.reverse();
            let mut weights = weights.clone();
            if let Some(weights) = &mut weights {
                weights.reverse();
            }
            let finite = reversed_knots
                .iter()
                .chain(control_points.iter().flat_map(|point| [&point.u, &point.v]))
                .all(|value| value.is_finite());
            finite.then_some(PcurveGeometry::Nurbs {
                degree: *degree,
                knots: reversed_knots,
                control_points,
                weights,
                periodic: *periodic,
            })
        }
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
            same_sense,
        } => Some(PcurveGeometry::Trimmed {
            parameter_range: *parameter_range,
            same_sense: *same_sense,
            basis: Box::new(reverse_pcurve_over_range(basis, [start, end])?),
        }),
        PcurveGeometry::Transformed { basis, transform } => Some(PcurveGeometry::Transformed {
            basis: Box::new(reverse_pcurve_over_range(basis, [start, end])?),
            transform: *transform,
        }),
        PcurveGeometry::Offset { distance, basis } => Some(PcurveGeometry::Offset {
            distance: -*distance,
            basis: Box::new(reverse_pcurve_over_range(basis, [start, end])?),
        }),
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } if reflection == 0.0 => Some(PcurveGeometry::Ellipse {
            center: *center,
            x_axis: *x_axis,
            y_axis: Point2::new(-y_axis.u, -y_axis.v),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        }),
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            Some(PcurveGeometry::Harmonic {
                center: *center,
                cosine: combine(*x_axis, major_radius * cosine, *y_axis, minor_radius * sine)?,
                sine: combine(
                    *x_axis,
                    major_radius * sine,
                    *y_axis,
                    -minor_radius * cosine,
                )?,
            })
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } if reflection == 0.0 => Some(PcurveGeometry::Parabola {
            vertex: *vertex,
            x_axis: *x_axis,
            y_axis: Point2::new(-y_axis.u, -y_axis.v),
            focal_distance: *focal_distance,
        }),
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } if start.is_finite()
            && end.is_finite()
            && start < end
            && focal_distance.is_finite()
            && *focal_distance != 0.0 =>
        {
            let point = |parameter: f64| {
                let axial = parameter * parameter / (4.0 * focal_distance);
                Point2::new(
                    vertex.u + axial * x_axis.u + parameter * y_axis.u,
                    vertex.v + axial * x_axis.v + parameter * y_axis.v,
                )
            };
            let first = point(end);
            let last = point(start);
            let derivative = Point2::new(
                -(end / (2.0 * focal_distance) * x_axis.u + y_axis.u),
                -(end / (2.0 * focal_distance) * x_axis.v + y_axis.v),
            );
            let half_span = (end - start) * 0.5;
            let middle = Point2::new(
                first.u + half_span * derivative.u,
                first.v + half_span * derivative.v,
            );
            [first.u, first.v, middle.u, middle.v, last.u, last.v]
                .into_iter()
                .all(f64::is_finite)
                .then_some(PcurveGeometry::Nurbs {
                    degree: 2,
                    knots: vec![start, start, start, end, end, end],
                    control_points: vec![first, middle, last],
                    weights: None,
                    periodic: false,
                })
        }
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } if reflection == 0.0 => Some(PcurveGeometry::Hyperbola {
            center: *center,
            x_axis: *x_axis,
            y_axis: Point2::new(-y_axis.u, -y_axis.v),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        }),
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = reflection.cosh();
            let sine = reflection.sinh();
            Some(PcurveGeometry::Hyperbolic {
                center: *center,
                cosine: combine(*x_axis, major_radius * cosine, *y_axis, minor_radius * sine)?,
                sine: combine(
                    *x_axis,
                    -major_radius * sine,
                    *y_axis,
                    -minor_radius * cosine,
                )?,
            })
        }
        PcurveGeometry::Parabola { .. } => None,
    }
}

#[cfg(test)]
pub(super) fn complete_intersection_pcurves_from_opposite_charts(ir: &mut CadIr) {
    let transfer_budget = new_transfer_budget();
    complete_intersection_pcurves_from_opposite_charts_with_budget(ir, &transfer_budget);
}

pub(super) fn complete_intersection_pcurves_from_opposite_charts_with_budget(
    ir: &mut CadIr,
    transfer_budget: &TransferBudget<'_>,
) {
    let edge_tolerances = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| {
            Some((
                edge.curve.clone()?,
                edge.tolerance
                    .filter(|value| value.is_finite() && *value >= 0.0)?,
            ))
        })
        .fold(
            BTreeMap::<CurveId, f64>::new(),
            |mut values, (curve, tolerance)| {
                values
                    .entry(curve)
                    .and_modify(|current| *current = current.min(tolerance))
                    .or_insert(tolerance);
                values
            },
        );
    let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
    let mut blend_contacts = BTreeMap::new();
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            let missing = context
                .sides
                .each_ref()
                .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
            if transfer_budget_exhausted(transfer_budget) {
                return None;
            }
            let target = match missing {
                [true, false] => 0,
                [false, true] => 1,
                _ => return None,
            };
            let source = 1 - target;
            let source_surface = context.sides[source].surface.as_ref()?;
            let source_pcurve = context.sides[source].pcurve.as_ref()?;
            let target_surface = context.sides[target].surface.as_ref()?;
            let tolerance = procedural
                .cache_fit_tolerance
                .or_else(|| edge_tolerances.get(&procedural.curve).copied())?;
            let tolerance = blend_spine_cache_fit_tolerance_with_index(
                &model_index,
                ir,
                target_surface,
                tolerance,
            );
            let blend_contact = blend_contacts
                .entry((source_surface.clone(), target_surface.clone()))
                .or_insert_with(|| {
                    blend_transfer_contact(&model_index, ir, source_surface, target_surface)
                })
                .as_ref()
                .copied();
            let pcurve = transfer_intersection_pcurve_with_contact_and_budget(
                &model_index,
                ir,
                &procedural.curve,
                source_surface,
                source_pcurve,
                target_surface,
                context.parameter_range,
                tolerance,
                blend_contact,
                transfer_budget,
            )?;
            Some((
                procedural.id.clone(),
                target,
                pcurve,
                tolerance,
                curve_is_cache_backed(ir, &procedural.curve),
            ))
        })
        .collect::<Vec<_>>();
    for (procedural_id, side, pcurve, tolerance, cache_backed) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
            if cache_backed {
                procedural.cache_fit_tolerance =
                    Some(procedural.cache_fit_tolerance.unwrap_or(0.0).max(tolerance));
            }
        }
    }
}

#[cfg(test)]
pub(super) fn complete_exact_boundary_intersection_pcurves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let transfer_budget = WorkBudget::new(MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES);
    complete_exact_boundary_intersection_pcurves_with_budget(ir, annotations, &transfer_budget);
}

pub(super) fn complete_exact_boundary_intersection_pcurves_with_budget(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    transfer_budget: &TransferBudget<'_>,
) {
    let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
    let vertex_points = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| {
            let point = ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)?;
            Some((vertex.id.clone(), point.position))
        })
        .collect::<BTreeMap<_, _>>();
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let edges = ir
                .model
                .edges
                .iter()
                .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
                .collect::<Vec<_>>();
            let [edge] = edges.as_slice() else {
                return None;
            };
            let (supports, endpoints, range, tolerance, tolerant) = match &procedural.definition {
                ProceduralCurveDefinition::Intersection { context, .. } => {
                    if !context
                        .sides
                        .iter()
                        .all(|side| pcurve_requires_completion(side.pcurve.as_ref()))
                    {
                        return None;
                    }
                    (
                        [
                            context.sides[0].surface.as_ref()?,
                            context.sides[1].surface.as_ref()?,
                        ],
                        [
                            *vertex_points.get(&edge.start)?,
                            *vertex_points.get(&edge.end)?,
                        ],
                        context.parameter_range,
                        edge.tolerance
                            .filter(|value| value.is_finite() && *value >= 0.0)?,
                        false,
                    )
                }
                ProceduralCurveDefinition::TolerantIntersection {
                    supports,
                    endpoints,
                    tolerance,
                    parameterization: None,
                } => {
                    let range = if edge.start == edge.end
                        && ir
                            .model
                            .curves
                            .iter()
                            .find(|candidate| candidate.id == procedural.curve)
                            .is_some_and(|curve| {
                                matches!(
                                    curve.geometry,
                                    CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }
                                )
                            }) {
                        [0.0, std::f64::consts::TAU]
                    } else {
                        [0.0, 1.0]
                    };
                    (supports.each_ref(), *endpoints, range, *tolerance, true)
                }
                _ => return None,
            };
            let [first_surface, second_surface] = supports;
            let candidates = [first_surface, second_surface].map(|surface| {
                exact_boundary_pcurve_with_index(
                    &model_index,
                    ir,
                    &procedural.curve,
                    surface,
                    endpoints,
                    range,
                    tolerance,
                )
            });
            let pcurves = match candidates {
                [Some(first), Some(second)] => {
                    if coincident_pcurve_pair_with_index(
                        &model_index,
                        ir,
                        [first_surface, second_surface],
                        [&first, &second],
                        range,
                        tolerance,
                    ) {
                        [first, second]
                    } else {
                        let transferred = [
                            transfer_intersection_pcurve(
                                &model_index,
                                ir,
                                &procedural.curve,
                                first_surface,
                                &first,
                                second_surface,
                                range,
                                tolerance,
                                transfer_budget,
                            )
                            .map(|transferred| [first.clone(), transferred]),
                            transfer_intersection_pcurve(
                                &model_index,
                                ir,
                                &procedural.curve,
                                second_surface,
                                &second,
                                first_surface,
                                range,
                                tolerance,
                                transfer_budget,
                            )
                            .map(|transferred| [transferred, second.clone()]),
                        ];
                        match transferred {
                            [Some(pair), None] | [None, Some(pair)] => pair,
                            _ => return None,
                        }
                    }
                }
                [Some(first), None] => [
                    first.clone(),
                    transfer_intersection_pcurve(
                        &model_index,
                        ir,
                        &procedural.curve,
                        first_surface,
                        &first,
                        second_surface,
                        range,
                        tolerance,
                        transfer_budget,
                    )?,
                ],
                [None, Some(second)] => [
                    transfer_intersection_pcurve(
                        &model_index,
                        ir,
                        &procedural.curve,
                        second_surface,
                        &second,
                        first_surface,
                        range,
                        tolerance,
                        transfer_budget,
                    )?,
                    second,
                ],
                [None, None] => return None,
            };
            Some((
                procedural.id.clone(),
                pcurves,
                tolerance,
                curve_is_cache_backed(ir, &procedural.curve),
                procedural.curve.clone(),
                range,
                tolerant,
            ))
        })
        .collect::<Vec<_>>();
    let mut bounded_tolerant_curves = Vec::new();
    for (procedural_id, pcurves, tolerance, cache_backed, curve, range, tolerant) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        match &mut procedural.definition {
            ProceduralCurveDefinition::Intersection { context, .. }
                if context
                    .sides
                    .iter()
                    .all(|side| pcurve_requires_completion(side.pcurve.as_ref())) =>
            {
                for (side, pcurve) in context.sides.iter_mut().zip(pcurves) {
                    side.pcurve = Some(pcurve);
                }
            }
            ProceduralCurveDefinition::TolerantIntersection {
                parameterization, ..
            } if parameterization.is_none() => {
                *parameterization = Some(TolerantIntersectionParameterization {
                    pcurves,
                    parameter_range: range,
                });
            }
            _ => continue,
        }
        if cache_backed {
            procedural.cache_fit_tolerance =
                Some(procedural.cache_fit_tolerance.unwrap_or(0.0).max(tolerance));
        }
        if tolerant {
            bounded_tolerant_curves.push((curve, range));
        }
    }
    for (curve, range) in bounded_tolerant_curves {
        if let Some(edge) = ir
            .model
            .edges
            .iter_mut()
            .find(|edge| edge.curve.as_ref() == Some(&curve))
        {
            edge.param_range = Some(range);
            annotations.derived(&edge.id, "param_range");
        }
    }
}

pub(crate) fn curve_is_cache_backed(ir: &CadIr, curve: &CurveId) -> bool {
    ir.model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)
        .is_some_and(|carrier| !matches!(&carrier.geometry, CurveGeometry::Procedural { .. }))
}

#[cfg(test)]
pub(crate) fn exact_boundary_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    surface: &SurfaceId,
    endpoints: [Point3; 2],
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    exact_boundary_pcurve_with_index(&index, ir, curve, surface, endpoints, range, tolerance)
}

#[allow(clippy::too_many_arguments)]
fn exact_boundary_pcurve_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    curve: &CurveId,
    surface: &SurfaceId,
    endpoints: [Point3; 2],
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    (range[0].is_finite()
        && range[1].is_finite()
        && range[0] < range[1]
        && tolerance.is_finite()
        && tolerance >= 0.0)
        .then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    if let Some(candidate) = exact_analytic_isocurve_pcurve(ir, curve, surface, range, tolerance) {
        return Some(candidate);
    }
    if matches!(&carrier.geometry, SurfaceGeometry::Plane { .. }) {
        let [first, second] =
            endpoints.map(|endpoint| analytic_surface_parameters(&carrier.geometry, endpoint));
        let [first, second] = [first?, second?];
        for (endpoint, parameter) in endpoints.into_iter().zip([first, second]) {
            if !parameter.u.is_finite() || !parameter.v.is_finite() {
                return None;
            }
            let mapped = decoded_surface_point_inner(index, surface, parameter.u, parameter.v, 0)?;
            let error = point_distance(mapped, endpoint);
            if !error.is_finite() || error > tolerance {
                return None;
            }
        }
        let parameter_span = range[1] - range[0];
        let direction = Point2::new(
            (second.u - first.u) / parameter_span,
            (second.v - first.v) / parameter_span,
        );
        (direction.u.is_finite()
            && direction.v.is_finite()
            && (direction.u != 0.0 || direction.v != 0.0))
            .then_some(())?;
        let candidate = PcurveGeometry::Line {
            origin: Point2::new(
                first.u - direction.u * range[0],
                first.v - direction.v * range[0],
            ),
            direction,
        };
        return exact_boundary_pcurve_matches_carrier_with_index(
            index, ir, curve, surface, &candidate, range, tolerance,
        )
        .then_some(candidate);
    }
    if matches!(
        &carrier.geometry,
        SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. }
    ) {
        let [first, second] =
            endpoints.map(|endpoint| analytic_surface_parameters(&carrier.geometry, endpoint));
        let [first, second] = [first?, second?];
        if [first.u, first.v, second.u, second.v]
            .into_iter()
            .any(|value| !value.is_finite())
        {
            return None;
        }
        let parameter_span = range[1] - range[0];
        let varying_scale = (second.v - first.v) / parameter_span;
        (varying_scale.is_finite() && varying_scale != 0.0).then_some(())?;
        let candidate = PcurveGeometry::Line {
            origin: Point2::new(first.u, first.v - varying_scale * range[0]),
            direction: Point2::new(0.0, varying_scale),
        };
        for (endpoint, parameter) in endpoints.into_iter().zip(range) {
            let uv = pcurve_uv(&candidate, parameter)?;
            let mapped = decoded_surface_point_inner(index, surface, uv.u, uv.v, 0)?;
            let error = point_distance(mapped, endpoint);
            if !error.is_finite() || error > tolerance {
                return None;
            }
        }
        return exact_boundary_pcurve_matches_carrier_with_index(
            index, ir, curve, surface, &candidate, range, tolerance,
        )
        .then_some(candidate);
    }
    let SurfaceGeometry::Nurbs(nurbs) = &carrier.geometry else {
        return None;
    };
    let domain = surface_parameter_domain(ir, surface)?;
    let parameters = [
        nurbs_surface_parameter_within_tolerance(nurbs, endpoints[0], None, tolerance)?,
        nurbs_surface_parameter_within_tolerance(nurbs, endpoints[1], None, tolerance)?,
    ];
    for index in 0..2 {
        if !parameters[index].u.is_finite() || !parameters[index].v.is_finite() {
            return None;
        }
        let point =
            cadmpeg_ir::eval::nurbs_surface_point(nurbs, parameters[index].u, parameters[index].v)?;
        let error = point_distance(point, endpoints[index]);
        if !error.is_finite() || error > tolerance {
            return None;
        }
    }
    let axes = [domain.0, domain.1];
    let candidates = axes
        .into_iter()
        .enumerate()
        .flat_map(|(constant_axis, axis_domain)| {
            axis_domain.into_iter().filter_map(move |boundary| {
                let varying = if constant_axis == 0 {
                    [parameters[0].v, parameters[1].v]
                } else {
                    [parameters[0].u, parameters[1].u]
                };
                let delta = (varying[1] - varying[0]) / (range[1] - range[0]);
                (delta.is_finite() && delta != 0.0).then(|| {
                    let (origin, direction) = if constant_axis == 0 {
                        (
                            Point2::new(boundary, varying[0] - delta * range[0]),
                            Point2::new(0.0, delta),
                        )
                    } else {
                        (
                            Point2::new(varying[0] - delta * range[0], boundary),
                            Point2::new(delta, 0.0),
                        )
                    };
                    PcurveGeometry::Line { origin, direction }
                })
            })
        })
        .filter(|candidate| {
            exact_boundary_pcurve_matches_carrier_with_index(
                index, ir, curve, surface, candidate, range, tolerance,
            )
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

#[allow(clippy::too_many_arguments)]
fn exact_boundary_pcurve_matches_carrier_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    curve: &CurveId,
    surface: &SurfaceId,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
    tolerance: f64,
) -> bool {
    let Some(carrier) = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)
    else {
        return false;
    };
    let Some(curve_breaks) = exact_boundary_curve_breaks(&carrier.geometry, range) else {
        return false;
    };
    let Some(surface_breaks) = boundary_curve_affine_breaks(ir, surface, pcurve, range) else {
        return false;
    };
    let mut breaks = curve_breaks;
    breaks.extend(surface_breaks);
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|first, second| first.to_bits() == second.to_bits());
    breaks.into_iter().all(|parameter| {
        let Some(uv) = pcurve_uv(pcurve, parameter) else {
            return false;
        };
        let Some(expected) = decoded_surface_point_inner(index, surface, uv.u, uv.v, 0) else {
            return false;
        };
        let Some(actual) = model_curve_point_with_index(index, curve, parameter) else {
            return false;
        };
        let error = point_distance(expected, actual);
        error.is_finite() && error <= tolerance
    })
}

pub(crate) fn exact_boundary_curve_breaks(
    geometry: &CurveGeometry,
    range: [f64; 2],
) -> Option<Vec<f64>> {
    let mut breaks = match geometry {
        CurveGeometry::Line { .. } => range.to_vec(),
        CurveGeometry::Nurbs(nurbs)
            if nurbs.degree == 1
                && !nurbs.periodic
                && !nurbs.weights.as_ref().is_some_and(|weights| {
                    weights
                        .windows(2)
                        .any(|pair| pair[0].to_bits() != pair[1].to_bits())
                }) =>
        {
            let degree = usize::try_from(nurbs.degree).ok()?;
            let count = nurbs.control_points.len();
            if degree > count {
                return None;
            }
            nurbs.knots.get(degree..=count)?.to_vec()
        }
        _ => return None,
    };
    breaks.retain(|parameter| {
        parameter.is_finite() && *parameter >= range[0] && *parameter <= range[1]
    });
    breaks.extend(range);
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|first, second| first.to_bits() == second.to_bits());
    Some(breaks)
}

pub(crate) fn exact_analytic_isocurve_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    surface: &SurfaceId,
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    const SAMPLE_INTERVALS: usize = 8;

    let curve = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)?;
    let curve_speed = match &curve.geometry {
        CurveGeometry::Circle { radius, .. } => radius.abs(),
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => major_radius.abs().max(minor_radius.abs()),
        _ => return None,
    };
    let surface_carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    matches!(
        surface_carrier.geometry,
        SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. }
    )
    .then_some(())?;
    let periods = surface_parameter_periods(ir, surface);
    let mut samples = Vec::with_capacity(SAMPLE_INTERVALS + 1);
    for index in 0..=SAMPLE_INTERVALS {
        let parameter = range[0] + (range[1] - range[0]) * index as f64 / SAMPLE_INTERVALS as f64;
        let point = curve_point(&curve.geometry, parameter)?;
        let mut uv = analytic_surface_parameters(&surface_carrier.geometry, point)?;
        if let Some(previous) = samples.last().map(|(_, uv): &(f64, Point2)| *uv) {
            if let Some(period) = periods[0] {
                uv.u = lift_periodic_parameter(uv.u, previous.u, period);
            }
            if let Some(period) = periods[1] {
                uv.v = lift_periodic_parameter(uv.v, previous.v, period);
            }
        }
        samples.push((parameter, uv));
    }
    let parameter_span = range[1] - range[0];
    let first = samples.first()?.1;
    let last = samples.last()?.1;
    let mut direction = Point2::new(
        (last.u - first.u) / parameter_span,
        (last.v - first.v) / parameter_span,
    );
    let angular_tolerance = (tolerance / curve_speed.max(tolerance)).max(1.0e-10);
    let u_constant = samples
        .iter()
        .all(|(_, uv)| (uv.u - first.u).abs() <= angular_tolerance);
    let v_constant = samples
        .iter()
        .all(|(_, uv)| (uv.v - first.v).abs() <= angular_tolerance);
    match (u_constant, v_constant) {
        (true, false) => direction.u = 0.0,
        (false, true) => direction.v = 0.0,
        _ => return None,
    }
    let varying_scale = if direction.u == 0.0 {
        &mut direction.v
    } else {
        &mut direction.u
    };
    (((*varying_scale).abs() - 1.0).abs() <= angular_tolerance).then_some(())?;
    *varying_scale = varying_scale.signum();
    let candidate = PcurveGeometry::Line {
        origin: Point2::new(
            first.u - direction.u * range[0],
            first.v - direction.v * range[0],
        ),
        direction,
    };
    let parameter = range[0];
    let uv = pcurve_uv(&candidate, parameter)?;
    let surface_jet = surface_second_partials(&surface_carrier.geometry, uv.u, uv.v)?;
    let curve_position = curve_point(&curve.geometry, parameter)?;
    let curve_tangent = curve_tangent(&curve.geometry, parameter)?;
    let curve_acceleration = curve_second_derivative(&curve.geometry, parameter)?;
    let surface_tangent = Vector3::new(
        direction.u * surface_jet.du.x + direction.v * surface_jet.dv.x,
        direction.u * surface_jet.du.y + direction.v * surface_jet.dv.y,
        direction.u * surface_jet.du.z + direction.v * surface_jet.dv.z,
    );
    let surface_acceleration = Vector3::new(
        direction.u * direction.u * surface_jet.duu.x
            + 2.0 * direction.u * direction.v * surface_jet.duv.x
            + direction.v * direction.v * surface_jet.dvv.x,
        direction.u * direction.u * surface_jet.duu.y
            + 2.0 * direction.u * direction.v * surface_jet.duv.y
            + direction.v * direction.v * surface_jet.dvv.y,
        direction.u * direction.u * surface_jet.duu.z
            + 2.0 * direction.u * direction.v * surface_jet.duv.z
            + direction.v * direction.v * surface_jet.dvv.z,
    );
    let vector_error = |first: Vector3, second: Vector3| {
        ((first.x - second.x).powi(2) + (first.y - second.y).powi(2) + (first.z - second.z).powi(2))
            .sqrt()
    };
    (point_distance(curve_position, surface_jet.point) <= tolerance
        && vector_error(curve_tangent, surface_tangent) <= tolerance
        && vector_error(curve_acceleration, surface_acceleration) <= tolerance)
        .then_some(())?;
    Some(candidate)
}

#[cfg(test)]
pub(crate) fn coincident_pcurve_pair(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    pcurves: [&PcurveGeometry; 2],
    range: [f64; 2],
    tolerance: f64,
) -> bool {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    coincident_pcurve_pair_with_index(&index, ir, surfaces, pcurves, range, tolerance)
}

#[allow(clippy::too_many_arguments)]
fn coincident_pcurve_pair_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    pcurves: [&PcurveGeometry; 2],
    range: [f64; 2],
    tolerance: f64,
) -> bool {
    const MAX_INTERVALS: usize = 100_000;

    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || !tolerance.is_finite()
        || tolerance < 0.0
    {
        return false;
    }
    let separation = |parameter| {
        let points = [0usize, 1usize].map(|side| {
            let uv = pcurve_uv(pcurves[side], parameter)?;
            decoded_surface_point_inner(index, surfaces[side], uv.u, uv.v, 0)
        });
        let [Some(first), Some(second)] = points else {
            return None;
        };
        let distance = point_distance(first, second);
        distance.is_finite().then_some(distance)
    };
    let affine_breaks = [0usize, 1usize]
        .map(|side| boundary_curve_affine_breaks(ir, surfaces[side], pcurves[side], range));
    if let [Some(first), Some(second)] = affine_breaks {
        let mut breaks = first;
        breaks.extend(second);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        return breaks
            .into_iter()
            .all(|parameter| separation(parameter).is_some_and(|value| value <= tolerance));
    }
    let Some(speed_bound) = [0usize, 1usize]
        .into_iter()
        .map(|side| boundary_curve_speed_bound_with_index(index, ir, surfaces[side], pcurves[side]))
        .sum::<Option<f64>>()
    else {
        return false;
    };
    if range
        .into_iter()
        .any(|parameter| !separation(parameter).is_some_and(|value| value <= tolerance))
    {
        return false;
    }
    let mut intervals = vec![range];
    let mut examined = 0usize;
    while let Some([start, end]) = intervals.pop() {
        examined += 1;
        if examined > MAX_INTERVALS {
            return false;
        }
        let middle = start + (end - start) * 0.5;
        let Some(middle_separation) = separation(middle) else {
            return false;
        };
        if middle_separation > tolerance {
            return false;
        }
        let maximum_separation = middle_separation + speed_bound * (end - start) * 0.5;
        if maximum_separation <= tolerance {
            continue;
        }
        if middle == start || middle == end {
            return false;
        }
        intervals.push([middle, end]);
        intervals.push([start, middle]);
    }
    true
}

pub(crate) fn boundary_curve_affine_breaks(
    ir: &CadIr,
    surface: &SurfaceId,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
) -> Option<Vec<f64>> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let PcurveGeometry::Line { origin, direction } = pcurve else {
        return None;
    };
    match &carrier.geometry {
        SurfaceGeometry::Plane { .. } => Some(range.to_vec()),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. }
            if direction.u == 0.0 && direction.v != 0.0 =>
        {
            Some(range.to_vec())
        }
        SurfaceGeometry::Nurbs(nurbs) => {
            let (fixed_axis, fixed_parameter, varying_origin, varying_scale) =
                if direction.u == 0.0 && direction.v != 0.0 {
                    (SurfaceParameterAxis::U, origin.u, origin.v, direction.v)
                } else if direction.v == 0.0 && direction.u != 0.0 {
                    (SurfaceParameterAxis::V, origin.v, origin.u, direction.u)
                } else {
                    return None;
                };
            let isocurve = nurbs_surface_isocurve(nurbs, fixed_axis, fixed_parameter)?;
            if isocurve.degree != 1
                || isocurve.weights.as_ref().is_some_and(|weights| {
                    weights
                        .windows(2)
                        .any(|pair| pair[0].to_bits() != pair[1].to_bits())
                })
            {
                return None;
            }
            let degree = usize::try_from(isocurve.degree).ok()?;
            let count = isocurve.control_points.len();
            let mut breaks = isocurve.knots.get(degree..=count)?.to_vec();
            for parameter in &mut breaks {
                *parameter = (*parameter - varying_origin) / varying_scale;
            }
            breaks.retain(|parameter| {
                parameter.is_finite() && *parameter >= range[0] && *parameter <= range[1]
            });
            breaks.extend(range);
            Some(breaks)
        }
        _ => None,
    }
}

fn boundary_curve_speed_bound_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    pcurve: &PcurveGeometry,
) -> Option<f64> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let PcurveGeometry::Line { origin, direction } = pcurve else {
        return None;
    };
    let affine_speed = || {
        let first = decoded_surface_point_inner(index, surface, origin.u, origin.v, 0)?;
        let second = decoded_surface_point_inner(
            index,
            surface,
            origin.u + direction.u,
            origin.v + direction.v,
            0,
        )?;
        let speed = point_distance(first, second);
        speed.is_finite().then_some(speed)
    };
    match &carrier.geometry {
        SurfaceGeometry::Plane { .. } => affine_speed(),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. }
            if direction.u == 0.0 && direction.v != 0.0 =>
        {
            affine_speed()
        }
        SurfaceGeometry::Cylinder { radius, .. } if direction.v == 0.0 && direction.u != 0.0 => {
            let speed = radius.abs() * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Cone {
            radius,
            ratio,
            half_angle,
            ..
        } if direction.v == 0.0 && direction.u != 0.0 => {
            let local_radius = radius + origin.v * half_angle.tan();
            let speed = local_radius.abs() * ratio.abs().max(1.0) * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Sphere { radius, .. } if direction.v == 0.0 && direction.u != 0.0 => {
            let speed = radius.abs() * origin.v.cos().abs() * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Sphere { radius, .. } if direction.u == 0.0 && direction.v != 0.0 => {
            let speed = radius.abs() * direction.v.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } if direction.v == 0.0 && direction.u != 0.0 => {
            let ring_radius = major_radius + minor_radius * origin.v.cos();
            let speed = ring_radius.abs() * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Torus { minor_radius, .. } if direction.u == 0.0 && direction.v != 0.0 => {
            let speed = minor_radius.abs() * direction.v.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Nurbs(nurbs) => {
            let (fixed_axis, fixed_parameter, varying_scale) =
                if direction.u == 0.0 && direction.v != 0.0 {
                    (SurfaceParameterAxis::U, origin.u, direction.v)
                } else if direction.v == 0.0 && direction.u != 0.0 {
                    (SurfaceParameterAxis::V, origin.v, direction.u)
                } else {
                    return None;
                };
            let isocurve = nurbs_surface_isocurve(nurbs, fixed_axis, fixed_parameter)?;
            let bound = nurbs_curve_speed_bound(&isocurve)? * varying_scale.abs();
            bound.is_finite().then_some(bound)
        }
        _ => None,
    }
}

/// Total transfer samples admitted while completing one model's opposite charts.
///
/// This second bound keeps many individually valid but expensive curves from
/// multiplying into an unbounded decode cost.
pub(super) const MAX_COMPLETION_TRANSFER_SAMPLES: usize = 1_024;

/// Total inverse-surface samples admitted while completing exact-boundary
/// pcurves in one model.
///
/// A partial pcurve would not satisfy the fit contract. This model-wide ceiling
/// keeps unusually curved boundaries from consuming an unbounded amount of
/// work while preserving the adaptive budget needed by valid carriers.
pub(super) const MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES: usize = 131_072;

#[derive(Clone, Copy)]
struct BlendTransferContact<'a> {
    support: &'a SurfaceId,
    support_geometry: &'a SurfaceGeometry,
    pcurve: &'a PcurveGeometry,
    boundary: usize,
}

fn blend_transfer_contact<'a>(
    index: &cadmpeg_ir::index::ModelIndex<'a>,
    ir: &'a CadIr,
    support: &'a SurfaceId,
    blend: &SurfaceId,
) -> Option<BlendTransferContact<'a>> {
    let (supports, spine, radius, _) = blend_surface_definition_with_index(index, blend)?;
    let matches = supports
        .iter()
        .enumerate()
        .filter(|(_, candidate)| parameterization_equivalent_surfaces(ir, candidate, support))
        .map(|(boundary, _)| boundary)
        .collect::<Vec<_>>();
    let [boundary] = matches.as_slice() else {
        return None;
    };
    Some(BlendTransferContact {
        support,
        support_geometry: &index.surfaces(support.0.as_str())?.geometry,
        pcurve: spine_contact_pcurve(ir, support, &spine, radius, 0)?,
        boundary: *boundary,
    })
}

pub(super) type TransferBudget<'a> = WorkBudget<'a>;

#[cfg(test)]
pub(super) fn new_transfer_budget() -> TransferBudget<'static> {
    WorkBudget::new(MAX_COMPLETION_TRANSFER_SAMPLES)
}

pub(super) fn transfer_budget_exhausted(budget: &TransferBudget<'_>) -> bool {
    budget.exhausted() || budget.remaining() == 0
}

#[allow(clippy::too_many_arguments)]
fn transfer_intersection_pcurve(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    parameter_range: [f64; 2],
    tolerance: f64,
    budget: &TransferBudget<'_>,
) -> Option<PcurveGeometry> {
    let blend_contact = blend_transfer_contact(index, ir, source_surface, target_surface);
    transfer_intersection_pcurve_with_contact_and_budget(
        index,
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        parameter_range,
        tolerance,
        blend_contact,
        budget,
    )
}

#[allow(clippy::too_many_arguments)]
fn transfer_intersection_pcurve_with_contact_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    parameter_range: [f64; 2],
    tolerance: f64,
    blend_contact: Option<BlendTransferContact<'_>>,
    budget: &TransferBudget<'_>,
) -> Option<PcurveGeometry> {
    let source_geometry = index
        .surfaces(source_surface.0.as_str())
        .map(|surface| &surface.geometry);
    let target_geometry = index
        .surfaces(target_surface.0.as_str())
        .map(|surface| &surface.geometry);
    transfer_intersection_pcurve_with_budget(
        index,
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        source_geometry,
        target_geometry,
        parameter_range,
        tolerance,
        blend_contact,
        budget,
    )
}

#[allow(clippy::too_many_arguments)]
fn transfer_intersection_pcurve_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    source_geometry: Option<&SurfaceGeometry>,
    target_geometry: Option<&SurfaceGeometry>,
    parameter_range: [f64; 2],
    tolerance: f64,
    blend_contact: Option<BlendTransferContact<'_>>,
    budget: &TransferBudget<'_>,
) -> Option<PcurveGeometry> {
    const CONTINUATION_STEPS: usize = 16;

    (parameter_range[0].is_finite()
        && parameter_range[1].is_finite()
        && parameter_range[0] < parameter_range[1]
        && tolerance.is_finite()
        && tolerance >= 0.0)
        .then_some(())?;
    let first = transferred_pcurve_sample_with_budget(
        index,
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        source_geometry,
        target_geometry,
        parameter_range[0],
        None,
        tolerance,
        blend_contact,
        budget,
    )?;
    let mut coarse = Vec::with_capacity(CONTINUATION_STEPS + 1);
    coarse.push(first);
    for sample_index in 1..=CONTINUATION_STEPS {
        let parameter = parameter_range[0]
            + (parameter_range[1] - parameter_range[0]) * sample_index as f64
                / CONTINUATION_STEPS as f64;
        let sample = transferred_pcurve_sample_with_budget(
            index,
            ir,
            curve,
            source_surface,
            source_pcurve,
            target_surface,
            source_geometry,
            target_geometry,
            parameter,
            coarse.last().map(|sample| sample.1),
            tolerance,
            blend_contact,
            budget,
        )?;
        coarse.push(sample);
    }
    let mut samples = vec![first];
    for pair in coarse.windows(2) {
        append_transferred_pcurve_segment_with_budget(
            index,
            ir,
            curve,
            source_surface,
            source_pcurve,
            target_surface,
            source_geometry,
            target_geometry,
            pair[0],
            pair[1],
            tolerance,
            0,
            &mut samples,
            blend_contact,
            budget,
        )?;
    }
    Some(PcurveGeometry::Nurbs {
        degree: 1,
        knots: linear_knots(&samples.iter().map(|sample| sample.0).collect::<Vec<_>>()),
        control_points: samples.iter().map(|sample| sample.1).collect(),
        weights: None,
        periodic: false,
    })
}

type TransferredPcurveSample = (f64, Point2, Point3);

#[allow(clippy::too_many_arguments)]
fn transferred_pcurve_sample_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    source_geometry: Option<&SurfaceGeometry>,
    target_geometry: Option<&SurfaceGeometry>,
    parameter: f64,
    seed: Option<Point2>,
    tolerance: f64,
    blend_contact: Option<BlendTransferContact<'_>>,
    budget: &TransferBudget<'_>,
) -> Option<TransferredPcurveSample> {
    if !budget.charge() {
        return None;
    }
    let source_uv = pcurve_uv(source_pcurve, parameter)?;
    let point = source_geometry
        .and_then(|geometry| {
            decoded_surface_point_with_geometry(
                index,
                source_surface,
                geometry,
                source_uv.u,
                source_uv.v,
                0,
            )
        })
        .or_else(|| decoded_surface_point_inner(index, source_surface, source_uv.u, source_uv.v, 0))
        .or_else(|| model_curve_point_with_index(index, curve, parameter))?;
    let target = BoundaryInverseTarget {
        point,
        seed,
        tolerance,
    };
    let target_uv = blend_contact
        .and_then(|contact| {
            blend_boundary_parameter_from_contact_pcurve_with_geometry(
                index,
                contact.support,
                contact.support_geometry,
                contact.pcurve,
                contact.boundary,
                source_pcurve,
                parameter,
                target,
            )
        })
        .or_else(|| {
            blend_boundary_parameter_from_support_pcurve(
                index,
                ir,
                target_surface,
                source_surface,
                source_pcurve,
                parameter,
                target,
            )
        })
        .or_else(|| {
            blend_boundary_parameter_from_support_spine_with_index(
                index,
                target_surface,
                source_surface,
                point,
                seed,
                tolerance,
            )
        })
        .or_else(|| {
            surface_parameters_for_fit_with_index(index, target_surface, point, seed, tolerance)
        })?;
    let accepted = blend_contact.is_some_and(|contact| {
        target_uv.v.to_bits() == (contact.boundary as f64).to_bits()
            && blend_transfer_point_with_index(index, contact, target_uv.u)
                .is_some_and(|candidate| point_distance(candidate, point) <= tolerance)
    }) || target_geometry
        .and_then(|geometry| {
            decoded_surface_point_with_geometry(
                index,
                target_surface,
                geometry,
                target_uv.u,
                target_uv.v,
                0,
            )
        })
        .or_else(|| decoded_surface_point_inner(index, target_surface, target_uv.u, target_uv.v, 0))
        .is_some_and(|candidate| point_distance(candidate, point) <= tolerance)
        || blend_boundary_spine_geometry_matches_with_index(
            index,
            target_surface,
            target_uv,
            point,
            tolerance,
        );
    accepted.then_some((parameter, target_uv, point))
}

fn blend_transfer_point_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    contact: BlendTransferContact<'_>,
    parameter: f64,
) -> Option<Point3> {
    let uv = pcurve_uv(contact.pcurve, parameter)?;
    decoded_surface_point_with_geometry(
        index,
        contact.support,
        contact.support_geometry,
        uv.u,
        uv.v,
        0,
    )
}

#[cfg(test)]
pub(crate) fn blend_boundary_parameter_from_support_spine(
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    blend_boundary_parameter_from_support_spine_with_index(
        &index, blend, support, point, seed, tolerance,
    )
}

pub(crate) fn blend_boundary_parameter_from_support_spine_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    blend: &SurfaceId,
    support: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    let ir = index.ir();
    let (supports, spine, _, _) = blend_surface_definition(ir, blend)?;
    let matches = supports
        .iter()
        .enumerate()
        .filter(|(_, candidate)| parameterization_equivalent_surfaces(ir, candidate, support))
        .map(|(boundary, _)| boundary)
        .collect::<Vec<_>>();
    let [boundary] = matches.as_slice() else {
        return None;
    };
    let parameter =
        closest_spine_parameter_with_index(index, &spine, point, seed.map(|seed| seed.u))?;
    let parameters = Point2::new(parameter, *boundary as f64);
    (blend_surface_point_inner_with_index(index, blend, parameters.u, parameters.v, 0)
        .is_some_and(|candidate| point_distance(candidate, point) <= tolerance)
        || blend_boundary_spine_geometry_matches_with_index(
            index, blend, parameters, point, tolerance,
        ))
    .then_some(parameters)
}

pub(crate) fn blend_boundary_spine_geometry_matches_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    blend: &SurfaceId,
    parameters: Point2,
    point: Point3,
    tolerance: f64,
) -> bool {
    let ir = index.ir();
    if parameters.v.to_bits() != 0.0f64.to_bits() && parameters.v.to_bits() != 1.0f64.to_bits() {
        return false;
    }
    let Some((_, spine, radius, _)) = blend_surface_definition(ir, blend) else {
        return false;
    };
    let Some(center) = model_curve_point_with_index(index, &spine, parameters.u) else {
        return false;
    };
    let radial = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let distance = radial.norm();
    if !distance.is_finite() || (distance - radius).abs() > tolerance {
        return false;
    }
    let Some(radial) = unit_vector(radial) else {
        return false;
    };
    let Some(tangent) = model_curve_tangent_with_index(index, &spine, parameters.u) else {
        return false;
    };
    let angular_tolerance = (tolerance / radius).max(1.0e-8);
    radial.dot(tangent).abs() <= angular_tolerance
}

#[allow(clippy::too_many_arguments)]
fn append_transferred_pcurve_segment_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    source_geometry: Option<&SurfaceGeometry>,
    target_geometry: Option<&SurfaceGeometry>,
    first: TransferredPcurveSample,
    last: TransferredPcurveSample,
    tolerance: f64,
    depth: usize,
    samples: &mut Vec<TransferredPcurveSample>,
    blend_contact: Option<BlendTransferContact<'_>>,
    budget: &TransferBudget<'_>,
) -> Option<()> {
    let midpoint_parameter = f64::midpoint(first.0, last.0);
    let midpoint_seed = Point2::new(
        f64::midpoint(first.1.u, last.1.u),
        f64::midpoint(first.1.v, last.1.v),
    );
    let midpoint = transferred_pcurve_sample_with_budget(
        index,
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        source_geometry,
        target_geometry,
        midpoint_parameter,
        Some(midpoint_seed),
        tolerance,
        blend_contact,
        budget,
    )?;
    let fits = [0.25, 0.5, 0.75].into_iter().all(|fraction| {
        let parameter = first.0 + fraction * (last.0 - first.0);
        let uv = Point2::new(
            first.1.u + fraction * (last.1.u - first.1.u),
            first.1.v + fraction * (last.1.v - first.1.v),
        );
        let Some(source_uv) = pcurve_uv(source_pcurve, parameter) else {
            return false;
        };
        let Some(source_point) = source_geometry
            .and_then(|geometry| {
                decoded_surface_point_with_geometry(
                    index,
                    source_surface,
                    geometry,
                    source_uv.u,
                    source_uv.v,
                    0,
                )
            })
            .or_else(|| {
                decoded_surface_point_inner(index, source_surface, source_uv.u, source_uv.v, 0)
            })
            .or_else(|| model_curve_point_with_index(index, curve, parameter))
        else {
            return false;
        };
        blend_contact.is_some_and(|contact| {
            uv.v.to_bits() == (contact.boundary as f64).to_bits()
                && blend_transfer_point_with_index(index, contact, uv.u).is_some_and(
                    |target_point| point_distance(source_point, target_point) <= tolerance,
                )
        }) || target_geometry
            .and_then(|geometry| {
                decoded_surface_point_with_geometry(index, target_surface, geometry, uv.u, uv.v, 0)
            })
            .or_else(|| decoded_surface_point_inner(index, target_surface, uv.u, uv.v, 0))
            .is_some_and(|target_point| point_distance(source_point, target_point) <= tolerance)
            || blend_boundary_spine_geometry_matches_with_index(
                index,
                target_surface,
                uv,
                source_point,
                tolerance,
            )
    });
    if fits {
        samples.push(last);
        return Some(());
    }
    (depth < 16).then_some(())?;
    append_transferred_pcurve_segment_with_budget(
        index,
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        source_geometry,
        target_geometry,
        first,
        midpoint,
        tolerance,
        depth + 1,
        samples,
        blend_contact,
        budget,
    )?;
    append_transferred_pcurve_segment_with_budget(
        index,
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        source_geometry,
        target_geometry,
        midpoint,
        last,
        tolerance,
        depth + 1,
        samples,
        blend_contact,
        budget,
    )
}

pub(crate) fn surface_parameters_for_fit_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    let ir = index.ir();
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => {
            nurbs_surface_parameter_within_tolerance(nurbs, point, seed, tolerance)
        }
        SurfaceGeometry::Procedural { .. } => offset_surface_parameters_with_tolerance_with_index(
            index,
            surface,
            point,
            seed,
            Some(tolerance),
        )
        .or_else(|| {
            blend_surface_parameters_for_fit_with_grid(
                index,
                surface,
                point,
                seed,
                tolerance,
                BlendParameterGrid::Build,
            )
        }),
        geometry => analytic_surface_parameters(geometry, point),
    }
}

pub(crate) fn attach_tolerant_edge_intersections(
    ir: &mut CadIr,
    graph: &Graph,
    edges: &BTreeMap<u32, EdgeId>,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let candidates = {
        let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
        let mut candidates = Vec::new();
        for (&xmt, edge_id) in edges {
            let Some(edge_fields) = graph.get(16, xmt).and_then(Node::edge_fields) else {
                continue;
            };
            let Some(first_fin) = graph.get(17, edge_fields.fin).and_then(Node::fin_fields) else {
                continue;
            };
            if edge_fields.curve != 1 || first_fin.curve_xmt != 1 || first_fin.other <= 1 {
                continue;
            }
            let Some(second_fin) = graph.get(17, first_fin.other).and_then(Node::fin_fields) else {
                continue;
            };
            if second_fin.other != edge_fields.fin || second_fin.edge != xmt {
                continue;
            }
            let Some(edge) = ir
                .model
                .edges
                .iter()
                .find(|candidate| &candidate.id == edge_id)
            else {
                continue;
            };
            let Some(tolerance) = edge.tolerance else {
                continue;
            };
            if edge.curve.is_some() {
                continue;
            }
            let support = |fin_xmt| {
                let coedge_id = CoedgeId(format!("{prefix}:fin#{fin_xmt}"));
                ir.model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == coedge_id && &coedge.edge == edge_id)
                    .and_then(|coedge| {
                        let face = ir
                            .model
                            .loops
                            .iter()
                            .find(|loop_| loop_.id == coedge.owner_loop)?
                            .face
                            .clone();
                        ir.model
                            .faces
                            .iter()
                            .find(|candidate| candidate.id == face)
                            .map(|face| face.surface.clone())
                    })
            };
            let Some(first_support) = support(edge_fields.fin) else {
                continue;
            };
            let Some(second_support) = support(first_fin.other) else {
                continue;
            };
            if first_support == second_support {
                continue;
            }
            let endpoint = |vertex_id: &VertexId| {
                let point_id = &ir
                    .model
                    .vertices
                    .iter()
                    .find(|vertex| &vertex.id == vertex_id)?
                    .point;
                ir.model
                    .points
                    .iter()
                    .find(|point| &point.id == point_id)
                    .map(|point| point.position)
            };
            let (Some(start), Some(end)) = (endpoint(&edge.start), endpoint(&edge.end)) else {
                continue;
            };
            let endpoints = [start, end];
            let supports = [first_support, second_support];
            let endpoints_bound_supports = supports.iter().all(|surface| {
                endpoints.iter().all(|point| {
                    surface_parameters_for_fit_with_index(
                        &model_index,
                        surface,
                        *point,
                        None,
                        tolerance,
                    )
                    .and_then(|uv| {
                        decoded_surface_point_inner(&model_index, surface, uv.u, uv.v, 0)
                    })
                    .is_some_and(|support_point| point_distance(*point, support_point) <= tolerance)
                })
            });
            if !endpoints_bound_supports {
                continue;
            }
            candidates.push((xmt, edge_id.clone(), supports, endpoints, tolerance));
        }
        candidates
    };

    for (xmt, edge_id, supports, endpoints, tolerance) in candidates {
        let curve_id = CurveId(format!("{prefix}:tolerant-curve#{xmt}"));
        let procedural_id = ProceduralCurveId(format!("{prefix}:tolerant-intersection#{xmt}"));
        let Some(edge) = ir
            .model
            .edges
            .iter_mut()
            .find(|candidate| candidate.id == edge_id)
        else {
            continue;
        };
        edge.curve = Some(curve_id.clone());
        annotations.derived(&edge_id, "curve");
        if let Some(node) = graph.get(16, xmt) {
            annotations
                .note(&curve_id, source_stream, node.pos as u64)
                .tag("TOLERANT_EDGE_INTERSECTION");
            annotations
                .note(&procedural_id, source_stream, node.pos as u64)
                .tag("TOLERANT_EDGE_INTERSECTION");
        }
        annotations.derived(&curve_id, "geometry");
        annotations.derived(&procedural_id, "definition");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Procedural {
                construction: procedural_id.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: procedural_id,
            curve: curve_id,
            definition: ProceduralCurveDefinition::TolerantIntersection {
                supports,
                endpoints,
                tolerance,
                parameterization: None,
            },
            cache_fit_tolerance: None,
        });
    }
}

#[cfg(test)]
pub(crate) fn pcurve_matches_edge(
    ir: &CadIr,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    fit_tolerance: Option<f64>,
) -> bool {
    pcurve_matches_edge_range(ir, edge_id, surface_id, geometry, None, fit_tolerance)
}

#[cfg(test)]
pub(crate) fn pcurve_matches_edge_range(
    ir: &CadIr,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    parameter_range: Option<[f64; 2]>,
    fit_tolerance: Option<f64>,
) -> bool {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    pcurve_matches_edge_range_with_index(
        ir,
        &index,
        edge_id,
        surface_id,
        geometry,
        parameter_range,
        fit_tolerance,
    )
}

pub(crate) fn pcurve_matches_edge_range_with_index(
    _ir: &CadIr,
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    parameter_range: Option<[f64; 2]>,
    fit_tolerance: Option<f64>,
) -> bool {
    let Some(edge) = index.edges(edge_id.0.as_str()) else {
        return false;
    };
    let Some([t0, t1]) = parameter_range.or_else(|| pcurve_parameter_range(geometry)) else {
        return false;
    };
    let (Some(first_uv), Some(second_uv)) = (pcurve_uv(geometry, t0), pcurve_uv(geometry, t1))
    else {
        return false;
    };
    let (Some(first), Some(second)) = (
        decoded_surface_point_inner(index, surface_id, first_uv.u, first_uv.v, 0),
        decoded_surface_point_inner(index, surface_id, second_uv.u, second_uv.v, 0),
    ) else {
        return false;
    };
    let coincident_surface = [first, second];
    let vertex = |id: &VertexId| {
        let vertex = index.vertices(id.0.as_str())?;
        let point = index.points(vertex.point.0.as_str())?;
        Some((point.position, vertex.tolerance))
    };
    let (Some((start, start_tolerance)), Some((end, end_tolerance))) =
        (vertex(&edge.start), vertex(&edge.end))
    else {
        return false;
    };
    let allowance = [
        edge.tolerance,
        start_tolerance,
        end_tolerance,
        fit_tolerance,
    ]
    .into_iter()
    .flatten()
    .fold(0.0_f64, f64::max);
    (point_distance(coincident_surface[0], start) <= allowance
        && point_distance(coincident_surface[1], end) <= allowance)
        || (point_distance(coincident_surface[0], end) <= allowance
            && point_distance(coincident_surface[1], start) <= allowance)
}
