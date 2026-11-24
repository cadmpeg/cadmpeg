// SPDX-License-Identifier: Apache-2.0
//! Primary partition topology and carrier projection.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralCurveId, RegionId,
    ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::index::StandardIndex;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{Magnification, PositiveLength};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary,
    LoopRing, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;
use cadmpeg_parasolid::records::primary::Node;
use cadmpeg_parasolid::records::{node_type as kind, PrimaryRecords};

use crate::geometry::{self, Nodes};
use crate::loss::SolidEdgeLossCode;
use cadmpeg_ir::report::loss::LossNote;

const MM_PER_METRE: f64 = Magnification::MILLIMETERS_PER_METER.get();

macro_rules! ids {
    ($($fn:ident: $ty:ident, $name:literal);* $(;)?) => {$ (
        fn $fn(stream: u32, node: u32) -> $ty {
            $ty::compose(&cadmpeg_ir::identity_namespace!("solidedge", "brep", $name),
                (u64::from(stream) << 32) | u64::from(node))
        }
    )*};
}
ids! { body_id: BodyId, "body"; region_id: RegionId, "region"; shell_id: ShellId, "shell";
face_id: FaceId, "face"; loop_id: LoopId, "loop"; coedge_id: CoedgeId, "coedge";
edge_id: EdgeId, "edge"; vertex_id: VertexId, "vertex"; point_id: PointId, "point";
curve_id: CurveId, "curve"; construction_id: ProceduralCurveId, "curve-construction"; surface_id: SurfaceId, "surface"; }

/// Identities already pushed to the model, so a repeated carrier or point is
/// recognised without scanning the arena.
#[derive(Default)]
struct Emitted {
    curves: BTreeMap<CurveId, usize>,
    points: BTreeSet<PointId>,
}

impl Emitted {
    fn push_curve(&mut self, ir: &mut CadIr, curve: Curve) {
        self.curves
            .insert(curve.id.clone(), ir.geometry.curves.len());
        ir.geometry.curves.push(curve);
    }
    fn curve<'ir>(&self, ir: &'ir CadIr, id: &CurveId) -> Option<&'ir Curve> {
        ir.geometry.curves.get(*self.curves.get(id)?)
    }
}

fn malformed(message: &str) -> CodecError {
    CodecError::Malformed(message.into())
}
fn ptr(node: Node<'_>, field: &str) -> Option<u32> {
    node.pointer(field).filter(|index| *index > 1)
}
fn sense(node: Node<'_>) -> Option<Sense> {
    match node.char("sense") {
        Some(b'+') => Some(Sense::Forward),
        Some(b'-') => Some(Sense::Reversed),
        _ => None,
    }
}
fn reverse(value: Sense) -> Sense {
    match value {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    }
}
fn tolerance(node: Node<'_>, losses: &mut Vec<LossNote>) -> Option<PositiveLength> {
    if node.is_null("tolerance") == Some(true) {
        return None;
    }
    let raw = node.real("tolerance");
    if raw == Some(0.0) {
        return None;
    }
    let value = raw.and_then(|value| PositiveLength::new(value * MM_PER_METRE));
    if value.is_none() {
        losses.push(
            SolidEdgeLossCode::TopologyToleranceInvalid
                .note("invalid topology tolerance uses the document tolerance"),
        );
    }
    value
}

/// The stated tolerance, widened to the departure of the edge's carrier from
/// its faces when that departure exceeds the document tolerance.
fn widened(
    stated: Option<PositiveLength>,
    departure: Option<f64>,
    document: f64,
) -> Option<PositiveLength> {
    let Some(departure) = departure.filter(|value| *value > document) else {
        return stated;
    };
    let wide = stated.map_or(departure, |value| value.get().max(departure));
    PositiveLength::new(wide).or(stated)
}

/// A vertex's position in millimetres.
fn vertex_position(nodes: &Nodes<'_>, vertex: u32) -> Option<Point3> {
    vertex_point(nodes, vertex).map(FinitePoint3::get)
}

fn vertex_point(nodes: &Nodes<'_>, vertex: u32) -> Option<FinitePoint3> {
    let point = nodes.get(&ptr(*nodes.get(&vertex)?, "point")?)?;
    let [x, y, z] = point.vector("pvec")?;
    FinitePoint3::new(Point3::new(
        x * MM_PER_METRE,
        y * MM_PER_METRE,
        z * MM_PER_METRE,
    ))
}

/// The nodes of an ownership chain, ending at the first link that does not
/// resolve or that repeats a node. The second value names that defect.
fn chain(nodes: &Nodes<'_>, start: Option<u32>, link: &str) -> (Vec<u32>, Option<&'static str>) {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = start;
    while let Some(index) = current {
        if !seen.insert(index) {
            return (result, Some("cyclic Parasolid ownership chain"));
        }
        let Some(node) = nodes.get(&index).copied() else {
            return (result, Some("unresolved Parasolid ownership chain"));
        };
        result.push(index);
        current = ptr(node, link);
    }
    (result, None)
}

fn chain_losses(defect: Option<&'static str>, owner: &str, losses: &mut Vec<LossNote>) {
    if let Some(defect) = defect {
        losses.push(SolidEdgeLossCode::ChainTruncated.note(format!(
            "{defect} of {owner}; the records after the defect are not transferred"
        )));
    }
}

/// What an edge states: its endpoint vertices in fin order and its carrier
/// node. An edge whose endpoints or carrier cannot be placed has no
/// admissible form, and the loops that use it are not transferred.
struct EdgeRead {
    /// Start and end vertex nodes before the carrier's sense applies; `None`
    /// is a vertexless ring edge.
    endpoints: Option<[u32; 2]>,
    /// The carrier node, when the edge names one that resolves.
    curve: Option<u32>,
    /// The edge names a carrier node that does not resolve.
    curve_unresolved: bool,
}

type EdgeReads = BTreeMap<u32, Result<EdgeRead, &'static str>>;

fn read_edge(nodes: &Nodes<'_>, edge: u32) -> Result<EdgeRead, &'static str> {
    let node = *nodes.get(&edge).ok_or("unresolved edge")?;
    let first = ptr(node, "halfedge")
        .and_then(|index| nodes.get(&index))
        .copied()
        .ok_or("edge has no resolved fin")?;
    let previous = ptr(first, "backward")
        .and_then(|index| nodes.get(&index))
        .copied()
        .ok_or("edge fin has no resolved predecessor")?;
    let endpoints = match (ptr(previous, "vertex"), ptr(first, "vertex")) {
        (Some(start), Some(end)) => {
            let sense = sense(first).ok_or("edge fin has an invalid sense")?;
            if vertex_position(nodes, start).is_none() || vertex_position(nodes, end).is_none() {
                return Err("edge vertex has no resolved position");
            }
            Some(if sense == Sense::Forward {
                [start, end]
            } else {
                [end, start]
            })
        }
        (None, None) => None,
        _ => return Err("edge states one endpoint vertex"),
    };
    let named = ptr(node, "curve");
    let curve = named.filter(|index| nodes.contains_key(index));
    if endpoints.is_none() && curve.is_none() && crate::sp_curve::locate(nodes, first).is_none() {
        return Err("vertexless edge has no carrier");
    }
    Ok(EdgeRead {
        endpoints,
        curve,
        curve_unresolved: named.is_some() && curve.is_none(),
    })
}

/// A fin of a loop's ring.
struct RingFin {
    fin: u32,
    edge: u32,
    vertex: Option<u32>,
}

enum LoopShape {
    Pole(u32),
    Ring(Vec<RingFin>),
}

/// Reads the boundary of a loop from its fin ring.
fn read_loop(
    nodes: &Nodes<'_>,
    edges: &mut EdgeReads,
    loop_index: u32,
) -> Result<LoopShape, &'static str> {
    let loop_ = *nodes.get(&loop_index).ok_or("unresolved loop")?;
    let first = ptr(loop_, "halfedge").ok_or("loop has no fin")?;
    let mut current = first;
    let mut ring = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current) {
            return Err("fin ring has a secondary cycle");
        }
        let fin = *nodes.get(&current).ok_or("unresolved fin")?;
        if fin.char("sense") == Some(b'?')
            && ptr(fin, "forward") == Some(current)
            && ptr(fin, "backward") == Some(current)
            && ptr(fin, "edge").is_none()
        {
            let vertex = ptr(fin, "vertex").ok_or("pole fin has no vertex")?;
            if vertex_position(nodes, vertex).is_none() {
                return Err("pole vertex has no resolved position");
            }
            return Ok(LoopShape::Pole(vertex));
        }
        let edge = ptr(fin, "edge").ok_or("fin has no edge")?;
        edges
            .entry(edge)
            .or_insert_with(|| read_edge(nodes, edge))
            .as_ref()
            .map_err(|reason| *reason)?;
        sense(fin).ok_or("fin has an invalid sense")?;
        ring.push(RingFin {
            fin: current,
            edge,
            vertex: ptr(fin, "vertex"),
        });
        current = ptr(fin, "forward").ok_or("fin has no successor")?;
        if current == first {
            return Ok(LoopShape::Ring(ring));
        }
    }
}

/// A face whose every loop read.
struct FaceRead {
    index: u32,
    shell: u32,
    /// The surface node, or the face's own node index for the unknown surface
    /// that stands for a missing reference.
    surface: u32,
    sense: Sense,
    loops: Vec<(u32, LoopShape)>,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    records: &PrimaryRecords<'_>,
    stream: u32,
    losses: &mut Vec<LossNote>,
) -> Result<CadIr, CodecError> {
    let mut nodes = BTreeMap::new();
    for node in records.nodes() {
        if nodes.insert(node.index(), node).is_some() {
            return Err(malformed("duplicate Parasolid node index"));
        }
    }
    let mut ir = CadIr::empty();
    let mut selected_shells = BTreeMap::new();
    let mut linear: Option<PositiveLength> = None;
    for body in nodes
        .values()
        .copied()
        .filter(|node| node.kind() == kind::BODY)
    {
        if let Some(value) = body
            .real("res_linear")
            .and_then(|value| PositiveLength::new(value * MM_PER_METRE))
        {
            linear = Some(linear.map_or(
                value,
                |current| if current > value { current } else { value },
            ));
        } else {
            losses.push(
                SolidEdgeLossCode::BodyResolutionInvalid
                    .note("invalid body linear resolution uses the document tolerance"),
            );
        }
        let body_kind = match body.byte("body_type") {
            Some(1) => BodyKind::Solid,
            Some(2) => BodyKind::Wire,
            Some(3) => BodyKind::Sheet,
            Some(6) => BodyKind::General,
            _ => {
                losses.push(
                    SolidEdgeLossCode::BodyKindAssumed
                        .note("unknown body kind uses general topology"),
                );
                BodyKind::General
            }
        };
        ir.geometry.bodies.push(Body {
            id: body_id(stream, body.index()),
            kind: body_kind,
            transform: None,
            name: None,
            color: None,
            visible: None,
            native_ref: None,
        });
        let mut candidates = Vec::new();
        for region in nodes
            .values()
            .copied()
            .filter(|node| node.kind() == kind::REGION && ptr(*node, "body") == Some(body.index()))
        {
            let (shells, defect) = chain(&nodes, ptr(region, "shell"), "next");
            chain_losses(defect, "a region's shells", losses);
            for shell in &shells {
                selected_shells.insert(*shell, region.index());
            }
            regions.push(Region {
                id: region_id(stream, region.index()),
                body: body_id(stream, body.index()),
                shells: shells
                    .into_iter()
                    .map(|shell| shell_id(stream, shell))
            let owns_faces = shells.iter().any(|shell| {
                nodes
                    .get(shell)
                    .and_then(|shell| ptr(*shell, "face"))
                    .is_some_and(|face| nodes.contains_key(&face))
            });
            candidates.push((region, shells, owns_faces));
        }
        // A void region that owns no face has its boundary in the shell of a
        // neighbouring region and transfers no region of its own when the body
        // also has a solid region or a region that owns a face.
        let has_material = candidates
            .iter()
            .any(|(region, _, owns_faces)| *owns_faces || region.char("type") == Some(b'S'));
        let mut regions = Vec::new();
        for (region, shells, owns_faces) in candidates {
            if has_material && !owns_faces && region.char("type") != Some(b'S') {
                continue;
            }
                    .collect(),
                native_ref: None,
            });
        }
        if regions.is_empty() {
            let (shells, defect) = chain(&nodes, ptr(body, "shell"), "next");
            chain_losses(defect, "a body's shells", losses);
            for shell in &shells {
                selected_shells.insert(*shell, body.index());
            }
            regions.push(Region {
                id: region_id(stream, body.index()),
                body: body_id(stream, body.index()),
                shells: shells
                    .into_iter()
                    .map(|shell| shell_id(stream, shell))
                    .collect(),
                native_ref: None,
            });
        }
        ir.geometry.regions.extend(regions);
    }
    let mut face_shells = BTreeMap::new();
    for (shell_index, region) in selected_shells {
        let Some(shell) = nodes.get(&shell_index).copied() else {
            continue;
        };
        if ptr(shell, "edge").is_some() || ptr(shell, "vertex").is_some() {
            losses.push(
                SolidEdgeLossCode::WireTopologyUnsupported
                    .note("free shell edges and vertices remain undecoded"),
            );
    // The shells of each region that hold faces.
    let mut region_face_shells: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        }
        ir.geometry.shells.push(Shell::new(
            shell_id(stream, shell_index),
            region_id(stream, region),
        ));
        let (shell_faces, defect) = chain(&nodes, ptr(shell, "face"), "next");
        chain_losses(defect, "a shell's faces", losses);
        for face in shell_faces {
            face_shells.entry(face).or_insert(shell_index);
        }
    }
    let mut edges = BTreeSet::new();
    let mut fit_tolerances: BTreeMap<CurveId, f64> = BTreeMap::new();
    // Largest departure of a carrier from the faces it bounds, where it
    // exceeds the document tolerance.
    let mut departures: BTreeMap<CurveId, f64> = BTreeMap::new();
        if !shell_faces.is_empty() {
            region_face_shells
                .entry(region)
                .or_default()
                .push(shell_index);
        }
    if let Some(linear) = linear {
        ir.tolerances.linear = linear;
    }
    let document_linear = ir.tolerances.linear.get();
    let mut vertices = BTreeSet::new();
    let mut surfaces = BTreeSet::new();
    // Faces with no surface reference stand on an unknown surface that takes
    // the face's own node index.
    let mut surfaceless = Vec::new();
    let mut emitted = Emitted::default();
    let mut edge_reads = EdgeReads::new();
    let mut face_reads = Vec::new();
    for (face_index, shell) in face_shells {
        let Some(face) = nodes.get(&face_index).copied() else {
            continue;
        };
        let Some(face_sense) = sense(face) else {
            losses.push(
                SolidEdgeLossCode::FaceSenseUnresolved.note(
                    "a face states no orientation; the face and its loops are not transferred",
                ),
            );
            continue;
        };
        let (loop_nodes, defect) = chain(&nodes, ptr(face, "loop"), "next");
        let mut complete = defect.is_none();
        chain_losses(defect, "a face's loops", losses);
        let mut loops = Vec::new();
        for loop_index in loop_nodes {
            match read_loop(&nodes, &mut edge_reads, loop_index) {
                Ok(shape) => loops.push((loop_index, shape)),
                Err(reason) => {
                    complete = false;
                    losses.push(
                        SolidEdgeLossCode::LoopUnreadable
                            .note(format!("a loop does not read: {reason}")),
                    );
                }
            }
        }
        if !complete {
            losses.push(
                SolidEdgeLossCode::LoopUnreadable
                    .note("a face with a loop that does not read is not transferred"),
            );
            continue;
        }
        let surface = if let Some(surface) = ptr(face, "surface") {
            surfaces.insert(surface);
            surface
        } else {
            surfaceless.push(face_index);
            face_index
        };
        for (_, shape) in &loops {
            match shape {
                LoopShape::Pole(vertex) => {
                    vertices.insert(*vertex);
                }
                LoopShape::Ring(ring) => {
                    for fin in ring {
                        edges.insert(fin.edge);
                        match fin.vertex {
                            Some(vertex) if vertex_position(&nodes, vertex).is_some() => {
                                vertices.insert(vertex);
                            }
                            Some(_) => losses.push(
                                SolidEdgeLossCode::VertexUnresolved
                                    .note("a fin names a vertex that has no resolved position"),
                            ),
                            None => {}
                        }
                    }
                }
            }
        }
        face_reads.push(FaceRead {
            index: face_index,
            shell,
            surface,
            sense: face_sense,
            loops,
        });
    }
    for edge in &edges {
        if let Some(Ok(EdgeRead {
            endpoints: Some(ends),
            ..
        })) = edge_reads.get(edge)
        {
            vertices.extend(ends);
        }
    }
    for edge in &edges {
        if let Some(source) = nodes
            .get(edge)
            .and_then(|edge| ptr(*edge, "curve"))
            .and_then(|curve| nodes.get(&curve))
            .copied()
            .map(|curve| geometry::trim_basis(&nodes, curve))
            .filter(|curve| curve.kind() == kind::INTERSECTION)
        {
            if let Some(supports) = source.pointer_array::<2>("surface") {
                surfaces.extend(
                    supports
                        .into_iter()
                        .filter(|index| *index > 1 && nodes.contains_key(index)),
                );
            }
        }
    }
    for surface in surfaces {
        let Some(node) = nodes.get(&surface).copied() else {
            losses.push(
                SolidEdgeLossCode::SurfaceUnresolved.note(
                    "a face names a surface node that does not resolve; its carrier is unknown",
                ),
            );
            ir.geometry.surfaces.push(Surface {
                id: surface_id(stream, surface),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
                native_ref: None,
            });
            continue;
        };
        if node.kind() == kind::SWEPT_SURF
            && swept_surface(&nodes, &mut ir, &mut emitted, stream, surface, node)?
        {
            continue;
        }
        let geometry = geometry::surface(&nodes, node).unwrap_or_else(|| {
            losses.push(SolidEdgeLossCode::SurfaceUnsupported.note(format!(
                "surface node type {} has an unsupported carrier",
                node.kind()
            )));
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
        });
        ir.geometry.surfaces.push(Surface {
            id: surface_id(stream, surface),
            geometry,
            source_object: None,
            native_ref: None,
        });
    }
    for face in &surfaceless {
        losses.push(
            SolidEdgeLossCode::SurfaceUnresolved
                .note("a face names no surface; its carrier is unknown"),
        );
        ir.geometry.surfaces.push(Surface {
            id: surface_id(stream, *face),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
            native_ref: None,
        });
    }
    for vertex in vertices {
        let Some(position) = vertex_point(&nodes, vertex) else {
            continue;
        };
        let Some(point) = nodes.get(&vertex).and_then(|node| ptr(*node, "point")) else {
            continue;
        };
        let Some(node) = nodes.get(&vertex).copied() else {
            continue;
        };
        if emitted.points.insert(point_id(stream, point)) {
            ir.geometry
                .points
                .push(Point::new(point_id(stream, point), position, None));
        }
        ir.geometry.vertices.push(Vertex {
            id: vertex_id(stream, vertex),
            point: point_id(stream, point),
            tolerance: tolerance(node, losses),
            native_ref: None,
        });
    }
    let mut dropped_edges = BTreeSet::new();
    for edge in edges {
        let (Some(node), Some(Ok(read))) = (nodes.get(&edge).copied(), edge_reads.get(&edge))
        else {
            continue;
        };
        let first = ptr(node, "halfedge").and_then(|index| nodes.get(&index).copied());
        let mut endpoints = read.endpoints;
        let mut carrier = EdgeCarrier::Free;
        if read.curve_unresolved {
            losses.push(
                SolidEdgeLossCode::CurveUnresolved
                    .note("an edge names a carrier node that does not resolve"),
            );
        }
        if let Some(curve_index) = read.curve {
            let Some(source) = nodes.get(&curve_index).copied() else {
                continue;
            };
            if source.char("sense") == Some(b'-') {
                endpoints = endpoints.map(|[start, end]| [end, start]);
            }
            if !emitted.curves.contains_key(&curve_id(stream, curve_index)) {
                let mut definition = None;
                let mut reason = "carrier fields are unsupported";
                let mut projected = geometry::curve(&nodes, source);
                let mut interior_isoline = false;
                let construction = geometry::trim_basis(&nodes, source);
                if projected.is_none() && construction.kind() == kind::INTERSECTION {
                    projected = crate::intersection::nurbs_isoline(ctx, &nodes, construction)?;
                    interior_isoline = projected.is_some();
                    if interior_isoline {
                        losses.push(SolidEdgeLossCode::IntersectionNurbsIsolineAssumed.note(
                            "the unique NURBS isoline contained in the support plane and containing both chart witnesses supplies the intersection branch",
                        ));
                    }
                }
                if projected.is_none() && construction.kind() == kind::INTERSECTION {
                    match crate::chart::project(ctx, &nodes, construction) {
                        Ok(mut chart)
                            if chart.supports.iter().all(|index| nodes.contains_key(index)) =>
                        {
                            let owner = curve_id(stream, curve_index);
                            if let Some(fit) = &chart.fit {
                                fit_tolerances.insert(owner.clone(), fit.fit_tolerance.get());
                            }
                            if let Some((departure, moved)) = crate::trace::conform(
                                ctx,
                                &nodes,
                                &mut chart,
                                endpoints.and_then(|[start, end]| {
                                    Some([
                                        vertex_position(&nodes, start)?,
                                        vertex_position(&nodes, end)?,
                                    ])
                                }),
                                document_linear,
                            )? {
                                if moved {
                                    // The curve holds the vertex positions, so the
                                    // trim admits only the carrier's departure.
                                    fit_tolerances.insert(owner.clone(), departure);
                                }
                                if departure > document_linear {
                                    departures.insert(owner, departure);
                                    losses.push(SolidEdgeLossCode::EdgeToleranceWidened.note(
                                        "an intersection carrier departs from its support surfaces by more than the document tolerance; the edge states that departure as its tolerance",
                                    ));
                                }
                            }
                            match cadmpeg_ir::geometry::intersection_curves::IntcurveSupportContext::try_new(
                                chart.supports.map(|support| {
                                    cadmpeg_ir::geometry::intersection_curves::IntcurveSupportSide {
                                        surface: Some(surface_id(stream, support)),
                                        pcurve: None,
                                    }
                                }),
                                chart.range,
                                [Vec::new(), Vec::new(), Vec::new()],
                            ) {
                                Ok(context) => {
                                    definition = Some(cadmpeg_ir::geometry::ProceduralCurve::new(
                                        construction_id(stream, curve_index),
                                        cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition::Intersection {
                                            context,
                                            discontinuity_flag: false,
                                            cache: chart.fit,
                                        },
                                    ));
                                    projected = Some(CurveGeometry::Solved(
                                        SolvedCurveGeometry::Nurbs(chart.curve),
                                    ));
                                    losses.push(SolidEdgeLossCode::IntersectionChartReduced.note(
                                        "surface-intersection construction and chart cache are retained, the chart cache moved onto its support surfaces; angular and parameter error fields remain unmapped",
                                    ));
                                }
                                Err(_) => {
                                    reason = "intersection support context is invalid";
                                }
                            }
                        }
                        Ok(_) => reason = "intersection support references do not resolve",
                        Err(crate::chart::ChartError::Data(message)) => reason = message,
                        Err(crate::chart::ChartError::Refusal(error)) => return Err(error),
                    }
                } else if source.kind() == kind::INTERSECTION {
                    if crate::intersection::frame_from_chart(&nodes, source) {
                        losses.push(SolidEdgeLossCode::IntersectionFrameFromChart.note("intersection support surfaces determine the exact carrier; chart order determines its parameter direction"));
                    } else {
                        losses.push(SolidEdgeLossCode::IntersectionFrameFromSupportsAssumed.note(
                            "ordered oriented support normals set the curve direction because its chart is unresolved",
                        ));
                    }
                    if crate::intersection::planar_nurbs_support(&nodes, source) {
                        losses.push(SolidEdgeLossCode::IntersectionPlanarSupportAssumed.note(
                            "coplanar NURBS poles define a plane for the intersection carrier",
                        ));
                    } else if !interior_isoline
                        && crate::intersection::support_kinds(&nodes, source)
                            .contains(&kind::B_SURFACE)
                    {
                        losses.push(SolidEdgeLossCode::IntersectionNurbsBoundaryAssumed.note(
                            "the unique clamped NURBS boundary contained in the support plane supplies the intersection branch",
                        ));
                    }
                }
                let geometry = projected.unwrap_or_else(|| {
                    losses.push(SolidEdgeLossCode::CurveUnsupported.note(format!(
                        "curve node type {} has an unsupported carrier with support types {:?}: {reason}",
                        source.kind(), crate::intersection::support_kinds(&nodes, construction),
                    )));
                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None })
                });
                let owner = curve_id(stream, curve_index);
                emitted.push_curve(
                    &mut ir,
                    Curve {
                        id: owner.clone(),
                        geometry,
                        parameter_range: None,
                        source_object: None,
                        native_ref: None,
                    },
                );
                if let Some(definition) = definition {
                    if let Err(error) =
                        cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.geometry)
                            .add_curve(&mut ir.geometry, &owner, definition)?
                    {
                        losses.push(SolidEdgeLossCode::CurveConstructionDropped.note(format!(
                            "the surface-intersection construction is not admitted: {error}"
                        )));
                    }
                }
            }
            carrier = EdgeCarrier::Curve(curve_id(stream, curve_index));
        } else if let Some((sp, stated, range)) =
            first.and_then(|first| crate::sp_curve::locate(&nodes, first))
        {
            let target = crate::sp_curve::FIT_TOLERANCE_MM.min(0.5 * document_linear);
            if let Some(sampled) = crate::sp_curve::sample(ctx, &nodes, sp, range, target)? {
                let owner = curve_id(stream, stated);
                if !emitted.curves.contains_key(&owner) {
                    fit_tolerances.insert(owner.clone(), sampled.fit_tolerance);
                    if sampled.departure > document_linear {
                        departures.insert(owner.clone(), sampled.departure);
                        losses.push(SolidEdgeLossCode::EdgeToleranceWidened.note(
                            "a sampled tolerant-edge carrier departs from its curve by more than the document tolerance; the edge states that departure as its tolerance",
                        ));
                    }
                    emitted.push_curve(
                        &mut ir,
                        Curve {
                            id: owner.clone(),
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                                sampled.curve,
                            )),
                            parameter_range: None,
                            source_object: None,
                            native_ref: None,
                        },
                    );
                    losses.push(SolidEdgeLossCode::EdgeCarrierSampled.note(
                        "a tolerant edge states its curve in the parameter space of its support surface; the model-space carrier is that curve sampled through the support",
                    ));
                }
                carrier = EdgeCarrier::Curve(owner);
            } else {
                losses.push(SolidEdgeLossCode::CurveUnsupported.note(
                    "a tolerant edge's parameter-space curve does not sample to a model-space carrier",
                ));
            }
        }
        let ends = match endpoints {
            Some([start, end]) => Some(EdgeEnds::new(
                vertex_id(stream, start),
                vertex_id(stream, end),
            )),
            None => ring_vertex(ctx, &mut ir, &mut emitted, stream, edge, &carrier)?,
        };
        if ends.is_none() && carrier.curve().is_none() {
            // A vertexless edge bounds no extent without a carrier.
            dropped_edges.insert(edge);
            continue;
        }
        let stated = tolerance(node, losses);
        let departure = carrier
            .curve()
            .and_then(|curve| departures.get(curve).copied());
        ir.geometry.edges.push(Edge {
            id: edge_id(stream, edge),
            carrier,
            ends,
            tolerance: widened(stated, departure, document_linear),
            native_ref: None,
        });
    }
    let mut fins = BTreeMap::new();
    for face in face_reads {
        let mut loop_ids = Vec::new();
        let mut face_loops = Vec::new();
        let mut usable = true;
        for (loop_index, shape) in face.loops {
            let boundary = match shape {
                LoopShape::Pole(vertex) => LoopBoundary::Vertex {
                    vertex: vertex_id(stream, vertex),
                    pcurves: Vec::new(),
                },
                LoopShape::Ring(ring) => {
                    if ring.iter().any(|fin| dropped_edges.contains(&fin.edge)) {
                        usable = false;
                        break;
                    }
                    let Ok(ring_boundary) = LoopRing::new(
                        ring.iter().map(|fin| coedge_id(stream, fin.fin)).collect(),
                        Vec::new(),
                    ) else {
                        usable = false;
                        break;
                    };
                    face_loops.extend(ring.iter().map(|fin| (fin.fin, loop_index, fin.edge)));
                    LoopBoundary::Ring(ring_boundary)
                }
            };
            loop_ids.push(loop_id(stream, loop_index));
            ir.geometry.loops.push(Loop {
                id: loop_id(stream, loop_index),
                face: face_id(stream, face.index),
                boundary,
                native_ref: None,
            });
        }
        if !usable {
            losses.push(
                SolidEdgeLossCode::LoopUnreadable
                    .note("a face whose loop uses an edge with no carrier is not transferred"),
            );
            // The loops already admitted for this face leave with it.
            let face_id = face_id(stream, face.index);
            ir.geometry.loops.retain(|loop_| loop_.face != face_id);
            continue;
        }
        fins.extend(
            face_loops
                .into_iter()
                .map(|(fin, loop_index, edge)| (fin, (loop_index, edge))),
        );
        let mut face_sense = face.sense;
        if let Some(carrier) = nodes.get(&face.surface).copied() {
            let negative_major = matches!(
                geometry::analytic(carrier),
                Some(cadmpeg_parasolid::geometry::Analytic::Surface {
                    negative_major: true,
                    ..
                })
            );
            if (carrier.char("sense") == Some(b'-')) != negative_major {
                face_sense = reverse(face_sense);
            }
        }
        let Some(face_node) = nodes.get(&face.index).copied() else {
            continue;
        };
        ir.geometry.faces.push(Face {
            id: face_id(stream, face.index),
            shell: shell_id(stream, face.shell),
            surface: surface_id(stream, face.surface),
            sense: face_sense,
            loops: FaceLoops::unspecified(loop_ids),
            name: None,
            color: None,
            tolerance: tolerance(face_node, losses),
            native_ref: None,
        });
    }
    for (fin, (loop_index, edge)) in &fins {
        let (Some(node), Some(edge_node)) = (nodes.get(fin).copied(), nodes.get(edge).copied())
        else {
            continue;
        };
        let Some(mut use_sense) = sense(node) else {
            continue;
        };
        let curve = ptr(edge_node, "curve")
            .and_then(|index| nodes.get(&index))
            .copied();
        if curve.is_some_and(|curve| curve.char("sense") == Some(b'-')) {
            use_sense = reverse(use_sense);
    let populated: BTreeSet<&ShellId> = ir.geometry.faces.iter().map(|face| &face.shell).collect();
    for shells in region_face_shells.values() {
        if !shells
            .iter()
            .any(|shell| populated.contains(&shell_id(stream, *shell)))
        {
            losses.push(
                SolidEdgeLossCode::RegionFacesUntransferred
                    .note("a region owns faces and none of them is transferred"),
            );
        }
    }
        }
        // The next fin around the edge that a transferred loop uses.
        let mut radial = *fin;
        let mut seen = BTreeSet::new();
        let mut walker = ptr(node, "other");
        while let Some(other) = walker {
            if fins.contains_key(&other) {
                radial = other;
                break;
            }
            if !seen.insert(other) {
                break;
            }
            walker = nodes.get(&other).and_then(|node| ptr(*node, "other"));
        }
        ir.geometry.coedges.push(Coedge {
            id: coedge_id(stream, *fin),
            owner_loop: loop_id(stream, *loop_index),
            edge: edge_id(stream, *edge),
            radial_next: coedge_id(stream, radial),
            sense: use_sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
    }
    trim_edges(ctx, &mut ir, &fit_tolerances, document_linear, losses)?;
    Ok(ir)
}

/// A `SWEPT_SURF` with a null scale as the linear sweep of its section curve
/// along its unit direction, at unit parameter speed. The section curve joins
/// the model when no edge has introduced it yet. A stated scale, a section
/// that has no curve carrier, or a degenerate sweep vector leaves the surface
/// unsolved.
fn swept_surface(
    nodes: &Nodes<'_>,
    ir: &mut CadIr,
    emitted: &mut Emitted,
    stream: u32,
    index: u32,
    node: Node<'_>,
) -> Result<bool, CodecError> {
    let (Some(section), Some(direction)) = (ptr(node, "section"), node.vector("sweep")) else {
        return Ok(false);
    };
    // A null scale leaves the sweep parameter equal to the distance along the
    // unit direction. The role of a stated scale is open (DR-1053), so such a
    // surface keeps an unsupported carrier.
    if node.is_null("scale") != Some(true) {
        return Ok(false);
    }
    let curve = curve_id(stream, section);
    if !emitted.curves.contains_key(&curve) {
        let Some(geometry) = nodes
            .get(&section)
            .and_then(|section| geometry::curve(nodes, *section))
        else {
            return Ok(false);
        };
        emitted.push_curve(
            ir,
            Curve {
                id: curve.clone(),
                geometry,
                parameter_range: None,
                source_object: None,
                native_ref: None,
            },
        );
    }
    let Ok(definition) =
        cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
            curve,
            cadmpeg_ir::math::Vector3::new(direction[0], direction[1], direction[2]),
        )
    else {
        return Ok(false);
    };
    let owner = surface_id(stream, index);
    let construction = cadmpeg_ir::ids::ProceduralSurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "brep", "surface-construction"),
        (u64::from(stream) << 32) | u64::from(index),
    );
    let carrier = Surface {
        id: owner.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
        native_ref: None,
    };
    ir.geometry.surfaces.push(carrier);
    let admitted = cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.geometry)
        .add_surface(
            &mut ir.geometry,
            &owner,
            cadmpeg_ir::geometry::ProceduralSurface::new(
                construction,
                cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition::LinearSweep(
                    definition,
                ),
                None,
            ),
        )?;
    if admitted.is_err() {
        // The surface keeps an unsupported carrier.
        ir.geometry.surfaces.pop();
        return Ok(false);
    }
    Ok(true)
}

/// The vertex that bounds a vertexless ring edge. Parasolid states a closed
/// edge on a periodic carrier without a vertex; the neutral topology bounds it
/// at the carrier's origin parameter, one vertex serving as both ends.
fn ring_vertex(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    emitted: &mut Emitted,
    stream: u32,
    edge: u32,
    carrier: &EdgeCarrier,
) -> Result<Option<EdgeEnds>, CodecError> {
    let Some(curve) = carrier.curve() else {
        return Ok(None);
    };
    let Some(geometry) = emitted
        .curve(ir, curve)
        .and_then(|value| value.geometry.solved())
    else {
        return Ok(None);
    };
    let parameter = match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => 0.0,
        SolvedCurveGeometry::Nurbs(nurbs) if nurbs.periodic() => {
            match cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs) {
                Some(domain) => domain.endpoints()[0],
                None => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    let Some(position) = cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::curve_point_solved(ctx, geometry, parameter),
    )?
    else {
        return Ok(None);
    };
    let point = point_id(stream, edge);
    let vertex = vertex_id(stream, edge);
    emitted.points.insert(point.clone());
    ir.geometry
        .points
        .push(Point::new(point.clone(), position, None));
    ir.geometry.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: None,
        native_ref: None,
    });
    Ok(Some(EdgeEnds::new(vertex.clone(), vertex)))
}

/// Places each edge's endpoints on its carrier. A carrier fitted through a
/// native chart admits its stated fit tolerance as the endpoint residual,
/// because the chart polyline departs from the exact intersection by that
/// amount; other carriers admit the document tolerance.
fn trim_edges(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    fit_tolerances: &BTreeMap<CurveId, f64>,
    document_linear: f64,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let mut carriers = Vec::new();
    let mut reversed = BTreeSet::new();
    for edge in &ir.geometry.edges {
        let Some(curve) = edge.carrier.curve() else {
            carriers.push(edge.carrier.clone());
            continue;
        };
        let Some(ends) = &edge.ends else {
            carriers.push(edge.carrier.clone());
            continue;
        };
        let positions: Option<Vec<_>> = [&ends.start, &ends.end]
            .into_iter()
            .map(|id| {
                let vertex = index.vertices(id.as_str(), StandardIndex)?;
                Some(
                    index
                        .points(vertex.point.as_str(), StandardIndex)?
                        .position()
                        .get(),
                )
            })
            .collect();
        let range = if let Some(positions) = positions {
            let admitted = fit_tolerances
                .get(curve)
                .map_or(document_linear, |fit| fit.max(document_linear));
            let start = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index_with_tolerance(
                ctx,
                &index,
                curve,
                positions[0],
                0.0,
                admitted,
            )?;
            let end = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index_with_tolerance(
                ctx,
                &index,
                curve,
                positions[1],
                start.map_or(0.0, cadmpeg_ir::scalar::FiniteReal::get),
                admitted,
            )?;
            start.zip(end).map(|(start, end)| [start.get(), end.get()])
        } else {
            None
        };
        let range = range.map(|[mut start, mut end]| {
            let period = index
                .curves(curve.as_str(), StandardIndex)
                .and_then(|value| match value.geometry.solved()? {
                    SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
                        Some([0.0, std::f64::consts::TAU])
                    }
                    SolvedCurveGeometry::Nurbs(nurbs) if nurbs.periodic() => {
                        cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)
                            .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
                    }
                    _ => None,
                });
            if let Some([lower, upper]) = period {
                [start, end] = periodic_range([start, end], lower, upper - lower);
            }
            if end < start {
                reversed.insert(edge.id.clone());
            }
            [start.min(end), start.max(end)]
        });
        if range.is_none() {
            losses.push(
                SolidEdgeLossCode::EdgeTrimUnresolved
                    .note("edge endpoints could not be inverted on the carrier"),
            );
        }
        carriers.push(
            EdgeCarrier::new(Some(curve.clone()), range).unwrap_or_else(|_| {
                // A closed or zero-length edge on a nonperiodic carrier has
                // equal parameters at both ends; the carrier stays unbounded.
                losses.push(SolidEdgeLossCode::EdgeTrimUnresolved.note(
                    "edge endpoints give an empty parameter range on the carrier; the edge keeps the whole carrier",
                ));
                EdgeCarrier::Curve(curve.clone())
            }),
        );
    }
    for (edge, carrier) in ir.geometry.edges.iter_mut().zip(carriers) {
        edge.carrier = carrier;
        if reversed.contains(&edge.id) {
            if let Some(ends) = &mut edge.ends {
                std::mem::swap(&mut ends.start, &mut ends.end);
            }
        }
    }
    for coedge in &mut ir.geometry.coedges {
        if reversed.contains(&coedge.edge) {
            coedge.sense = reverse(coedge.sense);
        }
    }
    Ok(())
}

fn periodic_range([start, end]: [f64; 2], lower: f64, period: f64) -> [f64; 2] {
    let mut start = lower + (start - lower).rem_euclid(period);
    // A start a rounding step below `lower` wraps to exactly one period above
    // it, outside the canonical turn `[lower, lower + period)`.
    if start >= lower + period {
        start -= period;
    }
    let mut end = lower + (end - lower).rem_euclid(period);
    if end <= start {
        end += period;
    }
    [start, end]
}

#[cfg(test)]
mod tests {
    use super::{curve_id, edge_id, periodic_range, point_id, trim_edges, vertex_id};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier, EdgeEnds, Point, Vertex};
    use cadmpeg_ir::CadIr;
    use std::collections::BTreeMap;

    #[test]
    fn closed_edge_on_a_nonperiodic_carrier_trims_without_failing() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0; 4], &arena, &policy).expect("context");
        let mut ir = CadIr::empty();
        let point = point_id(0, 1);
        ir.geometry.points.push(Point::new(
            point.clone(),
            FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite"),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex_id(0, 1),
            point,
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.curves.push(Curve {
            id: curve_id(0, 1),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                    .expect("line"),
            )),
            parameter_range: None,
            source_object: None,
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge_id(0, 1),
            carrier: EdgeCarrier::Curve(curve_id(0, 1)),
            ends: Some(EdgeEnds::new(vertex_id(0, 1), vertex_id(0, 1))),
            tolerance: None,
            native_ref: None,
        });
        let mut losses = Vec::new();
        trim_edges(&ctx, &mut ir, &BTreeMap::new(), 1.0e-6, &mut losses).expect("trim");
        assert_eq!(ir.geometry.edges[0].carrier.curve(), Some(&curve_id(0, 1)));
        assert!(losses.is_empty());
        ctx.finish_session().expect("session");
    }
    #[test]
    fn periodic_trim_starts_in_the_canonical_turn() {
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        assert_eq!(
            periodic_range([-FRAC_PI_2, FRAC_PI_2], 0.0, TAU),
            [PI + FRAC_PI_2, TAU + FRAC_PI_2]
        );
        assert_eq!(periodic_range([-PI, PI], 0.0, TAU), [PI, PI + TAU]);
        assert_eq!(periodic_range([0.0, TAU], 0.0, TAU), [0.0, TAU]);
        assert_eq!(periodic_range([-1.0, 6.0], 5.0, 4.0), [7.0, 10.0]);
        assert_eq!(
            periodic_range([-1.0e-17, FRAC_PI_2], 0.0, TAU),
            [0.0, FRAC_PI_2]
        );
        assert_eq!(periodic_range([-1.0e-17, TAU], 0.0, TAU), [0.0, TAU]);
    }
}
