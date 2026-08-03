// SPDX-License-Identifier: Apache-2.0
//! Decode `TSplines.BlobParts/*.tsm` Form control cages.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_codec_core::CodecError;
use cadmpeg_ir::ids::SubdId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use cadmpeg_ir::SourceObjectAssociation;

use crate::container::ContainerScan;

const ENTRY_MARKER: &str = "/TSplines.BlobParts/";

#[derive(Clone, Copy)]
struct HalfEdge {
    next: usize,
    previous: usize,
    mate: usize,
    vertex: usize,
    face: i64,
}

/// Decode every active-asset T-spline control cage in archive order.
///
/// A cage whose program is internally inconsistent degrades to an
/// error-severity loss note instead of failing the document decode; its
/// entry bytes remain retained in the container, and the serializer-backed
/// Form join leaves the affected Form on native retention.
pub(crate) fn decode(
    scan: &ContainerScan,
) -> Result<(Vec<SubdSurface>, Vec<cadmpeg_ir::report::LossNote>), CodecError> {
    let prefix = scan
        .asset_folder
        .as_ref()
        .map(|folder| format!("{folder}{ENTRY_MARKER}"));
    let mut cages = Vec::new();
    let mut losses = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        std::path::Path::new(&entry.name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tsm"))
            && prefix
                .as_ref()
                .is_none_or(|prefix| entry.name.starts_with(prefix))
    }) {
        match parse(&entry.name, scan.entry_bytes(&entry.name)?) {
            Ok(parsed) => {
                if parsed.unknown_records != 0 {
                    losses.push(cadmpeg_ir::report::LossNote {
                        code: cadmpeg_ir::report::LossKind::RecordNotTyped,
                        severity: cadmpeg_ir::report::Severity::Warning,
                        message: format!(
                            "{} T-spline record(s) were retained without typed semantics.",
                            parsed.unknown_records
                        ),
                        provenance: None,
                    });
                }
                cages.push(parsed.surface);
            }
            Err(error) => losses.push(cadmpeg_ir::report::LossNote {
                code: cadmpeg_ir::report::LossKind::GeometryNotTransferred,
                severity: cadmpeg_ir::report::Severity::Error,
                message: format!("T-spline control cage not decoded: {error}"),
                provenance: None,
            }),
        }
    }
    Ok((cages, losses))
}

fn malformed(name: &str, message: impl std::fmt::Display) -> CodecError {
    CodecError::Malformed(format!("T-spline cage {name}: {message}"))
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

fn parse(name: &str, bytes: &[u8]) -> Result<ParsedCage, CodecError> {
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
    let mut grip_vertices: Vec<Option<usize>> = Vec::new();
    let mut grip_points: Vec<Option<Point3>> = Vec::new();
    let mut in_grip_map = false;
    let mut declarations = BTreeSet::new();
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
                    grip_vertices.push(Some(parse_usize(
                        name,
                        fields.next(),
                        "grip vertex index",
                    )?));
                    require_end(name, fields, "primary grip map")?;
                }
                Some("gv") if in_grip_map => {
                    // A deleted grip slot carries `0m gv -1`, so the operand is
                    // signed. Either way the marker assigns no primary grip.
                    parse_i64(name, fields.next(), "secondary grip vertex index")?;
                    grip_vertices.push(None);
                    require_end(name, fields, "secondary grip map")?;
                }
                // TS-01 and TS-02 track the unresolved wedge partition and count.
                Some("cg") if in_grip_map => {}
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
            _ => unknown_records += 1,
        }
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
            let (Some(slot), Some(point)) = (marker, point) else {
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

    let mut crease_incidence = vec![0usize; live_vertices];
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

    #[test]
    fn parses_explicit_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = super::parse("synthetic.tsm", source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn parses_positional_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = super::parse("synthetic.tsm", source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn counts_records_without_typed_semantics() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             vendor-extension 1 2 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = super::parse("synthetic.tsm", source.as_bytes()).expect("quad cage");
        assert_eq!(cage.unknown_records, 1);
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
        let cage = super::parse("synthetic.tsm", source.as_bytes()).expect("quad cage");
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
        let cage = super::parse("synthetic.tsm", source.as_bytes()).expect("shifted quad cage");
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
        let error = super::parse("synthetic.tsm", source.as_bytes()).expect_err("deleted mate");
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
