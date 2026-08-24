// SPDX-License-Identifier: Apache-2.0
//! Decode `TSplines.BlobParts/*.tsm` Form control cages.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{alloc_filled, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::SubdId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use cadmpeg_ir::SourceObjectAssociation;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

const ENTRY_MARKER: &str = "/TSplines.BlobParts/";

#[derive(Clone, Copy)]
struct HalfEdge {
    next: usize,
    previous: usize,
    mate: usize,
    vertex: usize,
    face: i64,
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
    grip_indices: Vec<i64>,
}

#[derive(Debug, Default)]
struct SymmetryBlock {
    plane: Option<[f64; 12]>,
    map_kinds: BTreeSet<String>,
    face_forward: BTreeMap<usize, usize>,
    face_reverse: BTreeMap<usize, usize>,
    edge_forward: BTreeMap<usize, usize>,
    edge_reverse: BTreeMap<usize, usize>,
    vertex_forward: BTreeMap<usize, usize>,
    vertex_reverse: BTreeMap<usize, usize>,
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
    let mut vertex_live: Vec<bool> = Vec::new();
    let mut half_edges: Vec<Option<HalfEdge>> = Vec::new();
    let mut crease_edges = BTreeSet::new();
    let mut grip_vertices: Vec<GripVertexMarker> = Vec::new();
    let mut grip_points: Vec<Option<Point3>> = Vec::new();
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
                None => edge_roots.push(None),
                root => {
                    edge_roots.push(Some(parse_usize(name, root, "edge root")?));
                    // TS-03: the scalar's target quantity is not established;
                    // `ec` records independently define crease membership.
                    parse_f64(name, fields.next(), "edge scalar")?;
                    require_end(name, fields, "edge")?;
                }
            },
            Some("v") => match fields.next() {
                None => vertex_live.push(false),
                root => {
                    parse_usize(name, root, "vertex root")?;
                    if fields.next().is_none() {
                        return Err(malformed(name, "missing vertex direction"));
                    }
                    require_end(name, fields, "vertex")?;
                    vertex_live.push(true);
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
                        grip_indices,
                    });
                }
                _ => return Err(malformed(name, "unknown odd-grip-map record")),
            },
            Some("0g") => match fields.next() {
                None => grip_points.push(None),
                x => {
                    let point = Point3::new(
                        parse_f64(name, x, "grip x")? * 10.0,
                        parse_f64(name, fields.next(), "grip y")? * 10.0,
                        parse_f64(name, fields.next(), "grip z")? * 10.0,
                    );
                    let weight = parse_f64(name, fields.next(), "grip weight")?;
                    if weight <= 0.0 || fields.next().is_some() {
                        return Err(malformed(name, "grip weight is not positive"));
                    }
                    grip_points.push(Some(point));
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
                if parse_i64(name, fields.next(), "symmetry flags")? != 0 {
                    return Err(malformed(name, "unsupported symmetry flags"));
                }
                require_end(name, fields, "symmetry header")?;
                if let Some(block) = current_symmetry.replace(SymmetryBlock::default()) {
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
                if !block.map_kinds.insert(kind.into()) {
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

    // Slot indices address the program; IR indices address only populated slots.
    let vertex_ir = compact(vertex_live.iter().copied());
    let edge_ir = compact(edge_roots.iter().map(Option::is_some));
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
        for (slot, point) in grip_points.into_iter().enumerate() {
            if let (true, Some(point)) = (vertex_live[slot], point) {
                vertex_points.insert(vertex_of(slot)?, point);
            }
        }
    } else {
        for (marker, point) in grip_vertices.into_iter().zip(grip_points) {
            let (GripVertexMarker::Primary(slot), Some(point)) = (marker, point) else {
                continue;
            };
            if vertex_points.insert(vertex_of(slot)?, point).is_some() {
                return Err(malformed(name, "primary grip vertex map is inconsistent"));
            }
        }
    }
    if vertex_points.len() != live_vertices {
        return Err(malformed(name, "primary grip vertex map is incomplete"));
    }

    let mut edge_by_half = alloc_filled(half_edges.len(), None, "f3d T-spline half-edge map")?;
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
            SubdEdge {
                vertices,
                sharpness: [0.0, 0.0],
                tag: if crease {
                    SubdEdgeTag::Crease
                } else {
                    SubdEdgeTag::Smooth
                },
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
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0m cg 0 1 1 -1 -1\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             100edges 0 2\n100verts 1\n50000grip 0\n50000grip 1\n\
             105sym 0\n105plane 0 2 0 1 0 1 0 0 0 0 1 0\n\
             105a fr 0 0\n105a er 0 0 1 2\n105a e 2 1\n\
             105a vr 0 1\n105a v 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("typed metadata");
        assert_eq!(cage.unknown_records, 0);
        assert_quad(&cage.surface);
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
