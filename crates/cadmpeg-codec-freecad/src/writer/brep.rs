// SPDX-License-Identifier: Apache-2.0
//! Neutral topology to subshape-first OCCT text `BRep`.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::ExportLossCode;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Sense};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use super::geometry;
use crate::loss::FreecadLossCode;
mod native_text;
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
    current: &'a crate::topology_transfer::CurrentTopologyBindings,
}

impl<'a> RootKinds<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        ir: &'a CadIr,
        current: &'a crate::topology_transfer::CurrentTopologyBindings,
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
        Ok(Self { rows, current })
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

/// A payload whose typed tables still agree with the Model, emitted from
/// those tables (W1) with the topology uses its current neutral records bind.
/// An owner placement that moved off the stored root location is appended as
/// one more location, which the roots use.
pub(super) fn native_with_uses(
    payload: &crate::brep::ShapePayloadRecord,
    polygons: (
        &[crate::brep::Polygon3dRecord],
        &[crate::brep::PolygonOnTriangulationRecord],
    ),
    current: &crate::topology_transfer::CurrentTopologyBindings,
    root_override: Option<cadmpeg_ir::transform::Transform>,
) -> Option<SerializedShape> {
    let tables = crate::brep::Tables::from_payload(payload, polygons.0, polygons.1)
        .ok()
        .flatten()?;
    let mut root = None;
    if let Some(placement) = root_override {
        for use_ in tables.roots {
            if use_.location.resolve(tables.locations).ok()? != placement {
                root = Some(placement);
            }
        }
    }
    let text = native_text::text(payload, polygons.0, polygons.1, root)?;
    let count = tables.tshapes.len();
    let uses = current
        .payload_records(&payload.id)
        .filter_map(|(record, kind, shape, _)| {
            Some(EmittedTopologyUse {
                neutral_id: record.to_owned(),
                kind,
                native_shape: count.checked_sub(shape)?.checked_add(1)?,
            })
        })
        .collect();
    Some(SerializedShape {
        bytes: text.into_bytes(),
        uses,
    })
}

pub(super) fn serialize_with_uses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bodies: &[&Body],
    roots: &RootKinds<'_>,
    root_override: Option<cadmpeg_ir::transform::Transform>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<SerializedShape>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut topology = EmittedTopology {
        ctx,
        indices: Vec::new(),
        uses: Vec::new(),
    };
    let bytes = serialize_geometry(
        ctx,
        ir,
        bodies,
        roots,
        (root_override, &HashSet::new()),
        &mut topology,
        losses,
    )?;
    Ok(bytes.map(|bytes| SerializedShape {
        bytes,
        uses: topology.uses,
    }))
}

fn serialize_geometry(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bodies: &[&Body],
    roots: &RootKinds<'_>,
    placement_and_covered: (Option<cadmpeg_ir::transform::Transform>, &HashSet<String>),
    topology: &mut impl TopologyObserver,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Vec<u8>>, CodecError> {
    serialize_geometry_inner(
        ctx,
        ir,
        bodies,
        (false, roots, placement_and_covered.0),
        (placement_and_covered.1, &[]),
        topology,
        losses,
    )
}

pub(super) fn unowned_with_uses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    roots: &RootKinds<'_>,
    covered: &HashSet<String>,
    covered_points: &[&str],
) -> Result<Option<SerializedShape>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut topology = EmittedTopology {
        ctx,
        indices: Vec::new(),
        uses: Vec::new(),
    };
    let bytes = unowned_geometry(
        ctx,
        ir,
        losses,
        roots,
        covered,
        covered_points,
        &mut topology,
    )?;
    Ok(bytes.map(|bytes| SerializedShape {
        bytes,
        uses: topology.uses,
    }))
}

fn unowned_geometry(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    roots: &RootKinds<'_>,
    covered: &HashSet<String>,
    covered_points: &[&str],
    topology: &mut impl TopologyObserver,
) -> Result<Option<Vec<u8>>, CodecError> {
    let reachable = Reach::unowned(ir);
    for curve in ir
        .model
        .curves
        .iter()
        .filter(|curve| reachable.curves.contains(curve.id.as_str()))
        .filter(|curve| !covered_by_replay(covered, curve.source_object.as_ref()))
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
        .filter(|surface| !covered_by_replay(covered, surface.source_object.as_ref()))
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
    let bytes = serialize_geometry_inner(
        ctx,
        ir,
        &[],
        (true, roots, None),
        (covered, covered_points),
        topology,
        losses,
    )?;
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

/// Polygon tables re-emitted from the source records the written bodies'
/// payloads name, in list order (W1). Bodies without a source payload
/// contribute nothing. A list entry naming an absent record is dropped
/// with a records-omitted loss, so no stale table is written.
type PolygonSections = (Vec<String>, Vec<String>, HashMap<(String, usize), usize>);
type PcurveGroup<'a> = Vec<(usize, [f64; 2], Sense, &'a str)>;

fn polygon_sections(
    ir: &CadIr,
    bodies: &[&Body],
    faces: &HashSet<String>,
    edges: &HashSet<String>,
    losses: &mut Vec<LossNote>,
) -> Result<PolygonSections, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok((Vec::new(), Vec::new(), HashMap::new()));
    };
    let uses =
        namespace.arena_as::<crate::native::source_use::SourceUseSelector>("topology_uses")?;
    let payloads = namespace.arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")?;
    let polygons3d = namespace.arena_as::<crate::brep::Polygon3dRecord>("polygons3d")?;
    let indexed = namespace
        .arena_as::<crate::brep::PolygonOnTriangulationRecord>("polygons_on_triangulations")?;
    let mut seen = HashSet::new();
    let mut entries3d = Vec::new();
    let mut entries_indexed = Vec::new();
    let mut indexed_positions = HashMap::new();
    let natives = bodies
        .iter()
        .filter_map(|body| body.native_ref.as_deref())
        .chain(
            ir.model
                .faces
                .iter()
                .filter(|face| faces.contains(face.id.as_str()))
                .filter_map(|face| face.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .edges
                .iter()
                .filter(|edge| edges.contains(edge.id.as_str()))
                .filter_map(|edge| edge.native_ref.as_deref()),
        );
    for native in natives {
        let Some(payload) = uses
            .iter()
            .find(|use_record| use_record.id == native)
            .map(|use_record| use_record.owner.as_str())
            .and_then(|owner| payloads.iter().find(|payload| payload.id == owner))
        else {
            continue;
        };
        if !seen.insert(payload.id.as_str()) {
            continue;
        }
        let (table3d, table_indexed) = match &payload.payload {
            crate::brep::ShapePayload::Empty => continue,
            crate::brep::ShapePayload::Text { facts, .. }
            | crate::brep::ShapePayload::Binary { facts, .. } => {
                (&facts.polygons3d, &facts.polygons_on_triangulations)
            }
        };
        for id in table3d {
            match polygons3d.iter().find(|record| record.id == *id) {
                Some(record) => entries3d.push(crate::brep::polygon3d_text(record)),
                None => losses.push(ExportLossCode::NativeRecordsOmitted.note(format!(
                    "polygon record {id} named by {} is absent and its table entry is omitted",
                    payload.id
                ))),
            }
        }
        for (position, id) in table_indexed.iter().enumerate() {
            match indexed.iter().find(|record| record.id == *id) {
                Some(record) => {
                    entries_indexed.push(crate::brep::polygon_on_triangulation_text(record));
                    indexed_positions
                        .insert((payload.id.clone(), position + 1), entries_indexed.len());
                }
                None => losses.push(ExportLossCode::NativeRecordsOmitted.note(format!(
                    "polygon record {id} named by {} is absent and its table entry is omitted",
                    payload.id
                ))),
            }
        }
    }
    Ok((entries3d, entries_indexed, indexed_positions))
}

fn triangulation_sections(
    ir: &CadIr,
    bodies: &[&Body],
    faces: &HashSet<String>,
    free: bool,
    covered: &HashSet<String>,
    losses: &mut Vec<LossNote>,
) -> (Vec<String>, HashMap<String, usize>) {
    let mut rows = Vec::new();
    let mut face_rows = HashMap::new();
    for mesh in &ir.model.tessellations {
        if covered_by_replay(covered, mesh.source_object.as_ref()) {
            continue;
        }
        let selected = if free {
            mesh.faces.iter().any(|face| faces.contains(face.as_str()))
                || (mesh.faces.is_empty()
                    && mesh
                        .body
                        .as_ref()
                        .is_none_or(|body| ir.model.bodies.iter().all(|record| record.id != *body)))
        } else {
            mesh.faces.iter().any(|face| faces.contains(face.as_str()))
                || (mesh.faces.is_empty()
                    && mesh
                        .body
                        .as_ref()
                        .is_some_and(|body| bodies.iter().any(|record| record.id == *body)))
        };
        if !selected {
            continue;
        }
        let vertices = mesh.vertices();
        let triangles = mesh.triangles();
        let normals = mesh.vertex_normals();
        let has_normals = !normals.is_empty()
            && normals.len() == vertices.len()
            && normals.iter().all(|normal| {
                cadmpeg_ir::units::UnitVector3::normalized_nonzero(*normal).is_some()
            });
        if !normals.is_empty() && !has_normals {
            losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                "tessellation {} has a non-normalizable vertex normal and its normal lane is omitted", mesh.id
            )));
        }
        if !mesh.per_corner_normals().is_empty() || !mesh.texture_coordinates().is_empty() {
            losses.push(
                crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "tessellation {} has shading or texture data that is omitted",
                    mesh.id
                )),
            );
        }
        let mut row = format!(
            "{} {} 0 {} {}",
            vertices.len(),
            triangles.len(),
            u8::from(has_normals),
            mesh.chordal_deflection()
                .map_or(0.0, cadmpeg_ir::scalar::NonNegativeReal::get)
        );
        for vertex in vertices {
            write!(row, "\n{}", geometry::point(vertex.get())).expect("write to string");
        }
        for triangle in triangles {
            write!(
                row,
                "\n{} {} {}",
                u64::from(triangle[0]) + 1,
                u64::from(triangle[1]) + 1,
                u64::from(triangle[2]) + 1
            )
            .expect("write to string");
        }
        if has_normals {
            for normal in normals {
                let normal = normal.get();
                write!(row, "\n{} {} {}", normal.x, normal.y, normal.z).expect("write to string");
            }
        }
        rows.push(row);
        for face in &mesh.faces {
            face_rows
                .entry(face.as_str().to_owned())
                .or_insert(rows.len());
        }
    }
    (rows, face_rows)
}

/// Whether neutral carrier content replays through its native payload: a
/// record whose source object the writer replays with all its side entries
/// needs no independent `FreeGeometry` copy.
fn covered_by_replay(
    covered: &HashSet<String>,
    source: Option<&cadmpeg_ir::SourceObjectAssociation>,
) -> bool {
    source.is_some_and(|source| {
        source.format.as_str() == "fcstd" && covered.contains(source.object_id.as_str())
    })
}

fn serialize_geometry_inner(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bodies: &[&Body],
    (free, root_kinds, root_override): (
        bool,
        &RootKinds<'_>,
        Option<cadmpeg_ir::transform::Transform>,
    ),
    (covered, covered_points): (&HashSet<String>, &[&str]),
    topology: &mut impl TopologyObserver,
    losses: &mut Vec<LossNote>,
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
    } = reachable;
    let (triangulations, face_triangulations) =
        triangulation_sections(ir, bodies, &faces, free, covered, losses);
    let (mut polygons3d, mut polygons_indexed, indexed_positions) =
        polygon_sections(ir, bodies, &faces, &edges, losses)?;
    let sources = if polygons_indexed.is_empty() {
        None
    } else {
        super::polygon_sources::PolygonSources::new(ctx, ir)?
    };
    let mut polygon_representations = HashMap::new();
    let mut paired_curves = HashSet::new();
    for edge in ir
        .model
        .edges
        .iter()
        .filter(|edge| edges.contains(edge.id.as_str()))
    {
        let indexed = match &sources {
            Some(sources) => super::indexed_polygon::representation(
                ctx,
                ir,
                edge,
                sources,
                (&face_triangulations, covered),
                &mut polygons_indexed,
                &indexed_positions,
            )?,
            None => None,
        };
        let (indexed, paired_curve) = indexed.map_or((None, None), |(representation, curve)| {
            (Some(representation), curve)
        });
        let fallback = indexed
            .is_none()
            .then(|| super::polygon_edges::polygon3d(ir, edge, &mut polygons3d))
            .flatten();
        if fallback.is_some() {
            if let (Some(sources), Some(native)) = (&sources, edge.native_ref.as_deref()) {
                if let Some((_, source)) = sources.shape(ctx, native)? {
                    if matches!(&source.geometry, crate::brep::TextTShapeGeometry::Edge { representations, .. }
                        if representations.iter().any(|value| matches!(value,
                            crate::brep::TextEdgeRepresentation::PolygonOnTriangulation { .. }
                            | crate::brep::TextEdgeRepresentation::PolygonPair { .. })))
                    {
                        losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                            "edge {} keeps its polygon but its triangulation association cannot be written", edge.id
                        )));
                    }
                }
            }
        }
        if let Some(representation) = indexed.or(fallback) {
            if let Some(curve) = paired_curve {
                paired_curves.insert(curve);
            }
            polygon_representations.insert(edge.id.as_str(), representation);
        }
    }
    let mut locations = Vec::new();
    let mut curves = Vec::new();
    let mut curve_ids = HashMap::new();
    let mut curve_slots = HashMap::new();
    for curve in ir
        .model
        .curves
        .iter()
        .filter(|curve| curves_needed.contains(curve.id.as_str()))
        .filter(|curve| !covered_by_replay(covered, curve.source_object.as_ref()))
        .filter(|curve| !paired_curves.contains(curve.id.as_str()))
    {
        let mut edge_uses = ir
            .model
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()) && edge.curve() == Some(&curve.id))
            .peekable();
        if edge_uses.peek().is_some()
            && edge_uses.all(|edge| polygon_representations.contains_key(edge.id.as_str()))
        {
            continue;
        }
        let located = geometry::located_curve(ir, curve).or_else(|| {
            ir.model
                .edges
                .iter()
                .any(|edge| edges.contains(edge.id.as_str()) && edge.curve() == Some(&curve.id))
                .then(|| geometry::single_composite_line(ir, curve))
                .flatten()
                .and_then(|line| geometry::located_curve(ir, line))
        });
        if let Some((row, location)) = located {
            if !location_supported(location)
                || (free
                    && location != cadmpeg_ir::transform::Transform::identity()
                    && !ir.model.edges.iter().any(|edge| {
                        edges.contains(edge.id.as_str()) && edge.curve() == Some(&curve.id)
                    }))
            {
                continue;
            }
            if matches!(
                curve.geometry.solved(),
                Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Composite { .. })
            ) {
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "composite curve {} is written as its single segment",
                        curve.id
                    )),
                );
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
        .filter(|surface| !covered_by_replay(covered, surface.source_object.as_ref()))
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
    let tolerance = ir.tolerances.linear_or_default().get();
    for vertex in ir
        .model
        .vertices
        .iter()
        .filter(|vertex| vertices.contains(vertex.id.as_str()))
    {
        let Some(point) = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
        else {
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                "vertex {} references absent point {} and is omitted",
                vertex.id, vertex.point
            )));
            continue;
        };
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
            covered_points.binary_search(&point.id.as_str()).is_err()
                && !ir
                    .model
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
        let (Some(start), Some(end)) = (
            indices.get(edge.start.as_str()).copied(),
            indices.get(edge.end.as_str()).copied(),
        ) else {
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                "edge {} has unavailable endpoint {} or {} and is omitted",
                edge.id, edge.start, edge.end
            )));
            continue;
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
        } else if let Some(polygon) = polygon_representations.get(edge.id.as_str()) {
            polygon.clone()
        } else {
            match edge.curve() {
                Some(curve) => {
                    let Some((curve, location)) = curve_ids.get(curve.as_str()) else {
                        continue;
                    };
                    let curve_record = edge
                        .curve()
                        .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id));
                    let range = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get);
                    let range = if let Some(curve) = curve_record.filter(|curve| {
                        matches!(
                            curve.geometry.solved(),
                            Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Composite { .. })
                        )
                    }) {
                        composite_line_range(ir, edge, curve)
                            .filter(|derived| range.is_none_or(|stored| stored == *derived))
                    } else {
                        range
                    };
                    let Some(range) =
                        range.and_then(|range| occt_curve_range(ir, curve_record, range))
                    else {
                        continue;
                    };
                    format!("1 {curve} {location} {} {}", range[0], range[1])
                }
                None if start == end => String::new(),
                None => continue,
            }
        };
        let mut seen = std::collections::HashSet::new();
        let mut pcurve_groups: BTreeMap<_, PcurveGroup<'_>> = BTreeMap::new();
        for coedge in ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id && coedges.contains(coedge.id.as_str()))
        {
            if coedge.pcurves.is_empty() {
                continue;
            }
            let Some(face) = ir
                .model
                .loops
                .iter()
                .find(|wire| wire.id == coedge.owner_loop)
                .and_then(|wire| ir.model.faces.iter().find(|face| face.id == wire.face))
            else {
                for usage in &coedge.pcurves {
                    losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "pcurve {} on coedge {} has no available owning face and is omitted",
                        usage.pcurve, coedge.id
                    )));
                }
                continue;
            };
            let Some((surface, surface_location, _)) = surface_ids.get(face.surface.as_str())
            else {
                for usage in &coedge.pcurves {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                            "pcurve {} on coedge {} is omitted with unsupported face carrier {}",
                            usage.pcurve, coedge.id, face.id
                        )),
                    );
                }
                continue;
            };
            for usage in &coedge.pcurves {
                if !seen.insert((face.surface.as_str(), usage.pcurve.as_str())) {
                    continue;
                }
                let Some(curve) = pcurve_ids.get(&(usage.pcurve.as_str(), face.surface.as_str()))
                else {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                            "pcurve {} on coedge {} has no writable carrier",
                            usage.pcurve, coedge.id
                        )),
                    );
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
                    losses.push(
                        crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                            "pcurve {} on coedge {} has no parameter range",
                            usage.pcurve, coedge.id
                        )),
                    );
                    continue;
                };
                if range[0] > range[1] {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                            "pcurve {} on coedge {} has a reversed parameter range",
                            usage.pcurve, coedge.id
                        )),
                    );
                    continue;
                }
                pcurve_groups
                    .entry((*surface, *surface_location))
                    .or_default()
                    .push((*curve, range, coedge.sense, usage.pcurve.as_str()));
            }
        }
        for ((surface, location), mut rows) in pcurve_groups {
            rows.sort_by_key(|(_, _, sense, _)| *sense == Sense::Reversed);
            match rows.as_slice() {
                [(curve, range, _, _)] => {
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
                [(first, range, _, _), (second, other_range, _, _)] if range == other_range => {
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
                _ => {
                    if let Some((curve, [start, end], _, _)) = rows.first() {
                        write!(
                            representation,
                            " 2 {curve} {surface} {location} {start} {end}"
                        )
                        .expect("write to string");
                    }
                    for (_, _, _, pcurve) in rows.into_iter().skip(1) {
                        losses.push(
                            crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(
                                format!(
                                    "pcurve {pcurve} on edge {} cannot share surface {surface}",
                                    edge.id
                                ),
                            ),
                        );
                    }
                }
            }
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
            let Some(coedge) = ir.model.coedges.iter().find(|value| value.id == *coedge) else {
                losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "loop {} references absent coedge {coedge}",
                    wire.id
                )));
                complete = false;
                continue;
            };
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
        let triangulation = face_triangulations.get(face.id.as_str());
        let polygonal = ir.model.surfaces.iter().any(|surface| {
            surface.id == face.surface
                && matches!(
                    surface.geometry.solved(),
                    Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Polygonal(_))
                )
        });
        let Some((surface, surface_location)) = surface_ids
            .get(face.surface.as_str())
            .map(|(surface, location, _)| (*surface, *location))
            .or_else(|| (polygonal && triangulation.is_some()).then_some((0, 0)))
        else {
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
                    "0 {} {surface} {surface_location}{}",
                    face.tolerance
                        .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
                    triangulation.map_or_else(String::new, |index| format!(" 2 {index}"))
                ),
                children,
            });
        }
    }
    let mut roots = Vec::new();
    for body in bodies {
        let placement = root_override.or(body.transform).unwrap_or_default();
        let placement = if location_supported(placement) {
            placement
        } else {
            losses.push(
                crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "body {} has a placement that OCCT cannot encode and the placement is omitted",
                    body.id
                )),
            );
            cadmpeg_ir::transform::Transform::identity()
        };
        if let Some(RootKind::Face {
            region,
            shell,
            face,
        }) = root_kinds.get(body.id.as_str())
        {
            if let Some(&face) = indices.get(face) {
                topology.record(region, face)?;
                topology.record(shell, face)?;
                topology.record(body.id.as_str(), face)?;
                let location = register_location(placement, &mut locations);
                roots.push((face, location));
                continue;
            }
        }
        let mut regions = Vec::new();
        for region_id in &body.regions {
            let Some(region) = ir.model.regions.iter().find(|value| value.id == *region_id) else {
                losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "body {} references absent region {region_id}, which is omitted",
                    body.id
                )));
                continue;
            };
            let mut shells = Vec::new();
            let mut degraded = false;
            for shell_id in &region.shells {
                let mut shell_degraded = false;
                let Some(shell) = ir.model.shells.iter().find(|value| value.id == *shell_id) else {
                    degraded = true;
                    losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "region {} references absent shell {shell_id}, which is omitted",
                        region.id
                    )));
                    continue;
                };
                let mut children = Vec::new();
                for face_id in shell.faces() {
                    let Some(index) = indices.get(face_id.as_str()) else {
                        shell_degraded = true;
                        losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(
                            format!("face {face_id} in body {} has no OCCT representation and is omitted", body.id),
                        ));
                        continue;
                    };
                    let Some(face) = ir.model.faces.iter().find(|value| value.id == *face_id)
                    else {
                        shell_degraded = true;
                        losses.push(
                            crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(
                                format!(
                                    "face {face_id} in body {} is absent and is omitted",
                                    body.id
                                ),
                            ),
                        );
                        continue;
                    };
                    children.push((*index, face.sense));
                }
                for edge in shell.wire_edges() {
                    let Some(index) = indices.get(edge.as_str()) else {
                        shell_degraded = true;
                        losses.push(
                            crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(
                                format!(
                                "edge {edge} in body {} has no OCCT representation and is omitted",
                                body.id
                            ),
                            ),
                        );
                        continue;
                    };
                    children.push((*index, Sense::Forward));
                }
                for vertex in shell.free_vertices() {
                    let Some(index) = indices.get(vertex.as_str()) else {
                        shell_degraded = true;
                        losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(
                            format!("vertex {vertex} in body {} has no OCCT representation and is omitted", body.id),
                        ));
                        continue;
                    };
                    children.push((*index, Sense::Forward));
                }
                if shell_degraded {
                    degraded = true;
                    let mut retained_edges = HashSet::new();
                    let mut retained_vertices = std::collections::BTreeSet::new();
                    let mut represented_vertices = HashSet::new();
                    for edge in shell.wire_edges().iter().filter_map(|id| {
                        indices
                            .get(id.as_str())
                            .and_then(|_| ir.model.edges.iter().find(|edge| edge.id == *id))
                    }) {
                        retained_edges.insert(edge.id.as_str());
                        represented_vertices.insert(edge.start.as_str());
                        represented_vertices.insert(edge.end.as_str());
                    }
                    represented_vertices.extend(
                        shell
                            .free_vertices()
                            .iter()
                            .filter(|id| indices.contains_key(id.as_str()))
                            .map(cadmpeg_ir::ids::VertexId::as_str),
                    );
                    for face in shell.faces().iter().filter_map(|id| {
                        indices
                            .get(id.as_str())
                            .and_then(|_| ir.model.faces.iter().find(|face| face.id == *id))
                    }) {
                        for wire in face
                            .loops
                            .iter()
                            .filter_map(|id| ir.model.loops.iter().find(|wire| wire.id == *id))
                        {
                            for coedge in wire.coedges().iter().filter_map(|id| {
                                ir.model.coedges.iter().find(|coedge| coedge.id == *id)
                            }) {
                                retained_edges.insert(coedge.edge.as_str());
                                if let Some(edge) =
                                    ir.model.edges.iter().find(|edge| edge.id == coedge.edge)
                                {
                                    represented_vertices.insert(edge.start.as_str());
                                    represented_vertices.insert(edge.end.as_str());
                                }
                            }
                        }
                    }
                    for face_id in shell.faces() {
                        if indices.contains_key(face_id.as_str()) {
                            continue;
                        }
                        let Some(face) = ir.model.faces.iter().find(|face| face.id == *face_id)
                        else {
                            continue;
                        };
                        for wire_id in &face.loops {
                            let Some(wire) = ir.model.loops.iter().find(|wire| wire.id == *wire_id)
                            else {
                                continue;
                            };
                            for vertex in wire.vertices() {
                                retained_vertices.insert(vertex.as_str());
                            }
                            for coedge_id in wire.coedges() {
                                let Some(coedge) = ir
                                    .model
                                    .coedges
                                    .iter()
                                    .find(|coedge| coedge.id == *coedge_id)
                                else {
                                    continue;
                                };
                                if retained_edges.insert(coedge.edge.as_str()) {
                                    if let Some(index) = indices.get(coedge.edge.as_str()) {
                                        children.push((*index, Sense::Forward));
                                        if let Some(edge) = ir
                                            .model
                                            .edges
                                            .iter()
                                            .find(|edge| edge.id == coedge.edge)
                                        {
                                            represented_vertices.insert(edge.start.as_str());
                                            represented_vertices.insert(edge.end.as_str());
                                        }
                                    } else if let Some(edge) =
                                        ir.model.edges.iter().find(|edge| edge.id == coedge.edge)
                                    {
                                        retained_vertices.insert(edge.start.as_str());
                                        retained_vertices.insert(edge.end.as_str());
                                    }
                                }
                            }
                        }
                    }
                    for vertex in retained_vertices {
                        if !represented_vertices.contains(vertex) {
                            if let Some(index) = indices.get(vertex) {
                                children.push((*index, Sense::Forward));
                            }
                        }
                    }
                    if children.is_empty() {
                        continue;
                    }
                }
                let index = shapes.len();
                topology.record(shell.id.as_str(), index)?;
                shapes.push(Shape {
                    kind: if shell.faces().is_empty() || shell_degraded {
                        "Co"
                    } else {
                        "Sh"
                    },
                    geometry: String::new(),
                    children,
                });
                shells.push((index, Sense::Forward));
            }
            if shells.is_empty() && degraded {
                continue;
            }
            if degraded && body.kind != BodyKind::General {
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "body {} is written as a compound after unsupported members are omitted",
                        body.id
                    )),
                );
            }
            let index = shapes.len();
            topology.record(region.id.as_str(), index)?;
            shapes.push(Shape {
                kind: if !degraded
                    && (body.kind == BodyKind::Solid
                        || (body.kind == BodyKind::General
                            && region.shells.iter().all(|id| {
                                ir.model
                                    .shells
                                    .iter()
                                    .any(|shell| shell.id == *id && !shell.faces().is_empty())
                            })))
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
            continue;
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
                let children: Vec<_> = shapes[region]
                    .children
                    .iter()
                    .flat_map(|(shell, _)| shapes[*shell].children.clone())
                    .collect();
                let kind = if !children.is_empty()
                    && children
                        .iter()
                        .all(|(index, _)| shapes[*index].kind == "Ed")
                {
                    "Wi"
                } else {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                            "wire body {} is written as a compound to retain its non-edge members",
                            body.id
                        )),
                    );
                    "Co"
                };
                let root = shapes.len();
                shapes.push(Shape {
                    kind,
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
                    if kind == "Wi" && current_region.shells.len() == 1 {
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
        let location = register_location(placement, &mut locations);
        topology.record(body.id.as_str(), root)?;
        roots.push((root, location));
    }
    if !free && roots.is_empty() {
        return Ok(None);
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
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "independent face {} has no OCCT representation and is omitted",
                        face.id
                    )),
                );
                continue;
            };
            roots.push((*index, 0));
        }
        for wire in ir.model.loops.iter().filter(|wire| {
            loops.contains(wire.id.as_str())
                && !ir.model.faces.iter().any(|face| {
                    faces.contains(face.id.as_str())
                        && indices.contains_key(face.id.as_str())
                        && face.loops.iter().any(|id| id == &wire.id)
                })
        }) {
            let Some(index) = indices.get(wire.id.as_str()) else {
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "independent wire {} has no OCCT representation and is omitted",
                        wire.id
                    )),
                );
                continue;
            };
            roots.push((*index, 0));
        }
        for edge in ir.model.edges.iter().filter(|edge| {
            edges.contains(edge.id.as_str())
                && !ir.model.coedges.iter().any(|coedge| {
                    coedges.contains(coedge.id.as_str())
                        && coedge.edge == edge.id
                        && indices.contains_key(coedge.owner_loop.as_str())
                })
        }) {
            let Some(index) = indices.get(edge.id.as_str()) else {
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "independent edge {} has no OCCT representation and is omitted",
                        edge.id
                    )),
                );
                continue;
            };
            roots.push((*index, 0));
        }
        for vertex in ir.model.vertices.iter().filter(|vertex| {
            vertices.contains(vertex.id.as_str())
                && !ir.model.edges.iter().any(|edge| {
                    edges.contains(edge.id.as_str())
                        && indices.contains_key(edge.id.as_str())
                        && (edge.start == vertex.id || edge.end == vertex.id)
                })
        }) {
            let Some(index) = indices.get(vertex.id.as_str()) else {
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "independent vertex {} has no OCCT representation and is omitted",
                        vertex.id
                    )),
                );
                continue;
            };
            roots.push((*index, 0));
        }
        if roots.is_empty() && curves.is_empty() && surfaces.is_empty() && triangulations.is_empty()
        {
            return Ok(None);
        }
    }
    topology.finish(&shapes, &indices, &roots)?;
    // Empty tables keep the historical spelling so polygon-free members
    // byte-match their previous output.
    let polygons3d = if polygons3d.is_empty() {
        "0".to_owned()
    } else {
        format!("{}\n{}", polygons3d.len(), polygons3d.join("\n"))
    };
    let polygons_indexed = if polygons_indexed.is_empty() {
        "0".to_owned()
    } else {
        format!(
            "{}\n{}",
            polygons_indexed.len(),
            polygons_indexed.join("\n")
        )
    };
    let triangulations = if triangulations.is_empty() {
        "0".to_owned()
    } else {
        format!("{}\n{}", triangulations.len(), triangulations.join("\n"))
    };
    let mut output = format!("\nCASCADE Topology V3, (c) Open Cascade\nLocations {}\n{}\nCurve2ds {}\n{}\nCurves {}\n{}\nPolygon3D {}\nPolygonOnTriangulations {}\nSurfaces {}\n{}\nTriangulations {}\nTShapes {}\n", locations.len(), locations.join("\n"), pcurves.len(), pcurves.join("\n"), curves.len(), curves.join("\n"), polygons3d, polygons_indexed, surfaces.len(), surfaces.join("\n"), triangulations, shapes.len());
    let mut flags = Vec::new();
    ctx.reserve_working(&mut flags, shapes.len(), "FreeCAD topology flags")?;
    flags.resize(shapes.len(), None);
    for (id, index) in &indices {
        let Some(native) = root_kinds.current.native_for_record(id) else {
            continue;
        };
        let Some(kind) = root_kinds.current.source_kind(native) else {
            continue;
        };
        let Some(shape) = shapes.get(*index) else {
            continue;
        };
        let expected = match shape.kind {
            "Ve" => crate::brep::TextShapeKind::Vertex,
            "Ed" => crate::brep::TextShapeKind::Edge,
            "Wi" => crate::brep::TextShapeKind::Wire,
            "Fa" => crate::brep::TextShapeKind::Face,
            "Sh" => crate::brep::TextShapeKind::Shell,
            "So" => crate::brep::TextShapeKind::Solid,
            "CS" => crate::brep::TextShapeKind::CompSolid,
            "Co" => crate::brep::TextShapeKind::Compound,
            _ => continue,
        };
        if kind == expected {
            flags[*index] = root_kinds.current.source_flags(id);
        }
    }
    for (index, shape) in shapes.iter().enumerate() {
        output.push_str(shape.kind);
        output.push('\n');
        output.push_str(&shape.geometry);
        output.push_str("\n\n");
        for value in flags[index].unwrap_or([false, true, false, true, true, false, true]) {
            output.push(if value { '1' } else { '0' });
        }
        output.push('\n');
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
    all: &mut Vec<(String, String, Vec<EmittedTopologyUse>)>,
    owner: &str,
    entry: &str,
    uses: Vec<EmittedTopologyUse>,
) -> Result<(), CodecError> {
    ctx.reserve_working(all, 1, "FreeCAD emitted topology owners")?;
    let mut key = String::new();
    ctx.reserve_text_working(&mut key, owner.len(), "FreeCAD emitted topology owner")?;
    key.push_str(owner);
    let mut member = String::new();
    ctx.reserve_text_working(&mut member, entry.len(), "FreeCAD emitted shape member")?;
    member.push_str(entry);
    all.push((key, member, uses));
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
            .map(cadmpeg_ir::geometry::ProceduralSurface::definition)
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
    ir: &CadIr,
    curve: Option<&cadmpeg_ir::geometry::Curve>,
    range: [f64; 2],
) -> Option<[f64; 2]> {
    if let Some(curve) = curve {
        if geometry::curve_record(ir, curve).is_some_and(|row| row.starts_with("8 ")) {
            let construction = curve.geometry.procedural_construction()?;
            let definition = ir
                .model
                .procedural_curves
                .iter()
                .find(|record| record.id == *construction)?
                .definition();
            if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(subset) = definition {
                let first = subset.parameter_range().endpoints()[0];
                return Some(range.map(|value| value + first))
                    .filter(|range| range.iter().all(|value| value.is_finite()));
            }
        }
    }
    occt_solved_curve_range(curve.and_then(|curve| curve.geometry.solved()), range)
}

fn composite_line_range(
    ir: &CadIr,
    edge: &cadmpeg_ir::topology::Edge,
    curve: &cadmpeg_ir::geometry::Curve,
) -> Option<[f64; 2]> {
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    let curve = geometry::single_composite_line(ir, curve)?;
    let SolvedCurveGeometry::Line(line) = curve.geometry.solved()? else {
        return None;
    };
    let origin = line.origin().get();
    let direction = *line.direction().as_raw();
    let tolerance = edge
        .tolerance
        .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get);
    let parameter = |id: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir.model.vertices.iter().find(|vertex| vertex.id == *id)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)?
            .position()
            .get();
        let value = point.vector_from(origin).dot(direction);
        (value.is_finite() && point.distance(origin.translated(direction, value)) <= tolerance)
            .then_some(value)
    };
    let range = [parameter(&edge.start)?, parameter(&edge.end)?];
    (range[0] <= range[1]).then_some(range)
}

fn occt_solved_curve_range(
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
            occt_solved_curve_range(Some(placed.basis()), range)
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

    #[test]
    fn independent_face_selects_its_triangulation() {
        use cadmpeg_ir::features::FinitePoint3;
        use cadmpeg_ir::ids::FaceId;
        use cadmpeg_ir::math::Point3;
        use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};

        let face = FaceId::mint("cadir:model:face#independent").unwrap();
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ]
        .into_iter()
        .map(|point| FinitePoint3::new(point).unwrap())
        .collect();
        let mut mesh = Tessellation::from_parts(
            TessellationId::mint("cadir:model:tessellation#independent").unwrap(),
            TessellationMesh::List {
                vertices,
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap();
        mesh.faces.push(face.clone());
        let mut ir = CadIr::empty();
        ir.model.tessellations.push(mesh);
        let faces = HashSet::from([face.to_string()]);
        let (rows, face_rows) =
            triangulation_sections(&ir, &[], &faces, true, &HashSet::new(), &mut Vec::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(face_rows.get(face.as_str()), Some(&1));
    }

    #[test]
    fn missing_topology_members_do_not_refuse_sibling_geometry() {
        use cadmpeg_ir::{Codec, DecodeOptions};
        use std::io::Cursor;

        let input = crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::GEOMETRY),
                &DecodeOptions::default(),
            )
            .unwrap();
        for missing in ["point", "vertex", "coedge"] {
            let mut ir = input.ir().clone();
            let missing_id = match missing {
                "point" => {
                    let id = ir.model.vertices[0].point.clone();
                    ir.model.points.retain(|point| point.id != id);
                    id.to_string()
                }
                "vertex" => {
                    let id = ir.model.edges[0].start.clone();
                    ir.model.vertices.retain(|vertex| vertex.id != id);
                    id.to_string()
                }
                "coedge" => {
                    let id = ir.model.loops[0].coedges()[0].clone();
                    ir.model.coedges.retain(|coedge| coedge.id != id);
                    id.to_string()
                }
                _ => unreachable!(),
            };
            let ctx = cadmpeg_test_support::service_decode_context();
            let current = crate::topology_transfer::current_bindings(&ctx, &ir).unwrap();
            let roots = RootKinds::new(&ctx, &ir, &current).unwrap();
            let bodies: Vec<_> = ir.model.bodies.iter().collect();
            let mut losses = Vec::new();
            let result = serialize_with_uses(&ctx, &ir, &bodies, &roots, None, &mut losses)
                .unwrap()
                .unwrap();
            assert!(
                losses.iter().any(|loss| loss.message.contains(&missing_id)),
                "{missing}"
            );
            assert!(
                result
                    .uses
                    .iter()
                    .any(|item| item.neutral_id == bodies[0].id.as_str()),
                "{missing}"
            );
        }
    }

    #[test]
    fn unsupported_placement_keeps_other_bodies_in_the_same_shape() {
        use cadmpeg_ir::{Codec, DecodeOptions};
        use std::io::Cursor;

        let input = crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::GEOMETRY),
                &DecodeOptions::default(),
            )
            .unwrap();
        let mut ir = input.ir().clone();
        let mut unsupported = ir.model.bodies[0].clone();
        unsupported.id = cadmpeg_ir::ids::BodyId::mint("cadir:model:body#unsupported").unwrap();
        unsupported.transform = cadmpeg_ir::transform::Transform::affine([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        ir.model.bodies.push(unsupported);
        let ctx = cadmpeg_test_support::service_decode_context();
        let current = crate::topology_transfer::current_bindings(&ctx, &ir).unwrap();
        let roots = RootKinds::new(&ctx, &ir, &current).unwrap();
        let bodies: Vec<_> = ir.model.bodies.iter().collect();
        let mut losses = Vec::new();
        let result = serialize_with_uses(&ctx, &ir, &bodies, &roots, None, &mut losses)
            .unwrap()
            .unwrap();
        assert!(result
            .uses
            .iter()
            .any(|item| item.neutral_id == bodies[0].id.as_str()));
        assert!(result
            .uses
            .iter()
            .any(|item| item.neutral_id == bodies.last().unwrap().id.as_str()));
        assert!(losses
            .iter()
            .any(|loss| loss.message.contains(bodies.last().unwrap().id.as_str())));
        let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
        let archive = crate::test_support::test_archive::archive_entries(&[
            ("Document.xml", xml),
            ("Shape.brp", &result.bytes),
        ]);
        let output = crate::FcstdCodec
            .decode(&mut Cursor::new(archive), &DecodeOptions::default())
            .unwrap();
        assert_eq!(output.ir().model.bodies.len(), bodies.len());
        assert!(!output.ir().model.faces.is_empty());
    }

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
        serialize_geometry(
            ctx,
            ir,
            bodies,
            &roots,
            (None, &HashSet::new()),
            &mut NoTopology,
            &mut Vec::new(),
        )
    }
}
