// SPDX-License-Identifier: Apache-2.0
//! The mesh of a B-rep body with no tessellation of its own.

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh};
use cadmpeg_ir::topology::Body;

use cadmpeg_tessellate::Tessellator;

/// Mesh chord as a share of a body's box diagonal.
const CHORD_RATIO: f64 = 2e-3;
/// Smallest mesh chord, in model tolerances.
const MIN_CHORD_TOLERANCES: f64 = 10.0;
/// Largest normal turn across one mesh triangle, in radians.
const MESH_ANGLE: f64 = 0.3;

/// Expands `grow` by the axis-aligned box of an untrimmed analytic sphere or
/// the control net of a NURBS face. A complete sphere has no vertices.
fn grow_carrier(geometry: &SolvedSurfaceGeometry, grow: &mut impl FnMut(Point3)) {
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            for pole in nurbs.poles() {
                grow(pole.get());
            }
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let center = sphere.center().get();
            let radius = sphere.radius().get().abs();
            grow(Point3::new(
                center.x - radius,
                center.y - radius,
                center.z - radius,
            ));
            grow(Point3::new(
                center.x + radius,
                center.y + radius,
                center.z + radius,
            ));
        }
        _ => {}
    }
}

/// The tessellation limits of a body: a chord of a fixed share of its
/// vertex-and-carrier box diagonal, and pcurves to half the model tolerance.
pub(super) fn mesh_options(index: &ModelIndex<'_>, body: &Body) -> cadmpeg_tessellate::Options {
    let query = StandardIndex;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut grow = |position: Point3| {
        for (axis, value) in [position.x, position.y, position.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    };
    let faces = body
        .regions
        .iter()
        .filter_map(|region| index.regions(region.as_str(), query))
        .flat_map(|region| region.shells.iter())
        .filter_map(|shell| index.shells(shell.as_str(), query))
        .flat_map(|shell| shell.faces().iter())
        .filter_map(|face| index.faces(face.as_str(), query));
    for face in faces {
        if let Some(geometry) = index
            .surfaces(face.surface.as_str(), query)
            .and_then(|surface| surface.geometry.solved())
        {
            grow_carrier(geometry, &mut grow);
        }
        let edges = face
            .loops
            .iter()
            .filter_map(|loop_id| index.loops(loop_id.as_str(), query))
            .flat_map(|loop_| loop_.coedges().iter())
            .filter_map(|coedge| index.coedges(coedge.as_str(), query))
            .filter_map(|coedge| index.edges(coedge.edge.as_str(), query));
        for vertex in edges.flat_map(|edge| [&edge.start, &edge.end]) {
            if let Some(point) = index
                .vertices(vertex.as_str(), query)
                .and_then(|vertex| index.points(vertex.point.as_str(), query))
            {
                grow(point.position().get());
            }
        }
    }
    let diagonal = (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f64>()
        .sqrt();
    let tolerance = index.ir().tolerances.linear.get();
    let chord = if diagonal.is_finite() && diagonal > 0.0 {
        (diagonal * CHORD_RATIO).max(tolerance * MIN_CHORD_TOLERANCES)
    } else {
        cadmpeg_tessellate::Options::default().chord
    };
    cadmpeg_tessellate::Options {
        chord,
        angle: MESH_ANGLE,
        pcurve_tolerance: Some(tolerance / 2.0),
        ..cadmpeg_tessellate::Options::default()
    }
}

/// The mesh of `faces` of `body`, in order; each face keeps its own points.
/// Faces with no mesh are named with the reason.
pub(super) fn mesh(
    body: &Body,
    faces: &[&FaceId],
    tessellator: &mut Tessellator<'_>,
) -> (Option<Tessellation>, Vec<String>) {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut triangles = Vec::new();
    let mut unmeshed = Vec::new();
    for face in faces {
        let mesh = match tessellator.face(face) {
            Ok(mesh) => mesh,
            Err(refusal) => {
                unmeshed.push(format!("{face}: {refusal}"));
                continue;
            }
        };
        // Points in the order the triangles first reach them.
        let mut renumbered = std::collections::BTreeMap::new();
        for corners in &mesh.triangles {
            let mut triangle = [0_u32; 3];
            for (slot, corner) in triangle.iter_mut().zip(corners) {
                let next = positions.len();
                let index = *renumbered.entry(*corner).or_insert(next);
                if index == next {
                    positions.push(mesh.positions[*corner]);
                    normals.push(mesh.normals[*corner]);
                }
                let Ok(index) = u32::try_from(index) else {
                    unmeshed.push(format!("{face}: the mesh exceeds the u32 index range"));
                    return (None, unmeshed);
                };
                *slot = index;
            }
            triangles.push(triangle);
        }
    }
    if triangles.is_empty() {
        return (None, unmeshed);
    }
    let tessellation = TessellationMesh::from_list_lanes(positions, triangles, Some(normals))
        .map_err(|error| error.to_string())
        .and_then(|lanes| {
            Tessellation::new(
                crate::ids::tessellation(body.id.as_str()),
                lanes,
                Vec::new(),
            )
            .map_err(|error| error.to_string())
        });
    match tessellation {
        Ok(tessellation) => (
            Some(tessellation.with_body(Some(body.id.clone()))),
            unmeshed,
        ),
        Err(reason) => {
            unmeshed.push(format!("{}: {reason}", body.id));
            (None, unmeshed)
        }
    }
}

#[cfg(test)]
mod tests;
