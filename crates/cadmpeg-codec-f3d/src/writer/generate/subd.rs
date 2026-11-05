// SPDX-License-Identifier: Apache-2.0
//! Author dense T-spline cage programs from neutral topology.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::{index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::subd::{
    SubdEdgeTag, SubdRadialMapSelector, SubdSurface, SubdSymmetryKind, SubdUserKnot, SubdVertexTag,
};

#[derive(Clone, Copy)]
struct HalfEdge {
    next: Option<usize>,
    previous: Option<usize>,
    mate: usize,
    vertex: usize,
    face: Option<usize>,
}

pub(in crate::writer) fn encode(
    ctx: &DecodeContext<'_>,
    subd: &SubdSurface,
) -> Result<Option<String>, CodecError> {
    let cage = &subd.cage;
    let vertices = cage.vertices();
    let edges = cage.edges();
    let faces = cage.faces();
    if vertices.is_empty()
        || edges.is_empty()
        || faces.is_empty()
        || vertices
            .iter()
            .any(|vertex| vertex.secondary_grips().is_some())
    {
        return Ok(None);
    }
    let half_count = edges
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::InvalidInput("T-spline half-edge count overflows".into()))?;
    let face_uses = faces
        .iter()
        .try_fold(0usize, |count, face| count.checked_add(face.edges().len()))
        .ok_or_else(|| CodecError::InvalidInput("T-spline face use count overflows".into()))?;
    let symmetry_pairs = cage
        .symmetries()
        .iter()
        .try_fold(0usize, |count, symmetry| {
            let count = count
                .checked_add(symmetry.face_pairs().len())?
                .checked_add(symmetry.edge_pairs().len())?
                .checked_add(symmetry.vertex_pairs().len())?;
            match symmetry.kind() {
                SubdSymmetryKind::Correspondence {} => Some(count),
                SubdSymmetryKind::Radial(radial) => radial
                    .radial_maps()
                    .iter()
                    .try_fold(count, |count, map| count.checked_add(map.pairs.len())),
            }
        })
        .ok_or_else(|| CodecError::InvalidInput("T-spline symmetry count overflows".into()))?;
    let count = vertices
        .len()
        .checked_add(edges.len())
        .and_then(|count| count.checked_add(faces.len()))
        .and_then(|count| count.checked_add(half_count))
        .and_then(|count| count.checked_add(face_uses))
        .and_then(|count| count.checked_add(symmetry_pairs))
        .ok_or_else(|| CodecError::InvalidInput("T-spline entity count overflows".into()))?;
    ctx.charge_entities(
        u64::try_from(count).map_err(CodecError::malformed)?,
        "F3D T-spline cage",
    )?;
    let mut halves = ctx.alloc_filled(
        half_count,
        HalfEdge {
            next: None,
            previous: None,
            mate: 0,
            vertex: 0,
            face: None,
        },
        "F3D T-spline half edges",
    )?;
    let mut incidence =
        ctx.alloc_filled(vertices.len(), 0usize, "F3D T-spline crease incidence")?;
    for (index, edge) in edges.iter().enumerate() {
        let endpoints = edge.vertices().map(index_from_u32);
        if endpoints.iter().any(|vertex| *vertex >= vertices.len())
            || edge.knot_interval().is_none()
            || edge
                .sector_coefficients()
                .iter()
                .any(|value| value.get() != 0.0)
        {
            return Ok(None);
        }
        let sharpness = match edge.tag {
            SubdEdgeTag::Smooth => 0.0,
            SubdEdgeTag::Crease => 1.0,
            SubdEdgeTag::SmoothX => return Ok(None),
        };
        if edge
            .sharpness()
            .iter()
            .any(|value| value.get() != sharpness)
        {
            return Ok(None);
        }
        if edge.tag == SubdEdgeTag::Crease {
            for vertex in endpoints {
                incidence[vertex] += 1;
            }
        }
        let root = index * 2;
        halves[root] = HalfEdge {
            next: None,
            previous: None,
            mate: root + 1,
            vertex: endpoints[1],
            face: None,
        };
        halves[root + 1] = HalfEdge {
            next: None,
            previous: None,
            mate: root,
            vertex: endpoints[0],
            face: None,
        };
    }
    for (vertex, count) in vertices.iter().zip(incidence) {
        let tag = match count {
            0 => SubdVertexTag::Smooth,
            1 => SubdVertexTag::Dart,
            2 => SubdVertexTag::Crease,
            _ => SubdVertexTag::Corner,
        };
        let p = vertex.point().get();
        if vertex.tag != tag
            || [p.x, p.y, p.z]
                .into_iter()
                .any(|value| value / 10.0 * 10.0 != value)
        {
            return Ok(None);
        }
    }
    let mut face_roots = Vec::new();
    for (face_index, face) in faces.iter().enumerate() {
        let mut ring = Vec::new();
        for edge in face.edges() {
            let index = index_from_u32(edge.edge)
                .checked_mul(2)
                .and_then(|index| index.checked_add(usize::from(edge.reversed)))
                .ok_or_else(|| {
                    CodecError::InvalidInput("T-spline face edge index overflows".into())
                })?;
            if halves.get(index).is_none_or(|half| half.face.is_some()) {
                return Ok(None);
            }
            halves[index].face = Some(face_index);
            ring.push(index);
        }
        let Some(root) = ring.first().copied() else {
            return Ok(None);
        };
        face_roots.push(root);
        for (ordinal, &index) in ring.iter().enumerate() {
            halves[index].next = Some(ring[(ordinal + 1) % ring.len()]);
            halves[index].previous = Some(ring[(ordinal + ring.len() - 1) % ring.len()]);
        }
    }
    let mut boundary = BTreeMap::new();
    for (index, half) in halves
        .iter()
        .enumerate()
        .filter(|(_, half)| half.face.is_none())
    {
        if halves[half.mate].face.is_none()
            || boundary.insert(halves[half.mate].vertex, index).is_some()
        {
            return Ok(None);
        }
    }
    for &index in boundary.values() {
        let Some(&next) = boundary.get(&halves[index].vertex) else {
            return Ok(None);
        };
        if halves[next].previous.replace(index).is_some() {
            return Ok(None);
        }
        halves[index].next = Some(next);
    }
    let mut roots = ctx.alloc_filled(vertices.len(), None, "F3D T-spline vertex roots")?;
    let mut vertex_degree =
        ctx.alloc_filled(vertices.len(), 0usize, "F3D T-spline vertex degree")?;
    for (index, half) in halves.iter().enumerate() {
        roots[half.vertex].get_or_insert(index);
        vertex_degree[half.vertex] += 1;
        if half.next.is_none() || half.previous.is_none() {
            return Ok(None);
        }
    }
    if roots.iter().any(Option::is_none) {
        return Ok(None);
    }
    for (vertex, root) in roots.iter().enumerate() {
        let Some(root) = *root else { return Ok(None) };
        let mut current = root;
        let mut count = 0;
        let mut boundary_gaps = 0;
        loop {
            let half = &halves[current];
            if half.vertex != vertex {
                return Ok(None);
            }
            boundary_gaps += usize::from(half.face.is_none());
            let Some(next) = half.next else {
                return Ok(None);
            };
            current = halves[next].mate;
            count += 1;
            if current == root {
                break;
            }
            if count >= halves.len() {
                return Ok(None);
            }
        }
        if boundary_gaps > 1 || count != vertex_degree[vertex] {
            return Ok(None);
        }
    }
    let end_condition = if edges.iter().any(|edge| edge.user_knot.is_some()) {
        "MULTIPLE_KNOTS"
    } else {
        "SUBD_CREASES"
    };
    let mut output = format!("#TS0200\ndegree 3\ncap-type G1CAPS\nstar-smoothness 0\nunits 1 meters\nend-conditions {end_condition}\nstar-knot-rule NURCCS\n");
    for root in face_roots {
        writeln!(output, "f {root} 0").map_err(CodecError::malformed)?;
    }
    for (index, edge) in edges.iter().enumerate() {
        let Some(interval) = edge.knot_interval() else {
            return Ok(None);
        };
        writeln!(output, "e {} {}", index * 2, interval.get()).map_err(CodecError::malformed)?;
    }
    if edges.iter().any(|edge| edge.user_knot.is_some()) {
        for edge in edges {
            let value = match edge.user_knot {
                Some(SubdUserKnot::Override(value)) => value.get(),
                Some(SubdUserKnot::Inherit) => -1.0,
                None => return Ok(None),
            };
            writeln!(output, "106ek {value}").map_err(CodecError::malformed)?;
        }
    }
    for root in roots {
        let Some(root) = root else { return Ok(None) };
        writeln!(output, "v {root} NORTH").map_err(CodecError::malformed)?;
    }
    for (index, half) in halves.iter().enumerate() {
        let (Some(next), Some(previous)) = (half.next, half.previous) else {
            return Ok(None);
        };
        let face = half
            .face
            .map_or_else(|| "-1".to_owned(), |face| face.to_string());
        writeln!(
            output,
            "l {next} {previous} {} {} {face} {} 0",
            half.mate,
            half.vertex,
            index / 2
        )
        .map_err(CodecError::malformed)?;
    }
    for (index, _) in edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.tag == SubdEdgeTag::Crease)
    {
        writeln!(output, "ec {index} 0").map_err(CodecError::malformed)?;
    }
    for vertex in vertices {
        let point = vertex.point().get();
        writeln!(
            output,
            "0g {} {} {} 1",
            point.x / 10.0,
            point.y / 10.0,
            point.z / 10.0
        )
        .map_err(CodecError::malformed)?;
    }
    for symmetry in cage.symmetries() {
        let origin = symmetry.plane.origin().get();
        if [origin.x, origin.y, origin.z]
            .into_iter()
            .any(|value| value / 10.0 * 10.0 != value)
        {
            return Ok(None);
        }
        let first = *symmetry.plane.first_axis().as_raw();
        let second = *symmetry.plane.second_axis().as_raw();
        let mode = match symmetry.kind() {
            SubdSymmetryKind::Correspondence {} => 0,
            SubdSymmetryKind::Radial(_) => 1,
        };
        writeln!(
            output,
            "105sym {mode}\n105plane {} {} {} 1 {} {} {} 0 {} {} {} 0",
            origin.x / 10.0,
            origin.y / 10.0,
            origin.z / 10.0,
            first.x,
            first.y,
            first.z,
            second.x,
            second.y,
            second.z
        )
        .map_err(CodecError::malformed)?;
        let pairs = [
            (symmetry.face_pairs(), faces.len()),
            (symmetry.edge_pairs(), edges.len()),
            (symmetry.vertex_pairs(), vertices.len()),
        ];
        if pairs.iter().any(|(pairs, count)| {
            pairs
                .iter()
                .flatten()
                .any(|index| index_from_u32(*index) >= *count)
                || matches!(symmetry.kind(), SubdSymmetryKind::Correspondence {})
                    && pairs.windows(2).any(|rows| rows[0][0] >= rows[1][0])
        }) {
            return Ok(None);
        }
        match symmetry.kind() {
            SubdSymmetryKind::Correspondence {} => {
                for ((forward, _), (f, r)) in
                    pairs.iter().zip([("fr", "f"), ("er", "e"), ("vr", "v")])
                {
                    write_correspondence(&mut output, f, forward, false)?;
                    write_correspondence(&mut output, r, forward, true)?;
                }
            }
            SubdSymmetryKind::Radial(radial) => {
                writeln!(
                    output,
                    "105r segments {}\n105r sweep {}",
                    radial.segments(),
                    radial.sweep().get()
                )
                .map_err(CodecError::malformed)?;
                let selectors = [
                    (
                        SubdRadialMapSelector::Ef,
                        SubdRadialMapSelector::Er,
                        symmetry.edge_pairs(),
                        edges.len(),
                    ),
                    (
                        SubdRadialMapSelector::Ff,
                        SubdRadialMapSelector::Fr,
                        symmetry.face_pairs(),
                        faces.len(),
                    ),
                    (
                        SubdRadialMapSelector::Vf,
                        SubdRadialMapSelector::Vr,
                        symmetry.vertex_pairs(),
                        vertices.len(),
                    ),
                ];
                if radial.radial_maps().is_empty() {
                    for (forward, reverse, pairs, _) in selectors {
                        write_radial(
                            &mut output,
                            forward,
                            pairs
                                .iter()
                                .map(|pair| [u64::from(pair[0]), u64::from(pair[1])]),
                        )?;
                        write_radial(
                            &mut output,
                            reverse,
                            pairs
                                .iter()
                                .map(|pair| [u64::from(pair[1]), u64::from(pair[0])]),
                        )?;
                    }
                } else {
                    if radial.radial_maps().len() != 6 {
                        return Ok(None);
                    }
                    for (forward, reverse, expected, count) in selectors {
                        let Some(f) = radial
                            .radial_maps()
                            .iter()
                            .find(|map| map.selector == forward)
                        else {
                            return Ok(None);
                        };
                        let Some(r) = radial
                            .radial_maps()
                            .iter()
                            .find(|map| map.selector == reverse)
                        else {
                            return Ok(None);
                        };
                        if f.pairs.iter().any(|pair| {
                            pair.iter().any(|index| {
                                usize::try_from(*index).map_or(true, |index| index >= count)
                            })
                        }) || f
                            .pairs
                            .iter()
                            .map(|pair| [u32::try_from(pair[0]).ok(), u32::try_from(pair[1]).ok()])
                            .ne(expected.iter().map(|pair| [Some(pair[0]), Some(pair[1])]))
                        {
                            return Ok(None);
                        }
                        let reverse_set = r
                            .pairs
                            .iter()
                            .copied()
                            .collect::<std::collections::BTreeSet<_>>();
                        if reverse_set.len() != f.pairs.len()
                            || f.pairs
                                .iter()
                                .any(|pair| !reverse_set.contains(&[pair[1], pair[0]]))
                        {
                            return Ok(None);
                        }
                    }
                    for map in radial.radial_maps() {
                        write_radial(&mut output, map.selector, map.pairs.iter().copied())?;
                    }
                }
            }
        }
    }
    Ok(Some(output))
}

fn write_correspondence(
    out: &mut String,
    selector: &str,
    pairs: &[[u32; 2]],
    reverse: bool,
) -> Result<(), CodecError> {
    write!(out, "105a {selector}").map_err(CodecError::malformed)?;
    for [source, target] in pairs {
        if reverse && source == target {
            continue;
        }
        let (source, target) = if reverse {
            (target, source)
        } else {
            (source, target)
        };
        write!(out, " {source} {target}").map_err(CodecError::malformed)?;
    }
    writeln!(out).map_err(CodecError::malformed)
}

fn write_radial(
    out: &mut String,
    selector: SubdRadialMapSelector,
    pairs: impl Iterator<Item = [u64; 2]>,
) -> Result<(), CodecError> {
    let selector = match selector {
        SubdRadialMapSelector::Ef => "ef",
        SubdRadialMapSelector::Er => "er",
        SubdRadialMapSelector::Ff => "ff",
        SubdRadialMapSelector::Fr => "fr",
        SubdRadialMapSelector::Vf => "vf",
        SubdRadialMapSelector::Vr => "vr",
    };
    write!(out, "105r {selector}").map_err(CodecError::malformed)?;
    for [source, target] in pairs {
        write!(out, " {source} {target}").map_err(CodecError::malformed)?;
    }
    writeln!(out).map_err(CodecError::malformed)
}
