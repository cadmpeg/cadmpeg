// SPDX-License-Identifier: Apache-2.0
//! Lookups by ID and the ownership maps between topology entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::geometry::{Curve, ProceduralSurface, Surface};
use cadmpeg_ir::ids::{BodyId, ShellId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    Body, ChildLists, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;

/// Lookups of one document's topology and carriers.
pub(crate) struct Owners<'ir> {
    pub(crate) ir: &'ir CadIr,
    bodies: HashMap<&'ir str, &'ir Body>,
    regions: HashMap<&'ir str, &'ir Region>,
    shells: HashMap<&'ir str, &'ir Shell>,
    faces: HashMap<&'ir str, &'ir Face>,
    loops: HashMap<&'ir str, &'ir Loop>,
    coedges: HashMap<&'ir str, &'ir Coedge>,
    edges: HashMap<&'ir str, &'ir Edge>,
    vertices: HashMap<&'ir str, &'ir Vertex>,
    points: HashMap<&'ir str, &'ir Point>,
    surfaces: HashMap<&'ir str, &'ir Surface>,
    curves: HashMap<&'ir str, &'ir Curve>,
    pcurves: HashMap<&'ir str, &'ir Pcurve>,
    procedural_surfaces: HashMap<&'ir str, &'ir ProceduralSurface>,
    /// Faces of each shell and regions of each body, in arena order.
    children: ChildLists<'ir>,
    /// Coedges of each edge, in coedge arena order.
    edge_coedges: HashMap<&'ir str, Vec<&'ir Coedge>>,
    /// Edges of each body, sorted.
    body_edges: BTreeMap<&'ir str, BTreeSet<&'ir str>>,
    /// The first body in ID order that owns each edge.
    edge_owner: HashMap<&'ir str, &'ir str>,
    /// Vertices of each body, sorted.
    body_vertices: BTreeMap<&'ir str, BTreeSet<&'ir str>>,
    /// Faces of each body, sorted.
    body_faces: BTreeMap<&'ir str, BTreeSet<&'ir str>>,
}

fn by_id<'ir, T>(items: &'ir [T], id: impl Fn(&'ir T) -> &'ir str) -> HashMap<&'ir str, &'ir T> {
    items.iter().map(|item| (id(item), item)).collect()
}

impl<'ir> Owners<'ir> {
    pub(crate) fn new(ir: &'ir CadIr) -> Self {
        let geometry = &ir.geometry;
        let mut owners = Self {
            ir,
            bodies: by_id(&geometry.bodies, |item| item.id.as_str()),
            regions: by_id(&geometry.regions, |item| item.id.as_str()),
            shells: by_id(&geometry.shells, |item| item.id.as_str()),
            faces: by_id(&geometry.faces, |item| item.id.as_str()),
            loops: by_id(&geometry.loops, |item| item.id.as_str()),
            coedges: by_id(&geometry.coedges, |item| item.id.as_str()),
            edges: by_id(&geometry.edges, |item| item.id.as_str()),
            vertices: by_id(&geometry.vertices, |item| item.id.as_str()),
            points: by_id(&geometry.points, |item| item.id.as_str()),
            surfaces: by_id(&geometry.surfaces, |item| item.id.as_str()),
            curves: by_id(&geometry.curves, |item| item.id.as_str()),
            pcurves: by_id(&geometry.pcurves, |item| item.id.as_str()),
            procedural_surfaces: by_id(&geometry.procedural_surfaces, |item| item.id.as_str()),
            children: geometry.child_lists(),
            edge_coedges: HashMap::new(),
            body_edges: BTreeMap::new(),
            edge_owner: HashMap::new(),
            body_vertices: BTreeMap::new(),
            body_faces: BTreeMap::new(),
        };
        for coedge in &geometry.coedges {
            owners
                .edge_coedges
                .entry(coedge.edge.as_str())
                .or_default()
                .push(coedge);
        }
        for body in &geometry.bodies {
            let id = body.id.as_str();
            owners.body_edges.insert(id, BTreeSet::new());
            owners.body_vertices.insert(id, BTreeSet::new());
            owners.body_faces.insert(id, BTreeSet::new());
        }
        for shell in &geometry.shells {
            let Some(body) = owners.shell_body(shell.id.as_str()) else {
                continue;
            };
            let mut faces = BTreeSet::new();
            let mut edges = BTreeSet::new();
            let mut vertices = BTreeSet::new();
            for face in owners.shell_faces(&shell.id) {
                faces.insert(face.id.as_str());
                for loop_id in &face.loops {
                    let Some(face_loop) = owners.loop_(loop_id.as_str()) else {
                        continue;
                    };
                    vertices.extend(face_loop.vertices().map(cadmpeg_ir::ids::VertexId::as_str));
                    for coedge in face_loop.coedges() {
                        let Some(coedge) = owners.coedge(coedge.as_str()) else {
                            continue;
                        };
                        edges.insert(coedge.edge.as_str());
                    }
                }
            }
            edges.extend(shell.wire_edges().map(cadmpeg_ir::ids::EdgeId::as_str));
            vertices.extend(shell.free_vertices().map(cadmpeg_ir::ids::VertexId::as_str));
            for edge in &edges {
                if let Some(edge) = owners.edge(edge) {
                    vertices.extend(edge.vertices().map(cadmpeg_ir::ids::VertexId::as_str));
                }
            }
            if let Some(set) = owners.body_faces.get_mut(body) {
                set.extend(faces);
            }
            if let Some(set) = owners.body_edges.get_mut(body) {
                set.extend(edges);
            }
            if let Some(set) = owners.body_vertices.get_mut(body) {
                set.extend(vertices);
            }
        }
        for (body, edges) in &owners.body_edges {
            for edge in edges {
                owners.edge_owner.entry(*edge).or_insert(*body);
            }
        }
        owners
    }

    pub(crate) fn body(&self, id: &str) -> Option<&'ir Body> {
        self.bodies.get(id).copied()
    }

    pub(crate) fn region(&self, id: &str) -> Option<&'ir Region> {
        self.regions.get(id).copied()
    }

    pub(crate) fn shell(&self, id: &str) -> Option<&'ir Shell> {
        self.shells.get(id).copied()
    }

    pub(crate) fn face(&self, id: &str) -> Option<&'ir Face> {
        self.faces.get(id).copied()
    }

    pub(crate) fn loop_(&self, id: &str) -> Option<&'ir Loop> {
        self.loops.get(id).copied()
    }

    pub(crate) fn coedge(&self, id: &str) -> Option<&'ir Coedge> {
        self.coedges.get(id).copied()
    }

    pub(crate) fn edge(&self, id: &str) -> Option<&'ir Edge> {
        self.edges.get(id).copied()
    }

    pub(crate) fn vertex(&self, id: &str) -> Option<&'ir Vertex> {
        self.vertices.get(id).copied()
    }

    pub(crate) fn surface(&self, id: &str) -> Option<&'ir Surface> {
        self.surfaces.get(id).copied()
    }

    pub(crate) fn curve(&self, id: &str) -> Option<&'ir Curve> {
        self.curves.get(id).copied()
    }

    pub(crate) fn pcurve(&self, id: &str) -> Option<&'ir Pcurve> {
        self.pcurves.get(id).copied()
    }

    pub(crate) fn procedural_surface(&self, id: &str) -> Option<&'ir ProceduralSurface> {
        self.procedural_surfaces.get(id).copied()
    }

    /// The faces whose shell is `shell`, in arena order.
    pub(crate) fn shell_faces(&self, shell: &ShellId) -> &[&'ir Face] {
        self.children.shell_faces(shell)
    }

    /// The regions whose body is `body`, in arena order.
    pub(crate) fn body_regions(&self, body: &BodyId) -> &[&'ir Region] {
        self.children.body_regions(body)
    }

    /// The body-local position of a vertex.
    pub(crate) fn vertex_position(&self, id: &str) -> Option<Point3> {
        let vertex = self.vertex(id)?;
        self.points
            .get(vertex.point.as_str())
            .map(|point| point.position().get())
    }

    /// The body that owns a shell.
    pub(crate) fn shell_body(&self, shell: &str) -> Option<&'ir str> {
        let shell = self.shell(shell)?;
        let region = self.region(shell.region.as_str())?;
        Some(self.body(region.body.as_str())?.id.as_str())
    }

    /// The body that owns a face.
    pub(crate) fn face_body(&self, face: &Face) -> Option<&'ir str> {
        self.shell_body(face.shell.as_str())
    }

    /// The face that owns a coedge.
    pub(crate) fn coedge_face(&self, coedge: &Coedge) -> Option<&'ir str> {
        let owner = self.loop_(coedge.owner_loop.as_str())?;
        Some(self.face(owner.face.as_str())?.id.as_str())
    }

    /// The vertices a coedge starts and ends at, in traversal direction. An
    /// edge with no vertex starts and ends at its own identity.
    pub(crate) fn oriented_ends(&self, coedge: &'ir Coedge) -> (&'ir str, &'ir str) {
        let Some(edge) = self.edge(coedge.edge.as_str()) else {
            return (coedge.id.as_str(), coedge.edge.as_str());
        };
        let Some(ends) = &edge.ends else {
            return (edge.id.as_str(), edge.id.as_str());
        };
        match coedge.sense {
            Sense::Forward => (ends.start.as_str(), ends.end.as_str()),
            Sense::Reversed => (ends.end.as_str(), ends.start.as_str()),
        }
    }

    /// The coedges of an edge.
    pub(crate) fn edge_coedges(&self, edge: &str) -> &[&'ir Coedge] {
        self.edge_coedges.get(edge).map_or(&[], Vec::as_slice)
    }

    /// The faces that use an edge, sorted and distinct.
    pub(crate) fn edge_faces(&self, edge: &str) -> Vec<String> {
        let faces: BTreeSet<&str> = self
            .edge_coedges(edge)
            .iter()
            .filter_map(|coedge| self.coedge_face(coedge))
            .collect();
        faces.into_iter().map(str::to_owned).collect()
    }

    /// The body an edge belongs to, the first in ID order.
    pub(crate) fn edge_body(&self, edge: &str) -> Option<&'ir str> {
        self.edge_owner.get(edge).copied()
    }

    pub(crate) fn body_faces(&self, body: &str) -> impl Iterator<Item = &'ir str> + '_ {
        self.body_faces.get(body).into_iter().flatten().copied()
    }

    pub(crate) fn body_edges(&self, body: &str) -> impl Iterator<Item = &'ir str> + '_ {
        self.body_edges.get(body).into_iter().flatten().copied()
    }

    pub(crate) fn body_vertex_count(&self, body: &str) -> usize {
        self.body_vertices.get(body).map_or(0, BTreeSet::len)
    }

    /// The largest stored tolerance of a body's vertices.
    pub(crate) fn body_vertex_tolerance(&self, body: &str) -> Option<f64> {
        self.body_vertices
            .get(body)
            .into_iter()
            .flatten()
            .filter_map(|vertex| self.vertex(vertex)?.tolerance)
            .map(cadmpeg_ir::scalar::PositiveLength::get)
            .reduce(f64::max)
    }

    /// The body-local positions of a body's vertices, in vertex ID order.
    pub(crate) fn body_vertex_points(&self, body: &str) -> Vec<Point3> {
        self.body_vertices
            .get(body)
            .into_iter()
            .flatten()
            .filter_map(|vertex| self.vertex_position(vertex))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::ids::{BodyId, EdgeId, RegionId, ShellId};
    use cadmpeg_ir::topology::Shell;

    use super::Owners;
    use crate::test_support::builders::cuboid;

    /// An edge that two bodies own belongs to the body with the smaller ID,
    /// also when that body comes later in the arena.
    #[test]
    fn shared_edge_belongs_to_first_body_in_id_order() {
        let mut ir = cuboid([0.0; 3], [1.0; 3]);
        let later = BodyId::mint("test:model:body#0002").expect("body id");
        let first = BodyId::mint("test:model:body#0001").expect("body id");
        ir.geometry.bodies[0].id = later.clone();
        ir.geometry.regions[0].body = later.clone();

        let shell = ShellId::mint("test:model:shell#0001").expect("shell id");
        let region = RegionId::mint("test:model:region#0001").expect("region id");
        let edges: Vec<EdgeId> = ir
            .geometry
            .edges
            .iter()
            .map(|edge| edge.id.clone())
            .collect();
        ir.geometry.shells.push(Shell::with_members(
            shell.clone(),
            region.clone(),
            edges.clone(),
            Vec::new(),
        ));
        let mut first_region = ir.geometry.regions[0].clone();
        first_region.id = region;
        first_region.body = first.clone();
        first_region.shells = vec![shell];
        ir.geometry.regions.push(first_region);
        let mut first_body = ir.geometry.bodies[0].clone();
        first_body.id = first.clone();
        ir.geometry.bodies.push(first_body);

        let owners = Owners::new(&ir);

        assert_eq!(owners.body_edges(later.as_str()).count(), edges.len());
        assert_eq!(owners.body_edges(first.as_str()).count(), edges.len());
        for edge in &edges {
            assert_eq!(owners.edge_body(edge.as_str()), Some(first.as_str()));
        }
        assert_eq!(owners.edge_body("test:model:edge#9999"), None);
    }
}
