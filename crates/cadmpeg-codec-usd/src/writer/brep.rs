// SPDX-License-Identifier: Apache-2.0
//! The mesh of a B-rep body with no tessellation of its own.

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh};
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::CadIr;

use cadmpeg_tessellate::Tessellator;

/// Mesh chord as a share of a body's vertex box diagonal.
const CHORD_RATIO: f64 = 2e-3;
/// Smallest mesh chord, in model tolerances.
const MIN_CHORD_TOLERANCES: f64 = 10.0;
/// Largest normal turn across one mesh triangle, in radians.
const MESH_ANGLE: f64 = 0.3;

/// The tessellation limits of a body: a chord of a fixed share of its
/// vertex box diagonal, and pcurves to half the model tolerance.
pub(super) fn mesh_options(ir: &CadIr, body: &Body) -> cadmpeg_tessellate::Options {
    let model = &ir.model;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut grow = |position: cadmpeg_ir::math::Point3| {
        for (axis, value) in [position.x, position.y, position.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    };
    let faces = model
        .shells
        .iter()
        .filter(|shell| body.regions.contains(&shell.region))
        .flat_map(|shell| shell.faces().iter())
        .filter_map(|face| model.faces.iter().find(|candidate| &candidate.id == face));
    for face in faces {
        // A face with no edges spans its surface's control net.
        if let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .and_then(|surface| surface.geometry.solved())
        {
            for pole in nurbs.poles() {
                grow(pole.get());
            }
        }
        let edges = face
            .loops
            .iter()
            .filter_map(|loop_id| model.loops.iter().find(|loop_| &loop_.id == loop_id))
            .flat_map(|loop_| loop_.coedges().iter())
            .filter_map(|coedge| {
                model
                    .coedges
                    .iter()
                    .find(|candidate| &candidate.id == coedge)
            })
            .filter_map(|coedge| model.edges.iter().find(|edge| edge.id == coedge.edge));
        for vertex in edges.flat_map(|edge| [&edge.start, &edge.end]) {
            if let Some(point) = model
                .vertices
                .iter()
                .find(|candidate| &candidate.id == vertex)
                .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            {
                grow(point.position().get());
            }
        }
    }
    let diagonal = (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f64>()
        .sqrt();
    let tolerance = ir.tolerances.linear.get();
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
