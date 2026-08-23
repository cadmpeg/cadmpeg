// SPDX-License-Identifier: Apache-2.0
//! Decode `TSplines.BlobParts/*.tsm` Form control cages.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::SubdId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdGripDirection, SubdGripWedge, SubdPlaneFrame,
    SubdRadialMapSelector, SubdRadialSymmetryMap, SubdScheme, SubdSecondaryGrip, SubdSurface,
    SubdSymmetry, SubdSymmetryKind, SubdVertex, SubdVertexGripLayout, SubdVertexTag,
};
use cadmpeg_ir::SourceObjectAssociation;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

const ENTRY_MARKER: &str = "/TSplines.BlobParts/";
const CAGE_COORDINATE_SCALE: f64 = 10.0;
const FULL_CREASE_SHARPNESS: f64 = 1.0;
const SYMMETRY_FRAME_EPS: f64 = 1e-9;

#[derive(Clone, Copy)]
struct HalfEdge {
    next: usize,
    previous: usize,
    mate: usize,
    vertex: usize,
    face: i64,
}

#[derive(Clone, Copy)]
struct GripPoint {
    point: Point3,
    weight: f64,
}

#[derive(Clone, Copy)]
enum GripVertexMarker {
    Primary(usize),
    Secondary(Option<usize>),
}

/// Decode every active-asset T-spline control cage in archive order.
///
/// A cage whose program is internally inconsistent degrades to an
/// error-severity loss note instead of failing the document decode; its
/// entry bytes remain retained in the container, and the serializer-backed
/// Form join leaves the affected Form on native retention.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(Vec<SubdSurface>, Vec<cadmpeg_ir::report::LossNote>), CodecError> {
    let Some(folder) = scan.design_asset_folder() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let prefix = format!("{folder}{ENTRY_MARKER}");
    let mut cages = Vec::new();
    let mut losses = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        std::path::Path::new(&entry.name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tsm"))
            && entry.name.starts_with(&prefix)
    }) {
        match parse(ctx, &entry.name, scan.entry_bytes(&entry.name)?) {
            Ok(parsed) => {
                if parsed.unknown_records != 0 {
                    losses.push(F3dLossCode::TsplineRecordUntyped.note(format!(
                        "{} T-spline record(s) were retained without typed semantics.",
                        parsed.unknown_records
                    )));
                }
                cages.push(parsed.surface);
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => losses.push(
                F3dLossCode::TsplineCageUndecoded
                    .note(format!("T-spline control cage not decoded: {error}")),
            ),
        }
    }
    Ok((cages, losses))
}

fn malformed(name: &str, message: impl std::fmt::Display) -> CodecError {
    crate::error::malformed(format!("T-spline cage {name}: {message}"))
}

fn parse_usize(name: &str, value: Option<&str>, field: &str) -> Result<usize, CodecError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| malformed(name, format!("invalid {field}")))
}

fn parse_i64(name: &str, value: Option<&str>, field: &str) -> Result<i64, CodecError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| malformed(name, format!("invalid {field}")))
}

fn parse_f64(name: &str, value: Option<&str>, field: &str) -> Result<f64, CodecError> {
    value
        .and_then(|value| value.parse().ok())
        .filter(|value: &f64| value.is_finite())
        .ok_or_else(|| malformed(name, format!("invalid {field}")))
}

fn parse_direction(name: &str, value: Option<&str>) -> Result<SubdGripDirection, CodecError> {
    match value {
        Some("NORTH") => Ok(SubdGripDirection::North),
        Some("EAST") => Ok(SubdGripDirection::East),
        Some("SOUTH") => Ok(SubdGripDirection::South),
        Some("WEST") => Ok(SubdGripDirection::West),
        _ => Err(malformed(name, "invalid vertex direction")),
    }
}

/// Map each program slot to its IR index, or `None` for a deleted slot.
fn compact(live: impl Iterator<Item = bool>) -> Vec<Option<u32>> {
    let mut next = 0u32;
    live.map(|live| {
        live.then(|| {
            let index = next;
            next += 1;
            index
        })
    })
    .collect()
}

fn require_end<'a>(
    name: &str,
    mut fields: impl Iterator<Item = &'a str>,
    record: &str,
) -> Result<(), CodecError> {
    if fields.next().is_some() {
        return Err(malformed(name, format!("{record} has trailing fields")));
    }
    Ok(())
}

#[derive(Debug)]
struct ParsedCage {
    surface: SubdSurface,
    unknown_records: usize,
}

#[derive(Debug)]
struct DerivedGripConnectivity {
    vertex: usize,
    wedges: usize,
    spoke_lengths: Vec<usize>,
    grip_indices: Vec<i64>,
}

#[derive(Clone, Copy)]
struct FanSlot {
    half_edge: Option<usize>,
    face: Option<usize>,
    phantom: bool,
}

impl FanSlot {
    fn phantom() -> Self {
        Self {
            half_edge: None,
            face: None,
            phantom: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SymmetryMode {
    Correspondence,
    Radial,
}

#[derive(Debug)]
struct SymmetryBlock {
    mode: SymmetryMode,
    plane: Option<[f64; 12]>,
    radial_segments: Option<u32>,
    radial_sweep: Option<f64>,
    radial_maps: Vec<SubdRadialSymmetryMap>,
    record_kinds: BTreeSet<String>,
    face_forward: BTreeMap<usize, usize>,
    face_reverse: BTreeMap<usize, usize>,
    edge_forward: BTreeMap<usize, usize>,
    edge_reverse: BTreeMap<usize, usize>,
    vertex_forward: BTreeMap<usize, usize>,
    vertex_reverse: BTreeMap<usize, usize>,
}

impl SymmetryBlock {
    fn new(mode: SymmetryMode) -> Self {
        Self {
            mode,
            plane: None,
            radial_segments: None,
            radial_sweep: None,
            radial_maps: Vec::new(),
            record_kinds: BTreeSet::new(),
            face_forward: BTreeMap::new(),
            face_reverse: BTreeMap::new(),
            edge_forward: BTreeMap::new(),
            edge_reverse: BTreeMap::new(),
            vertex_forward: BTreeMap::new(),
            vertex_reverse: BTreeMap::new(),
        }
    }
}

fn parse_pairs<'a>(
    name: &str,
    fields: impl Iterator<Item = &'a str>,
    record: &str,
) -> Result<BTreeMap<usize, usize>, CodecError> {
    let values = fields
        .map(|value| parse_usize(name, Some(value), record))
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() % 2 != 0 {
        return Err(malformed(name, format!("{record} has an unpaired index")));
    }
    let mut pairs = BTreeMap::new();
    for pair in values.chunks_exact(2) {
        if pairs.insert(pair[0], pair[1]).is_some() {
            return Err(malformed(name, format!("{record} repeats a source index")));
        }
    }
    Ok(pairs)
}

fn parse_radial_pairs<'a>(
    name: &str,
    fields: impl Iterator<Item = &'a str>,
) -> Result<Vec<[u64; 2]>, CodecError> {
    let values = fields
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| malformed(name, "invalid radial symmetry map index"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() % 2 != 0 {
        return Err(malformed(name, "radial symmetry map has an unpaired index"));
    }
    let mut sources = BTreeSet::new();
    let mut pairs = Vec::with_capacity(values.len() / 2);
    for pair in values.chunks_exact(2) {
        if !sources.insert(pair[0]) {
            return Err(malformed(
                name,
                "radial symmetry map repeats a source index",
            ));
        }
        pairs.push([pair[0], pair[1]]);
    }
    Ok(pairs)
}

fn validate_symmetry_map(
    name: &str,
    forward: &BTreeMap<usize, usize>,
    reverse: &BTreeMap<usize, usize>,
    slots: &[bool],
    element: &str,
) -> Result<(), CodecError> {
    for (&source, &target) in forward {
        if !slots.get(source).copied().unwrap_or(false)
            || !slots.get(target).copied().unwrap_or(false)
            || (source != target && reverse.get(&target) != Some(&source))
        {
            return Err(malformed(
                name,
                format!("{element} symmetry map is inconsistent"),
            ));
        }
    }
    for (&source, &target) in reverse {
        if !slots.get(source).copied().unwrap_or(false)
            || !slots.get(target).copied().unwrap_or(false)
            || forward.get(&target) != Some(&source)
        {
            return Err(malformed(
                name,
                format!("{element} symmetry map is inconsistent"),
            ));
        }
    }
    Ok(())
}

fn symmetry_plane(name: &str, values: [f64; 12]) -> Result<SubdPlaneFrame, CodecError> {
    let origin = Point3::new(
        values[0] * CAGE_COORDINATE_SCALE,
        values[1] * CAGE_COORDINATE_SCALE,
        values[2] * CAGE_COORDINATE_SCALE,
    );
    let first_axis = Vector3::new(values[4], values[5], values[6]);
    let second_axis = Vector3::new(values[8], values[9], values[10]);
    if (values[3] - 1.0).abs() > SYMMETRY_FRAME_EPS
        || values[7].abs() > SYMMETRY_FRAME_EPS
        || values[11].abs() > SYMMETRY_FRAME_EPS
        || (first_axis.norm() - 1.0).abs() > SYMMETRY_FRAME_EPS
        || (second_axis.norm() - 1.0).abs() > SYMMETRY_FRAME_EPS
        || first_axis.dot(second_axis).abs() > SYMMETRY_FRAME_EPS
    {
        return Err(malformed(
            name,
            "symmetry plane is not a homogeneous orthonormal frame",
        ));
    }
    Ok(SubdPlaneFrame {
        origin,
        first_axis,
        second_axis,
    })
}

fn remap_symmetry_pairs(
    name: &str,
    map: &BTreeMap<usize, usize>,
    ir_indices: &[Option<u32>],
    element: &str,
) -> Result<Vec<[u32; 2]>, CodecError> {
    map.iter()
        .map(|(&source, &target)| {
            let source =
                ir_indices.get(source).copied().flatten().ok_or_else(|| {
                    malformed(name, format!("{element} symmetry source is deleted"))
                })?;
            let target =
                ir_indices.get(target).copied().flatten().ok_or_else(|| {
                    malformed(name, format!("{element} symmetry target is deleted"))
                })?;
            Ok([source, target])
        })
        .collect()
}

fn direction_offset(direction: SubdGripDirection) -> usize {
    match direction {
        SubdGripDirection::North => 0,
        SubdGripDirection::East => 1,
        SubdGripDirection::South => 2,
        SubdGripDirection::West => 3,
    }
}

fn build_fan(
    name: &str,
    vertex: usize,
    root: usize,
    half_edges: &[Option<HalfEdge>],
    face_live: &[bool],
) -> Result<Vec<FanSlot>, CodecError> {
    let root_half = half_edges
        .get(root)
        .and_then(Option::as_ref)
        .ok_or_else(|| malformed(name, "vertex root names a deleted half-edge"))?;
    if root_half.vertex != vertex {
        return Err(malformed(
            name,
            "vertex root does not terminate at its vertex",
        ));
    }

    let mut fan = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = root;
    loop {
        if !seen.insert(current) {
            if current == root {
                break;
            }
            return Err(malformed(
                name,
                "vertex half-edge fan repeats before its root",
            ));
        }
        let half = half_edges
            .get(current)
            .and_then(Option::as_ref)
            .ok_or_else(|| malformed(name, "vertex half-edge fan names a deleted slot"))?;
        if half.vertex != vertex {
            return Err(malformed(
                name,
                "vertex half-edge fan leaves its terminal vertex",
            ));
        }
        let face = match half.face {
            -1 => None,
            face if face >= 0 && face_live.get(face as usize).copied().unwrap_or(false) => {
                Some(face as usize)
            }
            _ => {
                return Err(malformed(
                    name,
                    "vertex half-edge fan names an invalid face",
                ))
            }
        };
        fan.push(FanSlot {
            half_edge: Some(current),
            face,
            phantom: false,
        });

        let next = half_edges
            .get(half.next)
            .and_then(Option::as_ref)
            .ok_or_else(|| malformed(name, "vertex fan next half-edge is deleted"))?;
        current = half_edges
            .get(next.mate)
            .and_then(Option::as_ref)
            .map(|mate| {
                if mate.vertex == vertex {
                    next.mate
                } else {
                    usize::MAX
                }
            })
            .ok_or_else(|| malformed(name, "vertex fan mate half-edge is deleted"))?;
        if current == usize::MAX {
            return Err(malformed(
                name,
                "vertex fan rotation leaves its terminal vertex",
            ));
        }
        if current == root {
            break;
        }
        if seen.len() >= half_edges.len() {
            return Err(malformed(name, "vertex half-edge fan does not close"));
        }
    }

    let gap_positions = fan
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.face.is_none().then_some(index))
        .collect::<Vec<_>>();
    if gap_positions.len() > 1 {
        return Err(malformed(
            name,
            "vertex half-edge fan has multiple boundary gaps",
        ));
    }
    if let Some(gap) = gap_positions.first().copied() {
        let phantom_count = 4usize.saturating_sub(fan.len());
        for _ in 0..phantom_count {
            fan.insert(gap + 1, FanSlot::phantom());
        }
    }
    Ok(fan)
}

struct GripDecodeContext<'a> {
    name: &'a str,
    vertex: usize,
    grip_vertices: &'a [GripVertexMarker],
    grip_points: &'a [Option<GripPoint>],
    grip_owners: &'a mut [Option<usize>],
}

impl GripDecodeContext<'_> {
    fn block(
        &mut self,
        indices: &[i64],
        cursor: &mut usize,
        count: usize,
    ) -> Result<Vec<Option<SubdSecondaryGrip>>, CodecError> {
        let end = cursor
            .checked_add(count)
            .ok_or_else(|| malformed(self.name, "derived-grip block arity overflows"))?;
        let values = indices.get(*cursor..end).ok_or_else(|| {
            malformed(
                self.name,
                "derived-grip run is shorter than its declared arity",
            )
        })?;
        *cursor = end;
        values
            .iter()
            .map(|index| match *index {
                -1 => Ok(None),
                index if index >= 0 => {
                    let index = usize::try_from(index)
                        .map_err(|_| malformed(self.name, "derived-grip index overflows"))?;
                    if !matches!(
                        self.grip_vertices.get(index),
                        Some(GripVertexMarker::Secondary(Some(owner))) if *owner == self.vertex
                    ) {
                        return Err(malformed(
                            self.name,
                            "derived-grip entry is not a secondary grip of its vertex",
                        ));
                    }
                    let point =
                        self.grip_points
                            .get(index)
                            .copied()
                            .flatten()
                            .ok_or_else(|| {
                                malformed(self.name, "derived-grip entry names a deleted grip")
                            })?;
                    let owner_slot = self.grip_owners.get_mut(index).ok_or_else(|| {
                        malformed(self.name, "derived-grip entry is out of range")
                    })?;
                    if owner_slot.replace(self.vertex).is_some() {
                        return Err(malformed(
                            self.name,
                            "secondary grip is named more than once",
                        ));
                    }
                    Ok(Some(SubdSecondaryGrip {
                        source_index: u32::try_from(index).map_err(|_| {
                            malformed(self.name, "secondary grip index overflows IR")
                        })?,
                        point: point.point,
                        weight: point.weight,
                    }))
                }
                _ => Err(malformed(self.name, "derived-grip index is below -1")),
            })
            .collect()
    }
}

struct SecondaryLayoutContext<'a> {
    name: &'a str,
    vertex_roots: &'a [Option<(usize, SubdGripDirection)>],
    vertex_live: &'a [bool],
    vertex_ir: &'a [Option<u32>],
    face_live: &'a [bool],
    face_ir: &'a [Option<u32>],
    half_edges: &'a [Option<HalfEdge>],
    edge_by_half: &'a [Option<(u32, bool)>],
    grip_vertices: &'a [GripVertexMarker],
    grip_points: &'a [Option<GripPoint>],
}

fn build_secondary_layouts(
    context: &SecondaryLayoutContext<'_>,
    derived_grips: &[DerivedGripConnectivity],
) -> Result<Vec<Option<SubdVertexGripLayout>>, CodecError> {
    let SecondaryLayoutContext {
        name,
        vertex_roots,
        vertex_live,
        vertex_ir,
        face_live,
        face_ir,
        half_edges,
        edge_by_half,
        grip_vertices,
        grip_points,
    } = *context;
    let live_vertices = vertex_ir.iter().flatten().count();
    let mut layouts = vec![None; live_vertices];
    let mut has_cg = vec![false; vertex_live.len()];
    let mut secondary_counts = vec![0usize; vertex_live.len()];
    let mut grip_owners = vec![None; grip_vertices.len()];
    for marker in grip_vertices {
        if let GripVertexMarker::Secondary(Some(vertex)) = marker {
            *secondary_counts
                .get_mut(*vertex)
                .ok_or_else(|| malformed(name, "secondary grip vertex is out of range"))? += 1;
        }
    }

    for connectivity in derived_grips {
        let vertex = connectivity.vertex;
        if !vertex_live.get(vertex).copied().unwrap_or(false) {
            return Err(malformed(name, "derived-grip vertex is out of range"));
        }
        if has_cg[vertex] {
            return Err(malformed(
                name,
                "vertex has more than one derived-grip record",
            ));
        }
        has_cg[vertex] = true;
        let (root, direction) = vertex_roots
            .get(vertex)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(name, "derived-grip vertex has no root direction"))?;
        let fan = build_fan(name, vertex, root, half_edges, face_live)?;
        if connectivity.wedges != fan.len() {
            return Err(malformed(
                name,
                "derived-grip wedge count does not match the completed vertex fan",
            ));
        }

        let offset = direction_offset(direction);
        let mut cursor = 0usize;
        let mut wedges = Vec::with_capacity(connectivity.wedges);
        for wedge in 0..connectivity.wedges {
            let spoke_count = connectivity.spoke_lengths[wedge];
            let sector_count = spoke_count
                .checked_mul(connectivity.spoke_lengths[(wedge + 1) % connectivity.wedges])
                .ok_or_else(|| malformed(name, "derived-grip sector arity overflows"))?;
            let slot = fan[(wedge + offset) % fan.len()];
            if slot.phantom && spoke_count != 0 {
                return Err(malformed(
                    name,
                    "phantom wedge carries a nonzero spoke length",
                ));
            }
            let edge = match slot.half_edge {
                Some(half) => Some(
                    edge_by_half
                        .get(half)
                        .copied()
                        .flatten()
                        .map(|(edge, _)| edge)
                        .ok_or_else(|| malformed(name, "fan half-edge has no owning edge"))?,
                ),
                None => None,
            };
            let sector_face = match slot.face {
                Some(face) => Some(
                    face_ir
                        .get(face)
                        .copied()
                        .flatten()
                        .ok_or_else(|| malformed(name, "fan sector names a deleted face"))?,
                ),
                None => None,
            };
            let mut grip_context = GripDecodeContext {
                name,
                vertex,
                grip_vertices,
                grip_points,
                grip_owners: &mut grip_owners,
            };
            let spokes =
                grip_context.block(&connectivity.grip_indices, &mut cursor, spoke_count)?;
            let sectors =
                grip_context.block(&connectivity.grip_indices, &mut cursor, sector_count)?;
            wedges.push(SubdGripWedge {
                edge,
                sector_face,
                phantom: slot.phantom,
                spokes,
                sectors,
            });
        }
        if cursor != connectivity.grip_indices.len() {
            return Err(malformed(name, "derived-grip run has trailing entries"));
        }
        let vertex_ir =
            vertex_ir[vertex].ok_or_else(|| malformed(name, "derived-grip vertex is deleted"))?;
        layouts[vertex_ir as usize] = Some(SubdVertexGripLayout { direction, wedges });
    }

    for (vertex, count) in secondary_counts.into_iter().enumerate() {
        if (count != 0) != has_cg[vertex] {
            return Err(malformed(
                name,
                "secondary-grip ownership does not have exactly one derived-grip record",
            ));
        }
    }
    for (index, marker) in grip_vertices.iter().enumerate() {
        if let GripVertexMarker::Secondary(Some(vertex)) = marker {
            if grip_owners[index] != Some(*vertex) {
                return Err(malformed(
                    name,
                    "secondary grip is not named exactly once by derived connectivity",
                ));
            }
        }
    }
    Ok(layouts)
}

fn parse(ctx: &DecodeContext<'_>, name: &str, bytes: &[u8]) -> Result<ParsedCage, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| malformed(name, format!("payload is not UTF-8: {error}")))?;
    if text.lines().next() != Some("#TS0200") {
        return Err(malformed(name, "unsupported header"));
    }

    // Every topology token occupies a slot in its own record order, and a bare
    // token is a deleted slot that occupies its index without defining an
    // element. Indices inside the program address slots, so slots are retained
    // through validation and compacted only when the IR cage is built.
    let mut face_roots: Vec<Option<usize>> = Vec::new();
    let mut edge_roots: Vec<Option<usize>> = Vec::new();
    let mut edge_knot_intervals: Vec<Option<f64>> = Vec::new();
    let mut vertex_roots: Vec<Option<(usize, SubdGripDirection)>> = Vec::new();
    let mut vertex_live: Vec<bool> = Vec::new();
    let mut half_edges: Vec<Option<HalfEdge>> = Vec::new();
    let mut crease_edges = BTreeSet::new();
    let mut grip_vertices: Vec<GripVertexMarker> = Vec::new();
    let mut grip_points: Vec<Option<GripPoint>> = Vec::new();
    let mut in_grip_map = false;
    let mut declarations = BTreeSet::new();
    let mut derived_grips = Vec::new();
    let mut selected_edges = BTreeSet::new();
    let mut selected_vertices = BTreeSet::new();
    let mut selected_grips = BTreeSet::new();
    let mut editor_declarations = BTreeSet::new();
    let mut symmetry_blocks = Vec::new();
    let mut current_symmetry: Option<SymmetryBlock> = None;
    let mut terminal_declarations = BTreeSet::new();
    let mut unknown_records = 0usize;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let mut fields = line.split_ascii_whitespace();
        match fields.next() {
            Some("#TS0200") => require_end(name, fields, "header")?,
            Some("degree") => {
                if parse_usize(name, fields.next(), "degree")? != 3 {
                    return Err(malformed(name, "unsupported degree"));
                }
                require_end(name, fields, "degree declaration")?;
                declarations.insert("degree");
            }
            Some(declaration @ ("cap-type" | "end-conditions" | "star-knot-rule")) => {
                if fields.next().is_none() {
                    return Err(malformed(name, format!("missing {declaration} value")));
                }
                require_end(name, fields, declaration)?;
                declarations.insert(declaration);
            }
            Some("star-smoothness") => {
                parse_f64(name, fields.next(), "star smoothness")?;
                require_end(name, fields, "star-smoothness declaration")?;
                declarations.insert("star-smoothness");
            }
            Some("units") => {
                if fields.next() != Some("1") || fields.next() != Some("meters") {
                    return Err(malformed(name, "unsupported units declaration"));
                }
                require_end(name, fields, "units declaration")?;
                declarations.insert("units");
            }
            Some("f") => match fields.next() {
                None => face_roots.push(None),
                root => {
                    face_roots.push(Some(parse_usize(name, root, "face root")?));
                    parse_i64(name, fields.next(), "face flags")?;
                    require_end(name, fields, "face")?;
                }
            },
            Some("e") => match fields.next() {
                None => {
                    edge_roots.push(None);
                    edge_knot_intervals.push(None);
                }
                root => {
                    edge_roots.push(Some(parse_usize(name, root, "edge root")?));
                    let knot_interval = parse_f64(name, fields.next(), "edge knot interval")?;
                    if knot_interval <= 0.0 {
                        return Err(malformed(name, "edge knot interval is not positive"));
                    }
                    edge_knot_intervals.push(Some(knot_interval));
                    require_end(name, fields, "edge")?;
                }
            },
            Some("v") => match fields.next() {
                None => {
                    vertex_live.push(false);
                    vertex_roots.push(None);
                }
                root => {
                    let root = parse_usize(name, root, "vertex root")?;
                    let direction = parse_direction(name, fields.next())?;
                    require_end(name, fields, "vertex")?;
                    vertex_live.push(true);
                    vertex_roots.push(Some((root, direction)));
                }
            },
            Some("l") => match fields.next() {
                None => half_edges.push(None),
                next => {
                    let half = HalfEdge {
                        next: parse_usize(name, next, "half-edge next index")?,
                        previous: parse_usize(name, fields.next(), "half-edge previous index")?,
                        mate: parse_usize(name, fields.next(), "half-edge mate index")?,
                        vertex: parse_usize(name, fields.next(), "half-edge vertex index")?,
                        face: parse_i64(name, fields.next(), "half-edge face index")?,
                    };
                    parse_i64(name, fields.next(), "half-edge edge index")?;
                    parse_i64(name, fields.next(), "half-edge flags")?;
                    if fields.next().is_some() {
                        return Err(malformed(name, "half-edge has trailing fields"));
                    }
                    half_edges.push(Some(half));
                }
            },
            Some("ec") => {
                crease_edges.insert(parse_usize(name, fields.next(), "crease edge index")?);
                parse_i64(name, fields.next(), "crease flags")?;
                require_end(name, fields, "crease")?;
            }
            Some("0m") => match fields.next() {
                Some("odd-grip-map") => {
                    require_end(name, fields, "odd-grip-map declaration")?;
                    in_grip_map = true;
                }
                Some("gvp") if in_grip_map => {
                    grip_vertices.push(GripVertexMarker::Primary(parse_usize(
                        name,
                        fields.next(),
                        "grip vertex index",
                    )?));
                    require_end(name, fields, "primary grip map")?;
                }
                Some("gv") if in_grip_map => {
                    let vertex = parse_i64(name, fields.next(), "secondary grip vertex index")?;
                    if vertex < -1 {
                        return Err(malformed(name, "secondary grip vertex is below -1"));
                    }
                    grip_vertices.push(GripVertexMarker::Secondary(
                        (vertex >= 0).then_some(vertex as usize),
                    ));
                    require_end(name, fields, "secondary grip map")?;
                }
                Some("cg") if in_grip_map => {
                    let vertex = parse_usize(name, fields.next(), "derived-grip vertex")?;
                    let wedges = parse_usize(name, fields.next(), "derived-grip wedge count")?;
                    if wedges == 0 {
                        return Err(malformed(name, "derived-grip wedge count is zero"));
                    }
                    let spoke_lengths = (0..wedges)
                        .map(|_| parse_usize(name, fields.next(), "derived-grip spoke length"))
                        .collect::<Result<Vec<_>, _>>()?;
                    let grip_count = (0..wedges).try_fold(0usize, |count, wedge| {
                        let cross = spoke_lengths[wedge]
                            .checked_mul(spoke_lengths[(wedge + 1) % wedges])
                            .ok_or_else(|| malformed(name, "derived-grip arity overflows"))?;
                        count
                            .checked_add(spoke_lengths[wedge])
                            .and_then(|count| count.checked_add(cross))
                            .ok_or_else(|| malformed(name, "derived-grip arity overflows"))
                    })?;
                    let grip_indices = (0..grip_count)
                        .map(|_| parse_i64(name, fields.next(), "derived-grip index"))
                        .collect::<Result<Vec<_>, _>>()?;
                    require_end(name, fields, "derived-grip connectivity")?;
                    derived_grips.push(DerivedGripConnectivity {
                        vertex,
                        wedges,
                        spoke_lengths,
                        grip_indices,
                    });
                }
                _ => return Err(malformed(name, "unknown odd-grip-map record")),
            },
            Some("0g") => match fields.next() {
                None => grip_points.push(None),
                x => {
                    let point = Point3::new(
                        parse_f64(name, x, "grip x")? * CAGE_COORDINATE_SCALE,
                        parse_f64(name, fields.next(), "grip y")? * CAGE_COORDINATE_SCALE,
                        parse_f64(name, fields.next(), "grip z")? * CAGE_COORDINATE_SCALE,
                    );
                    let weight = parse_f64(name, fields.next(), "grip weight")?;
                    if weight <= 0.0 || fields.next().is_some() {
                        return Err(malformed(name, "grip weight is not positive"));
                    }
                    grip_points.push(Some(GripPoint { point, weight }));
                }
            },
            Some(selection @ ("100edges" | "100verts" | "50000grip")) => {
                if selection != "50000grip" && !editor_declarations.insert(selection) {
                    return Err(malformed(name, format!("duplicate {selection} record")));
                }
                let values = fields
                    .map(|value| parse_usize(name, Some(value), selection))
                    .collect::<Result<BTreeSet<_>, _>>()?;
                match selection {
                    "100edges" => selected_edges = values,
                    "100verts" => selected_vertices = values,
                    "50000grip" => selected_grips.extend(values),
                    _ => unreachable!("selection is exhaustive"),
                }
            }
            Some("105sym") => {
                let mode = match parse_i64(name, fields.next(), "symmetry flags")? {
                    0 => SymmetryMode::Correspondence,
                    1 => SymmetryMode::Radial,
                    _ => return Err(malformed(name, "unsupported symmetry flags")),
                };
                require_end(name, fields, "symmetry header")?;
                if let Some(block) = current_symmetry.replace(SymmetryBlock::new(mode)) {
                    symmetry_blocks.push(block);
                }
            }
            Some("105plane") => {
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(name, "symmetry plane has no header"))?;
                let values = fields
                    .map(|value| parse_f64(name, Some(value), "symmetry plane coefficient"))
                    .collect::<Result<Vec<_>, _>>()?;
                let plane: [f64; 12] = values
                    .try_into()
                    .map_err(|_| malformed(name, "symmetry plane must have 12 coefficients"))?;
                if block.plane.replace(plane).is_some() {
                    return Err(malformed(name, "duplicate symmetry plane"));
                }
            }
            Some("105a") => {
                let kind = fields
                    .next()
                    .ok_or_else(|| malformed(name, "missing symmetry map kind"))?;
                let pairs = parse_pairs(name, fields, "symmetry map")?;
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(name, "symmetry map has no header"))?;
                if block.mode != SymmetryMode::Correspondence {
                    return Err(malformed(
                        name,
                        "correspondence map belongs to a radial symmetry block",
                    ));
                }
                if !block.record_kinds.insert(kind.into()) {
                    return Err(malformed(name, format!("duplicate {kind} symmetry map")));
                }
                let target = match kind {
                    "fr" => &mut block.face_forward,
                    "f" => &mut block.face_reverse,
                    "er" => &mut block.edge_forward,
                    "e" => &mut block.edge_reverse,
                    "vr" => &mut block.vertex_forward,
                    "v" => &mut block.vertex_reverse,
                    _ => return Err(malformed(name, "unknown symmetry map kind")),
                };
                *target = pairs;
            }
            Some("105r") => {
                let kind = fields
                    .next()
                    .ok_or_else(|| malformed(name, "missing radial symmetry record kind"))?;
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(name, "radial symmetry record has no header"))?;
                if block.mode != SymmetryMode::Radial {
                    return Err(malformed(
                        name,
                        "radial symmetry record belongs to a correspondence block",
                    ));
                }
                if !block.record_kinds.insert(kind.into()) {
                    return Err(malformed(
                        name,
                        format!("duplicate {kind} radial symmetry record"),
                    ));
                }
                match kind {
                    "segments" => {
                        let segments =
                            parse_usize(name, fields.next(), "radial symmetry segments")?;
                        if segments == 0 {
                            return Err(malformed(
                                name,
                                "radial symmetry segments is not positive",
                            ));
                        }
                        block.radial_segments =
                            Some(u32::try_from(segments).map_err(|_| {
                                malformed(name, "radial symmetry segments exceed u32")
                            })?);
                        require_end(name, fields, "radial symmetry segments")?;
                    }
                    "sweep" => {
                        block.radial_sweep =
                            Some(parse_f64(name, fields.next(), "radial symmetry sweep")?);
                        require_end(name, fields, "radial symmetry sweep")?;
                    }
                    kind @ ("ef" | "er" | "ff" | "fr" | "vf" | "vr") => {
                        let selector = match kind {
                            "ef" => SubdRadialMapSelector::Ef,
                            "er" => SubdRadialMapSelector::Er,
                            "ff" => SubdRadialMapSelector::Ff,
                            "fr" => SubdRadialMapSelector::Fr,
                            "vf" => SubdRadialMapSelector::Vf,
                            "vr" => SubdRadialMapSelector::Vr,
                            _ => unreachable!("radial selector is matched above"),
                        };
                        let pairs = parse_radial_pairs(name, fields)?;
                        block
                            .radial_maps
                            .push(SubdRadialSymmetryMap { selector, pairs });
                    }
                    _ => {
                        return Err(malformed(
                            name,
                            format!("unknown radial symmetry record {kind}"),
                        ));
                    }
                }
            }
            Some(declaration @ ("tol" | "geom-tol")) => {
                let tolerance = parse_f64(name, fields.next(), declaration)?;
                if tolerance <= 0.0 {
                    return Err(malformed(name, format!("{declaration} is not positive")));
                }
                require_end(name, fields, declaration)?;
                if !terminal_declarations.insert(declaration) {
                    return Err(malformed(name, format!("duplicate {declaration}")));
                }
            }
            Some(declaration @ ("ver" | "behavior-version" | "compat-version")) => {
                if fields.next().is_none() {
                    return Err(malformed(name, format!("missing {declaration} value")));
                }
                require_end(name, fields, declaration)?;
                if !terminal_declarations.insert(declaration) {
                    return Err(malformed(name, format!("duplicate {declaration}")));
                }
            }
            _ => unknown_records += 1,
        }
    }
    if let Some(block) = current_symmetry {
        symmetry_blocks.push(block);
    }

    let live_vertices = vertex_live.iter().filter(|live| **live).count();
    if declarations.len() != 6
        || !face_roots.iter().any(Option::is_some)
        || !edge_roots.iter().any(Option::is_some)
        || live_vertices == 0
        || !half_edges.iter().any(Option::is_some)
        || (!grip_vertices.is_empty() && grip_vertices.len() != grip_points.len())
    {
        return Err(malformed(name, "control cage is incomplete"));
    }
    let populated = |half: usize| half_edges.get(half).is_some_and(Option::is_some);

    let face_live = face_roots.iter().map(Option::is_some).collect::<Vec<_>>();
    let edge_live = edge_roots.iter().map(Option::is_some).collect::<Vec<_>>();
    for edge in &selected_edges {
        if !edge_live.get(*edge).copied().unwrap_or(false) {
            return Err(malformed(name, "selected edge is out of range or deleted"));
        }
    }
    for vertex in &selected_vertices {
        if !vertex_live.get(*vertex).copied().unwrap_or(false) {
            return Err(malformed(
                name,
                "selected vertex is out of range or deleted",
            ));
        }
    }
    for grip in &selected_grips {
        if !grip_points.get(*grip).is_some_and(Option::is_some) {
            return Err(malformed(name, "selected grip is out of range or deleted"));
        }
    }
    for block in &symmetry_blocks {
        if block.plane.is_none() {
            return Err(malformed(name, "symmetry block has no plane"));
        }
        if block.mode == SymmetryMode::Correspondence {
            validate_symmetry_map(
                name,
                &block.face_forward,
                &block.face_reverse,
                &face_live,
                "face",
            )?;
            validate_symmetry_map(
                name,
                &block.edge_forward,
                &block.edge_reverse,
                &edge_live,
                "edge",
            )?;
            validate_symmetry_map(
                name,
                &block.vertex_forward,
                &block.vertex_reverse,
                &vertex_live,
                "vertex",
            )?;
        } else {
            for required in ["segments", "sweep", "ef", "er", "ff", "fr", "vf", "vr"] {
                if !block.record_kinds.contains(required) {
                    return Err(malformed(
                        name,
                        format!("radial symmetry block is missing {required}"),
                    ));
                }
            }
        }
    }
    for connectivity in &derived_grips {
        if !vertex_live
            .get(connectivity.vertex)
            .copied()
            .unwrap_or(false)
            || connectivity.grip_indices.iter().any(|index| match *index {
                -1 => false,
                index if index >= 0 => !matches!(
                    grip_vertices.get(index as usize),
                    Some(GripVertexMarker::Secondary(Some(vertex)))
                        if *vertex == connectivity.vertex
                ),
                _ => true,
            })
        {
            return Err(malformed(name, "derived-grip connectivity is out of range"));
        }
    }
    for (marker, point) in grip_vertices.iter().zip(&grip_points) {
        match marker {
            GripVertexMarker::Primary(vertex) | GripVertexMarker::Secondary(Some(vertex)) => {
                if point.is_none() || !vertex_live.get(*vertex).copied().unwrap_or(false) {
                    return Err(malformed(name, "grip vertex map is inconsistent"));
                }
            }
            GripVertexMarker::Secondary(None) if point.is_some() => {
                return Err(malformed(name, "deleted grip marker has a point"));
            }
            GripVertexMarker::Secondary(None) => {}
        }
    }
    for (index, half) in half_edges.iter().enumerate() {
        let Some(half) = half else { continue };
        if !populated(half.mate) || !populated(half.next) || !populated(half.previous) {
            return Err(malformed(name, "half-edge names a deleted slot"));
        }
        let mate = half_edges[half.mate].expect("invariant: populated() checked half.mate");
        let next = half_edges[half.next].expect("invariant: populated() checked half.next");
        let previous =
            half_edges[half.previous].expect("invariant: populated() checked half.previous");
        if mate.mate != index
            || next.previous != index
            || previous.next != index
            || !vertex_live.get(half.vertex).copied().unwrap_or(false)
        {
            return Err(malformed(name, "half-edge topology is inconsistent"));
        }
    }
    for (vertex, live) in vertex_live.iter().copied().enumerate() {
        if live {
            let (root, _) = vertex_roots
                .get(vertex)
                .copied()
                .flatten()
                .ok_or_else(|| malformed(name, "live vertex has no root direction"))?;
            build_fan(name, vertex, root, &half_edges, &face_live)?;
        }
    }

    // Slot indices address the program; IR indices address only populated slots.
    let vertex_ir = compact(vertex_live.iter().copied());
    let edge_ir = compact(edge_roots.iter().map(Option::is_some));
    let face_ir = compact(face_roots.iter().map(Option::is_some));
    let symmetries = symmetry_blocks
        .iter()
        .map(|block| {
            let plane = symmetry_plane(
                name,
                block
                    .plane
                    .ok_or_else(|| malformed(name, "symmetry block has no plane"))?,
            )?;
            let kind = match block.mode {
                SymmetryMode::Correspondence => SubdSymmetryKind::Correspondence,
                SymmetryMode::Radial => SubdSymmetryKind::Radial {
                    segments: block.radial_segments.ok_or_else(|| {
                        malformed(name, "radial symmetry block has no segment count")
                    })?,
                    sweep: block
                        .radial_sweep
                        .ok_or_else(|| malformed(name, "radial symmetry block has no sweep"))?,
                },
            };
            let (face_pairs, edge_pairs, vertex_pairs, radial_maps) = match block.mode {
                SymmetryMode::Correspondence => (
                    remap_symmetry_pairs(name, &block.face_forward, &face_ir, "face")?,
                    remap_symmetry_pairs(name, &block.edge_forward, &edge_ir, "edge")?,
                    remap_symmetry_pairs(name, &block.vertex_forward, &vertex_ir, "vertex")?,
                    Vec::new(),
                ),
                SymmetryMode::Radial => (
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    block.radial_maps.clone(),
                ),
            };
            Ok(SubdSymmetry {
                kind,
                plane,
                face_pairs,
                edge_pairs,
                vertex_pairs,
                radial_maps,
            })
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    let edge_knot_intervals_ir = edge_knot_intervals
        .iter()
        .copied()
        .flatten()
        .collect::<Vec<_>>();
    let vertex_of = |slot: usize| {
        vertex_ir
            .get(slot)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(name, "half-edge names a deleted vertex slot"))
    };

    let mut vertex_points = BTreeMap::new();
    if grip_vertices.is_empty() {
        if grip_points.len() != vertex_live.len() {
            return Err(malformed(name, "positional grip vertex map is incomplete"));
        }
        for (slot, point) in grip_points.iter().enumerate() {
            if let (true, Some(point)) = (vertex_live[slot], point) {
                vertex_points.insert(vertex_of(slot)?, point.point);
            }
        }
    } else {
        for (marker, point) in grip_vertices.iter().zip(grip_points.iter()) {
            let (GripVertexMarker::Primary(slot), Some(point)) = (marker, point) else {
                continue;
            };
            if vertex_points
                .insert(vertex_of(*slot)?, point.point)
                .is_some()
            {
                return Err(malformed(name, "primary grip vertex map is inconsistent"));
            }
        }
    }
    if vertex_points.len() != live_vertices {
        return Err(malformed(name, "primary grip vertex map is incomplete"));
    }

    let mut edge_by_half = vec![None; half_edges.len()];
    let mut edge_vertices = Vec::with_capacity(live_vertices);
    for (edge_slot, root) in edge_roots.iter().copied().enumerate() {
        let Some(root) = root else { continue };
        if !populated(root) {
            return Err(malformed(name, "edge root names a deleted slot"));
        }
        let half = half_edges[root].expect("invariant: populated() checked the edge root");
        let edge = edge_ir[edge_slot].expect("invariant: compact() populated this edge slot");
        if edge_by_half[root].replace((edge, false)).is_some()
            || edge_by_half[half.mate].replace((edge, true)).is_some()
        {
            return Err(malformed(name, "edge roots reuse a half-edge"));
        }
        let mate = half_edges[half.mate].expect("invariant: half-edge validation checked the mate");
        edge_vertices.push([vertex_of(mate.vertex)?, vertex_of(half.vertex)?]);
    }
    if half_edges
        .iter()
        .zip(&edge_by_half)
        .any(|(half, edge)| half.is_some() && edge.is_none())
    {
        return Err(malformed(name, "edge roots do not cover every half-edge"));
    }
    if edge_knot_intervals_ir.len() != edge_vertices.len() {
        return Err(malformed(name, "edge knot interval map is incomplete"));
    }

    let secondary_layouts = build_secondary_layouts(
        &SecondaryLayoutContext {
            name,
            vertex_roots: &vertex_roots,
            vertex_live: &vertex_live,
            vertex_ir: &vertex_ir,
            face_live: &face_live,
            face_ir: &face_ir,
            half_edges: &half_edges,
            edge_by_half: &edge_by_half,
            grip_vertices: &grip_vertices,
            grip_points: &grip_points,
        },
        &derived_grips,
    )?;

    let mut faces = Vec::new();
    for (face_slot, start) in face_roots.iter().copied().enumerate() {
        let Some(start) = start else { continue };
        if !populated(start) {
            return Err(malformed(name, "face root names a deleted slot"));
        }
        let mut ring = Vec::new();
        let mut current = start;
        loop {
            let half = half_edges[current].expect("invariant: rings only walk populated slots");
            if half.face != face_slot as i64 {
                return Err(malformed(name, "face ring carries a different face index"));
            }
            let (edge, reversed) = edge_by_half[current]
                .ok_or_else(|| malformed(name, "face half-edge has no edge"))?;
            ring.push(SubdEdgeUse { edge, reversed });
            current = half.next;
            if current == start {
                break;
            }
            if ring.len() > half_edges.len() {
                return Err(malformed(name, "face ring does not close"));
            }
        }
        faces.push(SubdFace { edges: ring });
    }

    let mut crease_incidence =
        ctx.alloc_filled(live_vertices, 0usize, "f3d subd crease incidence")?;
    for edge in &crease_edges {
        let vertices = edge_ir
            .get(*edge)
            .copied()
            .flatten()
            .and_then(|edge| edge_vertices.get(edge as usize))
            .ok_or_else(|| malformed(name, "crease edge is out of range"))?;
        crease_incidence[vertices[0] as usize] += 1;
        crease_incidence[vertices[1] as usize] += 1;
    }
    let vertices = (0..live_vertices)
        .map(|index| SubdVertex {
            point: vertex_points[&(index as u32)],
            tag: match crease_incidence[index] {
                0 => SubdVertexTag::Smooth,
                1 => SubdVertexTag::Dart,
                2 => SubdVertexTag::Crease,
                _ => SubdVertexTag::Corner,
            },
            secondary_grips: secondary_layouts[index].clone(),
        })
        .collect();
    let creased_edges = crease_edges
        .iter()
        .filter_map(|slot| edge_ir.get(*slot).copied().flatten())
        .collect::<BTreeSet<_>>();
    let edges = edge_vertices
        .into_iter()
        .enumerate()
        .map(|(index, vertices)| {
            let crease = creased_edges.contains(&(index as u32));
            let sharpness = if crease { FULL_CREASE_SHARPNESS } else { 0.0 };
            SubdEdge {
                vertices,
                sharpness: [sharpness; 2],
                tag: if crease {
                    SubdEdgeTag::Crease
                } else {
                    SubdEdgeTag::Smooth
                },
                knot_interval: Some(edge_knot_intervals_ir[index]),
                sector_coefficients: [0.0, 0.0],
            }
        })
        .collect();
    let source_key = name
        .rsplit_once('/')
        .map_or(name, |(_, base)| base)
        .strip_suffix(".tsm")
        .unwrap_or(name);
    Ok(ParsedCage {
        surface: SubdSurface {
            id: SubdId(format!("f3d:tspline:subd#{source_key}")),
            scheme: SubdScheme::CatmullClark,
            vertices,
            edges,
            faces,
            symmetries,
            source_object: Some(SourceObjectAssociation {
                format: "f3d".into(),
                object_id: name.into(),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        },
        unknown_records,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    const EPS_KNOT_INTERVAL: f64 = 1e-12;

    const QUAD_TOPOLOGY: &str = "degree 3\n\
cap-type G1CAPS\n\
star-smoothness 0\n\
units 1 meters\n\
end-conditions SUBD_CREASES\n\
star-knot-rule NURCCS\n\
f 0 0\n\
e 0 1\ne 2 1\ne 4 1\ne 6 1\n\
v 0 NORTH\nv 2 NORTH\nv 4 NORTH\nv 6 NORTH\n\
l 2 6 1 0 0 0 0\nl 7 3 0 3 -1 0 0\n\
l 4 0 3 1 0 0 0\nl 1 5 2 0 -1 0 0\n\
l 6 2 5 2 0 0 0\nl 3 7 4 1 -1 0 0\n\
l 0 4 7 3 0 0 0\nl 5 1 6 2 -1 0 0\n\
ec 0 0\nec 1 0\nec 2 0\nec 3 0\n";

    fn parse_cage(bytes: &[u8]) -> Result<super::ParsedCage, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default())
            .expect("test decode context");
        super::parse(&ctx, "synthetic.tsm", bytes)
    }

    #[test]
    fn parses_explicit_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn parses_positional_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn counts_records_without_typed_semantics() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             vendor-extension 1 2 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_eq!(cage.unknown_records, 1);
        assert_quad(&cage.surface);
    }

    #[test]
    fn parses_editor_metadata_and_derived_grip_connectivity() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv 0\n\
             0m cg 0 4 1 0 0 0 4\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g 0.5 0 0 1\n\
             100edges 0 2\n100verts 1\n50000grip 0\n50000grip 1\n\
             105sym 0\n105plane 0 2 0 1 0 1 0 0 0 0 1 0\n\
             105a fr 0 0\n105a er 0 0 1 2\n105a e 2 1\n\
             105a vr 0 1\n105a v 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("typed metadata");
        assert_eq!(cage.unknown_records, 0);
        assert_quad(&cage.surface);
        let layout = cage.surface.vertices[0]
            .secondary_grips
            .as_ref()
            .expect("secondary grip layout");
        assert_eq!(layout.direction, cadmpeg_ir::SubdGripDirection::North);
        assert_eq!(layout.wedges.len(), 4);
        assert_eq!(layout.wedges[0].spokes[0].as_ref().unwrap().source_index, 4);
        assert!(layout.wedges[2..]
            .iter()
            .all(|wedge| wedge.phantom && wedge.spokes.is_empty()));
        assert!(layout.wedges[1].sector_face.is_none());

        assert_eq!(cage.surface.symmetries.len(), 1);
        let symmetry = &cage.surface.symmetries[0];
        assert_eq!(symmetry.kind, cadmpeg_ir::SubdSymmetryKind::Correspondence);
        assert_eq!(
            symmetry.plane.origin,
            cadmpeg_ir::math::Point3::new(0.0, 20.0, 0.0)
        );
        assert_eq!(
            symmetry.plane.first_axis,
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            symmetry.plane.second_axis,
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        assert_eq!(symmetry.face_pairs, vec![[0, 0]]);
        assert_eq!(symmetry.edge_pairs, vec![[0, 0], [1, 2]]);
        assert_eq!(symmetry.vertex_pairs, vec![[0, 1]]);
    }

    #[test]
    fn partitions_rectangular_sector_grids_with_product_arity() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0m gv 0\n0m gv 0\n0m gv 0\n0m gv 0\n0m gv 0\n\
             0m cg 0 4 2 1 0 0 4 5 6 7 8\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             0g 0.1 0 0 1\n0g 0.2 0 0 1\n0g 0.3 0 0 1\n0g 0.4 0 0 1\n0g 0.5 0 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("rectangular sector grid");
        let layout = cage.surface.vertices[0]
            .secondary_grips
            .as_ref()
            .expect("secondary grip layout");
        assert_eq!(layout.wedges[0].spokes.len(), 2);
        assert_eq!(layout.wedges[0].sectors.len(), 2);
        assert_eq!(layout.wedges[1].spokes.len(), 1);
        assert!(layout.wedges[1].sectors.is_empty());
        assert_eq!(
            layout.wedges[0]
                .spokes
                .iter()
                .chain(layout.wedges[0].sectors.iter())
                .chain(layout.wedges[1].spokes.iter())
                .map(|grip| grip.as_ref().unwrap().source_index)
                .collect::<Vec<_>>(),
            vec![4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn maps_compass_words_to_north_anchored_offsets() {
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::North),
            0
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::East),
            1
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::South),
            2
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::West),
            3
        );
    }

    #[test]
    fn transfers_knot_intervals_and_absolute_crease_sharpness() {
        let source = QUAD_TOPOLOGY
            .replace("e 0 1\n", "e 0 0.5\n")
            .replace("e 2 1\n", "e 2 0.25\n")
            .replace("e 4 1\n", "e 4 0.125\n")
            .replace("e 6 1\n", "e 6 0.0625\n");
        let cage = parse_cage(
            format!(
                "#TS0200\n{source}\
                 0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
            )
            .as_bytes(),
        )
        .expect("knot intervals");
        let expected = [0.5, 0.25, 0.125, 0.0625];
        for (edge, expected) in cage.surface.edges.iter().zip(expected) {
            let actual = edge.knot_interval.expect("knot interval");
            assert!((actual - expected).abs() < EPS_KNOT_INTERVAL);
            assert!(edge
                .sharpness
                .iter()
                .all(|sharpness| (*sharpness - 1.0).abs() < EPS_KNOT_INTERVAL));
        }
    }

    #[test]
    fn parses_radial_symmetry_metadata() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             105sym 1\n105plane 0 0 0 1 0 1 0 0 0 0 1 0\n\
             105r segments 4\n105r sweep 1\n\
             105r ef 0 1\n105r er 1 0\n105r ff 0 0\n105r fr 0 0\n\
             105r vf 0 1\n105r vr 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("radial symmetry metadata");
        assert_eq!(cage.unknown_records, 0);
        assert_quad(&cage.surface);
        assert_eq!(cage.surface.symmetries.len(), 1);
        let symmetry = &cage.surface.symmetries[0];
        assert_eq!(
            symmetry.kind,
            cadmpeg_ir::SubdSymmetryKind::Radial {
                segments: 4,
                sweep: 1.0
            }
        );
        assert_eq!(
            symmetry.plane.origin,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
        );
        assert_eq!(
            symmetry.plane.first_axis,
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            symmetry.plane.second_axis,
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        assert!(symmetry.face_pairs.is_empty());
        assert!(symmetry.edge_pairs.is_empty());
        assert!(symmetry.vertex_pairs.is_empty());
        assert_eq!(
            symmetry.radial_maps,
            vec![
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Ef,
                    pairs: vec![[0, 1]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Er,
                    pairs: vec![[1, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Ff,
                    pairs: vec![[0, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Fr,
                    pairs: vec![[0, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Vf,
                    pairs: vec![[0, 1]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Vr,
                    pairs: vec![[1, 0]],
                },
            ]
        );

        let unsupported = source.replace("105sym 1", "105sym 2");
        let error = parse_cage(unsupported.as_bytes()).expect_err("unsupported symmetry mode");
        assert!(
            error.to_string().contains("unsupported symmetry flags"),
            "unexpected error: {error}"
        );

        let missing = source.replace("105r vf 0 1\n", "");
        let error = parse_cage(missing.as_bytes()).expect_err("incomplete radial maps");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));

        let native_id = u64::MAX;
        let replacement = format!("105r ef {native_id} {native_id}\n");
        let native = source.replace("105r ef 0 1\n", &replacement);
        let cage = parse_cage(native.as_bytes()).expect("opaque radial native id");
        let ef = cage.surface.symmetries[0]
            .radial_maps
            .iter()
            .find(|map| map.selector == cadmpeg_ir::SubdRadialMapSelector::Ef)
            .expect("ef radial map");
        assert_eq!(ef.pairs, vec![[native_id, native_id]]);
    }

    #[test]
    fn rejects_nonorthonormal_symmetry_plane() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             105sym 0\n105plane 0 0 0 1 1 0 0 0 1 0 0 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let error = parse_cage(source.as_bytes()).expect_err("nonorthonormal symmetry plane");
        assert!(
            error
                .to_string()
                .contains("symmetry plane is not a homogeneous orthonormal frame"),
            "unexpected error: {error}"
        );
    }

    /// A bare topology token is a deleted slot: it consumes an index and
    /// defines no element. Appending one of each leaves the cage unchanged.
    #[test]
    fn deleted_slots_consume_an_index_without_defining_an_element() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}f\ne\nv\nl\n\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv -1\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    /// Deleted slots renumber the IR: a leading deleted vertex and edge slot
    /// shift every program index by one without changing the emitted cage.
    #[test]
    fn deleted_slots_renumber_the_cage() {
        let shifted = QUAD_TOPOLOGY
            .replace("e 0 1\n", "e\ne 0 1\n")
            .replace("v 0 NORTH\n", "v\nv 0 NORTH\n")
            .replace(
                "ec 0 0\nec 1 0\nec 2 0\nec 3 0\n",
                "ec 1 0\nec 2 0\nec 3 0\nec 4 0\n",
            );
        let shifted = shifted.replace("l 2 6 1 0 0 0 0", "l 2 6 1 1 0 0 0");
        let shifted = shifted.replace("l 7 3 0 3 -1 0 0", "l 7 3 0 4 -1 0 0");
        let shifted = shifted.replace("l 4 0 3 1 0 0 0", "l 4 0 3 2 0 0 0");
        let shifted = shifted.replace("l 1 5 2 0 -1 0 0", "l 1 5 2 1 -1 0 0");
        let shifted = shifted.replace("l 6 2 5 2 0 0 0", "l 6 2 5 3 0 0 0");
        let shifted = shifted.replace("l 3 7 4 1 -1 0 0", "l 3 7 4 2 -1 0 0");
        let shifted = shifted.replace("l 0 4 7 3 0 0 0", "l 0 4 7 4 0 0 0");
        let shifted = shifted.replace("l 5 1 6 2 -1 0 0", "l 5 1 6 3 -1 0 0");
        let source = format!(
            "#TS0200\n{shifted}\
             0m odd-grip-map\n0m gv -1\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gvp 4\n\
             0g\n0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("shifted quad cage");
        assert_quad(&cage.surface);
    }

    /// A populated half-edge may not name a deleted slot.
    #[test]
    fn a_half_edge_naming_a_deleted_slot_is_rejected() {
        let source = format!(
            "#TS0200\n{}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n",
            QUAD_TOPOLOGY.replace("l 5 1 6 2 -1 0 0", "l")
        );
        let error = parse_cage(source.as_bytes()).expect_err("deleted mate");
        assert!(
            error.to_string().contains("names a deleted slot"),
            "unexpected error: {error}"
        );
    }

    fn assert_quad(cage: &cadmpeg_ir::subd::SubdSurface) {
        assert_eq!(cage.vertices.len(), 4);
        assert_eq!(cage.edges.len(), 4);
        assert_eq!(cage.faces.len(), 1);
        assert_eq!(cage.vertices[1].point.x, 10.0);
        assert!(cage.faces[0].edges.iter().all(|use_| !use_.reversed));
    }
}
