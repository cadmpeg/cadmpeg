// SPDX-License-Identifier: Apache-2.0
//! Native topology records in the E5 `0D 03` stream family.

use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct E5Topology {
    pub bodies: Vec<E5Body>,
    pub faces: Vec<E5Face>,
    pub edges: BTreeMap<u32, E5Edge>,
    pub pcurves: BTreeMap<u32, E5Pcurve>,
    pub bounds: BTreeMap<u32, E5Bounds>,
    pub curve_supports: BTreeMap<u32, E5CurveSupport>,
    pub vertex_refs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct E5CurveSupport {
    pub record_id: u32,
    pub intersection: bool,
    pub pcurves: Vec<u32>,
    pub mode: u8,
    pub range: [f64; 2],
    pub tail: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct E5Bounds {
    pub record_id: u32,
    pub entries: Vec<E5BoundEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct E5BoundEntry {
    pub representation: u32,
    pub parameter: f64,
    pub code: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum E5Pcurve {
    Line {
        surface: u32,
        origin: [f64; 2],
        direction: [f64; 2],
        range: [f64; 2],
    },
    Circle {
        surface: u32,
        center: [f64; 2],
        codes: [u32; 2],
        radius: f64,
        range: [f64; 2],
    },
    Jet {
        surface: u32,
        degree: u32,
        knots: Vec<f64>,
        multiplicities: Vec<u32>,
        points: Vec<[f64; 2]>,
        first_derivatives: Vec<[f64; 2]>,
        second_derivatives: Vec<[f64; 2]>,
        range: [f64; 2],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5Body {
    pub record_id: u32,
    pub root_record_id: u32,
    pub faces: Vec<u32>,
    pub face_orientation_signs: Vec<i16>,
    pub extra_orientation_signs: [i16; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5Face {
    pub record_id: u32,
    pub surface: u32,
    pub trailer_sign: i16,
    pub loops: Vec<E5Loop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5Loop {
    pub record_id: u32,
    pub surface: u32,
    pub pcurves: Vec<u32>,
    pub edge_uses: Vec<u32>,
    pub reversed: Vec<bool>,
    pub absolute_reversed: Option<Vec<bool>>,
    pub outer: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct E5Edge {
    pub record_id: u32,
    pub support: u32,
    pub start_vertex: u32,
    pub end_vertex: u32,
    pub parameter_start: u32,
    pub parameter_end: u32,
    pub terminator: u32,
}

#[derive(Debug)]
struct Record<'a> {
    class: u8,
    id: u32,
    payload: &'a [u8],
}

#[derive(Debug)]
struct RawFace {
    id: u32,
    surface: u32,
    loops: Vec<u32>,
    trailer_sign: i16,
}

#[derive(Debug)]
struct RawLoop {
    id: u32,
    surface: u32,
    pcurves: Vec<u32>,
    edges: Vec<u32>,
    outer: Option<bool>,
}

/// Resolve E5 face→loop→edge-use references and determine each serialized
/// loop occurrence's unique head-to-tail traversal from stored vertex refs.
#[must_use]
pub fn parse_topology(bytes: &[u8]) -> Option<E5Topology> {
    let records = records(bytes);
    let by_id: HashMap<u32, &Record<'_>> =
        records.iter().map(|record| (record.id, record)).collect();
    if by_id.len() != records.len() {
        return None;
    }

    let edges: BTreeMap<u32, E5Edge> = records
        .iter()
        .filter(|record| record.class == 0xff)
        .map(|record| parse_edge(record).map(|edge| (record.id, edge)))
        .collect::<Option<_>>()?;
    let pcurves: BTreeMap<u32, E5Pcurve> = records
        .iter()
        .filter(|record| matches!(record.class, 0x96 | 0x97 | 0xa0))
        .map(|record| parse_pcurve(record).map(|pcurve| (record.id, pcurve)))
        .collect::<Option<_>>()?;
    let bounds: BTreeMap<u32, E5Bounds> = records
        .iter()
        .filter(|record| record.class == 0x0e)
        .map(|record| parse_bounds(record).map(|bounds| (record.id, bounds)))
        .collect::<Option<_>>()?;
    let curve_supports: BTreeMap<u32, E5CurveSupport> = records
        .iter()
        .filter(|record| matches!(record.class, 0xc0 | 0xc1))
        .map(|record| parse_curve_support(record).map(|support| (record.id, support)))
        .collect::<Option<_>>()?;
    let loops: HashMap<u32, RawLoop> = records
        .iter()
        .filter(|record| record.class == 0x09 && record.payload.len() != 43)
        .map(|record| parse_loop(record).map(|loop_| (record.id, loop_)))
        .collect::<Option<_>>()?;
    let raw_faces: Vec<RawFace> = records
        .iter()
        .filter(|record| record.class == 0x00)
        .map(|record| parse_face(record))
        .collect::<Option<_>>()?;
    let vertex_ids: HashSet<u32> = records
        .iter()
        .filter(|record| record.class == 0xfe)
        .map(|record| record.id)
        .collect();
    if raw_faces.is_empty() || loops.is_empty() || edges.is_empty() || vertex_ids.is_empty() {
        return None;
    }

    let mut faces = Vec::with_capacity(raw_faces.len());
    let mut reachable_edges = HashSet::new();
    for face in raw_faces {
        by_id.get(&face.surface)?;
        let mut resolved_loops = Vec::with_capacity(face.loops.len());
        for loop_id in face.loops {
            let raw = loops.get(&loop_id)?;
            if raw.surface != face.surface {
                return None;
            }
            let reversed = solve_loop_chain(&raw.edges, &edges)?;
            for pcurve_id in &raw.pcurves {
                let pcurve = pcurves.get(pcurve_id)?;
                let surface = match pcurve {
                    E5Pcurve::Line { surface, .. }
                    | E5Pcurve::Circle { surface, .. }
                    | E5Pcurve::Jet { surface, .. } => *surface,
                };
                if surface != raw.surface {
                    return None;
                }
            }
            for edge_id in &raw.edges {
                let edge = edges.get(edge_id)?;
                if !vertex_ids.contains(&edge.start_vertex)
                    || !vertex_ids.contains(&edge.end_vertex)
                {
                    return None;
                }
                reachable_edges.insert(*edge_id);
                if !curve_supports.is_empty() && !curve_supports.contains_key(&edge.support) {
                    return None;
                }
            }
            resolved_loops.push(E5Loop {
                record_id: raw.id,
                surface: raw.surface,
                pcurves: raw.pcurves.clone(),
                edge_uses: raw.edges.clone(),
                reversed,
                absolute_reversed: None,
                outer: raw.outer,
            });
        }
        faces.push(E5Face {
            record_id: face.id,
            surface: face.surface,
            trailer_sign: face.trailer_sign,
            loops: resolved_loops,
        });
    }
    solve_absolute_orientation(&mut faces);
    let edges = edges
        .into_iter()
        .filter(|(id, _)| reachable_edges.contains(id))
        .collect();
    let mut vertex_refs: Vec<u32> = vertex_ids.into_iter().collect();
    vertex_refs.sort_unstable();
    let bodies = parse_bodies(&records, &by_id)?;
    if !bodies.is_empty() {
        let roster: Vec<u32> = bodies
            .iter()
            .flat_map(|body| body.faces.iter().copied())
            .collect();
        let roster_set: HashSet<u32> = roster.iter().copied().collect();
        let face_set: HashSet<u32> = faces.iter().map(|face| face.record_id).collect();
        if roster.len() != roster_set.len() || roster_set != face_set {
            return None;
        }
    }
    Some(E5Topology {
        bodies,
        faces,
        edges,
        pcurves,
        bounds,
        curve_supports,
        vertex_refs,
    })
}

fn parse_curve_support(record: &Record<'_>) -> Option<E5CurveSupport> {
    let (pcurves, mut position) = counted_references(record.payload)?;
    let expected = if record.class == 0xc0 { 1 } else { 2 };
    if pcurves.len() != expected || record.payload.get(position) != Some(&0x81) {
        return None;
    }
    position += 1;
    let mode = *record.payload.get(position)?;
    position += 1;
    if record.payload.get(position) != Some(&0x00) {
        return None;
    }
    position += 1;
    let range = [
        f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        ),
        f64::from_le_bytes(
            record
                .payload
                .get(position + 8..position + 16)?
                .try_into()
                .ok()?,
        ),
    ];
    if range.iter().any(|value| !value.is_finite()) {
        return None;
    }
    position += 16;
    Some(E5CurveSupport {
        record_id: record.id,
        intersection: record.class == 0xc1,
        pcurves,
        mode,
        range,
        tail: record.payload[position..].to_vec(),
    })
}

fn parse_bounds(record: &Record<'_>) -> Option<E5Bounds> {
    let (representations, mut position) = counted_references(record.payload)?;
    if record.payload.get(position)
        != Some(&(0x80u8.checked_add(u8::try_from(representations.len()).ok()?)?))
    {
        return None;
    }
    position += 1;
    let mut entries = Vec::with_capacity(representations.len());
    for representation in representations {
        let parameter = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        position += 8;
        let code = read_u32(record.payload, &mut position)?;
        if !parameter.is_finite() {
            return None;
        }
        entries.push(E5BoundEntry {
            representation,
            parameter,
            code,
        });
    }
    (position == record.payload.len()).then_some(E5Bounds {
        record_id: record.id,
        entries,
    })
}

fn parse_pcurve(record: &Record<'_>) -> Option<E5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = reference(record.payload, &mut position)?;
    match record.class {
        0x96 => {
            let values = read_f64s(record.payload, &mut position, 6)?;
            if position != record.payload.len() || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(E5Pcurve::Line {
                surface,
                origin: [values[0], values[1]],
                direction: [values[2], values[3]],
                range: [values[4], values[5]],
            })
        }
        0x97 => {
            let center = read_f64s(record.payload, &mut position, 2)?;
            let codes = [
                read_u32(record.payload, &mut position)?,
                read_u32(record.payload, &mut position)?,
            ];
            let values = read_f64s(record.payload, &mut position, 5)?;
            if position != record.payload.len()
                || center.iter().chain(&values).any(|value| !value.is_finite())
                || values[0] <= 0.0
                || values[3] != 1.0
                || values[4] != 0.0
            {
                return None;
            }
            Some(E5Pcurve::Circle {
                surface,
                center: [center[0], center[1]],
                codes,
                radius: values[0],
                range: [values[1], values[2]],
            })
        }
        0xa0 => parse_jet_pcurve(record.payload, position, surface),
        _ => None,
    }
}

fn parse_jet_pcurve(payload: &[u8], mut position: usize, surface: u32) -> Option<E5Pcurve> {
    let degree = read_u32(payload, &mut position)?;
    let zero0 = read_u32(payload, &mut position)?;
    let zero1 = read_u32(payload, &mut position)?;
    let site_count = usize::try_from(read_u32(payload, &mut position)?).ok()?;
    let zero2 = read_u32(payload, &mut position)?;
    let zero3 = read_u32(payload, &mut position)?;
    let zero4 = read_u32(payload, &mut position)?;
    if degree != 5 || site_count == 0 || [zero0, zero1, zero2, zero3, zero4] != [0; 5] {
        return None;
    }
    let mut knots = vec![0.0];
    knots.extend(read_f64s(payload, &mut position, site_count - 1)?);
    let mut multiplicities = Vec::with_capacity(site_count);
    for _ in 0..site_count {
        multiplicities.push(read_u32(payload, &mut position)?);
    }
    if usize::try_from(read_u32(payload, &mut position)?).ok()? != site_count {
        return None;
    }
    let x = read_f64s(payload, &mut position, site_count)?;
    let y = read_f64s(payload, &mut position, site_count)?;
    let dx = read_f64s(payload, &mut position, site_count)?;
    let dy = read_f64s(payload, &mut position, site_count)?;
    if payload.get(position..position + 2) != Some(&1u16.to_le_bytes()) {
        return None;
    }
    position += 2;
    let ddx = read_f64s(payload, &mut position, site_count)?;
    let ddy = read_f64s(payload, &mut position, site_count)?;
    let range_values = read_f64s(payload, &mut position, 2)?;
    let expected_multiplicities: Vec<u32> = if site_count == 1 {
        vec![degree + 1]
    } else {
        std::iter::once(degree + 1)
            .chain(std::iter::repeat_n(3, site_count.saturating_sub(2)))
            .chain(std::iter::once(degree + 1))
            .collect()
    };
    if position != payload.len()
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities != expected_multiplicities
        || multiplicities.iter().sum::<u32>() != degree + 1 + 3 * u32::try_from(site_count).ok()?
        || range_values[0].abs() >= 1e-12
        || (range_values[1] - *knots.last()?).abs() >= 1e-9
        || x.iter()
            .chain(&y)
            .chain(&dx)
            .chain(&dy)
            .chain(&ddx)
            .chain(&ddy)
            .chain(&range_values)
            .any(|value| !value.is_finite())
    {
        return None;
    }
    Some(E5Pcurve::Jet {
        surface,
        degree,
        knots,
        multiplicities,
        points: x.into_iter().zip(y).map(|(u, v)| [u, v]).collect(),
        first_derivatives: dx.into_iter().zip(dy).map(|(u, v)| [u, v]).collect(),
        second_derivatives: ddx.into_iter().zip(ddy).map(|(u, v)| [u, v]).collect(),
        range: [range_values[0], range_values[1]],
    })
}

fn read_f64s(bytes: &[u8], position: &mut usize, count: usize) -> Option<Vec<f64>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let value = f64::from_le_bytes(bytes.get(*position..*position + 8)?.try_into().ok()?);
        *position += 8;
        values.push(value);
    }
    Some(values)
}

fn read_u32(bytes: &[u8], position: &mut usize) -> Option<u32> {
    let value = u32::from_le_bytes(bytes.get(*position..*position + 4)?.try_into().ok()?);
    *position += 4;
    Some(value)
}

fn solve_absolute_orientation(faces: &mut [E5Face]) {
    let mut locations = Vec::new();
    for (face_index, face) in faces.iter().enumerate() {
        for (loop_index, loop_) in face.loops.iter().enumerate() {
            if loop_.edge_uses.len() > 2 {
                locations.push((face_index, loop_index));
            }
        }
    }
    let mut occurrences = HashMap::<u32, Vec<(usize, i8)>>::new();
    for (node, &(face_index, loop_index)) in locations.iter().enumerate() {
        let loop_ = &faces[face_index].loops[loop_index];
        for (&edge, &reversed) in loop_.edge_uses.iter().zip(&loop_.reversed) {
            occurrences
                .entry(edge)
                .or_default()
                .push((node, if reversed { -1 } else { 1 }));
        }
    }
    if occurrences.values().any(|uses| uses.len() != 2) {
        return;
    }
    let mut adjacency = vec![Vec::<(usize, i8)>::new(); locations.len()];
    for uses in occurrences.values() {
        let [(left, left_r), (right, right_r)] = uses.as_slice() else {
            return;
        };
        let relation = -left_r * right_r;
        adjacency[*left].push((*right, relation));
        adjacency[*right].push((*left, relation));
    }
    let mut solved = vec![None; locations.len()];
    for root in 0..locations.len() {
        if solved[root].is_some() {
            continue;
        }
        solved[root] = Some(1i8);
        let mut component = vec![root];
        let mut cursor = 0;
        while cursor < component.len() {
            let node = component[cursor];
            cursor += 1;
            let value = solved[node].expect("queued orientation");
            for &(neighbor, relation) in &adjacency[node] {
                let expected = value * relation;
                match solved[neighbor] {
                    Some(actual) if actual != expected => return,
                    Some(_) => {}
                    None => {
                        solved[neighbor] = Some(expected);
                        component.push(neighbor);
                    }
                }
            }
        }
        let plus_matches = component
            .iter()
            .filter(|&&node| {
                let (face, _) = locations[node];
                i16::from(solved[node].expect("component value")) == faces[face].trailer_sign
            })
            .count();
        let minus_matches = component.len() - plus_matches;
        if plus_matches == minus_matches {
            return;
        }
        if minus_matches > plus_matches {
            for &node in &component {
                solved[node] = solved[node].map(|value| -value);
            }
        }
    }
    for (node, &(face_index, loop_index)) in locations.iter().enumerate() {
        let g = solved[node].expect("solved loop orientation");
        let loop_ = &mut faces[face_index].loops[loop_index];
        loop_.absolute_reversed = Some(
            loop_
                .reversed
                .iter()
                .map(|reversed| g * if *reversed { -1 } else { 1 } < 0)
                .collect(),
        );
    }
}

fn parse_bodies(records: &[Record<'_>], by_id: &HashMap<u32, &Record<'_>>) -> Option<Vec<E5Body>> {
    records
        .iter()
        .filter(|record| record.class == 0x01)
        .map(|record| {
            let (roots, end) = counted_references(record.payload)?;
            if roots.len() != 1 || end != record.payload.len() {
                return None;
            }
            let root = *by_id.get(&roots[0])?;
            if root.class != 0x08 {
                return None;
            }
            let (faces, mut position) = counted_references(root.payload)?;
            if root.payload.get(position)
                != Some(&(0x80u8.checked_add(u8::try_from(faces.len()).ok()?)?))
            {
                return None;
            }
            position += 1;
            let sign_bytes = root.payload.get(position..)?;
            if sign_bytes.len() != (faces.len() + 2) * 2 {
                return None;
            }
            let signs: Vec<i16> = sign_bytes
                .chunks_exact(2)
                .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                .collect();
            if signs.iter().any(|sign| !matches!(sign, -1 | 1))
                || faces
                    .iter()
                    .any(|face| by_id.get(face).is_none_or(|target| target.class != 0x00))
            {
                return None;
            }
            Some(E5Body {
                record_id: record.id,
                root_record_id: root.id,
                faces,
                face_orientation_signs: signs[..signs.len() - 2].to_vec(),
                extra_orientation_signs: [signs[signs.len() - 2], signs[signs.len() - 1]],
            })
        })
        .collect()
}

fn counted_references(payload: &[u8]) -> Option<(Vec<u32>, usize)> {
    let count = usize::from(payload.first()?.checked_sub(0x80)?);
    let mut position = 1;
    let mut references = Vec::with_capacity(count);
    for _ in 0..count {
        references.push(reference(payload, &mut position)?);
    }
    Some((references, position))
}

fn records(bytes: &[u8]) -> Vec<Record<'_>> {
    let mut records = Vec::new();
    let mut position = 0;
    while position + 13 <= bytes.len() {
        let Some(relative) = bytes[position..]
            .windows(3)
            .position(|value| value == [0xe5, 0x0d, 0x03])
        else {
            break;
        };
        let start = position + relative;
        let size = usize::from(u16::from_le_bytes([bytes[start + 5], bytes[start + 6]]));
        let Some(end) = start.checked_add(13 + size) else {
            break;
        };
        if end > bytes.len() {
            position = start + 1;
            continue;
        }
        records.push(Record {
            class: bytes[start + 3],
            id: u32::from_le_bytes(bytes[start + 9..start + 13].try_into().expect("record id")),
            payload: &bytes[start + 13..end],
        });
        position = end;
    }
    records
}

fn parse_face(record: &Record<'_>) -> Option<RawFace> {
    let count = usize::from(record.payload.first()?.checked_sub(0x81)?);
    if count == 0 {
        return None;
    }
    let mut position = 1;
    let surface = reference(record.payload, &mut position)?;
    let mut loops = Vec::with_capacity(count);
    for _ in 0..count {
        loops.push(reference(record.payload, &mut position)?);
    }
    let trailer_sign = i16::from_le_bytes(
        record
            .payload
            .get(position..position + 2)?
            .try_into()
            .ok()?,
    );
    if !matches!(trailer_sign, -1 | 1) || position + 2 != record.payload.len() {
        return None;
    }
    Some(RawFace {
        id: record.id,
        surface,
        loops,
        trailer_sign,
    })
}

fn parse_loop(record: &Record<'_>) -> Option<RawLoop> {
    let member_count = usize::from(record.payload.first()?.checked_sub(0x81)?);
    if member_count == 0 || member_count % 2 != 0 {
        return None;
    }
    let mut position = 1;
    let mut pcurves = Vec::with_capacity(member_count / 2);
    let mut edges = Vec::with_capacity(member_count / 2);
    for _ in 0..member_count / 2 {
        pcurves.push(reference(record.payload, &mut position)?);
        edges.push(reference(record.payload, &mut position)?);
    }
    let surface = reference(record.payload, &mut position)?;
    let outer = parse_loop_role(record.payload.get(position..)?, member_count / 2)?;
    Some(RawLoop {
        id: record.id,
        surface,
        pcurves,
        edges,
        outer,
    })
}

fn parse_loop_role(trailing: &[u8], edge_count: usize) -> Option<Option<bool>> {
    if trailing.is_empty() {
        return Some(None);
    }
    if trailing.first() != Some(&(0x80u8.checked_add(u8::try_from(edge_count).ok()?)?))
        || trailing.len() != 1 + 2 * (3 * edge_count + 4)
    {
        return None;
    }
    let signs: Vec<i16> = trailing[1..]
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    if signs.iter().any(|sign| !matches!(sign, -1 | 1)) {
        return None;
    }
    Some(Some(signs[1] == 1))
}

fn parse_edge(record: &Record<'_>) -> Option<E5Edge> {
    if record.payload.first() != Some(&0x85) {
        return None;
    }
    let mut position = 1;
    let support = reference(record.payload, &mut position)?;
    let start_vertex = reference(record.payload, &mut position)?;
    let end_vertex = reference(record.payload, &mut position)?;
    let parameter_start = reference(record.payload, &mut position)?;
    let parameter_end = reference(record.payload, &mut position)?;
    let terminator = reference(record.payload, &mut position)?;
    Some(E5Edge {
        record_id: record.id,
        support,
        start_vertex,
        end_vertex,
        parameter_start,
        parameter_end,
        terminator,
    })
}

fn reference(bytes: &[u8], position: &mut usize) -> Option<u32> {
    let lead = *bytes.get(*position)?;
    let (value, width) = match lead {
        0x38 => (
            u32::from_le_bytes([
                *bytes.get(*position + 1)?,
                *bytes.get(*position + 2)?,
                *bytes.get(*position + 3)?,
                0,
            ]),
            4,
        ),
        0x18 => (
            u32::from(u16::from_le_bytes([
                *bytes.get(*position + 1)?,
                *bytes.get(*position + 2)?,
            ])),
            3,
        ),
        0x10 => (u32::from(*bytes.get(*position + 1)?) << 8, 2),
        0x08 => (u32::from(*bytes.get(*position + 1)?), 2),
        value if value >= 0x80 => (u32::from(value - 0x80), 1),
        _ => return None,
    };
    *position += width;
    Some(value)
}

fn solve_loop_chain(edge_ids: &[u32], edges: &BTreeMap<u32, E5Edge>) -> Option<Vec<bool>> {
    let first = edges.get(edge_ids.first()?)?;
    let mut solutions = Vec::new();
    for first_reversed in [false, true] {
        let initial = if first_reversed {
            first.end_vertex
        } else {
            first.start_vertex
        };
        let mut current = if first_reversed {
            first.start_vertex
        } else {
            first.end_vertex
        };
        let mut senses = vec![first_reversed];
        let mut valid = true;
        for edge_id in &edge_ids[1..] {
            let edge = edges.get(edge_id)?;
            match (edge.start_vertex == current, edge.end_vertex == current) {
                (true, false) => {
                    senses.push(false);
                    current = edge.end_vertex;
                }
                (false, true) => {
                    senses.push(true);
                    current = edge.start_vertex;
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if valid && current == initial {
            solutions.push(senses);
        }
    }
    (solutions.len() == 1).then(|| solutions.remove(0))
}
