// SPDX-License-Identifier: Apache-2.0
//! Writes the solved B-rep as a build123d program.
//!
//! Each face is rebuilt from its surface carrier and its boundary edges, and
//! the faces are sewn into solids. The result is exact geometry rather than
//! editable history, which is what a document without a feature timeline can
//! offer.
//!
//! Faces are emitted by one of three strategies, in order of preference:
//!
//! 1. **Revolution band.** Every boundary edge is a full circle about the
//!    carrier axis, so the face is a parametric band and needs no wire.
//! 2. **Planar wire.** A planar face is built from its boundary wires, widest
//!    first and the rest as holes.
//! 3. **Wire on carrier.** Any other analytic carrier, trimmed by a wire
//!    rebuilt from the solved edges.
//!
//! Anything else is reported as loss rather than approximated. Two of those
//! refusals exist because `OpenCascade` aborts the process instead of returning
//! an error: a carrier the IR cannot supply, and a wire that does not lie on
//! the carrier it would trim. Both are decided here, before the kernel sees
//! them.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::report::{LossKind, LossNote};
use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Vertex};

use crate::geom::{self, Vec3};

/// Preamble shared by every emitted program.
const PREAMBLE: &str = r"import sys

from build123d import *
from OCP.gp import gp_Ax3, gp_Dir, gp_Pnt
from OCP.Geom import (
    Geom_ConicalSurface,
    Geom_CylindricalSurface,
    Geom_Plane,
    Geom_SphericalSurface,
    Geom_ToroidalSurface,
)
from OCP.BRepBuilderAPI import (
    BRepBuilderAPI_MakeFace,
    BRepBuilderAPI_MakeSolid,
    BRepBuilderAPI_Sewing,
)
from OCP.BRepLib import BRepLib
from OCP.TopAbs import TopAbs_SHELL
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS


def _frame(origin, axis, reference):
    return gp_Ax3(gp_Pnt(*origin), gp_Dir(*axis), gp_Dir(*reference))


faces = []
";

/// Closing section that sews the faces and reports what was built.
const EPILOGUE: &str = r#"
sewing = BRepBuilderAPI_Sewing(1e-6)
for face in faces:
    sewing.Add(face.wrapped)
sewing.Perform()
sewn = sewing.SewedShape()

solids = []
explorer = TopExp_Explorer(sewn, TopAbs_SHELL)
while explorer.More():
    try:
        shell = TopoDS.Shell_s(explorer.Current())
        solid = Solid(BRepBuilderAPI_MakeSolid(shell).Solid())
        if solid.volume < 0:
            # The sewn shell came out inward-facing.
            solid = Solid(solid.wrapped.Reversed())
        solids.append(solid)
    except Exception as error:
        print("warning: a shell did not close into a solid (%s)" % error)
    explorer.Next()

if solids:
    result = solids[0] if len(solids) == 1 else Compound(children=solids)
    print("volume: %.6f" % sum(solid.volume for solid in solids))
elif faces:
    # An open sheet body: the document carried no closed shell.
    result = faces[0] if len(faces) == 1 else Compound(children=faces)
    print("volume: 0.000000 (sheet body, %d face(s))" % len(faces))
else:
    result = None
    print("volume: 0.000000 (empty)")
"#;

/// Everything the writer needs to look up by identity.
struct Index<'a> {
    surfaces: HashMap<&'a str, &'a Surface>,
    loops: HashMap<&'a str, &'a Loop>,
    coedges: HashMap<&'a str, &'a Coedge>,
    edges: HashMap<&'a str, &'a Edge>,
    vertices: HashMap<&'a str, &'a Vertex>,
    points: HashMap<&'a str, &'a Point>,
    curves: HashMap<&'a str, &'a Curve>,
}

impl<'a> Index<'a> {
    fn new(ir: &'a CadIr) -> Self {
        Self {
            surfaces: ir
                .model
                .surfaces
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            loops: ir
                .model
                .loops
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            coedges: ir
                .model
                .coedges
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            edges: ir
                .model
                .edges
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            vertices: ir
                .model
                .vertices
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            points: ir
                .model
                .points
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            curves: ir
                .model
                .curves
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
        }
    }

    /// Solved endpoints of an edge, in start-then-end order.
    fn edge_points(&self, edge: &Edge) -> Vec<Vec3> {
        [&edge.start, &edge.end]
            .into_iter()
            .filter_map(|vertex| self.vertices.get(vertex.as_str()))
            .filter_map(|vertex| self.points.get(vertex.point.as_str()))
            .map(|point| Vec3::from(point.position))
            .collect()
    }

    /// Every edge bounding a face, in loop order and without repetition.
    fn face_edges(&self, face: &Face) -> Vec<&'a Edge> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for loop_id in &face.loops {
            let Some(owner) = self.loops.get(loop_id.as_str()) else {
                continue;
            };
            for coedge_id in &owner.coedges {
                let Some(coedge) = self.coedges.get(coedge_id.as_str()) else {
                    continue;
                };
                let Some(edge) = self.edges.get(coedge.edge.as_str()) else {
                    continue;
                };
                if seen.insert(edge.id.as_str()) {
                    out.push(*edge);
                }
            }
        }
        out
    }

    /// Edges a face uses more than once, which is how a closed carrier records
    /// its seam.
    fn seam_edges(&self, face: &Face) -> HashSet<&'a str> {
        let mut uses: HashMap<&str, usize> = HashMap::new();
        for loop_id in &face.loops {
            let Some(owner) = self.loops.get(loop_id.as_str()) else {
                continue;
            };
            for coedge_id in &owner.coedges {
                if let Some(coedge) = self.coedges.get(coedge_id.as_str()) {
                    *uses.entry(coedge.edge.as_str()).or_default() += 1;
                }
            }
        }
        uses.into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(edge, _)| edge)
            .collect()
    }
}

/// Emits a build123d program that rebuilds the document's solved B-rep.
pub(crate) struct Writer<'a> {
    ir: &'a CadIr,
    index: Index<'a>,
    lines: Vec<String>,
    indent: usize,
    wire_seq: usize,
    losses: Vec<LossNote>,
    counts: BTreeMap<String, usize>,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(ir: &'a CadIr) -> Self {
        Self {
            ir,
            index: Index::new(ir),
            lines: Vec::new(),
            indent: 0,
            wire_seq: 0,
            losses: Vec::new(),
            counts: BTreeMap::new(),
        }
    }

    /// Writes the whole program, returning it with its losses and census.
    pub(crate) fn write(mut self) -> (String, Vec<LossNote>, BTreeMap<String, usize>) {
        self.push_raw(&header(self.ir));
        self.push_raw(PREAMBLE);
        let mut built = 0usize;
        for face in &self.ir.model.faces {
            if self.face(face) {
                built += 1;
            }
        }
        if built == 0 && !self.ir.model.faces.is_empty() {
            self.losses.push(LossNote::new(
                LossKind::NoExportableSolids,
                "no face of the solved body could be rebuilt from its carriers",
            ));
        }
        self.push_raw(EPILOGUE);
        (self.lines.join("\n"), self.losses, self.counts)
    }

    fn push_raw(&mut self, text: &str) {
        for line in text.lines() {
            self.lines.push(line.to_owned());
        }
    }

    fn push(&mut self, line: &str) {
        if line.is_empty() {
            self.lines.push(String::new());
        } else {
            self.lines
                .push(format!("{}{line}", "    ".repeat(self.indent)));
        }
    }

    fn count(&mut self, key: &str) {
        *self.counts.entry(key.to_owned()).or_default() += 1;
    }

    fn loss(&mut self, kind: LossKind, message: String) {
        self.losses.push(LossNote::new(kind, message));
    }

    // -- face dispatch ------------------------------------------------------

    fn face(&mut self, face: &'a Face) -> bool {
        let Some(surface) = self.index.surfaces.get(face.surface.as_str()) else {
            self.loss(
                LossKind::UnknownSurfaceFaceOmitted,
                format!("face {} has no surface carrier", face.id),
            );
            return false;
        };
        let geometry = &surface.geometry;
        if geom::surface_frame(geometry).is_none() {
            self.loss(
                LossKind::UnsupportedObjectFamily,
                format!(
                    "face {} lies on a carrier this encoder does not rebuild",
                    face.id
                ),
            );
            return false;
        }

        let label = short_label(face.id.as_str());
        self.push(&format!("# face {label}"));
        let mark = self.lines.len();
        self.push("try:");
        self.indent += 1;
        let built = self.face_body(face, geometry);
        self.indent -= 1;
        if !built {
            self.lines.truncate(mark);
            self.push("");
            return false;
        }
        self.push("except Exception as error:");
        self.push(&format!(
            "    print(\"warning: face {label} was refused by the kernel (%s)\" % error)"
        ));
        self.push("");
        true
    }

    fn face_body(&mut self, face: &'a Face, geometry: &SurfaceGeometry) -> bool {
        if is_revolution_carrier(geometry) && self.is_revolution_band(face, geometry) {
            return self.revolution_band(face, geometry);
        }
        if !self.index.seam_edges(face).is_empty() {
            // A seamed periodic face needs its parameter-space curves to be
            // trimmed; the seam appears twice, so a 3D wire is degenerate and
            // the kernel aborts rather than refusing it.
            self.loss(
                LossKind::PcurveOmitted,
                format!(
                    "face {} is a seamed periodic face that is not a full revolution \
                     band; trimming it needs parameter-space curves the IR did not carry",
                    face.id
                ),
            );
            return false;
        }
        let Some(wires) = self.face_wires(face) else {
            self.loss(
                LossKind::CurvelessEdgeOmitted,
                format!("face {} has no reconstructable boundary", face.id),
            );
            return false;
        };
        if matches!(geometry, SurfaceGeometry::Plane { .. }) {
            self.planar_face(wires)
        } else {
            self.trimmed_face(face, geometry, wires)
        }
    }

    // -- strategy 1: parametric band ----------------------------------------

    /// True when the face spans a full revolution bounded by circle edges.
    ///
    /// A closed cylindrical or toroidal face is bounded by its two circles plus
    /// a seam edge used twice in one loop. The seam carries no shape
    /// information here, so it is skipped and the band comes from the circles.
    fn is_revolution_band(&self, face: &'a Face, geometry: &SurfaceGeometry) -> bool {
        let Some((_, axis, _)) = geom::surface_frame(geometry) else {
            return false;
        };
        let seams = self.index.seam_edges(face);
        let edges: Vec<_> = self
            .index
            .face_edges(face)
            .into_iter()
            .filter(|edge| !seams.contains(edge.id.as_str()))
            .collect();
        if edges.is_empty() {
            return false;
        }
        edges.iter().all(|edge| {
            let Some(curve) = edge
                .curve
                .as_ref()
                .and_then(|id| self.index.curves.get(id.as_str()))
            else {
                return false;
            };
            let CurveGeometry::Circle {
                axis: circle_axis, ..
            } = &curve.geometry
            else {
                return false;
            };
            if !Vec3::from(*circle_axis).is_parallel_to(axis) {
                return false;
            }
            edge.param_range.is_none_or(|range| {
                ((range[1] - range[0]).abs() - std::f64::consts::TAU).abs() < 1e-6
            })
        })
    }

    fn revolution_band(&mut self, face: &'a Face, geometry: &SurfaceGeometry) -> bool {
        let Some(bounds) = self.parametric_bounds(face, geometry) else {
            self.loss(
                LossKind::UnsupportedObjectFamily,
                format!("face {} cannot be trimmed parametrically", face.id),
            );
            return false;
        };
        let Some(carrier) = carrier_expression(geometry) else {
            return false;
        };
        let [u_min, u_max, v_min, v_max] = bounds;
        self.push(&format!("carrier = {carrier}"));
        self.push(&format!(
            "builder = BRepBuilderAPI_MakeFace(carrier, {}, {}, {}, {}, 1e-7)",
            geom::number(u_min),
            geom::number(u_max),
            geom::number(v_min),
            geom::number(v_max)
        ));
        self.push("faces.append(Face(builder.Face()))");
        self.count("faces");
        self.count("faces_parametric");
        true
    }

    /// Parametric bounds of a full-revolution face.
    ///
    /// The minor-angle band of a toroidal blend is where the sign of
    /// `minor_radius` earns its keep: a concave blend is a quarter tube, and
    /// dropping the sign leaves the complementary band equally consistent with
    /// the two boundary circles.
    fn parametric_bounds(&self, face: &'a Face, geometry: &SurfaceGeometry) -> Option<[f64; 4]> {
        let (origin, axis, _) = geom::surface_frame(geometry)?;
        let mut stations = Vec::new();
        let mut angles = Vec::new();
        for edge in self.index.face_edges(face) {
            for point in self.index.edge_points(edge) {
                let delta = point.sub(origin);
                let along = delta.dot(axis);
                stations.push(along);
                if let SurfaceGeometry::Torus { major_radius, .. } = geometry {
                    let radial = delta.reject(axis).length();
                    angles.push(
                        along
                            .atan2(radial - *major_radius)
                            .rem_euclid(std::f64::consts::TAU),
                    );
                }
            }
        }
        if stations.is_empty() {
            return None;
        }
        let low = stations.iter().copied().fold(f64::INFINITY, f64::min);
        let high = stations.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        match geometry {
            SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
                Some([0.0, std::f64::consts::TAU, low, high])
            }
            SurfaceGeometry::Sphere { radius, .. } => {
                let latitude = |station: f64| (station / *radius).clamp(-1.0, 1.0).asin();
                Some([0.0, std::f64::consts::TAU, latitude(low), latitude(high)])
            }
            SurfaceGeometry::Torus { .. } => {
                let (start, end) = torus_band(&angles);
                Some([0.0, std::f64::consts::TAU, start, end])
            }
            _ => None,
        }
    }

    // -- strategies 2 and 3: wires ------------------------------------------

    fn planar_face(&mut self, wires: Vec<Vec<String>>) -> bool {
        let names = self.emit_wires(wires);
        self.push(&format!("face = Face({})", names[0]));
        if names.len() > 1 {
            self.push(&format!(
                "face = face.make_holes([{}])",
                names[1..].join(", ")
            ));
        }
        self.push("faces.append(face)");
        self.count("faces");
        self.count("faces_planar");
        true
    }

    fn trimmed_face(
        &mut self,
        face: &'a Face,
        geometry: &SurfaceGeometry,
        wires: Vec<Vec<String>>,
    ) -> bool {
        if !self.boundary_lies_on(face, geometry) {
            self.loss(
                LossKind::UnsupportedObjectFamily,
                format!(
                    "face {} has a boundary that does not lie on its carrier; it is not \
                     rebuilt, because trimming an off-carrier wire aborts the kernel",
                    face.id
                ),
            );
            return false;
        }
        let Some(carrier) = carrier_expression(geometry) else {
            return false;
        };
        let names = self.emit_wires(wires);
        self.push(&format!("carrier = {carrier}"));
        self.push(&format!(
            "builder = BRepBuilderAPI_MakeFace(carrier, {}.wrapped, True)",
            names[0]
        ));
        for name in &names[1..] {
            self.push(&format!("builder.Add({name}.wrapped)"));
        }
        self.push("if builder.IsDone():");
        self.push("    trimmed = builder.Face()");
        self.push("    BRepLib.BuildCurves3d_s(trimmed)");
        self.push("    faces.append(Face(trimmed))");
        self.count("faces");
        self.count("faces_trimmed");
        true
    }

    /// Whether the face's solved boundary sits on its carrier.
    fn boundary_lies_on(&self, face: &'a Face, geometry: &SurfaceGeometry) -> bool {
        let tolerance = (self.ir.tolerances.linear * 1000.0).max(1e-3);
        let mut any = false;
        for edge in self.index.face_edges(face) {
            for point in self.index.edge_points(edge) {
                any = true;
                match geom::distance_to_surface(geometry, point) {
                    Some(distance) if distance.abs() <= tolerance => {}
                    _ => return false,
                }
            }
        }
        any
    }

    /// Boundary wires of a face as edge expressions, widest ring first.
    fn face_wires(&mut self, face: &'a Face) -> Option<Vec<Vec<String>>> {
        let mut rings: Vec<(f64, Vec<String>)> = Vec::new();
        for loop_id in &face.loops {
            let owner = self.index.loops.get(loop_id.as_str())?;
            let mut expressions = Vec::new();
            let mut extent = 0.0f64;
            let mut usable = true;
            for coedge_id in &owner.coedges {
                let Some(coedge) = self.index.coedges.get(coedge_id.as_str()) else {
                    continue;
                };
                let Some(edge) = self.index.edges.get(coedge.edge.as_str()) else {
                    continue;
                };
                match self.edge_expression(edge) {
                    Some((expression, size)) => {
                        expressions.push(expression);
                        extent = extent.max(size);
                    }
                    None => {
                        usable = false;
                        break;
                    }
                }
            }
            if usable && !expressions.is_empty() {
                rings.push((extent, expressions));
            }
        }
        if rings.is_empty() {
            return None;
        }
        rings.sort_by(|left, right| right.0.total_cmp(&left.0));
        Some(rings.into_iter().map(|(_, ring)| ring).collect())
    }

    /// Emits one `Wire` per ring and returns the variable names.
    ///
    /// `Wire.combine` is used rather than the strict constructor: coedges are
    /// in traversal order but each edge's geometry follows its own curve
    /// direction, so half of them run backwards, and solved endpoints agree
    /// only to the document tolerance.
    fn emit_wires(&mut self, wires: Vec<Vec<String>>) -> Vec<String> {
        let tolerance = geom::number((self.ir.tolerances.linear * 10.0).max(1e-6));
        let mut names = Vec::new();
        for ring in wires {
            let name = format!("wire{}", self.wire_seq);
            self.wire_seq += 1;
            self.push(&format!("{name} = Wire.combine(["));
            for expression in ring {
                self.push(&format!("    {expression},"));
            }
            self.push(&format!("], tol={tolerance})[0]"));
            names.push(name);
        }
        names
    }

    /// A build123d expression for one edge, with its characteristic size.
    fn edge_expression(&mut self, edge: &'a Edge) -> Option<(String, f64)> {
        let points = self.index.edge_points(edge);
        let curve = edge
            .curve
            .as_ref()
            .and_then(|id| self.index.curves.get(id.as_str()));
        let Some(curve) = curve else {
            if points.len() == 2 && points[0].sub(points[1]).length() > geom::DIRECTION_TOLERANCE {
                self.loss(
                    LossKind::CurvelessEdgeOmitted,
                    format!(
                        "edge {} has no curve carrier; it is emitted as a straight chord \
                         between its solved vertices",
                        edge.id
                    ),
                );
                return Some((
                    format!(
                        "Edge.make_line({}, {})",
                        geom::tuple(points[0]),
                        geom::tuple(points[1])
                    ),
                    points[0].sub(points[1]).length(),
                ));
            }
            return None;
        };

        match &curve.geometry {
            CurveGeometry::Line { origin, direction } => {
                let (start, end) = if points.len() == 2 {
                    (points[0], points[1])
                } else {
                    let range = edge.param_range.unwrap_or([0.0, 1.0]);
                    let origin = Vec3::from(*origin);
                    let direction = Vec3::from(*direction);
                    (
                        origin.add(direction.scale(range[0])),
                        origin.add(direction.scale(range[1])),
                    )
                };
                Some((
                    format!(
                        "Edge.make_line({}, {})",
                        geom::tuple(start),
                        geom::tuple(end)
                    ),
                    start.sub(end).length(),
                ))
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                let axis = Vec3::from(*axis).unit();
                let mut reference = Vec3::from(*ref_direction);
                if reference.length() < geom::DIRECTION_TOLERANCE {
                    reference = geom::derived_ref_direction(axis);
                }
                let plane = format!(
                    "Plane(origin={}, x_dir={}, z_dir={})",
                    geom::tuple(Vec3::from(*center)),
                    geom::tuple(reference.reject(axis).unit()),
                    geom::tuple(axis)
                );
                let full = edge.param_range.is_none_or(|range| {
                    ((range[1] - range[0]).abs() - std::f64::consts::TAU).abs() < 1e-9
                });
                let expression = if full {
                    format!("Edge.make_circle({}, {plane})", geom::number(*radius))
                } else {
                    let range = edge.param_range.unwrap_or([0.0, std::f64::consts::TAU]);
                    format!(
                        "Edge.make_circle({}, {plane}, start_angle={}, end_angle={})",
                        geom::number(*radius),
                        geom::number(range[0].to_degrees()),
                        geom::number(range[1].to_degrees())
                    )
                };
                Some((expression, *radius))
            }
            other => {
                self.loss(
                    LossKind::UnsupportedObjectFamily,
                    format!(
                        "edge {} carries a {} curve, which this encoder does not rebuild",
                        edge.id,
                        curve_family(other)
                    ),
                );
                None
            }
        }
    }
}

/// Documentation header naming the source and the encoder's contract.
fn header(ir: &CadIr) -> String {
    let format = ir
        .source
        .as_ref()
        .map_or_else(|| "unknown".to_owned(), |source| source.format.clone());
    format!(
        "\"\"\"build123d program generated by cadmpeg.\n\
         \n\
         Source format : {format}\n\
         IR version    : {}\n\
         \n\
         Faces are rebuilt from the solved carriers and sewn. The result is exact\n\
         geometry, not editable history.\n\
         \"\"\"\n",
        ir.ir_version
    )
}

fn is_revolution_carrier(geometry: &SurfaceGeometry) -> bool {
    matches!(
        geometry,
        SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. }
    )
}

fn curve_family(geometry: &CurveGeometry) -> &'static str {
    match geometry {
        CurveGeometry::Line { .. } => "line",
        CurveGeometry::Circle { .. } => "circle",
        CurveGeometry::Ellipse { .. } => "ellipse",
        CurveGeometry::Parabola { .. } => "parabola",
        CurveGeometry::Hyperbola { .. } => "hyperbola",
        CurveGeometry::Nurbs(_) => "NURBS",
        _ => "non-analytic",
    }
}

/// The shorter arc between the sampled minor angles of a toroidal face.
fn torus_band(angles: &[f64]) -> (f64, f64) {
    if angles.len() < 2 {
        return (0.0, std::f64::consts::TAU);
    }
    let low = angles.iter().copied().fold(f64::INFINITY, f64::min);
    let high = angles.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if high - low > std::f64::consts::PI {
        // The band crosses the seam.
        (high, low + std::f64::consts::TAU)
    } else {
        (low, high)
    }
}

/// A Python expression constructing the analytic carrier, or `None` when the IR
/// cannot supply one. A null carrier must never reach `MakeFace`.
fn carrier_expression(geometry: &SurfaceGeometry) -> Option<String> {
    let (origin, axis, reference) = geom::surface_frame(geometry)?;
    let frame = format!(
        "_frame({}, {}, {})",
        geom::tuple(origin),
        geom::tuple(axis),
        geom::tuple(reference)
    );
    let expression = match geometry {
        SurfaceGeometry::Plane { .. } => format!("Geom_Plane({frame})"),
        SurfaceGeometry::Cylinder { radius, .. } => {
            format!(
                "Geom_CylindricalSurface({frame}, {})",
                geom::number(*radius)
            )
        }
        SurfaceGeometry::Cone {
            radius, half_angle, ..
        } => format!(
            "Geom_ConicalSurface({frame}, {}, {})",
            geom::number(*half_angle),
            geom::number(*radius)
        ),
        SurfaceGeometry::Sphere { radius, .. } => {
            format!("Geom_SphericalSurface({frame}, {})", geom::number(*radius))
        }
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => format!(
            "Geom_ToroidalSurface({frame}, {}, {})",
            geom::number(*major_radius),
            geom::number(minor_radius.abs())
        ),
        _ => return None,
    };
    Some(expression)
}

/// The trailing component of an identity, for use in a generated comment.
fn short_label(identity: &str) -> String {
    identity
        .rsplit(['#', ':'])
        .next()
        .unwrap_or(identity)
        .to_owned()
}
