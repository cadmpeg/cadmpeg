// SPDX-License-Identifier: Apache-2.0
//! The neutral topology and carriers of an intrinsic primitive's B-rep
//! records (spec §11.5).
//!
//! Lengths are scaled to millimetres. Each carrier keeps its record's
//! parameterization, so a line parameter maps to the IR parameter by the
//! length scale and an angle maps unchanged.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, LineCurve, PlaneSurface, SphereSurface,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, FaceId, LoopId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point,
    Region, Sense, Shell, Vertex,
};

use crate::brep::{Brep, CurveKind, Frame, SurfaceKind};
use crate::ids::brep as id;

/// The catalog name in the identities of the carriers.
const CATALOG: &str = "analytic";

/// The entities of one B-rep, in arena order.
#[derive(Debug, Default)]
pub(crate) struct Imported {
    pub(crate) points: Vec<Point>,
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) curves: Vec<Curve>,
    pub(crate) surfaces: Vec<Surface>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) coedges: Vec<Coedge>,
    pub(crate) loops: Vec<Loop>,
    pub(crate) faces: Vec<Face>,
    pub(crate) shells: Vec<Shell>,
    pub(crate) regions: Vec<Region>,
    pub(crate) kind: Option<BodyKind>,
}

/// Why a B-rep has no neutral form.
type Refusal = String;

fn vector(values: [f64; 3]) -> Vector3 {
    Vector3::new(values[0], values[1], values[2])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(values: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(values, values).sqrt();
    (length.is_finite() && length > 0.0).then(|| values.map(|value| value / length))
}

/// A frame's origin, unit axis, and unit reference direction, with the
/// reference direction made orthogonal to the axis.
fn orthonormal(frame: &Frame, scale: f64) -> Result<(Point3, Vector3, Vector3), Refusal> {
    let axis = unit(frame.axis).ok_or("a frame axis is zero or not finite")?;
    let along = dot(frame.ref_direction, axis);
    let reference = unit([
        frame.ref_direction[0] - along * axis[0],
        frame.ref_direction[1] - along * axis[1],
        frame.ref_direction[2] - along * axis[2],
    ])
    .ok_or("a frame reference direction is parallel to its axis")?;
    let [x, y, z] = frame.position.map(|value| value * scale);
    Ok((Point3::new(x, y, z), vector(axis), vector(reference)))
}

fn finite_point(point: [f64; 3], scale: f64) -> Result<FinitePoint3, Refusal> {
    FinitePoint3::new(Point3::new(
        point[0] * scale,
        point[1] * scale,
        point[2] * scale,
    ))
    .ok_or_else(|| "a position is not finite".to_owned())
}

fn curve_geometry(brep: &Brep, index: usize, scale: f64) -> Result<SolvedCurveGeometry, Refusal> {
    let record = &brep.curves[index];
    let param = |position: usize| record.params.get(position).copied().unwrap_or_default();
    let geometry = match record.kind {
        CurveKind::Line => {
            let axis = unit(record.frame.axis).ok_or("a line direction is zero")?;
            let [x, y, z] = record.frame.position.map(|value| value * scale);
            LineCurve::try_new(Point3::new(x, y, z), vector(axis)).map(SolvedCurveGeometry::Line)
        }
        CurveKind::Circle => {
            let (origin, axis, reference) = orthonormal(&record.frame, scale)?;
            CircleCurve::try_new(origin, axis, reference, param(0) * scale)
                .map(SolvedCurveGeometry::Circle)
        }
    };
    geometry.map_err(|error| format!("analytic curve {index}: {error}"))
}

fn surface_geometry(
    brep: &Brep,
    index: usize,
    scale: f64,
) -> Result<SolvedSurfaceGeometry, Refusal> {
    let record = &brep.surfaces[index];
    let param = |position: usize| record.params.get(position).copied().unwrap_or_default();
    let (origin, axis, reference) = orthonormal(&record.frame, scale)?;
    match record.kind {
        SurfaceKind::Plane => {
            PlaneSurface::try_new(origin, axis, reference).map(SolvedSurfaceGeometry::Plane)
        }
        SurfaceKind::Cylinder => {
            CylinderSurface::try_new(origin, axis, reference, param(0) * scale)
                .map(SolvedSurfaceGeometry::Cylinder)
        }
        SurfaceKind::Cone => {
            ConeSurface::try_new(origin, axis, reference, param(0) * scale, 1.0, param(1))
                .map(SolvedSurfaceGeometry::Cone)
        }
        SurfaceKind::Sphere => SphereSurface::try_new(origin, axis, reference, param(0) * scale)
            .map(SolvedSurfaceGeometry::Sphere),
    }
    .map_err(|error| format!("analytic surface {index}: {error}"))
}

/// Imports the B-rep of definition `key`, its lengths multiplied by `scale`.
pub(super) fn import(brep: &Brep, key: &str, scale: f64) -> Result<Imported, Refusal> {
    let mut out = Imported::default();

    // Ownership: each loop in one face, each oriented edge in one loop, each
    // face in one shell, each shell in at most one solid.
    let mut loop_face = vec![None; brep.loops.len()];
    for (face, record) in brep.faces.iter().enumerate() {
        for loop_ in &record.loops {
            if loop_face[*loop_].replace(face).is_some() {
                return Err(format!("loop {loop_} bounds more than one face"));
            }
        }
    }
    let mut oriented_loop = vec![None; brep.oriented_edges.len()];
    for (loop_, record) in brep.loops.iter().enumerate() {
        for oriented in &record.oriented_edges {
            if oriented_loop[*oriented].replace(loop_).is_some() {
                return Err(format!("oriented edge {oriented} is in more than one loop"));
            }
        }
    }
    let mut face_shell = vec![None; brep.faces.len()];
    for (shell, record) in brep.shells.iter().enumerate() {
        for face in &record.faces {
            if face_shell[*face].replace(shell).is_some() {
                return Err(format!("face {face} is in more than one shell"));
            }
        }
    }
    if let Some(face) = face_shell.iter().position(Option::is_none) {
        return Err(format!("face {face} is in no shell"));
    }
    let mut shell_region = vec![None; brep.shells.len()];
    let mut regions: Vec<Vec<usize>> = Vec::new();
    for solid in &brep.solids {
        let region = regions.len();
        let mut shells = vec![solid.outer];
        shells.extend(solid.voids.iter().copied());
        for shell in &shells {
            if shell_region[*shell].replace(region).is_some() {
                return Err(format!("shell {shell} bounds more than one solid"));
            }
        }
        regions.push(shells);
    }
    let solids = regions.len();
    for (shell, region) in shell_region.iter_mut().enumerate() {
        if region.is_none() {
            *region = Some(regions.len());
            regions.push(vec![shell]);
        }
    }
    out.kind = Some(match (solids, regions.len() - solids) {
        (0, _) => BodyKind::Sheet,
        (_, 0) => BodyKind::Solid,
        _ => BodyKind::General,
    });

    // Carriers that the topology reaches.
    let used_edges: BTreeSet<usize> = brep
        .oriented_edges
        .iter()
        .map(|oriented| oriented.edge)
        .collect();
    let curve_refs: BTreeSet<usize> = used_edges
        .iter()
        .map(|edge| brep.edges[*edge].curve)
        .collect();
    let surface_refs: BTreeSet<usize> = brep.faces.iter().map(|face| face.surface).collect();
    let mut curve_ids: BTreeMap<usize, CurveId> = BTreeMap::new();
    for index in curve_refs {
        let curve_id = id::curve(key, CATALOG, index);
        out.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(curve_geometry(brep, index, scale)?),
            parameter_range: None,
            source_object: None,
        });
        curve_ids.insert(index, curve_id);
    }
    let mut surface_ids: BTreeMap<usize, SurfaceId> = BTreeMap::new();
    for index in surface_refs {
        let surface_id = id::surface(key, CATALOG, index);
        out.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(surface_geometry(brep, index, scale)?),
            source_object: None,
        });
        surface_ids.insert(index, surface_id);
    }

    // Vertices that edges reach.
    let used_vertices: BTreeSet<usize> = used_edges
        .iter()
        .flat_map(|edge| brep.edges[*edge].vertices)
        .collect();
    for vertex in &used_vertices {
        let point = id::point(key, *vertex);
        out.points.push(Point::new(
            point.clone(),
            finite_point(brep.vertices[*vertex], scale)?,
            None,
        ));
        out.vertices.push(Vertex {
            id: id::vertex(key, *vertex),
            point,
            tolerance: None,
        });
    }

    // Edges. The IR edge runs from its start to its end in the direction of
    // increasing curve parameter.
    let edge_ids: BTreeMap<usize, EdgeId> = used_edges
        .iter()
        .map(|edge| (*edge, id::edge(key, *edge)))
        .collect();
    for (edge, edge_id) in &edge_ids {
        let record = &brep.edges[*edge];
        let factor = match brep.curves[record.curve].kind {
            CurveKind::Line => scale,
            CurveKind::Circle => 1.0,
        };
        let range = record.range.map(|value| value * factor);
        if range[0].partial_cmp(&range[1]) != Some(std::cmp::Ordering::Less) {
            return Err(format!(
                "edge {edge} has the range ({}, {})",
                record.range[0], record.range[1]
            ));
        }
        let carrier = EdgeCarrier::new(curve_ids.get(&record.curve).cloned(), Some(range))
            .map_err(|error| format!("edge {edge}: {error}"))?;
        let [start, end] = record.vertices;
        out.edges.push(Edge {
            id: edge_id.clone(),
            carrier,
            start: id::vertex(key, start),
            end: id::vertex(key, end),
            tolerance: None,
        });
    }

    // Loops and their coedges.
    let mut radial: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (loop_, record) in brep.loops.iter().enumerate() {
        let Some(face) = loop_face[loop_] else {
            continue;
        };
        if record.oriented_edges.is_empty() {
            continue;
        }
        let loop_id = id::loop_(key, loop_);
        let mut coedge_ids = Vec::new();
        for oriented in &record.oriented_edges {
            let use_ = &brep.oriented_edges[*oriented];
            let coedge_id = id::coedge(key, *oriented);
            radial.entry(use_.edge).or_default().push(out.coedges.len());
            out.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: id::edge(key, use_.edge),
                radial_next: coedge_id.clone(),
                sense: if use_.orientation {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
            });
            coedge_ids.push(coedge_id);
        }
        let ring = LoopRing::new(coedge_ids, Vec::new())
            .map_err(|error| format!("loop {loop_}: {error}"))?;
        out.loops.push(Loop {
            id: loop_id,
            face: id::face(key, face),
            boundary: LoopBoundary::Ring(ring),
        });
    }
    for members in radial.values() {
        for (position, member) in members.iter().enumerate() {
            let next = out.coedges[members[(position + 1) % members.len()]]
                .id
                .clone();
            out.coedges[*member].radial_next = next;
        }
    }

    // Faces, shells, regions.
    let written_loops: BTreeSet<LoopId> = out.loops.iter().map(|loop_| loop_.id.clone()).collect();
    for (face, record) in brep.faces.iter().enumerate() {
        let loops: Vec<LoopId> = record
            .loops
            .iter()
            .map(|loop_| id::loop_(key, *loop_))
            .filter(|loop_| written_loops.contains(loop_))
            .collect();
        let loops = match loops.split_first() {
            Some((outer, inner)) => FaceLoops::classified(outer.clone(), inner.to_vec()),
            None => FaceLoops::unspecified(Vec::new()),
        };
        let shell = face_shell[face].unwrap_or_default();
        out.faces.push(Face {
            id: id::face(key, face),
            shell: id::shell(key, shell),
            surface: surface_ids[&record.surface].clone(),
            sense: Sense::Forward,
            loops,
            name: None,
            color: None,
            tolerance: None,
        });
    }
    for (shell, record) in brep.shells.iter().enumerate() {
        let region = shell_region[shell].unwrap_or_default();
        let faces: Vec<FaceId> = record
            .faces
            .iter()
            .map(|face| id::face(key, *face))
            .collect();
        let shell_record = Shell::new(
            id::shell(key, shell),
            id::region(key, region),
            faces,
            Vec::new(),
            Vec::new(),
        )
        .map_err(|_| format!("shell {shell} holds no face"))?;
        out.shells.push(shell_record);
    }
    for (region, shells) in regions.iter().enumerate() {
        out.regions.push(Region {
            id: id::region(key, region),
            body: crate::ids::body(key),
            shells: shells.iter().map(|shell| id::shell(key, *shell)).collect(),
        });
    }
    Ok(out)
}

impl super::Builder<'_, '_, '_, '_> {
    /// Adds the body of the B-rep records of intrinsic primitive `prim`.
    pub(super) fn primitive_body(
        &mut self,
        prim: &crate::stage::Prim<'_>,
        key: &str,
        records: &crate::brep::Brep,
    ) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
        let imported = match import(records, key, self.scale) {
            Ok(imported) => imported,
            Err(reason) => {
                let name = format!("{}: {reason}", prim.path);
                *self.tallies.intrinsic.entry(name).or_default() += 1;
                return Ok(None);
            }
        };
        let Imported {
            points,
            vertices,
            curves,
            surfaces,
            edges,
            coedges,
            loops,
            faces,
            shells,
            regions,
            kind,
        } = imported;
        let model = &mut self.ir.model;
        let region_ids = regions.iter().map(|region| region.id.clone()).collect();
        model.points.extend(points);
        model.vertices.extend(vertices);
        model.curves.extend(curves);
        model.surfaces.extend(surfaces);
        model.edges.extend(edges);
        model.coedges.extend(coedges);
        model.loops.extend(loops);
        model.faces.extend(faces);
        model.shells.extend(shells);
        model.regions.extend(regions);
        let tolerance = records.tolerance * self.scale;
        if tolerance > self.ir.tolerances.linear.get() {
            if let Some(tolerance) = cadmpeg_ir::scalar::PositiveLength::new(tolerance) {
                self.ir.tolerances.linear = tolerance;
            }
        }
        let body = crate::ids::body(key);
        self.body(prim, key, kind.unwrap_or(BodyKind::General), region_ids);
        if let Some(record) = self.ir.model.bodies.last_mut() {
            record.name = Some(prim.name.clone());
        }
        self.transferred = true;
        self.admit("usd_brep")?;
        Ok(Some(body))
    }
}
