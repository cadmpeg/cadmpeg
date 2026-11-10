// SPDX-License-Identifier: Apache-2.0
//! CADIR entity construction.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    derive_reference_direction, Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, LoopId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Color, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary,
    LoopRing, Point, Region, Sense, Shell, Vertex,
};

use super::geom::{norm, sub, Slot, NONE, V3};
use super::loops::Loops;
use super::mesh::Mesh;
use super::regions::Regions;
use super::runs::RunSet;
use super::MeshBrepError;

macro_rules! mesh_id {
    ($type:ident, $kind:literal, $key:expr) => {
        $type::compose(
            &cadmpeg_ir::identity_namespace!("edrawings", "model", $kind),
            $key,
        )
    };
}

/// Naming inputs of the emitted body.
pub(super) struct Naming<'a> {
    pub(super) scope: &'a IdentityKey,
    pub(super) name: Option<&'a str>,
    pub(super) color: Option<Color>,
    pub(super) closed: bool,
}

/// Entities of one body, not yet added to a model.
#[derive(Default)]
pub(super) struct Entities {
    pub(super) points: Vec<Point>,
    pub(super) vertices: Vec<Vertex>,
    pub(super) curves: Vec<Curve>,
    pub(super) edges: Vec<Edge>,
    pub(super) surfaces: Vec<Surface>,
    pub(super) coedges: Vec<Coedge>,
    pub(super) loops: Vec<Loop>,
    pub(super) faces: Vec<Face>,
    pub(super) shells: Vec<Shell>,
    pub(super) regions: Vec<Region>,
    pub(super) bodies: Vec<Body>,
}

fn model_error(message: impl std::fmt::Display) -> MeshBrepError {
    MeshBrepError::Model(message.to_string())
}

fn vector(v: V3) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

fn point(v: V3) -> Point3 {
    Point3::new(v[0], v[1], v[2])
}

/// Shell structure of the mesh: the component of every triangle and, for a
/// void component, the component that holds it.
pub(super) struct Shells<'a> {
    pub(super) component_of: &'a [usize],
    pub(super) void_parent: &'a [Option<usize>],
}

/// Build the entities of one body.
pub(super) fn emit(
    mesh: &Mesh,
    regions: &Regions,
    loops: &Loops,
    run_set: &RunSet,
    shells: &Shells<'_>,
    naming: &Naming<'_>,
) -> Result<Entities, MeshBrepError> {
    let scope = naming.scope;
    let key = |index: usize| scope.clone().colon(index);
    let mut out = Entities::default();

    // Vertices used by at least one edge, in welded order.
    let mut used = vec![false; mesh.pos.len()];
    for run in run_set.runs.iter().filter(|run| run.live) {
        *used.slot_mut(run.start)? = true;
        *used.slot_mut(run.end)? = true;
    }
    for (index, &flag) in used.iter().enumerate() {
        if !flag {
            continue;
        }
        let position = FinitePoint3::new(point(*mesh.pos.slot(index)?))
            .ok_or_else(|| model_error(Point::NON_FINITE_POSITION))?;
        out.points.push(Point::new(
            mesh_id!(PointId, "point", key(index)),
            position,
            None,
        ));
        out.vertices.push(Vertex {
            id: mesh_id!(VertexId, "vertex", key(index)),
            point: mesh_id!(PointId, "point", key(index)),
            tolerance: None,
        });
    }

    // Edges: one per run pair, numbered by the lower run.
    let mut edge_of_run = vec![NONE; run_set.runs.len()];
    let mut edge_count = 0_usize;
    for (index, run) in run_set.runs.iter().enumerate() {
        if !run.live || run.partner < index {
            continue;
        }
        let from = *mesh.pos.slot(run.start)?;
        let to = *mesh.pos.slot(run.end)?;
        let delta = sub(to, from);
        let length = norm(delta);
        let unit = [delta[0] / length, delta[1] / length, delta[2] / length];
        let line = LineCurve::try_new(point(from), vector(unit)).map_err(model_error)?;
        out.curves.push(Curve {
            parameter_range: None,
            id: mesh_id!(CurveId, "curve", key(edge_count)),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(line)),
            source_object: None,
        });
        out.edges.push(Edge {
            id: mesh_id!(EdgeId, "edge", key(edge_count)),
            carrier: EdgeCarrier::new(
                Some(mesh_id!(CurveId, "curve", key(edge_count))),
                Some([0.0, length]),
            )
            .map_err(model_error)?,
            start: mesh_id!(VertexId, "vertex", key(run.start)),
            end: mesh_id!(VertexId, "vertex", key(run.end)),
            tolerance: None,
        });
        *edge_of_run.slot_mut(index)? = edge_count;
        *edge_of_run.slot_mut(run.partner)? = edge_count;
        edge_count += 1;
    }

    // Faces, loops, coedges. Regions without a boundary cannot occur for a
    // finite planar patch and are skipped.
    let mut face_regions = Vec::new();
    for (region_index, region_rings) in run_set.rings.iter().enumerate() {
        if region_rings.is_empty() {
            continue;
        }
        face_regions.push(region_index);
    }
    let mut loop_count = 0_usize;
    let mut first_triangle = vec![NONE; regions.list.len()];
    for (t, &region) in regions.region_of.iter().enumerate().rev() {
        *first_triangle.slot_mut(region)? = t;
    }
    let mut face_component: Vec<usize> = Vec::new();
    for (face_index, &region_index) in face_regions.iter().enumerate() {
        let region = regions.list.slot(region_index)?;
        let region_rings = run_set.rings.slot(region_index)?;
        let region_loops = loops.regions.slot(region_index)?;
        let surface_id = mesh_id!(SurfaceId, "surface", key(face_index));
        let frame_normal = vector(region.normal);
        let plane = PlaneSurface::try_new(
            point(region.origin),
            frame_normal,
            derive_reference_direction(frame_normal),
        )
        .map_err(model_error)?;
        out.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)),
            source_object: None,
        });
        let face_id = mesh_id!(FaceId, "face", key(face_index));
        let mut loop_ids = Vec::with_capacity(region_rings.len());
        for ring in region_rings {
            let loop_id = mesh_id!(LoopId, "loop", key(loop_count));
            loop_count += 1;
            let mut coedge_ids = Vec::with_capacity(ring.len());
            for &run_index in ring {
                let run = run_set.runs.slot(run_index)?;
                let forward = run.partner >= run_index;
                let coedge_id = mesh_id!(CoedgeId, "coedge", key(run_index));
                out.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: mesh_id!(EdgeId, "edge", key(*edge_of_run.slot(run_index)?)),
                    radial_next: mesh_id!(CoedgeId, "coedge", key(run.partner)),
                    sense: if forward {
                        Sense::Forward
                    } else {
                        Sense::Reversed
                    },
                    pcurves: Vec::new(),
                    use_curve: None,
                });
                coedge_ids.push(coedge_id);
            }
            out.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary: LoopBoundary::Ring(
                    LoopRing::new(coedge_ids, Vec::new()).map_err(model_error)?,
                ),
            });
            loop_ids.push(loop_id);
        }
        let loops_of_face = match region_loops.outer {
            Some(outer) => {
                let outer_id = loop_ids.slot(outer)?.clone();
                let inner = loop_ids
                    .iter()
                    .enumerate()
                    .filter(|&(position, _)| position != outer)
                    .map(|(_, id)| id.clone())
                    .collect();
                FaceLoops::classified(outer_id, inner)
            }
            None => FaceLoops::unspecified(loop_ids),
        };
        out.faces.push(Face {
            id: face_id,
            shell: mesh_id!(ShellId, "shell", key(0)),
            surface: surface_id,
            sense: Sense::Forward,
            loops: loops_of_face,
            name: None,
            color: naming.color,
            tolerance: None,
        });
        face_component.push(
            *shells
                .component_of
                .slot(*first_triangle.slot(region_index)?)?,
        );
    }

    // Shells: the faces of one component form one shell. A component with no
    // void parent bounds its own region; a void joins the region of its parent.
    let mut shell_of_component = vec![NONE; shells.void_parent.len()];
    let mut shell_faces: Vec<Vec<usize>> = Vec::new();
    let mut shell_component: Vec<usize> = Vec::new();
    for (face_index, &component) in face_component.iter().enumerate() {
        let shell = shell_of_component.slot_mut(component)?;
        if *shell == NONE {
            *shell = shell_faces.len();
            shell_faces.push(Vec::new());
            shell_component.push(component);
        }
        let shell_index = *shell;
        shell_faces.slot_mut(shell_index)?.push(face_index);
        out.faces.slot_mut(face_index)?.shell = mesh_id!(ShellId, "shell", key(shell_index));
    }
    let parent_shell = |shell: usize| -> Result<Option<usize>, MeshBrepError> {
        let component = *shell_component.slot(shell)?;
        let parent = shells.void_parent.get(component).copied().flatten();
        Ok(parent
            .and_then(|p| shell_of_component.get(p).copied())
            .filter(|&p| p != NONE))
    };
    let mut region_of_shell = vec![NONE; shell_faces.len()];
    let mut region_shells: Vec<Vec<usize>> = Vec::new();
    for shell_index in 0..shell_faces.len() {
        if parent_shell(shell_index)?.is_none() {
            *region_of_shell.slot_mut(shell_index)? = region_shells.len();
            region_shells.push(vec![shell_index]);
        }
    }
    for shell_index in 0..shell_faces.len() {
        if let Some(parent) = parent_shell(shell_index)? {
            let region = *region_of_shell.slot(parent)?;
            *region_of_shell.slot_mut(shell_index)? = region;
            region_shells.slot_mut(region)?.push(shell_index);
        }
    }
    let body_id = mesh_id!(BodyId, "body", scope.clone());
    let mut region_ids = Vec::with_capacity(region_shells.len());
    for (shell_index, faces) in shell_faces.iter().enumerate() {
        let region_id = mesh_id!(RegionId, "region", key(*region_of_shell.slot(shell_index)?));
        let mut face_ids = Vec::with_capacity(faces.len());
        for &face_index in faces {
            face_ids.push(out.faces.slot(face_index)?.id.clone());
        }
        out.shells.push(
            Shell::new(
                mesh_id!(ShellId, "shell", key(shell_index)),
                region_id,
                face_ids,
                Vec::new(),
                Vec::new(),
            )
            .map_err(model_error)?,
        );
    }
    for (region_index, members) in region_shells.iter().enumerate() {
        let region_id = mesh_id!(RegionId, "region", key(region_index));
        out.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: members
                .iter()
                .map(|&shell| mesh_id!(ShellId, "shell", key(shell)))
                .collect(),
        });
        region_ids.push(region_id);
    }
    out.bodies.push(Body {
        id: body_id,
        kind: if naming.closed {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        },
        regions: region_ids,
        transform: None,
        name: naming.name.map(str::to_owned),
        color: naming.color,
        visible: None,
    });
    Ok(out)
}
