//! Outward support faces for an unequal two-distance chamfer.
//!
//! Coedge sense and face sense exist on the live brep inside
//! [`super::historical_topology`], then the replayed historical rows drop them.
//! This map keeps that sense, keyed by the history stream and the edge and face
//! slots, until feature-input projection reads it. A sense field on the coedge
//! row would change every transition snapshot.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use cadmpeg_ir::features::edge_treatments::ChamferSpec;
use cadmpeg_ir::features::{EdgeSelection, EdgeSupport, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::units::UnitVector3;

use crate::history_records::{AsmHistoricalCylinder, AsmHistoricalTopology};

/// How strongly `C × F_in` must agree with the cylinder's radial direction.
const OUTWARD_AGREEMENT: f64 = 0.9;
/// A radial direction this close to the XY plane has no Z either way around.
const FLAT_RADIAL_Z: f64 = 1e-6;

#[derive(Clone, Copy)]
struct FaceSense {
    /// The coedge runs with the edge, so the face lies on the left.
    with_edge: bool,
    /// The face normal agrees with the stored surface normal.
    face_forward: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SupportKind {
    Plane,
    Cylinder,
}

type SenseKey = (Arc<str>, i64, i64);

thread_local! {
    static CURRENT: RefCell<Arc<str>> = RefCell::new(Arc::from(""));
    static SENSES: RefCell<HashMap<SenseKey, FaceSense>> = RefCell::new(HashMap::new());
}

/// Drop every recorded sense. The next design decode starts empty.
pub(super) fn clear() {
    CURRENT.with(|current| *current.borrow_mut() = Arc::from(""));
    SENSES.with(|senses| senses.borrow_mut().clear());
}

/// Remember `history` for the senses [`note`] records, and forget it on drop.
#[must_use]
pub(super) struct HistoryScope;

impl HistoryScope {
    pub(super) fn enter(history: &str) -> Self {
        CURRENT.with(|current| *current.borrow_mut() = Arc::from(history));
        Self
    }
}

impl Drop for HistoryScope {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = Arc::from(""));
    }
}

/// Record coedge and face sense from the brep snapshot being diffed.
pub(super) fn note(brep: &cadmpeg_asm::brep::AsmBrep) {
    let history = CURRENT.with(|current| Arc::clone(&current.borrow()));
    if history.is_empty() {
        return;
    }
    let mut face_of_loop = HashMap::new();
    for face in &brep.faces {
        let Some(slot) = super::stable_ref(face.id.as_str()) else {
            continue;
        };
        let face_forward = face.sense == Sense::Forward;
        for loop_ in &face.loops {
            face_of_loop.insert(loop_.as_str(), (slot, face_forward));
        }
    }
    SENSES.with(|senses| {
        let mut senses = senses.borrow_mut();
        for coedge in &brep.coedges {
            let Some(edge) = super::stable_ref(coedge.edge.as_str()) else {
                continue;
            };
            let Some(&(face, face_forward)) = face_of_loop.get(coedge.owner_loop.as_str()) else {
                continue;
            };
            senses.insert(
                (Arc::clone(&history), edge, face),
                FaceSense {
                    with_edge: coedge.sense == Sense::Forward,
                    face_forward,
                },
            );
        }
    });
}

fn lookup(history: &str, edge: i64, face: i64) -> Option<FaceSense> {
    SENSES.with(|senses| {
        senses
            .borrow()
            .get(&(Arc::from(history), edge, face))
            .copied()
    })
}

/// Fill support faces on edges selected by an unequal two-distance chamfer.
pub(super) fn attach(
    feature: &cadmpeg_ir::features::Feature,
    history_id: &str,
    topology: &AsmHistoricalTopology,
    prefix: &cadmpeg_ir::ids::IdentityKey,
    geometry: &mut [cadmpeg_ir::features::HistoricalEdgeGeometry],
) {
    let slots = unequal_edges(feature);
    if slots.is_empty() {
        return;
    }
    for sample in geometry {
        let Some(slot) = trailing_slot(sample.edge.as_str()) else {
            continue;
        };
        if !slots.contains(&slot) {
            continue;
        }
        if let Some(supports) = supports_for(history_id, topology, slot, prefix) {
            sample.supports = supports;
        }
    }
}

fn unequal_edges(feature: &cadmpeg_ir::features::Feature) -> Vec<i64> {
    let groups = match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
        | FeatureDefinition::PostProcess {
            operation: FeatureOperation::Chamfer { groups, .. },
            ..
        } => groups.as_slice(),
        _ => return Vec::new(),
    };
    let mut slots = Vec::new();
    for group in groups {
        let ChamferSpec::TwoDistances { first, second } = group.spec else {
            continue;
        };
        if first.get() == second.get() {
            continue;
        }
        let EdgeSelection::Historical { edges, .. } = &group.edges else {
            continue;
        };
        for edge in edges {
            let Some(slot) = trailing_slot(edge.as_str()) else {
                continue;
            };
            if !slots.contains(&slot) {
                slots.push(slot);
            }
        }
    }
    slots
}

fn trailing_slot(id: &str) -> Option<i64> {
    id.rsplit_once(':')?.1.parse().ok()
}

fn supports_for(
    history_id: &str,
    topology: &AsmHistoricalTopology,
    edge: i64,
    prefix: &cadmpeg_ir::ids::IdentityKey,
) -> Option<Vec<EdgeSupport>> {
    let faces = faces_of(topology, edge);
    if faces.len() != 2 {
        return None;
    }
    let sample = topology
        .edge_samples
        .iter()
        .find(|sample| sample.edge == edge)?;
    let tangent = sample.tangent?.unit()?;
    let mut built = Vec::new();
    for face in faces {
        let sense = lookup(history_id, edge, face)?;
        let (kind, normal) = outward(topology, face, edge, tangent, sample.interior, sense)?;
        built.push((
            kind,
            EdgeSupport {
                face: crate::ids::history_input_face_id(prefix, face),
                normal: UnitVector3::normalized(normal)?,
                with_edge: sense.with_edge,
            },
        ));
    }
    let planes = built
        .iter()
        .filter(|(kind, _)| *kind == SupportKind::Plane)
        .count();
    let cylinders = built.len() - planes;
    if !((planes == 2 && cylinders == 0) || (planes == 1 && cylinders == 1)) {
        return None;
    }
    if built
        .iter()
        .filter(|(_, support)| support.with_edge)
        .count()
        != 1
    {
        return None;
    }
    Some(built.into_iter().map(|(_, support)| support).collect())
}

fn faces_of(topology: &AsmHistoricalTopology, edge: i64) -> Vec<i64> {
    let mut loop_faces = HashMap::new();
    for relation in &topology.face_loops {
        for loop_ in &relation.member_refs {
            loop_faces.insert(*loop_, relation.owner_ref);
        }
    }
    let mut faces = Vec::new();
    for coedge in &topology.coedge_topology {
        if coedge.edge != edge {
            continue;
        }
        let Some(face) = loop_faces.get(&coedge.owner_loop).copied() else {
            continue;
        };
        if !faces.contains(&face) {
            faces.push(face);
        }
    }
    faces
}

fn outward(
    topology: &AsmHistoricalTopology,
    face: i64,
    edge: i64,
    tangent: Vector3,
    interior: Point3,
    sense: FaceSense,
) -> Option<(SupportKind, Vector3)> {
    let surface = topology
        .face_surfaces
        .iter()
        .find(|binding| binding.entity == face)?
        .carrier;
    if let Some(plane) = topology
        .surface_planes
        .iter()
        .find(|plane| plane.surface == surface)
    {
        let mut normal = plane.normal.unit()?;
        if !sense.face_forward {
            normal = normal.scale(-1.0);
        }
        return Some((SupportKind::Plane, normal));
    }
    let cylinder = topology
        .surface_cylinders
        .iter()
        .find(|cylinder| cylinder.surface == surface)?;
    let normal = cylinder_outward(topology, cylinder, face, edge, tangent, interior, sense)?;
    Some((SupportKind::Cylinder, normal))
}

fn cylinder_outward(
    topology: &AsmHistoricalTopology,
    cylinder: &AsmHistoricalCylinder,
    face: i64,
    edge: i64,
    tangent: Vector3,
    interior: Point3,
    sense: FaceSense,
) -> Option<Vector3> {
    let axis = cylinder.axis.unit()?;
    let radial = radial_unit(interior, cylinder.origin, axis)?;
    let along = if sense.with_edge {
        tangent
    } else {
        tangent.scale(-1.0)
    };
    if let Some(into) = face_inward(topology, face, edge, interior, radial) {
        if let Some(normal) = along.cross(into).unit() {
            let score = normal.dot(radial);
            if score.abs() > OUTWARD_AGREEMENT {
                return Some(if score > 0.0 {
                    radial
                } else {
                    radial.scale(-1.0)
                });
            }
        }
    }
    // A wall whose radial direction has no Z cannot win or lose the up-most
    // test, so the sign of the normal does not change the chamfer.
    (radial.z.abs() <= FLAT_RADIAL_Z).then_some(radial)
}

fn radial_unit(point: Point3, origin: Point3, axis: Vector3) -> Option<Vector3> {
    let delta = point.vector_from(origin);
    let along = delta.dot(axis);
    Vector3::new(
        delta.x - along * axis.x,
        delta.y - along * axis.y,
        delta.z - along * axis.z,
    )
    .unit()
}

fn face_inward(
    topology: &AsmHistoricalTopology,
    face: i64,
    edge: i64,
    interior: Point3,
    plane_normal: Vector3,
) -> Option<Vector3> {
    let mut sum = Vector3::new(0.0, 0.0, 0.0);
    let mut count = 0.0;
    for vertex in other_vertices(topology, face, edge) {
        let Some(position) = vertex_position(topology, vertex) else {
            continue;
        };
        let delta = position.vector_from(interior);
        let along = delta.dot(plane_normal);
        sum = sum
            + Vector3::new(
                delta.x - along * plane_normal.x,
                delta.y - along * plane_normal.y,
                delta.z - along * plane_normal.z,
            );
        count += 1.0;
    }
    if count == 0.0 {
        return None;
    }
    sum.scale(1.0 / count).unit()
}

fn other_vertices(topology: &AsmHistoricalTopology, face: i64, skip_edge: i64) -> Vec<i64> {
    let mut vertices = Vec::new();
    let Some(loops) = topology
        .face_loops
        .iter()
        .find(|relation| relation.owner_ref == face)
    else {
        return vertices;
    };
    for &loop_ in &loops.member_refs {
        let Some(coedges) = topology
            .loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == loop_)
        else {
            continue;
        };
        for &coedge in &coedges.member_refs {
            let Some(row) = topology
                .coedge_topology
                .iter()
                .find(|row| row.coedge == coedge)
            else {
                continue;
            };
            if row.edge == skip_edge {
                continue;
            }
            let Some(ends) = topology
                .edge_vertices
                .iter()
                .find(|item| item.edge == row.edge)
            else {
                continue;
            };
            if !vertices.contains(&ends.start_vertex) {
                vertices.push(ends.start_vertex);
            }
            if !vertices.contains(&ends.end_vertex) {
                vertices.push(ends.end_vertex);
            }
        }
    }
    vertices
}

fn vertex_position(topology: &AsmHistoricalTopology, vertex: i64) -> Option<Point3> {
    let point = topology
        .vertex_points
        .iter()
        .find(|binding| binding.entity == vertex)?
        .carrier;
    topology
        .point_positions
        .iter()
        .find(|item| item.point == point)
        .map(|item| item.position)
}
