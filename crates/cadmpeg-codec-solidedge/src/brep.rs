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
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{Magnification, PositiveLength, PositiveReal};
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

fn malformed(message: &str) -> CodecError {
    CodecError::Malformed(message.into())
}
fn ptr(node: Node<'_>, field: &str) -> Option<u32> {
    node.pointer(field).filter(|index| *index > 1)
}
fn required(node: Node<'_>, field: &str) -> Result<u32, CodecError> {
    ptr(node, field).ok_or_else(|| malformed("unresolved Parasolid node reference"))
}
fn sense(node: Node<'_>) -> Result<Sense, CodecError> {
    match node.char("sense") {
        Some(b'+') => Ok(Sense::Forward),
        Some(b'-') => Ok(Sense::Reversed),
        _ => Err(malformed("invalid Parasolid sense")),
    }
}
fn reverse(value: Sense) -> Sense {
    match value {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    }
}
fn tolerance(node: Node<'_>, losses: &mut Vec<LossNote>) -> Option<PositiveReal> {
    if node.is_null("tolerance") == Some(true) {
        return None;
    }
    let raw = node.real("tolerance");
    if raw == Some(0.0) {
        return None;
    }
    let value = raw.and_then(|value| PositiveReal::new(value * MM_PER_METRE));
    if value.is_none() {
        losses.push(
            SolidEdgeLossCode::TopologyToleranceInvalid
                .note("invalid topology tolerance uses the document tolerance"),
        );
    }
    value
}

fn chain(nodes: &Nodes<'_>, start: Option<u32>, link: &str) -> Result<Vec<u32>, CodecError> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = start;
    while let Some(index) = current {
        if !seen.insert(index) {
            return Err(malformed("cyclic Parasolid ownership chain"));
        }
        let node = nodes
            .get(&index)
            .copied()
            .ok_or_else(|| malformed("unresolved Parasolid ownership chain"))?;
        result.push(index);
        current = ptr(node, link);
    }
    Ok(result)
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
        let mut regions = Vec::new();
        for region in nodes.values().copied().filter(|node| {
            node.kind() == kind::REGION
                && ptr(*node, "body") == Some(body.index())
                && node.char("type") == Some(b'S')
        }) {
            let shells = chain(&nodes, ptr(region, "shell"), "next")?;
            for shell in &shells {
                selected_shells.insert(*shell, region.index());
            }
            regions.push(Region {
                id: region_id(stream, region.index()),
                body: body_id(stream, body.index()),
                shells: shells
                    .into_iter()
                    .map(|shell| shell_id(stream, shell))
                    .collect(),
                native_ref: None,
            });
        }
        if regions.is_empty() {
            let shells = chain(&nodes, ptr(body, "shell"), "next")?;
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
    let mut faces = BTreeSet::new();
    for (shell_index, region) in selected_shells {
        let shell = *nodes
            .get(&shell_index)
            .ok_or_else(|| malformed("unresolved shell"))?;
        if ptr(shell, "edge").is_some() || ptr(shell, "vertex").is_some() {
            losses.push(
                SolidEdgeLossCode::WireTopologyUnsupported
                    .note("free shell edges and vertices remain undecoded"),
            );
        }
        ir.geometry.shells.push(Shell::new(
            shell_id(stream, shell_index),
            region_id(stream, region),
        ));
        faces.extend(chain(&nodes, ptr(shell, "face"), "next")?);
    }
    let mut fins = BTreeSet::new();
    let mut edges = BTreeSet::new();
    let mut fit_tolerances: BTreeMap<CurveId, f64> = BTreeMap::new();
    let mut vertices = BTreeSet::new();
    let mut surfaces = BTreeSet::new();
    for face_index in faces {
        let face = *nodes
            .get(&face_index)
            .ok_or_else(|| malformed("unresolved face"))?;
        let surface = required(face, "surface")?;
        surfaces.insert(surface);
        let loops = chain(&nodes, ptr(face, "loop"), "next")?;
        for loop_index in &loops {
            let loop_ = *nodes
                .get(loop_index)
                .ok_or_else(|| malformed("unresolved loop"))?;
            let first = required(loop_, "halfedge")?;
            let mut current = first;
            let mut ring = Vec::new();
            let mut seen = BTreeSet::new();
            let boundary = loop {
                if !seen.insert(current) {
                    return Err(malformed("Parasolid fin ring has a secondary cycle"));
                }
                let fin = *nodes
                    .get(&current)
                    .ok_or_else(|| malformed("unresolved fin"))?;
                if fin.char("sense") == Some(b'?')
                    && ptr(fin, "forward") == Some(current)
                    && ptr(fin, "backward") == Some(current)
                    && ptr(fin, "edge").is_none()
                {
                    let vertex = required(fin, "vertex")?;
                    vertices.insert(vertex);
                    break LoopBoundary::Vertex {
                        vertex: vertex_id(stream, vertex),
                        pcurves: Vec::new(),
                    };
                }
                fins.insert(current);
                edges.insert(required(fin, "edge")?);
                if let Some(vertex) = ptr(fin, "vertex") {
                    vertices.insert(vertex);
                }
                ring.push(coedge_id(stream, current));
                current = required(fin, "forward")?;
                if current == first {
                    break LoopBoundary::Ring(
                        LoopRing::new(ring, Vec::new()).map_err(CodecError::malformed)?,
                    );
                }
            };
            ir.geometry.loops.push(Loop {
                id: loop_id(stream, *loop_index),
                face: face_id(stream, face_index),
                boundary,
                native_ref: None,
            });
        }
        let carrier = *nodes
            .get(&surface)
            .ok_or_else(|| malformed("unresolved surface"))?;
        let mut face_sense = sense(face)?;
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
        ir.geometry.faces.push(Face {
            id: face_id(stream, face_index),
            shell: shell_id(stream, required(face, "shell")?),
            surface: surface_id(stream, surface),
            sense: face_sense,
            loops: FaceLoops::unspecified(
                loops
                    .into_iter()
                    .map(|index| loop_id(stream, index))
                    .collect(),
            ),
            name: None,
            color: None,
            tolerance: tolerance(face, losses),
            native_ref: None,
        });
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
        let node = *nodes
            .get(&surface)
            .ok_or_else(|| malformed("unresolved surface"))?;
        if node.kind() == kind::SWEPT_SURF && swept_surface(&nodes, &mut ir, stream, surface, node)?
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
    for vertex in vertices {
        let node = *nodes
            .get(&vertex)
            .ok_or_else(|| malformed("unresolved vertex"))?;
        let point = required(node, "point")?;
        let source = *nodes
            .get(&point)
            .ok_or_else(|| malformed("unresolved point"))?;
        let [x, y, z] = source
            .vector("pvec")
            .ok_or_else(|| malformed("invalid point"))?;
        let position = FinitePoint3::new(Point3::new(
            x * MM_PER_METRE,
            y * MM_PER_METRE,
            z * MM_PER_METRE,
        ))
        .ok_or_else(|| malformed("nonfinite point"))?;
        if !ir
            .geometry
            .points
            .iter()
            .any(|value| value.id == point_id(stream, point))
        {
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
    for fin in &fins {
        let node = nodes[fin];
        let edge = required(node, "edge")?;
        let edge_node = *nodes
            .get(&edge)
            .ok_or_else(|| malformed("unresolved edge"))?;
        let curve = ptr(edge_node, "curve")
            .and_then(|index| nodes.get(&index))
            .copied();
        let mut use_sense = sense(node)?;
        if curve.is_some_and(|curve| curve.char("sense") == Some(b'-')) {
            use_sense = reverse(use_sense);
        }
        let radial = ptr(node, "other")
            .filter(|other| fins.contains(other))
            .unwrap_or(*fin);
        ir.geometry.coedges.push(Coedge {
            id: coedge_id(stream, *fin),
            owner_loop: loop_id(stream, required(node, "loop")?),
            edge: edge_id(stream, edge),
            radial_next: coedge_id(stream, radial),
            sense: use_sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
    }
    for edge in edges {
        let node = nodes[&edge];
        let first = *nodes
            .get(&required(node, "halfedge")?)
            .ok_or_else(|| malformed("unresolved edge fin"))?;
        let previous = *nodes
            .get(&required(first, "backward")?)
            .ok_or_else(|| malformed("unresolved previous fin"))?;
        let mut endpoints = match (ptr(previous, "vertex"), ptr(first, "vertex")) {
            (Some(start), Some(end)) => {
                let ends = if sense(first)? == Sense::Forward {
                    [start, end]
                } else {
                    [end, start]
                };
                Some(ends)
            }
            (None, None) => None,
            _ => return Err(malformed("incomplete edge endpoints")),
        };
        let mut carrier = EdgeCarrier::Free;
        if let Some(curve_index) = ptr(node, "curve") {
            let source = *nodes
                .get(&curve_index)
                .ok_or_else(|| malformed("unresolved curve"))?;
            if source.char("sense") == Some(b'-') {
                endpoints = endpoints.map(|[start, end]| [end, start]);
            }
            if !ir
                .geometry
                .curves
                .iter()
                .any(|curve| curve.id == curve_id(stream, curve_index))
            {
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
                        Ok(chart)
                            if chart.supports.iter().all(|index| nodes.contains_key(index)) =>
                        {
                            if let Some(fit) = &chart.fit {
                                fit_tolerances
                                    .insert(curve_id(stream, curve_index), fit.fit_tolerance.get());
                            }
                            let context = cadmpeg_ir::geometry::intersection_curves::IntcurveSupportContext::try_new(
                                chart.supports.map(|support| {
                                    cadmpeg_ir::geometry::intersection_curves::IntcurveSupportSide {
                                        surface: Some(surface_id(stream, support)),
                                        pcurve: None,
                                    }
                                }),
                                chart.range,
                                [Vec::new(), Vec::new(), Vec::new()],
                            )
                            .map_err(malformed)?;
                            definition = Some(cadmpeg_ir::geometry::ProceduralCurve::new(
                                construction_id(stream, curve_index),
                                cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition::Intersection {
                                    context,
                                    discontinuity_flag: false,
                                    cache: chart.fit,
                                },
                            ));
                            projected = Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                                chart.curve,
                            )));
                            losses.push(SolidEdgeLossCode::IntersectionChartReduced.note(
                                "surface-intersection construction and chart cache are retained; angular and parameter error fields remain unmapped",
                            ));
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
                ir.geometry.curves.push(Curve {
                    id: owner.clone(),
                    geometry,
                    parameter_range: None,
                    source_object: None,
                    native_ref: None,
                });
                if let Some(definition) = definition {
                    cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.geometry)
                        .add_curve(&mut ir.geometry, &owner, definition)?
                        .map_err(|error| CodecError::Malformed(error.to_string()))?;
                }
            }
            carrier = EdgeCarrier::Curve(curve_id(stream, curve_index));
        } else if let Some((sp, stated, range)) = crate::sp_curve::locate(&nodes, first) {
            if let Some(sampled) = crate::sp_curve::sample(ctx, &nodes, sp, range)? {
                let owner = curve_id(stream, stated);
                if !ir.geometry.curves.iter().any(|curve| curve.id == owner) {
                    fit_tolerances.insert(owner.clone(), sampled.fit_tolerance);
                    ir.geometry.curves.push(Curve {
                        id: owner.clone(),
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(sampled.curve)),
                        parameter_range: None,
                        source_object: None,
                        native_ref: None,
                    });
                    losses.push(SolidEdgeLossCode::EdgeCarrierSampled.note(
                        "a tolerant edge states its curve in the parameter space of its support surface; the model-space carrier is that curve sampled through the support",
                    ));
                }
                carrier = EdgeCarrier::Curve(owner);
            }
        }
        let ends = match endpoints {
            Some([start, end]) => Some(EdgeEnds::new(
                vertex_id(stream, start),
                vertex_id(stream, end),
            )),
            None => ring_vertex(ctx, &mut ir, stream, edge, &carrier)?,
        };
        ir.geometry.edges.push(Edge {
            id: edge_id(stream, edge),
            carrier,
            ends,
            tolerance: tolerance(node, losses),
            native_ref: None,
        });
    }
    trim_edges(ctx, &mut ir, &fit_tolerances, losses)?;
    if let Some(linear) = linear {
        ir.tolerances.linear = linear;
    }
    Ok(ir)
}

/// A `SWEPT_SURF` as the linear sweep of its section curve. The stored sweep
/// is a unit direction and a length scale in metres, or a null scale for unit
/// parameter speed; the section curve joins
/// the model when no edge has introduced it yet. A section that has no curve
/// carrier or a degenerate sweep vector leaves the surface unsolved.
fn swept_surface(
    nodes: &Nodes<'_>,
    ir: &mut CadIr,
    stream: u32,
    index: u32,
    node: Node<'_>,
) -> Result<bool, CodecError> {
    let (Some(section), Some(direction), Some(scale)) = (
        ptr(node, "section"),
        node.vector("sweep"),
        node.real("scale"),
    ) else {
        return Ok(false);
    };
    // A null scale leaves the sweep parameter equal to the distance along the
    // unit direction, in the document length unit.
    let length = if node.is_null("scale") == Some(true) {
        1.0
    } else {
        scale * MM_PER_METRE
    };
    let curve = curve_id(stream, section);
    if !ir.geometry.curves.iter().any(|value| value.id == curve) {
        let Some(geometry) = nodes
            .get(&section)
            .and_then(|section| geometry::curve(nodes, *section))
        else {
            return Ok(false);
        };
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            geometry,
            parameter_range: None,
            source_object: None,
            native_ref: None,
        });
    }
    let Ok(definition) =
        cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
            curve,
            cadmpeg_ir::math::Vector3::new(
                direction[0] * length,
                direction[1] * length,
                direction[2] * length,
            ),
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
    cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.geometry)
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
        )?
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok(true)
}

/// The vertex that bounds a vertexless ring edge. Parasolid states a closed
/// edge on a periodic carrier without a vertex; the neutral topology bounds it
/// at the carrier's origin parameter, one vertex serving as both ends.
fn ring_vertex(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    stream: u32,
    edge: u32,
    carrier: &EdgeCarrier,
) -> Result<Option<EdgeEnds>, CodecError> {
    let Some(curve) = carrier.curve() else {
        return Ok(None);
    };
    let Some(geometry) = ir
        .geometry
        .curves
        .iter()
        .find(|value| &value.id == curve)
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
                let vertex = ir
                    .geometry
                    .vertices
                    .iter()
                    .find(|vertex| &vertex.id == id)?;
                Some(
                    ir.geometry
                        .points
                        .iter()
                        .find(|point| point.id == vertex.point)?
                        .position()
                        .get(),
                )
            })
            .collect();
        let range = if let Some(positions) = positions {
            let admitted = fit_tolerances
                .get(curve)
                .map_or(ir.tolerances.linear.get(), |fit| {
                    fit.max(ir.tolerances.linear.get())
                });
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
            let period = ir
                .geometry
                .curves
                .iter()
                .find(|value| &value.id == curve)
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
        carriers.push(EdgeCarrier::new(Some(curve.clone()), range).map_err(malformed)?);
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
    use super::periodic_range;
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
