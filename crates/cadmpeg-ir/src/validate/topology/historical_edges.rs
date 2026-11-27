// SPDX-License-Identifier: Apache-2.0
//! Exact input-edge paths in consumed history-body nests.

use std::collections::HashSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::features::evaluation::{
    FeatureInputTopology, HistoricalEdgeGeometry, HistoricalEdgePath,
};
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::history_body::BodyTopology;
use crate::math::portable::Portable;
use crate::math::Point3;
use crate::report::check::Finding;
use crate::topology::{Edge, EdgeCarrier, Sense};

pub(super) fn check(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    state: &FeatureInputTopology,
    geometry: &HistoricalEdgeGeometry,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let first = findings.len();
    check_binding(ctx, ir, state, geometry, findings)?;
    for finding in &mut findings[first..] {
        finding.related.push(geometry.edge.as_str().to_owned());
    }
    Ok(())
}

fn check_binding(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    state: &FeatureInputTopology,
    geometry: &HistoricalEdgeGeometry,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let Some(reference) = &geometry.topology else {
        return Ok(());
    };
    let owner = state.id.as_str();
    let mut bodies = ir
        .model
        .history_bodies
        .iter()
        .filter(|body| body.id == reference.history_body);
    let (Some(body), None) = (bodies.next(), bodies.next()) else {
        super::ref_error(
            findings,
            owner,
            "history body",
            reference.history_body.as_str(),
        );
        return Ok(());
    };
    if body.consumed_by.as_ref() != Some(&state.input_of) {
        super::geometry_error(
            findings,
            owner,
            crate::rule!(
                "topology.input-edge-history-body-has-a-different-consumer",
                "input-edge history body has a different consumer"
            ),
        );
    }
    let nest = &body.topology;
    let mut edges = Vec::new();
    match &reference.path {
        HistoricalEdgePath::Edge { edge } => {
            if let Some(edge) = nest.edges.iter().find(|candidate| candidate.id == *edge) {
                edges.push((edge, Sense::Forward));
            } else {
                super::ref_error(findings, owner, "history-body edge", edge.as_str());
            }
        }
        HistoricalEdgePath::Loop { loop_id } => {
            let Some(ring) = nest.loops.iter().find(|candidate| candidate.id == *loop_id) else {
                super::ref_error(findings, owner, "history-body loop", loop_id.as_str());
                return Ok(());
            };
            if ring.coedges().is_empty() {
                super::geometry_error(
                    findings,
                    owner,
                    crate::rule!(
                        "topology.input-edge-loop-requires-an-ordered-coedge-ring",
                        "input-edge loop requires an ordered coedge ring"
                    ),
                );
            }
            let mut endpoints = Vec::new();
            for id in ring.coedges() {
                let Some(coedge) = nest.coedges.iter().find(|candidate| candidate.id == *id) else {
                    continue;
                };
                let Some(edge) = nest
                    .edges
                    .iter()
                    .find(|candidate| candidate.id == coedge.edge)
                else {
                    continue;
                };
                endpoints.push(match coedge.sense {
                    Sense::Forward => (edge.start(), edge.end()),
                    Sense::Reversed => (edge.end(), edge.start()),
                });
                edges.push((edge, coedge.sense));
            }
            if edges.len() != ring.coedges().len()
                || endpoints.iter().enumerate().any(|(i, pair)| {
                    (endpoints.len() > 1 && (pair.0.is_none() || pair.1.is_none()))
                        || pair.1 != endpoints[(i + 1) % endpoints.len()].0
                })
            {
                super::geometry_error(
                    findings,
                    owner,
                    crate::rule!(
                        "topology.input-edge-loop-is-incomplete-or-open",
                        "input-edge loop is incomplete or open"
                    ),
                );
            }
            if !closed_carrier_ring(ctx, nest, &edges, ir.tolerances.linear.get())? {
                super::geometry_error(
                    findings,
                    owner,
                    crate::rule!(
                        "topology.input-edge-loop-requires-closed-carrier-domains-in-coedge-order",
                        "input-edge loop requires closed carrier domains in coedge order"
                    ),
                );
            }
        }
    }
    let mut keys = HashSet::new();
    let mut mapped = HashSet::new();
    for binding in &reference.faces {
        if !geometry.faces.contains(&binding.face) || !keys.insert(&binding.face) {
            super::geometry_error(
                findings,
                owner,
                crate::rule!(
                    "topology.input-edge-face-mapping-has-an-invalid-or-repeated-local-face",
                    "input-edge face mapping has an invalid or repeated local face"
                ),
            );
        }
        let mut targets = HashSet::new();
        for face in &binding.topology_faces {
            if !targets.insert(face) {
                super::geometry_error(
                    findings,
                    owner,
                    crate::rule!(
                        "topology.input-edge-face-mapping-repeats-a-target-face",
                        "input-edge face mapping repeats a target face"
                    ),
                );
            }
            mapped.insert(face);
        }
    }
    if keys.len() != geometry.faces.len() {
        super::geometry_error(
            findings,
            owner,
            crate::rule!(
                "topology.input-edge-face-mapping-omits-an-adjacent-local-face",
                "input-edge face mapping omits an adjacent local face"
            ),
        );
    }
    let mut incident = HashSet::new();
    for (edge, _) in &edges {
        let uses = nest
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .collect::<Vec<_>>();
        if uses.len() != 2 {
            super::geometry_error(
                findings,
                owner,
                crate::rule!(
                    "topology.input-edge-path-requires-two-incident-coedge-uses-per-edge",
                    "input-edge path requires two incident coedge uses per edge"
                ),
            );
        }
        for coedge in uses {
            if let Some(ring) = nest.loops.iter().find(|ring| ring.id == coedge.owner_loop) {
                incident.insert(&ring.face);
            }
        }
        for coedge in nest.coedges.iter().filter(|coedge| coedge.edge == edge.id) {
            let surface = nest
                .loops
                .iter()
                .find(|ring| ring.id == coedge.owner_loop)
                .and_then(|ring| nest.faces.iter().find(|face| face.id == ring.face))
                .and_then(|face| {
                    nest.surfaces
                        .iter()
                        .find(|surface| surface.id == face.surface)
                });
            if let Some(surface) = surface {
                if !edge_on_surface(ctx, nest, edge, surface, ir.tolerances.linear.get())? {
                    super::geometry_error(
                        findings,
                        owner,
                        crate::rule!(
                            "topology.input-edge-carrier-differs-from-an-adjacent-surface",
                            "input-edge carrier differs from an adjacent surface"
                        ),
                    );
                }
            }
        }
        if !admitted_curve(nest, edge) {
            super::geometry_error(
                findings,
                owner,
                crate::rule!("topology.input-edge-path-requires-a-solved-curve-and-a-finite-admitted-interval", "input-edge path requires a solved curve and a finite admitted interval"),
            );
        }
    }
    if mapped != incident {
        super::geometry_error(
            findings,
            owner,
            crate::rule!(
                "topology.input-edge-face-mapping-differs-from-nest-incidence",
                "input-edge face mapping differs from nest incidence"
            ),
        );
    }
    for id in &incident {
        let surface = nest
            .faces
            .iter()
            .find(|face| &face.id == *id)
            .and_then(|face| {
                nest.surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface)
            });
        if !surface.is_some_and(|surface| matches!(&surface.geometry,
            SurfaceGeometry::Solved(geometry) if !matches!(geometry, SolvedSurfaceGeometry::Unknown { .. }))) {
            super::geometry_error(findings, owner, crate::rule!("topology.input-edge-incidence-requires-solved-surfaces", "input-edge incidence requires solved surfaces"));
        }
    }
    match (&reference.path, edges.as_slice()) {
        (HistoricalEdgePath::Edge { .. }, [(edge, _)]) => {
            if !edge_samples_agree(ctx, nest, edge, geometry, ir.tolerances.linear.get())? {
                super::geometry_error(
                    findings,
                    owner,
                    crate::rule!(
                        "topology.input-edge-samples-differ-from-the-resolved-carrier-parameters",
                        "input-edge samples differ from the resolved carrier parameters"
                    ),
                );
            }
        }
        _ => {
            for (sample, tangent) in [
                (Some(geometry.interior), geometry.tangent),
                (geometry.start, None),
                (geometry.end, None),
            ] {
                let Some(sample) = sample else { continue };
                let mut agrees = false;
                for (edge, sense) in &edges {
                    if sample_on_edge(
                        ctx,
                        nest,
                        edge,
                        sample.get(),
                        tangent,
                        *sense,
                        ir.tolerances,
                    )? {
                        agrees = true;
                        break;
                    }
                }
                if !agrees {
                    super::geometry_error(
                        findings,
                        owner,
                        crate::rule!(
                            "topology.input-edge-sample-differs-from-the-resolved-carrier-interval",
                            "input-edge sample differs from the resolved carrier interval"
                        ),
                    );
                }
            }
        }
    }
    if let (HistoricalEdgePath::Edge { .. }, Some(tangent), [(edge, _)]) =
        (&reference.path, geometry.tangent, edges.as_slice())
    {
        if !tangent_agrees(
            ctx,
            nest,
            edge,
            tangent,
            None,
            Sense::Forward,
            ir.tolerances.angular.get(),
        )? {
            super::geometry_error(
                findings,
                owner,
                crate::rule!(
                    "topology.input-edge-tangent-differs-from-the-resolved-carrier-direction",
                    "input-edge tangent differs from the resolved carrier direction"
                ),
            );
        }
    }
    Ok(())
}

fn point_in_frame(
    ctx: &DecodeContext<'_>,
    nest: &BodyTopology,
    curve: &crate::geometry::Curve,
    parameter: f64,
) -> Result<Option<crate::features::geometry::FinitePoint3>, CodecError> {
    let Some(solved) = curve.geometry.solved() else {
        return Ok(None);
    };
    let point = match crate::eval::decode::curve_point_solved(ctx, solved, parameter) {
        Ok(point) => point,
        Err(failure) => {
            failure.non_finite()?;
            return Ok(None);
        }
    };
    Ok(match nest.body.transform {
        Some(transform) => transform.apply_point(point.get()),
        None => Some(point),
    })
}

fn closed_carrier_ring(
    ctx: &DecodeContext<'_>,
    nest: &BodyTopology,
    edges: &[(&Edge, Sense)],
    tolerance: f64,
) -> Result<bool, CodecError> {
    let mut first: Option<crate::features::geometry::FinitePoint3> = None;
    let mut previous: Option<crate::features::geometry::FinitePoint3> = None;
    for (edge, sense) in edges {
        let Some(id) = edge.carrier.curve() else {
            return Ok(false);
        };
        let Some(curve) = nest.curves.iter().find(|curve| curve.id == *id) else {
            return Ok(false);
        };
        let Some(range) = edge.carrier.param_range() else {
            return Ok(false);
        };
        let [start, end] = range.get();
        let (start, end) = match sense {
            Sense::Forward => (start, end),
            Sense::Reversed => (end, start),
        };
        let Some(start) = point_in_frame(ctx, nest, curve, start)? else {
            return Ok(false);
        };
        let Some(end) = point_in_frame(ctx, nest, curve, end)? else {
            return Ok(false);
        };
        if previous
            .is_some_and(|previous| previous.get().vector_from(start.get()).norm() > tolerance)
        {
            return Ok(false);
        }
        if first.is_none() {
            first = Some(start);
        }
        previous = Some(end);
    }
    Ok(match (first, previous) {
        (Some(first), Some(last)) => first.get().vector_from(last.get()).norm() <= tolerance,
        _ => false,
    })
}

fn edge_samples_agree(
    ctx: &DecodeContext<'_>,
    nest: &BodyTopology,
    edge: &Edge,
    geometry: &HistoricalEdgeGeometry,
    tolerance: f64,
) -> Result<bool, CodecError> {
    let EdgeCarrier::Bounded(id, interval) = &edge.carrier else {
        return Ok(false);
    };
    let Some(curve) = nest.curves.iter().find(|curve| curve.id == *id) else {
        return Ok(false);
    };
    let [start, end] = interval.endpoints();
    for (sample, parameter) in [
        (Some(geometry.interior), start.midpoint(end)),
        (geometry.start, start),
        (geometry.end, end),
    ] {
        let Some(sample) = sample else { continue };
        let Some(point) = point_in_frame(ctx, nest, curve, parameter)? else {
            return Ok(false);
        };
        if sample.get().vector_from(point.get()).norm() > tolerance {
            return Ok(false);
        }
    }
    Ok(true)
}

fn tangent_agrees(
    ctx: &DecodeContext<'_>,
    nest: &BodyTopology,
    edge: &Edge,
    tangent: crate::units::UnitVector3,
    parameter: Option<f64>,
    sense: Sense,
    tolerance: f64,
) -> Result<bool, CodecError> {
    let EdgeCarrier::Bounded(id, interval) = &edge.carrier else {
        return Ok(false);
    };
    let Some(curve) = nest.curves.iter().find(|curve| curve.id == *id) else {
        return Ok(false);
    };
    let Some(solved) = curve.geometry.solved() else {
        return Ok(false);
    };
    let [start, end] = interval.endpoints();
    let derivative = match crate::eval::curve_tangent_solved(
        ctx,
        solved,
        parameter.unwrap_or(start.midpoint(end)),
    ) {
        Ok(vector) => vector,
        Err(failure) => {
            failure.non_finite()?;
            return Ok(false);
        }
    };
    let derivative = if let Some(transform) = nest.body.transform {
        let Some(vector) = transform.apply_vector(derivative.get()) else {
            return Ok(false);
        };
        vector.get()
    } else {
        derivative.get()
    };
    let derivative = match sense {
        Sense::Forward => derivative,
        Sense::Reversed => crate::math::Vector3::new(-derivative.x, -derivative.y, -derivative.z),
    };
    let Some(direction) = crate::units::UnitVector3::normalized(derivative) else {
        return Ok(false);
    };
    let direction = *direction.as_raw();
    let tangent = *tangent.as_raw();
    Ok(direction
        .cross(tangent)
        .norm()
        .portable_atan2(direction.dot(tangent))
        <= tolerance)
}

fn admitted_curve(nest: &BodyTopology, edge: &Edge) -> bool {
    let EdgeCarrier::Bounded(id, _) = &edge.carrier else {
        return false;
    };
    nest.curves.iter().any(|curve| {
        curve.id == *id
            && matches!(&curve.geometry,
        CurveGeometry::Solved(geometry) if !matches!(geometry, SolvedCurveGeometry::Unknown { .. }))
    })
}

fn sample_on_edge(
    ctx: &DecodeContext<'_>,
    nest: &BodyTopology,
    edge: &Edge,
    sample: Point3,
    tangent: Option<crate::units::UnitVector3>,
    sense: Sense,
    tolerances: crate::units::Tolerances,
) -> Result<bool, CodecError> {
    let EdgeCarrier::Bounded(id, interval) = &edge.carrier else {
        return Ok(false);
    };
    let Some(curve) = nest.curves.iter().find(|curve| curve.id == *id) else {
        return Ok(false);
    };
    let comparison_sample = sample;
    let (sample, local_tolerance) = if let Some(transform) = nest.body.transform {
        let Ok(inverse) = transform.try_inverse_affine() else {
            return Ok(false);
        };
        let Some(point) = inverse.apply_point(sample) else {
            return Ok(false);
        };
        let scale = inverse
            .affine_rows()
            .into_iter()
            .flat_map(|row| row.into_iter().take(3))
            .fold(0.0_f64, f64::portable_hypot);
        (point.get(), (tolerances.linear.get() * scale).min(f64::MAX))
    } else {
        (sample, tolerances.linear.get())
    };
    let [start, end] = interval.endpoints();
    let Some(parameter) = crate::eval::curve_parameter_near_point(
        ctx,
        &curve.geometry,
        sample,
        start.midpoint(end),
        local_tolerance,
    )?
    else {
        return Ok(false);
    };
    let parameter = parameter.get().clamp(start, end);
    let Some(point) = point_in_frame(ctx, nest, curve, parameter)? else {
        return Ok(false);
    };
    if comparison_sample.vector_from(point.get()).norm() > tolerances.linear.get() {
        return Ok(false);
    }
    match tangent {
        Some(tangent) => tangent_agrees(
            ctx,
            nest,
            edge,
            tangent,
            Some(parameter),
            sense,
            tolerances.angular.get(),
        ),
        None => Ok(true),
    }
}

fn edge_on_surface(
    ctx: &DecodeContext<'_>,
    nest: &BodyTopology,
    edge: &Edge,
    surface: &crate::geometry::Surface,
    tolerance: f64,
) -> Result<bool, CodecError> {
    let EdgeCarrier::Bounded(id, interval) = &edge.carrier else {
        return Ok(false);
    };
    let Some(curve) = nest.curves.iter().find(|curve| curve.id == *id) else {
        return Ok(false);
    };
    let Some(solved) = curve.geometry.solved() else {
        return Ok(false);
    };
    let [start, end] = interval.endpoints();
    for parameter in [start, start.midpoint(end), end] {
        let point = match crate::eval::decode::curve_point_solved(ctx, solved, parameter) {
            Ok(point) => point.get(),
            Err(failure) => {
                failure.non_finite()?;
                return Ok(false);
            }
        };
        let uv = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                crate::eval::nurbs_surface_parameter_near_point(ctx, nurbs, point, None)?
            }
            _ => crate::eval::analytic_surface_parameters(&surface.geometry, point),
        };
        let Some(uv) = uv else { return Ok(false) };
        let uv = uv.get();
        let evaluated = match crate::eval::decode::surface_point(ctx, &surface.geometry, uv.u, uv.v)
        {
            Ok(point) => point.get(),
            Err(failure) => {
                failure.non_finite()?;
                return Ok(false);
            }
        };
        let residual = point.vector_from(evaluated);
        let residual = if let Some(transform) = nest.body.transform {
            let Some(residual) = transform.apply_vector(residual) else {
                return Ok(false);
            };
            residual.get()
        } else {
            residual
        };
        if residual.norm() > tolerance {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
