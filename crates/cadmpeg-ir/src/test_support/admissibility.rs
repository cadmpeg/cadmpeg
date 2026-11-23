// SPDX-License-Identifier: Apache-2.0
//! Accept/reject fixtures for the full validator and the admission subsets.

use crate::features::geometry::FinitePoint3;
use crate::ids::{IdentityError, PointId, RegionId, ShellId, VertexId};
use crate::math::Point3;
use crate::topology::{Point, Shell, Vertex};
use crate::CadIr;

/// Empty document: accepted by the full neutral validator.
pub(crate) fn accepted_empty() -> CadIr {
    CadIr::empty()
}

/// Vertex → missing point: rejected (`ReferentialIntegrity`).
pub(crate) fn rejected_missing_point(prefix: &str) -> Result<CadIr, IdentityError> {
    let mut ir = CadIr::empty();
    ir.geometry.vertices.push(Vertex {
        id: VertexId::mint(format!("{prefix}:vertex#0"))?,
        point: PointId::mint(format!("{prefix}:point#missing"))?,
        tolerance: None,
        native_ref: None,
    });
    Ok(ir)
}

/// Shell → missing region: rejected (`ReferentialIntegrity` / topology).
pub(crate) fn rejected_missing_region(prefix: &str) -> Result<CadIr, IdentityError> {
    let mut ir = CadIr::empty();
    let point = PointId::mint(format!("{prefix}:point#0"))?;
    let vertex = VertexId::mint(format!("{prefix}:vertex#0"))?;
    let position = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("the origin is finite");
    ir.geometry
        .points
        .push(Point::new(point.clone(), position, None));
    ir.geometry.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.shells.push(Shell::with_free_vertex(
        ShellId::mint(format!("{prefix}:shell#0"))?,
        RegionId::mint(format!("{prefix}:region#missing"))?,
        vertex,
    ));
    Ok(ir)
}
