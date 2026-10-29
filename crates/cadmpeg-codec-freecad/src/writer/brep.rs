// SPDX-License-Identifier: Apache-2.0
//! Neutral topology to subshape-first OCCT text `BRep`.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::{Body, BodyKind, Sense};
use std::collections::HashMap;

use super::geometry;
mod reachable;
use reachable::Reach;

const LOCATION_SIMILARITY_TOLERANCE: f64 = 1.0e-12;

struct Shape {
    kind: &'static str,
    geometry: String,
    children: Vec<(usize, Sense)>,
}

/// Actual kernel indexed uses produced by this serialization. These are
/// ephemeral writer joins, not persisted native backlink fields.
pub(super) struct EmittedTopologyUse {
    pub(super) neutral_id: String,
    pub(super) kind: crate::brep::TextShapeKind,
    pub(super) native_shape: usize,
}

pub(super) struct SerializedShape {
    pub(super) bytes: Vec<u8>,
    pub(super) uses: Vec<EmittedTopologyUse>,
}

#[derive(Clone, Copy)]
enum RootKind<'a> {
    Container(crate::brep::TextShapeKind),
    Face {
        region: &'a str,
        shell: &'a str,
        face: &'a str,
    },
}

/// Current root roles resolved once from actual typed native-use bindings.
/// This plan holds identities/kinds only, never a previous geometry value.
pub(super) struct RootKinds<'a> {
    rows: Vec<(&'a str, RootKind<'a>)>,
}

impl<'a> RootKinds<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        ir: &'a CadIr,
        current: &crate::topology_transfer::CurrentTopologyBindings,
    ) -> Result<Self, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut regions = Vec::new();
        ctx.reserve_working(
            &mut regions,
            ir.model.regions.len(),
            "FreeCAD root region index",
        )?;
        regions.extend(
            ir.model
                .regions
                .iter()
                .map(|record| (record.id.as_str(), record)),
        );
        regions.sort_unstable_by(|a, b| a.0.cmp(b.0));
        let mut shells = Vec::new();
        ctx.reserve_working(
            &mut shells,
            ir.model.shells.len(),
            "FreeCAD root shell index",
        )?;
        shells.extend(
            ir.model
                .shells
                .iter()
                .map(|record| (record.id.as_str(), record)),
        );
        shells.sort_unstable_by(|a, b| a.0.cmp(b.0));
        let mut faces = Vec::new();
        ctx.reserve_working(&mut faces, ir.model.faces.len(), "FreeCAD root face index")?;
        faces.extend(
            ir.model
                .faces
                .iter()
                .map(|record| (record.id.as_str(), record)),
        );
        faces.sort_unstable_by(|a, b| a.0.cmp(b.0));
        let mut rows = Vec::new();
        ctx.reserve_working(
            &mut rows,
            ir.model.bodies.len(),
            "FreeCAD current root kinds",
        )?;
        for body in &ir.model.bodies {
            let Some(native) = current.native_for_record(body.id.as_str()) else {
                continue;
            };
            let Some(kind) = current.source_kind(native) else {
                continue;
            };
            match kind {
                crate::brep::TextShapeKind::Compound | crate::brep::TextShapeKind::CompSolid => {
                    rows.push((body.id.as_str(), RootKind::Container(kind)));
                }
                crate::brep::TextShapeKind::Face => {
                    if body.kind != BodyKind::Sheet {
                        continue;
                    }
                    let [region_id] = body.regions.as_slice() else {
                        continue;
                    };
                    let Ok(at) = regions.binary_search_by(|row| row.0.cmp(region_id.as_str()))
                    else {
                        continue;
                    };
                    let region = regions[at].1;
                    if region.body != body.id
                        || current.native_for_record(region.id.as_str()) != Some(native)
                    {
                        continue;
                    }
                    let [shell_id] = region.shells.as_slice() else {
                        continue;
                    };
                    let Ok(at) = shells.binary_search_by(|row| row.0.cmp(shell_id.as_str())) else {
                        continue;
                    };
                    let shell = shells[at].1;
                    if shell.region != region.id
                        || current.native_for_record(shell.id.as_str()) != Some(native)
                        || !shell.wire_edges().is_empty()
                        || !shell.free_vertices().is_empty()
                    {
                        continue;
                    }
                    let [face_id] = shell.faces() else {
                        continue;
                    };
                    let Ok(at) = faces.binary_search_by(|row| row.0.cmp(face_id.as_str())) else {
                        continue;
                    };
                    let face = faces[at].1;
                    if face.shell != shell.id
                        || current.native_for_record(face.id.as_str()) != Some(native)
                    {
                        continue;
                    }
                    rows.push((
                        body.id.as_str(),
                        RootKind::Face {
                            region: region.id.as_str(),
                            shell: shell.id.as_str(),
                            face: face.id.as_str(),
                        },
                    ));
                }
                _ => {}
            }
        }
        rows.sort_unstable_by(|a, b| a.0.cmp(b.0));
        Ok(Self { rows })
    }

    fn get(&self, body: &str) -> Option<RootKind<'a>> {
        self.rows
            .binary_search_by(|row| row.0.cmp(body))
            .ok()
            .map(|at| self.rows[at].1)
    }
}

/// Geometry emits topology events without owning an allocation policy. The
/// production observer admits its temporary metadata through the actual caller.
trait TopologyObserver {
    fn record(&mut self, id: &str, index: usize) -> Result<(), CodecError>;
    fn finish(
        &mut self,
        shapes: &[Shape],
        indices: &HashMap<&str, usize>,
        roots: &[(usize, usize)],
    ) -> Result<(), CodecError>;
}

struct EmittedTopology<'ctx, 'input> {
    ctx: &'ctx DecodeContext<'input>,
    indices: Vec<(String, usize)>,
    uses: Vec<EmittedTopologyUse>,
}

impl TopologyObserver for EmittedTopology<'_, '_> {
    fn record(&mut self, id: &str, index: usize) -> Result<(), CodecError> {
        self.ctx.reserve_working(
            &mut self.indices,
            1,
            "FreeCAD emitted topology record index",
        )?;
        let mut identity = String::new();
        self.ctx.reserve_text_working(
            &mut identity,
            id.len(),
            "FreeCAD emitted topology record identity",
        )?;
        identity.push_str(id);
        self.indices.push((identity, index));
        Ok(())
    }

    fn finish(
        &mut self,
        shapes: &[Shape],
        indices: &HashMap<&str, usize>,
        roots: &[(usize, usize)],
    ) -> Result<(), CodecError> {
        for (id, index) in indices {
            self.record(id, *index)?;
        }
        self.uses = emitted_topology_uses(self.ctx, shapes, &self.indices, roots)?;
        Ok(())
    }
}

pub(super) fn serialize_with_uses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bodies: &[&Body],
    roots: &RootKinds<'_>,
) -> Result<Option<SerializedShape>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut topology = EmittedTopology {
        ctx,
        indices: Vec::new(),
        uses: Vec::new(),
    };
    let bytes = serialize_geometry(ir, bodies, roots, &mut topology)?;
    Ok(bytes.map(|bytes| SerializedShape {
        bytes,
        uses: topology.uses,
    }))
}

fn serialize_geometry(
    ir: &CadIr,
    bodies: &[&Body],
    roots: &RootKinds<'_>,
    topology: &mut impl TopologyObserver,
) -> Result<Option<Vec<u8>>, CodecError> {
    if bodies.iter().any(|body| {
        body.transform
            .is_some_and(|transform| !location_supported(transform))
    }) {
        return Ok(None);
    }
    serialize_geometry_inner(ir, bodies, false, roots, topology)
}

pub(super) fn unowned_with_uses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    roots: &RootKinds<'_>,
) -> Result<Option<SerializedShape>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut topology = EmittedTopology {
        ctx,
        indices: Vec::new(),
        uses: Vec::new(),
    };
    let bytes = unowned_geometry(ir, losses, roots, &mut topology)?;
    Ok(bytes.map(|bytes| SerializedShape {
        bytes,
        uses: topology.uses,
    }))
}

fn unowned_geometry(
    ir: &CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    roots: &RootKinds<'_>,
    topology: &mut impl TopologyObserver,
) -> Result<Option<Vec<u8>>, CodecError> {
    let reachable = Reach::unowned(ir);
    for curve in ir
        .model
        .curves
        .iter()
        .filter(|curve| reachable.curves.contains(curve.id.as_str()))
    {
        if geometry::curve_record(ir, curve).is_none() {
            losses.push(
                crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "standalone curve {} has no OCCT encoding and is omitted",
                    curve.id
                )),
            );
        }
    }
    for surface in ir
        .model
        .surfaces
        .iter()
        .filter(|surface| reachable.surfaces.contains(surface.id.as_str()))
    {
        if geometry::surface_record(ir, surface).is_none() {
            losses.push(
                crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "standalone surface {} has no OCCT encoding and is omitted",
                    surface.id
                )),
            );
        }
    }
    for pcurve in ir.model.pcurves.iter().filter(|pcurve| {
        !ir.model
            .coedges
            .iter()
            .any(|coedge| coedge.pcurves.iter().any(|usage| usage.pcurve == pcurve.id))
    }) {
        losses.push(
            crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                "unbound parameter-space curve {} is omitted",
                pcurve.id
            )),
        );
    }
    let bytes = serialize_geometry_inner(ir, &[], true, roots, topology)?;
    if bytes.is_none()
        && (!reachable.faces.is_empty()
            || !reachable.edges.is_empty()
            || !reachable.vertices.is_empty())
    {
        losses.push(
            crate::loss::FreecadLossCode::ExportGeometryNotTransferred
                .note("independent topology has an unsupported required carrier and is omitted"),
        );
    }
    Ok(bytes)
}

fn serialize_geometry_inner(
    ir: &CadIr,
    bodies: &[&Body],
    free: bool,
    root_kinds: &RootKinds<'_>,
    topology: &mut impl TopologyObserver,
) -> Result<Option<Vec<u8>>, CodecError> {
    let reachable = if free {
        Reach::unowned(ir)
    } else {
        Reach::bodies(ir, bodies)
    };
    let Reach {
        faces,
        loops,
        coedges,
        edges,
        vertices,
        curves: curves_needed,
        surfaces: surfaces_needed,
        pcurves: _pcurves_needed,
    } = reachable;
    let mut locations = Vec::new();
    let mut curves = Vec::new();
    let mut curve_ids = HashMap::new();
    let mut curve_slots = HashMap::new();
    for curve in ir
        .model
        .curves
        .iter()
        .filter(|curve| curves_needed.contains(curve.id.as_str()))
    {
        if let Some((row, location)) = geometry::located_curve(ir, curve) {
            if !location_supported(location)
                || (free
                    && location != cadmpeg_ir::transform::Transform::identity()
                    && !ir.model.edges.iter().any(|edge| {
                        edges.contains(edge.id.as_str()) && edge.curve() == Some(&curve.id)
                    }))
            {
                continue;
            }
            let key = geometry::curve_layout_key(ir, curve);
            let index = *curve_slots.entry(key).or_insert_with(|| {
                curves.push(row);
                curves.len()
            });
            curve_ids.insert(
                curve.id.as_str(),
                (index, register_location(location, &mut locations)),
            );
        }
    }
    let mut surfaces = Vec::new();
    let mut surface_ids = HashMap::new();
    let mut surface_slots = HashMap::new();
    let trimmed_supports = trimmed_supports(ir, &surfaces_needed);
    let mut support_charts = Vec::new();
    for surface in ir
        .model
        .surfaces
        .iter()
        .filter(|surface| surfaces_needed.contains(surface.id.as_str()))
    {
        if let Some((row, location)) = geometry::located_surface(ir, surface) {
            if let Some(trimmed) = trimmed_supports.get(surface.id.as_str()) {
                if location == cadmpeg_ir::transform::Transform::identity() {
                    support_charts.push((surface.id.as_str(), *trimmed, row));
                    continue;
                }
            }
            if !location_supported(location)
                || (free
                    && location != cadmpeg_ir::transform::Transform::identity()
                    && !ir
                        .model
                        .faces
                        .iter()
                        .any(|face| faces.contains(face.id.as_str()) && face.surface == surface.id))
            {
                continue;
            }
            let affine = crate::brep::generated_surface_parameters(&row)?;
            let Some(chart) = cadmpeg_ir::transform::Transform2::affine([
                [1.0 / affine.u_scale, 0.0, -affine.u_offset / affine.u_scale],
                [0.0, 1.0 / affine.v_scale, -affine.v_offset / affine.v_scale],
            ]) else {
                continue;
            };
            let key = geometry::surface_layout_key(ir, surface);
            let index = *surface_slots.entry(key).or_insert_with(|| {
                surfaces.push(row);
                surfaces.len()
            });
            surface_ids.insert(
                surface.id.as_str(),
                (index, register_location(location, &mut locations), chart),
            );
        }
    }
    // A face on the support of a trimmed surface uses the trimmed row, which
    // keeps the support's parameters.
    for (support, trimmed, row) in support_charts {
        let affine = crate::brep::generated_surface_parameters(&row)?;
        let (index, location) = match surface_ids.get(trimmed) {
            Some(&(index, location, _)) => (index, location),
            None => {
                surfaces.push(row);
                (
                    surfaces.len(),
                    register_location(cadmpeg_ir::transform::Transform::identity(), &mut locations),
                )
            }
        };
        let Some(chart) = cadmpeg_ir::transform::Transform2::affine([
            [1.0 / affine.u_scale, 0.0, -affine.u_offset / affine.u_scale],
            [0.0, 1.0 / affine.v_scale, -affine.v_offset / affine.v_scale],
        ]) else {
            continue;
        };
        surface_ids.insert(support, (index, location, chart));
    }
    let mut pcurves = Vec::new();
    let mut pcurve_ids = HashMap::new();
    let mut shared_pcurves = HashMap::new();
    for coedge in ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedges.contains(coedge.id.as_str()))
    {
        let Some(face) = ir
            .model
            .loops
            .iter()
            .find(|wire| wire.id == coedge.owner_loop)
            .and_then(|wire| ir.model.faces.iter().find(|face| face.id == wire.face))
        else {
            continue;
        };
        let Some((_, _, chart)) = surface_ids.get(face.surface.as_str()) else {
            continue;
        };
        for usage in &coedge.pcurves {
            let Some(pcurve) = ir
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == usage.pcurve)
            else {
                continue;
            };
            if pcurve_ids.contains_key(&(pcurve.id.as_str(), face.surface.as_str())) {
                continue;
            }
            let domain = pcurve
                .parameter_range()
                .map(cadmpeg_ir::units::FiniteVector::get)
                .or_else(|| {
                    usage
                        .parameter_range
                        .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
                });
            if let Some(row) = super::pcurves::serialize(&pcurve.geometry, domain, *chart) {
                let index =
                    if let Some(index) = shared_pcurves.get(&(pcurve.id.as_str(), row.clone())) {
                        *index
                    } else {
                        pcurves.push(row.clone());
                        shared_pcurves.insert((pcurve.id.as_str(), row), pcurves.len());
                        pcurves.len()
                    };
                pcurve_ids.insert((pcurve.id.as_str(), face.surface.as_str()), index);
            }
        }
    }
    let mut shapes = Vec::new();
    let mut indices = HashMap::new();
    // OCCT carries a numeric tolerance. Zero preserves the absence of an
    // entity-specific positive tolerance instead of inventing document defaults.
    let tolerance = ir.tolerances.linear.get();
    for vertex in ir
        .model
        .vertices
        .iter()
        .filter(|vertex| vertices.contains(vertex.id.as_str()))
    {
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "vertex {} references absent point {}",
                    vertex.id, vertex.point
                ))
            })?;
        indices.insert(vertex.id.as_str(), shapes.len());
        shapes.push(Shape {
            kind: "Ve",
            geometry: format!(
                "{} {} 0 0",
                vertex
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
                geometry::point(point.position().get())
            ),
            children: Vec::new(),
        });
    }
    if free {
        for point in ir.model.points.iter().filter(|point| {
            !ir.model
                .vertices
                .iter()
                .any(|vertex| vertex.point == point.id)
        }) {
            indices.insert(point.id.as_str(), shapes.len());
            shapes.push(Shape {
                kind: "Ve",
                geometry: format!(
                    "{tolerance} {} 0 0",
                    geometry::point(point.position().get())
                ),
                children: Vec::new(),
            });
        }
    }
    for edge in ir
        .model
        .edges
        .iter()
        .filter(|edge| edges.contains(edge.id.as_str()))
    {
        let Some(start) = indices.get(edge.start.as_str()).copied() else {
            return Err(CodecError::malformed(format_args!(
                "edge {} references absent start vertex",
                edge.id
            )));
        };
        let Some(end) = indices.get(edge.end.as_str()).copied() else {
            return Err(CodecError::malformed(format_args!(
                "edge {} references absent end vertex",
                edge.id
            )));
        };
        let degenerate = edge.curve().is_none()
            || edge
                .curve()
                .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
                .and_then(|curve| curve.geometry.solved())
                .is_some_and(|curve| {
                    matches!(
                        curve,
                        cadmpeg_ir::geometry::SolvedCurveGeometry::Degenerate(_)
                    )
                });
        let mut representation = if degenerate {
            String::new()
        } else {
            match edge.curve() {
                Some(curve) => {
                    let Some((curve, location)) = curve_ids.get(curve.as_str()) else {
                        continue;
                    };
                    let Some(range) = edge.param_range() else {
                        continue;
                    };
                    let geometry = edge
                        .curve()
                        .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
                        .and_then(|curve| curve.geometry.solved());
                    let Some(range) = occt_curve_range(geometry, range.get()) else {
                        continue;
                    };
                    format!("1 {curve} {location} {} {}", range[0], range[1])
                }
                None if start == end => String::new(),
                None => continue,
            }
        };
        let mut complete = true;
        let mut seen = std::collections::HashSet::new();
        let mut pcurve_groups =
            std::collections::BTreeMap::<(usize, usize), Vec<(usize, [f64; 2], Sense)>>::new();
        for coedge in ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id && coedges.contains(coedge.id.as_str()))
        {
            let face = ir
                .model
                .loops
                .iter()
                .find(|wire| wire.id == coedge.owner_loop)
                .and_then(|wire| ir.model.faces.iter().find(|face| face.id == wire.face))
                .ok_or_else(|| CodecError::malformed("coedge has an absent loop or face"))?;
            let Some((surface, surface_location, _)) = surface_ids.get(face.surface.as_str())
            else {
                complete = false;
                continue;
            };
            for usage in &coedge.pcurves {
                if !seen.insert((face.surface.as_str(), usage.pcurve.as_str())) {
                    continue;
                }
                let Some(curve) = pcurve_ids.get(&(usage.pcurve.as_str(), face.surface.as_str()))
                else {
                    complete = false;
                    continue;
                };
                let range = usage
                    .parameter_range
                    .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
                    .or_else(|| {
                        ir.model
                            .pcurves
                            .iter()
                            .find(|curve| curve.id == usage.pcurve)
                            .and_then(cadmpeg_ir::geometry::pcurve::Pcurve::parameter_range)
                            .map(cadmpeg_ir::units::FiniteVector::get)
                    })
                    .or_else(|| edge.param_range().map(cadmpeg_ir::units::FiniteVector::get));
                let Some(range) = range else {
                    complete = false;
                    continue;
                };
                if range[0] > range[1] {
                    complete = false;
                    continue;
                }
                pcurve_groups
                    .entry((*surface, *surface_location))
                    .or_default()
                    .push((*curve, range, coedge.sense));
            }
        }
        for ((surface, location), mut rows) in pcurve_groups {
            rows.sort_by_key(|(_, _, sense)| *sense == Sense::Reversed);
            match rows.as_slice() {
                [(curve, range, _)] => {
                    representation.push_str(" 2 ");
                    representation.push_str(&curve.to_string());
                    representation.push(' ');
                    representation.push_str(&surface.to_string());
                    representation.push(' ');
                    representation.push_str(&location.to_string());
                    representation.push(' ');
                    representation.push_str(&range[0].to_string());
                    representation.push(' ');
                    representation.push_str(&range[1].to_string());
                }
                [(first, range, _), (second, other_range, _)] if range == other_range => {
                    representation.push_str(" 3 ");
                    representation.push_str(&first.to_string());
                    representation.push(' ');
                    representation.push_str(&second.to_string());
                    representation.push_str(" C0 ");
                    representation.push_str(&surface.to_string());
                    representation.push(' ');
                    representation.push_str(&location.to_string());
                    representation.push(' ');
                    representation.push_str(&range[0].to_string());
                    representation.push(' ');
                    representation.push_str(&range[1].to_string());
                }
                _ => complete = false,
            }
        }
        if !complete {
            continue;
        }
        indices.insert(edge.id.as_str(), shapes.len());
        shapes.push(Shape {
            kind: "Ed",
            geometry: format!(
                "{} 1 1 {} {representation} 0",
                edge.tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
                u8::from(degenerate)
            ),
            children: vec![(start, Sense::Forward), (end, Sense::Reversed)],
        });
    }
    for wire in ir
        .model
        .loops
        .iter()
        .filter(|wire| loops.contains(wire.id.as_str()))
    {
        let mut children = Vec::new();
        let mut complete = true;
        for coedge in wire.coedges() {
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|value| value.id == *coedge)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("loop {} references absent coedge", wire.id))
                })?;
            if let Some(index) = indices.get(coedge.edge.as_str()) {
                children.push((*index, coedge.sense));
            } else {
                complete = false;
            }
        }
        if complete {
            indices.insert(wire.id.as_str(), shapes.len());
            shapes.push(Shape {
                kind: "Wi",
                geometry: String::new(),
                children,
            });
        }
    }
    for face in ir
        .model
        .faces
        .iter()
        .filter(|face| faces.contains(face.id.as_str()))
    {
        let Some((surface, surface_location, _)) = surface_ids.get(face.surface.as_str()) else {
            continue;
        };
        let mut children = Vec::new();
        let mut complete = true;
        for wire in &face.loops {
            if let Some(index) = indices.get(wire.as_str()) {
                // Neutral coedge senses already include the face orientation.
                // Cancel that parent orientation on the wire reference.
                children.push((*index, face.sense));
            } else {
                complete = false;
            }
        }
        if complete {
            indices.insert(face.id.as_str(), shapes.len());
            shapes.push(Shape {
                kind: "Fa",
                geometry: format!(
                    "0 {} {surface} {surface_location}",
                    face.tolerance
                        .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                ),
                children,
            });
        }
    }
    let mut roots = Vec::new();
    for body in bodies {
        if let Some(RootKind::Face {
            region,
            shell,
            face,
        }) = root_kinds.get(body.id.as_str())
        {
            let Some(&face) = indices.get(face) else {
                return Ok(None);
            };
            topology.record(region, face)?;
            topology.record(shell, face)?;
            topology.record(body.id.as_str(), face)?;
            let location = register_location(body.transform.unwrap_or_default(), &mut locations);
            roots.push((face, location));
            continue;
        }
        let mut regions = Vec::new();
        for region_id in &body.regions {
            let region = ir
                .model
                .regions
                .iter()
                .find(|value| value.id == *region_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("body {} references absent region", body.id))
                })?;
            let mut shells = Vec::new();
            for shell_id in &region.shells {
                let shell = ir
                    .model
                    .shells
                    .iter()
                    .find(|value| value.id == *shell_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "region {} references absent shell",
                            region.id
                        ))
                    })?;
                let mut children = Vec::new();
                for face_id in shell.faces() {
                    let Some(index) = indices.get(face_id.as_str()) else {
                        return Ok(None);
                    };
                    let face = ir
                        .model
                        .faces
                        .iter()
                        .find(|value| value.id == *face_id)
                        .ok_or_else(|| CodecError::malformed("shell references absent face"))?;
                    children.push((*index, face.sense));
                }
                for edge in shell.wire_edges() {
                    let Some(index) = indices.get(edge.as_str()) else {
                        return Ok(None);
                    };
                    children.push((*index, Sense::Forward));
                }
                for vertex in shell.free_vertices() {
                    let Some(index) = indices.get(vertex.as_str()) else {
                        return Ok(None);
                    };
                    children.push((*index, Sense::Forward));
                }
                let index = shapes.len();
                topology.record(shell.id.as_str(), index)?;
                shapes.push(Shape {
                    kind: if shell.faces().is_empty() { "Co" } else { "Sh" },
                    geometry: String::new(),
                    children,
                });
                shells.push((index, Sense::Forward));
            }
            let index = shapes.len();
            topology.record(region.id.as_str(), index)?;
            shapes.push(Shape {
                kind: if body.kind == BodyKind::Solid
                    || (body.kind == BodyKind::General
                        && region.shells.iter().all(|id| {
                            ir.model
                                .shells
                                .iter()
                                .any(|shell| shell.id == *id && !shell.faces().is_empty())
                        }))
                {
                    "So"
                } else {
                    "Co"
                },
                geometry: String::new(),
                children: shells,
            });
            regions.push((index, Sense::Forward));
        }
        if regions.is_empty() {
            return Ok(None);
        }
        let root = if matches!(
            root_kinds.get(body.id.as_str()),
            Some(RootKind::Container(crate::brep::TextShapeKind::CompSolid))
        ) && regions.iter().all(|(index, _)| shapes[*index].kind == "So")
        {
            let root = shapes.len();
            shapes.push(Shape {
                kind: "CS",
                geometry: String::new(),
                children: regions,
            });
            root
        } else if regions.len() == 1 {
            let region = regions[0].0;
            if body.kind == BodyKind::Sheet && shapes[region].children.len() == 1 {
                shapes[region].children[0].0
            } else if body.kind == BodyKind::Wire {
                let children = shapes[region]
                    .children
                    .iter()
                    .flat_map(|(shell, _)| shapes[*shell].children.clone())
                    .collect();
                let root = shapes.len();
                shapes.push(Shape {
                    kind: "Wi",
                    geometry: String::new(),
                    children,
                });
                // The existing wire serializer flattens its helper container.
                // With exactly one current shell, that shell's complete member
                // sequence is precisely this actual Wi; no other shell use is
                // claimed by the aggregate output.
                if let Some(current_region) = ir
                    .model
                    .regions
                    .iter()
                    .find(|value| value.id == body.regions[0])
                {
                    if current_region.shells.len() == 1 {
                        topology.record(current_region.shells[0].as_str(), root)?;
                    }
                }
                root
            } else if body.kind == BodyKind::General && shapes[region].kind == "So" {
                let root = shapes.len();
                shapes.push(Shape {
                    kind: "Co",
                    geometry: String::new(),
                    children: vec![(region, Sense::Forward)],
                });
                root
            } else {
                region
            }
        } else {
            let root = shapes.len();
            shapes.push(Shape {
                kind: "Co",
                geometry: String::new(),
                children: regions,
            });
            root
        };
        let root = match root_kinds.get(body.id.as_str()) {
            Some(RootKind::Container(crate::brep::TextShapeKind::Compound))
                if shapes[root].kind != "Co" =>
            {
                let index = shapes.len();
                shapes.push(Shape {
                    kind: "Co",
                    geometry: String::new(),
                    children: vec![(root, Sense::Forward)],
                });
                index
            }
            _ => root,
        };
        let location = register_location(body.transform.unwrap_or_default(), &mut locations);
        topology.record(body.id.as_str(), root)?;
        roots.push((root, location));
    }
    if free {
        for point in ir.model.points.iter().filter(|point| {
            !ir.model
                .vertices
                .iter()
                .any(|vertex| vertex.point == point.id)
        }) {
            if let Some(index) = indices.get(point.id.as_str()) {
                roots.push((*index, 0));
            }
        }
        for face in ir
            .model
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
        {
            let Some(index) = indices.get(face.id.as_str()) else {
                return Ok(None);
            };
            roots.push((*index, 0));
        }
        for wire in ir.model.loops.iter().filter(|wire| {
            loops.contains(wire.id.as_str())
                && !ir.model.faces.iter().any(|face| {
                    faces.contains(face.id.as_str()) && face.loops.iter().any(|id| id == &wire.id)
                })
        }) {
            let Some(index) = indices.get(wire.id.as_str()) else {
                return Ok(None);
            };
            roots.push((*index, 0));
        }
        for edge in
            ir.model.edges.iter().filter(|edge| {
                edges.contains(edge.id.as_str())
                    && !ir.model.coedges.iter().any(|coedge| {
                        coedges.contains(coedge.id.as_str()) && coedge.edge == edge.id
                    })
            })
        {
            let Some(index) = indices.get(edge.id.as_str()) else {
                return Ok(None);
            };
            roots.push((*index, 0));
        }
        for vertex in ir.model.vertices.iter().filter(|vertex| {
            vertices.contains(vertex.id.as_str())
                && !ir.model.edges.iter().any(|edge| {
                    edges.contains(edge.id.as_str())
                        && (edge.start == vertex.id || edge.end == vertex.id)
                })
        }) {
            let Some(index) = indices.get(vertex.id.as_str()) else {
                return Ok(None);
            };
            roots.push((*index, 0));
        }
        if roots.is_empty() && curves.is_empty() && surfaces.is_empty() {
            return Ok(None);
        }
    }
    topology.finish(&shapes, &indices, &roots)?;
    let mut output = format!("\nCASCADE Topology V3, (c) Open Cascade\nLocations {}\n{}\nCurve2ds {}\n{}\nCurves {}\n{}\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces {}\n{}\nTriangulations 0\nTShapes {}\n", locations.len(), locations.join("\n"), pcurves.len(), pcurves.join("\n"), curves.len(), curves.join("\n"), surfaces.len(), surfaces.join("\n"), shapes.len());
    for shape in &shapes {
        output.push_str(shape.kind);
        output.push('\n');
        output.push_str(&shape.geometry);
        output.push_str("\n\n0101101\n");
        for (index, sense) in &shape.children {
            output.push(if *sense == Sense::Forward { '+' } else { '-' });
            output.push_str(&(shapes.len() - index).to_string());
            output.push_str(" 0 ");
        }
        output.push_str("*\n");
    }
    if roots.is_empty() {
        output.push_str("*\n");
    }
    for (root, location) in roots {
        output.push('+');
        output.push_str(&(shapes.len() - root).to_string());
        output.push(' ');
        output.push_str(&location.to_string());
        output.push('\n');
    }
    Ok(Some(output.into_bytes()))
}

pub(super) fn remember_uses(
    ctx: &DecodeContext<'_>,
    all: &mut Vec<(String, Vec<EmittedTopologyUse>)>,
    owner: &str,
    uses: Vec<EmittedTopologyUse>,
) -> Result<(), CodecError> {
    ctx.reserve_working(all, 1, "FreeCAD emitted topology owners")?;
    let mut key = String::new();
    ctx.reserve_text_working(&mut key, owner.len(), "FreeCAD emitted topology owner")?;
    key.push_str(owner);
    all.push((key, uses));
    Ok(())
}

fn emitted_topology_uses(
    ctx: &DecodeContext<'_>,
    shapes: &[Shape],
    indices: &[(String, usize)],
    roots: &[(usize, usize)],
) -> Result<Vec<EmittedTopologyUse>, CodecError> {
    use crate::brep::TextShapeKind;
    fn kind(code: &str) -> Option<TextShapeKind> {
        match code {
            "Ve" => Some(TextShapeKind::Vertex),
            "Ed" => Some(TextShapeKind::Edge),
            "Wi" => Some(TextShapeKind::Wire),
            "Fa" => Some(TextShapeKind::Face),
            "Sh" => Some(TextShapeKind::Shell),
            "So" => Some(TextShapeKind::Solid),
            "CS" => Some(TextShapeKind::CompSolid),
            "Co" => Some(TextShapeKind::Compound),
            _ => None,
        }
    }
    // A stored TShape is not itself an emitted use: flattened wire bodies can
    // leave helper shells/regions unreachable. Follow actual output links once,
    // without expanding each shared shape again for every root placement.
    let mut reachable = Vec::new();
    ctx.reserve_working(
        &mut reachable,
        shapes.len(),
        "FreeCAD emitted topology reachability",
    )?;
    reachable.resize(shapes.len(), false);
    let mut stack = Vec::new();
    ctx.reserve_working(&mut stack, roots.len(), "FreeCAD emitted topology roots")?;
    stack.extend(roots.iter().map(|(root, _)| *root));
    while let Some(index) = stack.pop() {
        let Some(visited) = reachable.get_mut(index) else {
            continue;
        };
        if *visited {
            continue;
        }
        *visited = true;
        let Some(shape) = shapes.get(index) else {
            continue;
        };
        ctx.reserve_working(
            &mut stack,
            shape.children.len(),
            "FreeCAD emitted topology children",
        )?;
        stack.extend(shape.children.iter().map(|(index, _)| *index));
    }
    let mut result = Vec::new();
    ctx.reserve_working(&mut result, indices.len(), "FreeCAD emitted topology uses")?;
    for (id, index) in indices {
        if !reachable.get(*index).copied().unwrap_or(false) {
            continue;
        }
        let Some(shape) = shapes.get(*index) else {
            continue;
        };
        let Some(kind) = kind(shape.kind) else {
            continue;
        };
        let Some(native_shape) = shapes.len().checked_sub(*index) else {
            continue;
        };
        let mut neutral_id = String::new();
        ctx.reserve_text_working(
            &mut neutral_id,
            id.len(),
            "FreeCAD emitted topology identity",
        )?;
        neutral_id.push_str(id);
        result.push(EmittedTopologyUse {
            neutral_id,
            kind,
            native_shape,
        });
    }
    Ok(result)
}

/// Surfaces that are the inline support of a needed trimmed surface, keyed by
/// support id.
fn trimmed_supports<'a>(
    ir: &'a CadIr,
    needed: &std::collections::HashSet<String>,
) -> HashMap<&'a str, &'a str> {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
    let mut supports = HashMap::new();
    for surface in ir
        .model
        .surfaces
        .iter()
        .filter(|surface| needed.contains(surface.id.as_str()))
    {
        let Some(construction) = surface.geometry.procedural_construction() else {
            continue;
        };
        let Some(ProceduralSurfaceDefinition::Subset(subset)) = ir
            .model
            .procedural_surfaces
            .iter()
            .find(|record| record.id == *construction)
            .map(|record| record.definition())
        else {
            continue;
        };
        if geometry::surface_record(ir, surface).is_some_and(|row| row.starts_with("10 "))
            && needed.contains(subset.support().as_str())
        {
            supports
                .entry(subset.support().as_str())
                .or_insert(surface.id.as_str());
        }
    }
    supports
}

/// Map a neutral curve parameter range to the OCCT parameterization that
/// `topology_transfer::normalize_occt_curve_range` reads.
fn occt_curve_range(
    curve: Option<&cadmpeg_ir::geometry::SolvedCurveGeometry>,
    range: [f64; 2],
) -> Option<[f64; 2]> {
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::scalar::FiniteReal;
    match curve {
        Some(SolvedCurveGeometry::Parabola(parabola)) => {
            let focal_distance = parabola.focal_distance().magnitude();
            let [start, end] = range.map(FiniteReal::new);
            Some([
                cadmpeg_ir::math::multiply_divide(start?, focal_distance, FiniteReal::HALF)?.get(),
                cadmpeg_ir::math::multiply_divide(end?, focal_distance, FiniteReal::HALF)?.get(),
            ])
        }
        Some(SolvedCurveGeometry::Transformed(placed)) => {
            occt_curve_range(Some(placed.basis()), range)
        }
        _ => Some(range),
    }
}

fn register_location(
    transform: cadmpeg_ir::transform::Transform,
    locations: &mut Vec<String>,
) -> usize {
    if transform == cadmpeg_ir::transform::Transform::identity() {
        return 0;
    }
    let row = format!(
        "1\n{}",
        transform
            .affine_rows()
            .iter()
            .map(|row| row
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if let Some(index) = locations.iter().position(|existing| existing == &row) {
        return index + 1;
    }
    locations.push(row);
    locations.len()
}

/// OCCT Locations hold similarities, not arbitrary affine deformations.
fn location_supported(transform: cadmpeg_ir::transform::Transform) -> bool {
    let rows = transform.affine_rows();
    let dot = |a: usize, b: usize| (0..3).map(|row| rows[row][a] * rows[row][b]).sum::<f64>();
    let scale = dot(0, 0);
    scale.is_finite()
        && scale > 0.0
        && (1..3)
            .all(|axis| (dot(axis, axis) - scale).abs() <= LOCATION_SIMILARITY_TOLERANCE * scale)
        && [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .all(|(a, b)| dot(a, b).abs() <= LOCATION_SIMILARITY_TOLERANCE * scale)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::writer) fn serialize(
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        bodies: &[&Body],
    ) -> Result<Option<Vec<u8>>, CodecError> {
        struct NoTopology;
        impl TopologyObserver for NoTopology {
            fn record(&mut self, _: &str, _: usize) -> Result<(), CodecError> {
                Ok(())
            }

            fn finish(
                &mut self,
                _: &[Shape],
                _: &HashMap<&str, usize>,
                _: &[(usize, usize)],
            ) -> Result<(), CodecError> {
                Ok(())
            }
        }
        let current = crate::topology_transfer::current_bindings(ctx, ir)?;
        let roots = RootKinds::new(ctx, ir, &current)?;
        serialize_geometry(ir, bodies, &roots, &mut NoTopology)
    }
}
