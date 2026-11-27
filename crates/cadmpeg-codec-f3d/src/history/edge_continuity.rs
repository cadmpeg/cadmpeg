// SPDX-License-Identifier: Apache-2.0
//! Tangent continuity of the loop edges of preceding support faces.

use std::collections::BTreeSet;

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::evaluation::HistoricalFaceSurface;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::historical_context::DesignHistoricalFaceSupportContext;

/// Greatest sine of the angle between the unit surface normals of two faces
/// that meet with tangent continuity at an edge.
const EPS_HISTORY_EDGE_TANGENT_NORMAL_SINE: f64 = 1.0e-9;

/// Loop edges of support-context boundaries, by continuity class. An edge in
/// neither list is sharp.
#[derive(Debug, Default, PartialEq)]
pub(super) struct BoundaryEdgeContinuities {
    pub(super) tangent: Vec<i64>,
    pub(super) undetermined: Vec<i64>,
}

/// Classify every coedge edge of every loop of every preceding face boundary
/// in `contexts`, in increasing edge-slot order.
pub(super) fn support_boundary_edge_continuities<'a>(
    contexts: impl IntoIterator<Item = &'a DesignHistoricalFaceSupportContext>,
    topology: &AsmHistoricalTopology,
) -> Result<BoundaryEdgeContinuities, ResourceLimit> {
    let edges = contexts
        .into_iter()
        .flat_map(|context| &context.preceding_face_boundaries)
        .flat_map(|boundary| &boundary.loops)
        .flat_map(|loop_| loop_.boundary.coedges().map(|row| row.edge_slot))
        .collect::<BTreeSet<_>>();
    let mut continuities = BoundaryEdgeContinuities::default();
    for edge in edges {
        match edge_is_tangent(edge, topology)? {
            Some(true) => continuities.tangent.push(edge),
            Some(false) => {}
            None => continuities.undetermined.push(edge),
        }
    }
    Ok(continuities)
}

/// Whether the two faces adjacent to `edge` meet with tangent continuity at
/// the edge's carrier midpoint. The result is absent when the edge has other
/// than two coedges on two distinct faces, when the midpoint or a face carrier
/// is absent, or when a carrier is free-form.
pub(super) fn edge_is_tangent(
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<bool>, ResourceLimit> {
    let Some(normals) = adjacent_face_normals(edge, topology)? else {
        return Ok(None);
    };
    let [first, second] = normals;
    Ok(Some(
        first.cross(second).norm() <= EPS_HISTORY_EDGE_TANGENT_NORMAL_SINE,
    ))
}

fn adjacent_face_normals(
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<[Vector3; 2]>, ResourceLimit> {
    let mut loops = topology
        .coedge_topology
        .iter()
        .filter(|coedge| coedge.edge == edge)
        .map(|coedge| coedge.owner_loop);
    let (Some(first_loop), Some(second_loop), None) = (loops.next(), loops.next(), loops.next())
    else {
        return Ok(None);
    };
    let Some(first_face) = unique(
        topology
            .face_loops
            .iter()
            .filter(|relation| relation.member_refs.contains(&first_loop))
            .map(|relation| relation.owner_ref),
    ) else {
        return Ok(None);
    };
    let Some(second_face) = unique(
        topology
            .face_loops
            .iter()
            .filter(|relation| relation.member_refs.contains(&second_loop))
            .map(|relation| relation.owner_ref),
    ) else {
        return Ok(None);
    };
    if first_face == second_face {
        return Ok(None);
    }
    let Some(point) = unique(
        topology
            .edge_samples
            .iter()
            .filter(|sample| sample.edge == edge)
            .map(|sample| sample.interior),
    ) else {
        return Ok(None);
    };
    let (Some(first), Some(second)) = (
        face_normal(topology, first_face, point)?,
        face_normal(topology, second_face, point)?,
    ) else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

fn face_normal(
    topology: &AsmHistoricalTopology,
    face: i64,
    point: Point3,
) -> Result<Option<Vector3>, ResourceLimit> {
    let Some(surface) = unique(
        topology
            .face_samples
            .iter()
            .filter(|sample| sample.face == face)
            .map(|sample| &sample.surface),
    ) else {
        return Ok(None);
    };
    if matches!(surface, HistoricalFaceSurface::Nurbs(_)) {
        return Ok(None);
    }
    let geometry = surface.to_solved();
    let Some(parameters) = cadmpeg_ir::eval::analytic_surface_parameters_solved(&geometry, point)
    else {
        return Ok(None);
    };
    let parameters = parameters.get();
    let Some(partials) = finite_or_refusal(cadmpeg_ir::eval::surface_partials_solved(
        EvaluationAdmission::Standard,
        &geometry,
        parameters.u,
        parameters.v,
    ))?
    else {
        return Ok(None);
    };
    let partials = partials.into_raw();
    Ok(partials.du.cross(partials.dv).unit())
}

fn unique<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

#[cfg(test)]
mod tests;
