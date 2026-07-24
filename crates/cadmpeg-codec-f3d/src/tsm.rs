// SPDX-License-Identifier: Apache-2.0
//! Decode `TSplines.BlobParts/*.tsm` Form control cages.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::ids::SubdId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::provenance::SourceObjectAssociation;
use cadmpeg_ir::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};

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
pub(crate) fn decode(scan: &ContainerScan) -> Result<Vec<SubdSurface>, CodecError> {
    let prefix = scan
        .asset_folder
        .as_ref()
        .map(|folder| format!("{folder}{ENTRY_MARKER}"));
    scan.entries
        .iter()
        .filter(|entry| {
            std::path::Path::new(&entry.name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("tsm"))
                && prefix
                    .as_ref()
                    .is_none_or(|prefix| entry.name.starts_with(prefix))
        })
        .map(|entry| parse(&entry.name, scan.entry_bytes(&entry.name)?))
        .collect()
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

fn parse(name: &str, bytes: &[u8]) -> Result<SubdSurface, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| malformed(name, format!("payload is not UTF-8: {error}")))?;
    if text.lines().next() != Some("#TS0200") {
        return Err(malformed(name, "unsupported header"));
    }

    let mut face_roots = Vec::new();
    let mut edge_roots = Vec::new();
    let mut vertex_count = 0usize;
    let mut half_edges = Vec::new();
    let mut crease_edges = BTreeSet::new();
    let mut grip_vertices = Vec::new();
    let mut grip_points = Vec::new();
    let mut in_grip_map = false;
    let mut declarations = BTreeSet::new();
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
            Some("f") => {
                face_roots.push(parse_usize(name, fields.next(), "face root")?);
                parse_i64(name, fields.next(), "face flags")?;
                require_end(name, fields, "face")?;
            }
            Some("e") => {
                edge_roots.push(parse_usize(name, fields.next(), "edge root")?);
                parse_f64(name, fields.next(), "edge scalar")?;
                require_end(name, fields, "edge")?;
            }
            Some("v") => {
                parse_usize(name, fields.next(), "vertex root")?;
                if fields.next().is_none() {
                    return Err(malformed(name, "missing vertex direction"));
                }
                require_end(name, fields, "vertex")?;
                vertex_count += 1;
            }
            Some("l") => {
                let half = HalfEdge {
                    next: parse_usize(name, fields.next(), "half-edge next index")?,
                    previous: parse_usize(name, fields.next(), "half-edge previous index")?,
                    mate: parse_usize(name, fields.next(), "half-edge mate index")?,
                    vertex: parse_usize(name, fields.next(), "half-edge vertex index")?,
                    face: parse_i64(name, fields.next(), "half-edge face index")?,
                };
                parse_i64(name, fields.next(), "half-edge sector index")?;
                parse_i64(name, fields.next(), "half-edge flags")?;
                if fields.next().is_some() {
                    return Err(malformed(name, "half-edge has trailing fields"));
                }
                half_edges.push(half);
            }
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
                    parse_usize(name, fields.next(), "secondary grip vertex index")?;
                    grip_vertices.push(None);
                    require_end(name, fields, "secondary grip map")?;
                }
                Some("cg") if in_grip_map => {}
                _ => return Err(malformed(name, "unknown odd-grip-map record")),
            },
            Some("0g") => {
                let point = Point3::new(
                    parse_f64(name, fields.next(), "grip x")? * 10.0,
                    parse_f64(name, fields.next(), "grip y")? * 10.0,
                    parse_f64(name, fields.next(), "grip z")? * 10.0,
                );
                let weight = parse_f64(name, fields.next(), "grip weight")?;
                if weight <= 0.0 || fields.next().is_some() {
                    return Err(malformed(name, "grip weight is not positive"));
                }
                grip_points.push(point);
            }
            _ => {}
        }
    }

    if declarations.len() != 6
        || face_roots.is_empty()
        || edge_roots.is_empty()
        || vertex_count == 0
        || half_edges.is_empty()
        || (!grip_vertices.is_empty() && grip_vertices.len() != grip_points.len())
    {
        return Err(malformed(name, "control cage is incomplete"));
    }
    for (index, half) in half_edges.iter().enumerate() {
        let mate = half_edges
            .get(half.mate)
            .ok_or_else(|| malformed(name, "half-edge mate is out of range"))?;
        if mate.mate != index
            || half.next >= half_edges.len()
            || half.previous >= half_edges.len()
            || half_edges[half.next].previous != index
            || half_edges[half.previous].next != index
            || half.vertex >= vertex_count
        {
            return Err(malformed(name, "half-edge topology is inconsistent"));
        }
    }

    let mut vertex_points = BTreeMap::new();
    if grip_vertices.is_empty() {
        if grip_points.len() != vertex_count {
            return Err(malformed(name, "positional grip vertex map is incomplete"));
        }
        vertex_points.extend(grip_points.into_iter().enumerate());
    } else {
        for (marker, point) in grip_vertices.into_iter().zip(grip_points) {
            if let Some(vertex) = marker {
                if vertex >= vertex_count || vertex_points.insert(vertex, point).is_some() {
                    return Err(malformed(name, "primary grip vertex map is inconsistent"));
                }
            }
        }
    }
    if vertex_points.len() != vertex_count {
        return Err(malformed(name, "primary grip vertex map is incomplete"));
    }

    let mut edge_by_half = vec![None; half_edges.len()];
    let mut edge_vertices = Vec::with_capacity(edge_roots.len());
    for (edge_index, root) in edge_roots.iter().copied().enumerate() {
        let half = half_edges
            .get(root)
            .ok_or_else(|| malformed(name, "edge root is out of range"))?;
        if edge_by_half[root].replace((edge_index, false)).is_some()
            || edge_by_half[half.mate]
                .replace((edge_index, true))
                .is_some()
        {
            return Err(malformed(name, "edge roots reuse a half-edge"));
        }
        edge_vertices.push([half_edges[half.mate].vertex as u32, half.vertex as u32]);
    }
    if edge_by_half.iter().any(Option::is_none) {
        return Err(malformed(name, "edge roots do not cover every half-edge"));
    }

    let mut faces = Vec::with_capacity(face_roots.len());
    for (face_index, start) in face_roots.iter().copied().enumerate() {
        if start >= half_edges.len() {
            return Err(malformed(name, "face root is out of range"));
        }
        let mut ring = Vec::new();
        let mut current = start;
        loop {
            let half = half_edges[current];
            if half.face != face_index as i64 {
                return Err(malformed(name, "face ring carries a different face index"));
            }
            let (edge, reversed) = edge_by_half[current]
                .ok_or_else(|| malformed(name, "face half-edge has no edge"))?;
            ring.push(SubdEdgeUse {
                edge: edge as u32,
                reversed,
            });
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

    let mut crease_incidence = vec![0usize; vertex_count];
    for edge in &crease_edges {
        let vertices = edge_vertices
            .get(*edge)
            .ok_or_else(|| malformed(name, "crease edge is out of range"))?;
        crease_incidence[vertices[0] as usize] += 1;
        crease_incidence[vertices[1] as usize] += 1;
    }
    let vertices = (0..vertex_count)
        .map(|index| SubdVertex {
            point: vertex_points[&index],
            tag: match crease_incidence[index] {
                0 => SubdVertexTag::Smooth,
                1 => SubdVertexTag::Dart,
                2 => SubdVertexTag::Crease,
                _ => SubdVertexTag::Corner,
            },
        })
        .collect();
    let edges = edge_vertices
        .into_iter()
        .enumerate()
        .map(|(index, vertices)| {
            let crease = crease_edges.contains(&index);
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
    Ok(SubdSurface {
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
        assert_quad(&cage);
    }

    #[test]
    fn parses_positional_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = super::parse("synthetic.tsm", source.as_bytes()).expect("quad cage");
        assert_quad(&cage);
    }

    fn assert_quad(cage: &cadmpeg_ir::subd::SubdSurface) {
        assert_eq!(cage.vertices.len(), 4);
        assert_eq!(cage.edges.len(), 4);
        assert_eq!(cage.faces.len(), 1);
        assert_eq!(cage.vertices[1].point.x, 10.0);
        assert!(cage.faces[0].edges.iter().all(|use_| !use_.reversed));
    }
}
