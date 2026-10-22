// SPDX-License-Identifier: Apache-2.0
//! Lookups by ID and the ownership maps between topology entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::geometry::{Curve, ProceduralSurface, Surface};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
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
    /// Coedges of each edge, in coedge arena order.
    edge_coedges: HashMap<&'ir str, Vec<&'ir Coedge>>,
    /// Edges of each body, sorted.
    body_edges: BTreeMap<&'ir str, BTreeSet<&'ir str>>,
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
        let model = &ir.model;
        let mut owners = Self {
            ir,
            bodies: by_id(&model.bodies, |item| item.id.as_str()),
            regions: by_id(&model.regions, |item| item.id.as_str()),
            shells: by_id(&model.shells, |item| item.id.as_str()),
            faces: by_id(&model.faces, |item| item.id.as_str()),
            loops: by_id(&model.loops, |item| item.id.as_str()),
            coedges: by_id(&model.coedges, |item| item.id.as_str()),
            edges: by_id(&model.edges, |item| item.id.as_str()),
            vertices: by_id(&model.vertices, |item| item.id.as_str()),
            points: by_id(&model.points, |item| item.id.as_str()),
            surfaces: by_id(&model.surfaces, |item| item.id.as_str()),
            curves: by_id(&model.curves, |item| item.id.as_str()),
            pcurves: by_id(&model.pcurves, |item| item.id.as_str()),
            procedural_surfaces: by_id(&model.procedural_surfaces, |item| item.id.as_str()),
            edge_coedges: HashMap::new(),
            body_edges: BTreeMap::new(),
            body_vertices: BTreeMap::new(),
            body_faces: BTreeMap::new(),
        };
        for coedge in &model.coedges {
            owners
                .edge_coedges
                .entry(coedge.edge.as_str())
                .or_default()
                .push(coedge);
        }
        for body in &model.bodies {
            let id = body.id.as_str();
            owners.body_edges.insert(id, BTreeSet::new());
            owners.body_vertices.insert(id, BTreeSet::new());
            owners.body_faces.insert(id, BTreeSet::new());
        }
        for shell in &model.shells {
            let Some(body) = owners.shell_body(shell.id.as_str()) else {
                continue;
            };
            let mut faces = BTreeSet::new();
            let mut edges = BTreeSet::new();
            let mut vertices = BTreeSet::new();
            for face in shell.faces() {
                faces.insert(face.as_str());
                let Some(face) = owners.face(face.as_str()) else {
                    continue;
                };
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
            edges.extend(
                shell
                    .wire_edges()
                    .iter()
                    .map(cadmpeg_ir::ids::EdgeId::as_str),
            );
            vertices.extend(
                shell
                    .free_vertices()
                    .iter()
                    .map(cadmpeg_ir::ids::VertexId::as_str),
            );
            for edge in &edges {
                if let Some(edge) = owners.edge(edge) {
                    vertices.insert(edge.start.as_str());
                    vertices.insert(edge.end.as_str());
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
        self.body_edges
            .iter()
            .find(|(_, edges)| edges.contains(edge))
            .map(|(body, _)| *body)
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
