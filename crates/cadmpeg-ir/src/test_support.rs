// SPDX-License-Identifier: Apache-2.0
//! Shared IR fixture builders for `#[cfg(test)]` suites.

use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::ids::UnknownId;

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
pub(crate) fn make_first_face_surface_unknown(
    ir: &mut crate::CadIr,
    record: Option<UnknownId>,
) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.as_str().to_owned();
    for s in &mut ir.model.surfaces {
        if s.id.as_str() == surface_id {
            s.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record });
            break;
        }
    }
    surface_id
}
