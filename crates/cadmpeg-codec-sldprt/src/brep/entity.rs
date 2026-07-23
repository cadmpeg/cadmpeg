// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity records needed for body membership.

use super::{u16_be, u32_be};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::wire::be::f64_at as f64_be;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct BodyRecord {
    pub attr: u16,
    pub kind: BodyKind,
    pub refs: Vec<u16>,
    pub offset: usize,
    pub regions: Vec<RegionRecord>,
}

#[derive(Debug, Clone)]
pub struct RegionRecord {
    pub attr: u16,
    pub offset: usize,
    pub shells: Vec<ShellRecord>,
}

#[derive(Debug, Clone)]
pub struct ShellRecord {
    pub attr: u16,
    pub offset: usize,
    pub refs: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct FaceColor {
    pub face_attr: u16,
    pub color_attr: u16,
    pub color: Color,
    pub offset: usize,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Number of framed top-level model entity records in the stream.
    pub entity_count: usize,
    pub bodies: Vec<BodyRecord>,
    /// Cluster-key chain bodies ([spec §6]); consulted when `bodies` binds no face.
    pub cluster_bodies: Vec<BodyRecord>,
    pub face_colors: Vec<FaceColor>,
}

#[derive(Debug, Clone)]
struct EntityRecord {
    attr: u16,
    flags: u32,
    seq: u32,
    disc: u16,
    refs: Vec<u16>,
    offset: usize,
}

impl EntityRecord {
    fn flo(&self) -> u8 {
        (self.flags & 0xff) as u8
    }
}

fn slot_count(disc: u16, flo: u8) -> usize {
    match (disc, flo) {
        (0x0018 | 0x0025 | 0x0020, 1) => 6,
        (0x001d | 0x001e, 2) => 7,
        (0x0020 | 0x0027 | 0x0024, 4) => 9,
        _ => 6,
    }
}

fn refs(body: &[u8], at: usize, count: usize) -> Option<Vec<u16>> {
    if body.get(at) == Some(&1) {
        let mut out = Vec::with_capacity(count);
        let mut prefixed = true;
        for index in 0..count {
            let p = at + index * 3;
            if body.get(p) != Some(&1) {
                prefixed = false;
                break;
            }
            out.push(u16_be(body, p + 1)?);
        }
        if prefixed && body.get(at + count * 3) == Some(&0) {
            return Some(out);
        }
    }
    (0..count)
        .map(|index| u16_be(body, at + index * 2))
        .collect()
}

fn scan_entities(body: &[u8]) -> Vec<EntityRecord> {
    let mut out = Vec::new();
    for off in 0..body.len().saturating_sub(25) {
        if body.get(off..off + 2) != Some(&[0x00, 0x51]) {
            continue;
        }
        let mut p = off + 2;
        if body.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(flags) = u32_be(body, p) else {
            continue;
        };
        let Some(attr) = u16_be(body, p + 4) else {
            continue;
        };
        let Some(seq) = u32_be(body, p + 6) else {
            continue;
        };
        let Some(disc) = u16_be(body, p + 10) else {
            continue;
        };
        let flo = (flags & 0xff) as u8;
        if attr <= 1 || seq == 0 || !(1..=0x20).contains(&flo) {
            continue;
        }
        let Some(refs) = refs(body, p + 12, slot_count(disc, flo)) else {
            continue;
        };
        out.push(EntityRecord {
            attr,
            flags,
            seq,
            disc,
            refs,
            offset: off,
        });
    }
    out
}

fn color_record(body: &[u8], off: usize) -> Option<(u16, Color, usize)> {
    if body.get(off..off + 2) != Some(&[0x00, 0x53]) {
        return None;
    }
    let mut p = off + 2;
    if body.get(p) == Some(&0xff) {
        p += 1;
    }
    if u32_be(body, p)? & 0xff != 3 {
        return None;
    }
    let attr = u16_be(body, p + 4)?;
    let [r, g, b] = [
        f64_be(body, p + 6)?,
        f64_be(body, p + 14)?,
        f64_be(body, p + 22)?,
    ];
    if attr <= 1
        || ![r, g, b]
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        return None;
    }
    Some((
        attr,
        Color {
            r: r as f32,
            g: g as f32,
            b: b as f32,
            a: 1.0,
        },
        p + 30,
    ))
}

pub fn scan(body: &[u8]) -> Facts {
    let entities = scan_entities(body);
    let mut colors = HashMap::new();
    for off in 0..body.len().saturating_sub(31) {
        if let Some((attr, color, _end)) = color_record(body, off) {
            colors.insert(attr, (color, off));
        }
    }
    let mut face_colors = HashMap::new();
    for face in &entities {
        if face.disc == 0x0015 || face.disc == 0x001f {
            if let Some(color_attr) = face.refs.get(5).copied() {
                if let Some((color, offset)) = colors.get(&color_attr) {
                    face_colors.insert(
                        face.attr,
                        FaceColor {
                            face_attr: face.attr,
                            color_attr,
                            color: *color,
                            offset: *offset,
                            target: None,
                        },
                    );
                }
            }
        }
        if face.disc == 0x0014 {
            let at =
                face.offset + 2 + usize::from(body.get(face.offset + 2) == Some(&0xff)) + 12 + 12;
            if let Some((color_attr, color, _end)) = color_record(body, at) {
                face_colors.insert(
                    face.attr,
                    FaceColor {
                        face_attr: face.attr,
                        color_attr,
                        color,
                        offset: at,
                        target: None,
                    },
                );
            }
        }
    }
    Facts {
        entity_count: entities.len(),
        bodies: bodies(&entities),
        cluster_bodies: cluster_chain_bodies(&entities),
        face_colors: face_colors.into_values().collect(),
    }
}

/// Decode cluster-key chain bodies ([spec §6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#6-body-records)).
///
/// A body list head is a `flo == 2` record shaped `[key, root, 1, 1, ...]`
/// whose root is a record with `slot0 == key` and `slot2` naming the
/// head back. The root begins a descending chain of records sharing
/// `slot0 == key` linked through `slot1`; each valid chain is one stored body.
/// The entity records between one head and the next, in stream order, form the
/// body's section interval; a body owns the face entities in its interval.
fn cluster_chain_bodies(entities: &[EntityRecord]) -> Vec<BodyRecord> {
    let mut by_attr: HashMap<u16, &EntityRecord> = HashMap::new();
    for record in entities {
        if by_attr
            .get(&record.attr)
            .is_none_or(|current| record.seq >= current.seq)
        {
            by_attr.insert(record.attr, record);
        }
    }
    let mut heads = Vec::new();
    for head in by_attr.values() {
        if head.flo() != 2 || head.refs.len() < 3 || head.refs[2..].iter().any(|slot| *slot != 1) {
            continue;
        }
        let (key, root_attr) = (head.refs[0], head.refs[1]);
        if key <= 1 || root_attr <= 1 {
            continue;
        }
        let Some(root) = by_attr.get(&root_attr).copied() else {
            continue;
        };
        if root.refs.first() != Some(&key) || root.refs.get(2) != Some(&head.attr) {
            continue;
        }
        // Walk the descending chain; a body needs the root plus one member.
        let mut chain = vec![root.attr];
        let mut cursor = root;
        loop {
            let next = cursor.refs.get(1).copied().unwrap_or(1);
            if next <= 1 {
                break;
            }
            let Some(node) = by_attr.get(&next).copied() else {
                break;
            };
            if node.refs.first() != Some(&key) || chain.contains(&node.attr) {
                break;
            }
            chain.push(node.attr);
            cursor = node;
        }
        if chain.len() >= 2 {
            heads.push((head.offset, key, root, chain));
        }
    }
    heads.sort_by_key(|(offset, ..)| *offset);
    let mut out = Vec::new();
    for (index, (offset, _key, root, chain)) in heads.iter().enumerate() {
        let start = if index == 0 { 0 } else { *offset };
        let end = heads
            .get(index + 1)
            .map_or(usize::MAX, |(next_offset, ..)| *next_offset);
        let mut refs: Vec<u16> = entities
            .iter()
            .filter(|record| (start..end).contains(&record.offset))
            .map(|record| record.attr)
            .chain(chain.iter().copied())
            .collect();
        refs.sort_unstable();
        refs.dedup();
        out.push(BodyRecord {
            attr: root.attr,
            kind: BodyKind::Solid,
            refs: refs.clone(),
            offset: root.offset,
            regions: vec![RegionRecord {
                attr: root.attr,
                offset: root.offset,
                shells: vec![ShellRecord {
                    attr: root.attr,
                    offset: root.offset,
                    refs,
                }],
            }],
        });
    }
    out.sort_by_key(|record| record.attr);
    out
}

/// Decode explicit `MANIFOLD_SOLID_BREP` entity-51 records.
fn bodies(entities: &[EntityRecord]) -> Vec<BodyRecord> {
    let mut by_attr = HashMap::new();
    for record in entities {
        if by_attr
            .get(&record.attr)
            .is_none_or(|current: &&EntityRecord| record.seq >= current.seq)
        {
            by_attr.insert(record.attr, record);
        }
    }
    let mut out = Vec::new();
    for root in by_attr.values().filter(|record| {
        (record.flags == 2 || record.flags & 0xff00_0000 == 0xff00_0000) && record.disc == 0x0017
    }) {
        let solid_regions = body_regions(&by_attr, root, 0x001b, None);
        let sheet_regions = body_regions(&by_attr, root, 0x001d, Some(1));
        let mut refs = HashSet::new();
        let mut pending: Vec<u16> = root
            .refs
            .iter()
            .copied()
            .filter(|reference| *reference > 1)
            .collect();
        while let Some(reference) = pending.pop() {
            if !refs.insert(reference) {
                continue;
            }
            if let Some(record) = by_attr.get(&reference) {
                pending.extend(
                    record
                        .refs
                        .iter()
                        .copied()
                        .filter(|reference| *reference > 1),
                );
            }
        }
        let mut refs = refs.into_iter().collect::<Vec<_>>();
        refs.sort_unstable();
        let regions = solid_regions
            .iter()
            .chain(&sheet_regions)
            .map(|region| {
                let mut shells = linked_all(&by_attr, region, 0x001f)
                    .into_iter()
                    .flat_map(|lump| linked_all(&by_attr, lump, 0x0021))
                    .map(|shell_link| {
                        linked_all(&by_attr, shell_link, 0x0023)
                            .into_iter()
                            .next()
                            .unwrap_or(shell_link)
                    })
                    .map(|shell| ShellRecord {
                        attr: shell.attr,
                        offset: shell.offset,
                        refs: reachable_refs(&by_attr, shell),
                    })
                    .collect::<Vec<_>>();
                if shells.is_empty() {
                    shells.push(ShellRecord {
                        attr: region.attr,
                        offset: region.offset,
                        refs: reachable_refs(&by_attr, region),
                    });
                }
                RegionRecord {
                    attr: region.attr,
                    offset: region.offset,
                    shells,
                }
            })
            .collect();
        out.push(BodyRecord {
            attr: root.attr,
            kind: if solid_regions.is_empty() && !sheet_regions.is_empty() {
                BodyKind::Sheet
            } else {
                BodyKind::Solid
            },
            refs,
            offset: root.offset,
            regions,
        });
    }
    bind_schema_32001_faces(entities, &mut out);
    bind_schema_33103_faces(entities, &mut out);
    if out.is_empty() {
        out.extend(disc14_bodies(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_bodies(&by_attr));
    }
    if out.is_empty() {
        out.extend(schema_36001_extended_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(compact_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(sparse_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(direct_shell_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(shifted_disc16_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(shifted_disc18_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(compact_disc16_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(compact_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc04_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(compact_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc22_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc22_disc18_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc14_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc10_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(direct_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_compact_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_compact_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_direct_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_compact_disc04_face_root_body(&by_attr));
    }
    out.sort_by_key(|record| record.attr);
    out
}

fn disc1c_compact_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001c && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(region, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_12, 0x000e, 2) else {
        return Vec::new();
    };
    if disc_0e.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0004, 1);
    if faces == 0 || faces != count(0x0018, 1) || faces != count(0x001e, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1e_direct_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1c) = follows(region, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_14, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 1) else {
        return Vec::new();
    };
    if disc_0e.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0004, 1);
    if faces == 0 || faces != count(0x0018, 1) || faces != count(0x0020, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc20_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(region, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_14, 0x0010, 2) else {
        return Vec::new();
    };
    if follows(disc_10, 0x0004, 1).is_none() {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0012, 1);
    if faces == 0 || faces != count(0x001e, 1) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc20_compact_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1e) = follows(region, 0x001e, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_1e, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(disc_16) = follows(shell, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_16, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 1) else {
        return Vec::new();
    };
    if disc_0e.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0004, 1);
    if faces == 0 || faces != count(0x001a, 1) || faces != count(0x0022, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1e_compact_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(region, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 1) else {
        return Vec::new();
    };
    if disc_0e.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0004, 1);
    if faces == 0 || faces != count(0x001c, 1) || faces != count(0x0020, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn direct_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(shell) = follows(region, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_14, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 2) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x0004, 1).is_none() {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0012, 1);
    if faces == 0 || faces != count(0x0018, 1) || faces + 2 != count(0x001c, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1e_disc10_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1c) = follows(region, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(disc_1a) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_14, 0x000e, 2) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x0004, 1).is_none() {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0010, 1);
    if faces == 0 || faces != count(0x0018, 1) || faces != count(0x0020, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1e_disc14_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(region, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_12, 0x000e, 2) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x0004, 1).is_none() {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0014, 1);
    if faces == 0 || faces != count(0x001c, 1) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc22_disc18_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0022 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_20) = follows(region, 0x0020, 2) else {
        return Vec::new();
    };
    let Some(disc_1a) = follows(disc_20, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(shell, 0x0010, 2) else {
        return Vec::new();
    };
    if disc_10.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0018, 1);
    if faces == 0 || faces != count(0x001e, 1) || faces != count(0x0024, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc22_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0022 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_20) = follows(region, 0x0020, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_20, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    if disc_14.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0012, 1);
    if faces == 0 || faces != count(0x001e, 1) || faces + 1 != count(0x0024, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn compact_disc0e_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1e) = follows(region, 0x001e, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_1e, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_14, 0x0010, 2) else {
        return Vec::new();
    };
    if disc_10.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x001a, 1) || faces != count(0x0022, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc04_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0004 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    let linked = |record: &EntityRecord, slot: usize, disc: u16, flo: u8| {
        record
            .refs
            .get(slot)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(shell) = linked(region, 1, 0x0010, 2) else {
        return Vec::new();
    };
    if shell.refs.get(2) != Some(&region.attr) {
        return Vec::new();
    }
    let Some(disc_12) = linked(shell, 1, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = linked(disc_12, 1, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_18) = linked(disc_14, 1, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(disc_1a) = linked(disc_18, 1, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = linked(disc_1a, 1, 0x001c, 2) else {
        return Vec::new();
    };
    if disc_1c.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0016, 1) || faces != count(0x001e, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1e_disc0e_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(region, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(mut disc_16) = follows(shell, 0x0016, 2) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let disc_14 = loop {
        if !seen.insert(disc_16.attr) {
            return Vec::new();
        }
        let Some(next_attr) = disc_16.refs.get(2) else {
            return Vec::new();
        };
        let Some(next) = by_attr.get(next_attr).copied() else {
            return Vec::new();
        };
        if next.disc == 0x0014 && next.flo() == 2 {
            break next;
        }
        if next.disc != 0x0016 || next.flo() != 2 {
            return Vec::new();
        }
        disc_16 = next;
    };
    let Some(disc_10) = follows(disc_14, 0x0010, 2) else {
        return Vec::new();
    };
    if disc_10.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0012, 1) || faces != count(0x001c, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn compact_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1e) = follows(region, 0x001e, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_1e, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(shell, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 2) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x0004, 1).is_none() {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0012, 1);
    if faces == 0 || faces != count(0x001a, 1) || faces != count(0x0022, 4) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn compact_disc16_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(shell) = follows(region, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(shell, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 2) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x0004, 1).is_none() {
        return Vec::new();
    }
    let disc16_faces = by_attr
        .values()
        .filter(|record| record.disc == 0x0016 && record.flo() == 1)
        .count();
    let disc18_uses = by_attr
        .values()
        .filter(|record| record.disc == 0x0018 && record.flo() == 1)
        .count();
    if disc16_faces == 0 || disc16_faces != disc18_uses {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1c) = follows(region, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(shell, 0x0018, 1) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(disc_18, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    if follows(disc_12, 0x000e, 2).is_none()
        || !by_attr
            .values()
            .any(|record| record.disc == 0x0004 && record.flo() == 1)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1e_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1c) = follows(region, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_16) = follows(shell, 0x0016, 1) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(disc_16, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    if follows(disc_10, 0x000e, 2).is_none()
        || !by_attr
            .values()
            .any(|record| record.disc == 0x0004 && record.flo() == 1)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_18) = follows(region, 0x0018) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(shell, 0x0010) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x0004).is_none()
        || !by_attr
            .values()
            .any(|record| record.disc == 0x0012 && record.flo() == 1)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1e_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_1a) = follows(region, 0x001a) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1a, 0x0018) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012) else {
        return Vec::new();
    };
    let Some(mut disc_10) = follows(disc_12, 0x0010) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(disc_10.attr) {
            return Vec::new();
        }
        let Some(next_attr) = disc_10.refs.get(2).copied() else {
            return Vec::new();
        };
        if next_attr <= 1 {
            break;
        }
        let Some(next) = by_attr.get(&next_attr).copied() else {
            return Vec::new();
        };
        if next.disc != 0x0010 || next.flo() != 2 {
            return Vec::new();
        }
        disc_10 = next;
    }
    if !by_attr
        .values()
        .any(|record| record.disc == 0x000e && record.flo() == 1)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn shifted_disc18_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_1c) = follows(region, 0x001c) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x001a) else {
        return Vec::new();
    };
    let Some(disc_16) = follows(shell, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(disc_16, 0x0014) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_14, 0x000e) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x0004).is_none()
        || !by_attr
            .values()
            .any(|record| record.disc == 0x0018 && record.flo() == 1)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn shifted_disc16_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001c && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_1a) = follows(region, 0x001a) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0018) else {
        return Vec::new();
    };
    let lower_complete = if let Some(disc_12) = follows(shell, 0x0012) {
        follows(disc_12, 0x0010).is_some_and(|disc_10| follows(disc_10, 0x000e).is_some())
    } else if let Some(disc_14) = follows(shell, 0x0014) {
        follows(disc_14, 0x0010).is_some_and(|disc_10| {
            follows(disc_10, 0x000e).is_some_and(|disc_0e| follows(disc_0e, 0x0004).is_some())
        })
    } else {
        false
    };
    if !lower_complete
        || !by_attr
            .values()
            .any(|record| record.disc == 0x0016 && record.flo() == 1)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc20_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_1e) = follows(root, 0x001e) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_1e, 0x001c) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1c, 0x0018) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010) else {
        return Vec::new();
    };
    if follows(disc_10, 0x000e).is_none()
        || !by_attr
            .values()
            .any(|record| record.disc == 0x0022 && record.flo() == 4)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: root.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: root.offset,
        regions: vec![RegionRecord {
            attr: root.attr,
            offset: root.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn direct_shell_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(shell) = follows(region, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e) else {
        return Vec::new();
    };
    if follows(disc_0e, 0x000c).is_none() || !by_attr.values().any(|record| record.disc == 0x0014) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc1c_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001c && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_18) = follows(root, 0x0018) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012) else {
        return Vec::new();
    };
    if follows(disc_12, 0x0010).is_none() || !by_attr.values().any(|record| record.disc == 0x000e) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: root.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: root.offset,
        regions: vec![RegionRecord {
            attr: root.attr,
            offset: root.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn sparse_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    let follows = |record: &EntityRecord, disc: u16| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_18) = follows(region, 0x0018) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010) else {
        return Vec::new();
    };
    if follows(disc_10, 0x000e).is_none() || !by_attr.values().any(|record| record.disc == 0x0014) {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn compact_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    let follows = |record: &EntityRecord, slot: usize, disc: u16| {
        record
            .refs
            .get(slot)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    if region.refs.get(1).is_some_and(|attr| *attr > 1) {
        let Some(disc_1c) = follows(region, 1, 0x001c) else {
            return Vec::new();
        };
        if disc_1c.refs.get(1).is_some_and(|attr| *attr > 1)
            && follows(disc_1c, 1, 0x001e).is_none()
        {
            return Vec::new();
        }
    }
    let Some(disc_18) = follows(region, 2, 0x0018) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 2, 0x0014) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 2, 0x0012) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 2, 0x0010) else {
        return Vec::new();
    };
    if follows(disc_10, 2, 0x000e).is_none()
        && !(disc_10.refs.get(2).is_none_or(|attr| *attr <= 1)
            && by_attr.values().any(|record| record.disc == 0x000e))
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn schema_36001_extended_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a && record.flo() == 1)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    let follows = |record: &EntityRecord, slot: usize, disc: u16| {
        record
            .refs
            .get(slot)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_20) = follows(region, 1, 0x0020) else {
        return Vec::new();
    };
    let Some(disc_28) = follows(disc_20, 1, 0x0028) else {
        return Vec::new();
    };
    let Some(disc_2a) = follows(disc_28, 1, 0x002a) else {
        return Vec::new();
    };
    if follows(disc_2a, 1, 0x002c).is_none() {
        return Vec::new();
    }
    let Some(disc_18) = follows(region, 2, 0x0018) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 2, 0x0016) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 2, 0x0014) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_14, 2, 0x0010) else {
        return Vec::new();
    };
    if follows(disc_10, 2, 0x000e).is_none() {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: region.offset,
        regions: vec![RegionRecord {
            attr: region.attr,
            offset: region.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn disc20_bodies(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a)
        .collect::<Vec<_>>();
    let faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020 && record.flo() == 1)
        .collect::<Vec<_>>();
    if regions.len() != 1 || faces.is_empty() {
        return Vec::new();
    }
    let shells = reachable_records(by_attr, regions[0], 0x0016);
    let [shell] = shells.as_slice() else {
        return Vec::new();
    };
    let per_face_lattice = faces.iter().all(|face| {
        face.refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .filter(|node| node.disc == 0x0024 && node.flo() == 4)
            .filter(|node| node.refs.get(2) == Some(&face.attr))
            .and_then(|node| node.refs.get(1).and_then(|attr| by_attr.get(attr)))
            .is_some_and(|use_record| {
                use_record.disc == 0x0026
                    && use_record.flo() == 3
                    && use_record.refs.get(2) == face.refs.get(1)
            })
    });
    let schema_36001_lattice = schema_36001_single_region_lattice(by_attr, regions[0]);
    if !per_face_lattice && !schema_36001_lattice {
        return Vec::new();
    }
    let mut refs = faces.iter().map(|face| face.attr).collect::<Vec<_>>();
    if schema_36001_lattice {
        refs.extend(by_attr.keys().copied());
    }
    refs.sort_unstable();
    refs.dedup();
    vec![BodyRecord {
        attr: regions[0].attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: regions[0].offset,
        regions: vec![RegionRecord {
            attr: regions[0].attr,
            offset: regions[0].offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}

fn schema_36001_single_region_lattice(
    by_attr: &HashMap<u16, &EntityRecord>,
    region: &EntityRecord,
) -> bool {
    let follows = |record: &EntityRecord, slot: usize, disc: u16| {
        record
            .refs
            .get(slot)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc)
    };
    let Some(disc_18) = follows(region, 2, 0x0018) else {
        return false;
    };
    let Some(disc_16) = follows(disc_18, 2, 0x0016) else {
        return false;
    };
    if follows(disc_16, 2, 0x0014).is_none() {
        return false;
    }
    let Some(disc_1c) = follows(region, 1, 0x001c) else {
        return false;
    };
    let Some(disc_22) = follows(disc_1c, 1, 0x0022) else {
        return false;
    };
    let Some(disc_24) = follows(disc_22, 1, 0x0024) else {
        return false;
    };
    let Some(disc_26) = follows(disc_24, 1, 0x0026) else {
        return false;
    };
    follows(disc_26, 1, 0x002e).is_some()
}

fn bind_schema_32001_faces(entities: &[EntityRecord], bodies: &mut [BodyRecord]) {
    let mut primary_heads = entities
        .iter()
        .filter(|record| record.disc == 0x0015 && record.flo() == 2)
        .collect::<Vec<_>>();
    let secondary_heads = entities
        .iter()
        .filter(|record| record.disc == 0x000f && record.flo() == 1)
        .map(|record| (record.attr, record))
        .collect::<HashMap<_, _>>();
    let faces = entities
        .iter()
        .filter(|record| record.disc == 0x001f && record.flo() == 1)
        .collect::<Vec<_>>();
    if primary_heads.is_empty() || faces.is_empty() || bodies.is_empty() {
        return;
    }
    primary_heads.sort_by_key(|record| record.offset);
    let mut all_heads = primary_heads.clone();
    all_heads.extend(secondary_heads.values().copied());
    all_heads.sort_by_key(|record| record.offset);

    let mut interval_faces = HashMap::<u16, Vec<u16>>::new();
    for (index, head) in all_heads.iter().enumerate() {
        let end = all_heads
            .get(index + 1)
            .map_or(usize::MAX, |record| record.offset);
        interval_faces.insert(
            head.attr,
            faces
                .iter()
                .filter(|face| face.offset >= head.offset && face.offset < end)
                .map(|face| face.attr)
                .collect(),
        );
    }

    let primary_by_attr = primary_heads
        .into_iter()
        .map(|record| (record.attr, record))
        .collect::<HashMap<_, _>>();
    let roots = entities
        .iter()
        .filter(|record| record.disc == 0x0017 && record.flo() == 2)
        .map(|record| (record.attr, record))
        .collect::<HashMap<_, _>>();
    if roots.len() != bodies.len() {
        return;
    }
    let faces_by_attr = faces
        .iter()
        .map(|face| (face.attr, *face))
        .collect::<HashMap<_, _>>();

    let mut assignments = HashMap::<u16, Vec<u16>>::new();
    let mut assigned_faces = HashSet::new();
    for body in bodies.iter() {
        let Some(root) = roots.get(&body.attr) else {
            return;
        };
        let Some(head) = root.refs.get(2).and_then(|attr| primary_by_attr.get(attr)) else {
            return;
        };
        if head.refs.get(1) != Some(&body.attr) {
            return;
        }
        let active_head = head
            .refs
            .get(2)
            .and_then(|attr| secondary_heads.get(attr))
            .copied()
            .unwrap_or(head);
        let Some(face_attrs) = interval_faces.get(&active_head.attr) else {
            return;
        };
        if face_attrs
            .iter()
            .any(|face_attr| !assigned_faces.insert(*face_attr))
        {
            return;
        }
        let mut membership = face_attrs.clone();
        membership.extend(face_attrs.iter().filter_map(|face_attr| {
            faces_by_attr
                .get(face_attr)
                .and_then(|face| face.refs.first())
                .copied()
                .filter(|reference| *reference > 1)
        }));
        assignments.insert(body.attr, membership);
    }
    if assigned_faces.len() != faces.len() {
        return;
    }

    for body in bodies {
        let face_attrs = &assignments[&body.attr];
        body.refs.extend(face_attrs.iter().copied());
        body.refs.sort_unstable();
        body.refs.dedup();
        for shell in body
            .regions
            .iter_mut()
            .flat_map(|region| &mut region.shells)
        {
            shell.refs.extend(face_attrs.iter().copied());
            shell.refs.sort_unstable();
            shell.refs.dedup();
        }
    }
}

fn disc14_bodies(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001a)
        .collect::<Vec<_>>();
    if regions.is_empty() {
        return Vec::new();
    }

    let canonical_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0014)
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    let face_use_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0020)
        .filter_map(|face_use| face_from_face_use(by_attr, face_use))
        .collect::<HashSet<_>>();
    if regions.len() == 1 {
        let shells = reachable_records(by_attr, regions[0], 0x0016);
        if let [shell] = shells.as_slice() {
            if !canonical_faces.is_empty() && face_use_faces == canonical_faces {
                let mut refs = canonical_faces.into_iter().collect::<Vec<_>>();
                refs.sort_unstable();
                return vec![BodyRecord {
                    attr: regions[0].attr,
                    kind: BodyKind::Solid,
                    refs: refs.clone(),
                    offset: regions[0].offset,
                    regions: vec![RegionRecord {
                        attr: regions[0].attr,
                        offset: regions[0].offset,
                        shells: vec![ShellRecord {
                            attr: shell.attr,
                            offset: shell.offset,
                            refs,
                        }],
                    }],
                }];
            }
        }
    }

    let mut region_records = Vec::new();
    let mut body_refs = HashSet::new();
    for region in regions {
        let shells = reachable_records(by_attr, region, 0x0016)
            .into_iter()
            .filter_map(|shell| {
                let face_attrs = shell_face_ring(by_attr, shell)?;
                body_refs.extend(face_attrs.iter().copied());
                Some(ShellRecord {
                    attr: shell.attr,
                    offset: shell.offset,
                    refs: face_attrs,
                })
            })
            .collect::<Vec<_>>();
        if !shells.is_empty() {
            region_records.push(RegionRecord {
                attr: region.attr,
                offset: region.offset,
                shells,
            });
        }
    }
    if region_records.is_empty() {
        return Vec::new();
    }
    let mut refs = body_refs.into_iter().collect::<Vec<_>>();
    refs.sort_unstable();
    vec![BodyRecord {
        attr: region_records[0].attr,
        kind: BodyKind::Solid,
        refs,
        offset: region_records[0].offset,
        regions: region_records,
    }]
}

fn reachable_records<'a>(
    by_attr: &HashMap<u16, &'a EntityRecord>,
    root: &'a EntityRecord,
    disc: u16,
) -> Vec<&'a EntityRecord> {
    let mut seen = HashSet::new();
    let mut pending = root.refs.clone();
    let mut found = Vec::new();
    while let Some(attr) = pending.pop() {
        if attr <= 1 || !seen.insert(attr) {
            continue;
        }
        let Some(record) = by_attr.get(&attr).copied() else {
            continue;
        };
        if record.disc == disc {
            found.push(record);
        } else {
            pending.extend(record.refs.iter().copied());
        }
    }
    found.sort_by_key(|record| record.offset);
    found
}

fn shell_face_ring(
    by_attr: &HashMap<u16, &EntityRecord>,
    shell: &EntityRecord,
) -> Option<Vec<u16>> {
    let first = reachable_records(by_attr, shell, 0x0020)
        .into_iter()
        .next()?;
    let mut current = first.attr;
    let mut seen = HashSet::new();
    let mut faces = Vec::new();
    while seen.insert(current) {
        let face_use = by_attr.get(&current)?;
        if face_use.disc != 0x0020 {
            return None;
        }
        faces.push(face_from_face_use(by_attr, face_use)?);
        let next = *face_use.refs.get(3)?;
        if next == first.attr {
            break;
        }
        current = next;
    }
    (!faces.is_empty()).then_some(faces)
}

fn face_from_face_use(
    by_attr: &HashMap<u16, &EntityRecord>,
    face_use: &EntityRecord,
) -> Option<u16> {
    let mut current = *by_attr.get(face_use.refs.get(2)?)?;
    for _ in 0..3 {
        match current.disc {
            0x0014 => return Some(current.attr),
            0x0018 | 0x001e => current = *by_attr.get(current.refs.get(2)?)?,
            _ => return None,
        }
    }
    None
}

fn bind_schema_33103_faces(entities: &[EntityRecord], bodies: &mut [BodyRecord]) {
    let faces = entities
        .iter()
        .filter(|record| record.disc == 0x0015 && record.flo() == 1)
        .collect::<Vec<_>>();
    let face_attrs = faces
        .iter()
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    if face_attrs.is_empty() {
        return;
    }

    let by_attr = faces
        .iter()
        .map(|record| (record.attr, *record))
        .collect::<HashMap<_, _>>();
    let mut unseen = face_attrs.clone();
    let mut components = Vec::new();
    while let Some(start) = unseen.iter().next().copied() {
        let mut component = HashSet::new();
        let mut pending = vec![start];
        while let Some(attr) = pending.pop() {
            if !unseen.remove(&attr) {
                continue;
            }
            component.insert(attr);
            if let Some(face) = by_attr.get(&attr) {
                pending.extend(
                    face.refs
                        .iter()
                        .copied()
                        .filter(|reference| face_attrs.contains(reference)),
                );
            }
        }
        components.push(component);
    }

    let mut heads = entities
        .iter()
        .filter(|record| record.disc == 0x0013 && record.flo() == 2)
        .collect::<Vec<_>>();
    heads.sort_by_key(|record| record.offset);
    let mut assigned = HashSet::new();
    for (index, head) in heads.iter().enumerate() {
        let Some(cluster) = head.refs.first() else {
            continue;
        };
        if *cluster <= 1 {
            continue;
        }
        let Some(body) = bodies.iter_mut().find(|body| {
            entities
                .iter()
                .any(|record| record.attr == body.attr && record.refs.first() == Some(cluster))
        }) else {
            continue;
        };
        let interval_end = heads.get(index + 1).map_or(usize::MAX, |next| next.offset);
        let Some((component_index, component)) = components
            .iter()
            .enumerate()
            .filter(|(component_index, _)| !assigned.contains(component_index))
            .max_by_key(|(_, component)| {
                component
                    .iter()
                    .filter_map(|attr| by_attr.get(attr))
                    .filter(|face| face.offset >= head.offset && face.offset < interval_end)
                    .count()
            })
        else {
            continue;
        };
        let overlap = component
            .iter()
            .filter_map(|attr| by_attr.get(attr))
            .filter(|face| face.offset >= head.offset && face.offset < interval_end)
            .count();
        if overlap == 0 {
            continue;
        }
        assigned.insert(component_index);
        body.refs.extend(component.iter().copied());
        body.refs.sort_unstable();
        body.refs.dedup();
        for shell in body
            .regions
            .iter_mut()
            .flat_map(|region| &mut region.shells)
        {
            shell.refs.extend(component.iter().copied());
            shell.refs.sort_unstable();
            shell.refs.dedup();
        }
    }
}

fn body_regions<'a>(
    by_attr: &HashMap<u16, &'a EntityRecord>,
    body: &'a EntityRecord,
    disc: u16,
    flo: Option<u8>,
) -> Vec<&'a EntityRecord> {
    let matches = |record: &&EntityRecord| {
        record.disc == disc && flo.is_none_or(|expected| record.flo() == expected)
    };
    let mut regions = body
        .refs
        .iter()
        .filter_map(|reference| by_attr.get(reference))
        .copied()
        .filter(matches)
        .collect::<Vec<_>>();
    for connector in linked_all(by_attr, body, 0x0019) {
        regions.extend(
            connector
                .refs
                .iter()
                .filter_map(|reference| by_attr.get(reference))
                .copied()
                .filter(matches),
        );
    }
    regions.sort_by_key(|record| record.attr);
    regions.dedup_by_key(|record| record.attr);
    regions
}

fn linked_all<'a>(
    by_attr: &HashMap<u16, &'a EntityRecord>,
    record: &'a EntityRecord,
    disc: u16,
) -> Vec<&'a EntityRecord> {
    record
        .refs
        .iter()
        .filter_map(|reference| by_attr.get(reference))
        .copied()
        .filter(|target| target.disc == disc)
        .collect()
}

fn reachable_refs(by_attr: &HashMap<u16, &EntityRecord>, root: &EntityRecord) -> Vec<u16> {
    let mut refs = HashSet::new();
    let mut pending = root.refs.clone();
    while let Some(reference) = pending.pop() {
        if reference <= 1 || !refs.insert(reference) {
            continue;
        }
        if let Some(record) = by_attr.get(&reference) {
            pending.extend(record.refs.iter().copied());
        }
    }
    let mut refs = refs.into_iter().collect::<Vec<_>>();
    refs.sort_unstable();
    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(attr: u16, disc: u16, refs: [u16; 6]) -> EntityRecord {
        EntityRecord {
            attr,
            flags: 1,
            seq: u32::from(attr),
            disc,
            refs: refs.to_vec(),
            offset: usize::from(attr),
        }
    }

    #[test]
    fn schema_36001_root_lattice_owns_all_disc20_faces() {
        let records = vec![
            record(10, 0x1a, [1, 11, 12, 1, 1, 1]),
            record(11, 0x1c, [1, 15, 10, 1, 1, 1]),
            record(12, 0x18, [1, 10, 13, 1, 1, 1]),
            record(13, 0x16, [1, 12, 14, 1, 1, 1]),
            record(14, 0x14, [1, 13, 1, 1, 1, 1]),
            record(15, 0x22, [1, 16, 11, 1, 1, 1]),
            record(16, 0x24, [1, 17, 15, 1, 1, 1]),
            record(17, 0x26, [1, 18, 16, 1, 1, 1]),
            record(18, 0x2e, [1, 1, 17, 1, 1, 1]),
            record(20, 0x20, [1; 6]),
            record(21, 0x20, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc20_bodies(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one schema-36001 body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.kind, BodyKind::Solid);
        assert_eq!(body.regions.len(), 1);
        assert_eq!(body.regions[0].shells.len(), 1);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(by_attr.keys().all(|attr| body.refs.contains(attr)));
    }

    #[test]
    fn schema_36001_extended_root_lattice_owns_the_site() {
        let records = vec![
            record(10, 0x1a, [1, 11, 12, 1, 1, 1]),
            record(11, 0x20, [1, 13, 10, 1, 1, 1]),
            record(12, 0x18, [1, 10, 16, 1, 1, 1]),
            record(13, 0x28, [1, 14, 11, 1, 1, 1]),
            record(14, 0x2a, [1, 15, 13, 1, 1, 1]),
            record(15, 0x2c, [1, 1, 14, 1, 1, 1]),
            record(16, 0x16, [1, 12, 17, 1, 1, 1]),
            record(17, 0x14, [1, 16, 18, 1, 1, 1]),
            record(18, 0x10, [1, 17, 19, 1, 1, 1]),
            record(19, 0x0e, [1, 18, 1, 1, 1, 1]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = schema_36001_extended_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one schema-36001 body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.kind, BodyKind::Solid);
        assert_eq!(body.regions.len(), 1);
        assert_eq!(body.regions[0].shells.len(), 1);
        assert_eq!(body.regions[0].shells[0].attr, 16);
        assert!(by_attr.keys().all(|attr| body.refs.contains(attr)));

        let incomplete = records
            .iter()
            .filter(|record| record.attr != 19)
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert!(schema_36001_extended_root_body(&incomplete).is_empty());
    }

    #[test]
    fn compact_root_lattice_owns_the_site_with_or_without_companion_branch() {
        let records = vec![
            record(10, 0x1a, [1, 11, 12, 1, 1, 1]),
            record(11, 0x1c, [1, 13, 10, 1, 1, 1]),
            record(12, 0x18, [1, 10, 14, 1, 1, 1]),
            record(13, 0x1e, [1, 1, 11, 1, 1, 1]),
            record(14, 0x14, [1, 12, 15, 1, 1, 1]),
            record(15, 0x12, [1, 14, 16, 1, 1, 1]),
            record(16, 0x10, [1, 15, 17, 1, 1, 1]),
            record(17, 0x0e, [1, 16, 1, 1, 1, 1]),
        ]
        .into_iter()
        .map(|mut record| {
            if record.attr == 10 {
                record.flags = 2;
            }
            record
        })
        .collect::<Vec<_>>();
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = compact_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one schema-36001 body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.kind, BodyKind::Solid);
        assert_eq!(body.regions[0].shells[0].attr, 14);
        assert!(by_attr.keys().all(|attr| body.refs.contains(attr)));

        let incomplete = records
            .iter()
            .filter(|record| record.attr != 13)
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert!(compact_root_body(&incomplete).is_empty());

        let without_companion = records
            .iter()
            .filter(|record| record.attr != 13)
            .cloned()
            .map(|mut record| {
                if record.attr == 11 {
                    record.refs[1] = 1;
                }
                record
            })
            .collect::<Vec<_>>();
        let without_companion = without_companion
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert_eq!(compact_root_body(&without_companion).len(), 1);

        let sentinel_upper_and_lower = [
            flo2(30, 0x1a, [3, 1, 31, 1, 1, 1]),
            flo2(31, 0x18, [3, 30, 32, 1, 1, 1]),
            flo2(32, 0x14, [3, 31, 33, 1, 1, 1]),
            flo2(33, 0x12, [3, 32, 34, 1, 1, 1]),
            flo2(34, 0x10, [3, 33, 1, 1, 1, 1]),
            record(40, 0x0e, [1; 6]),
        ];
        let sentinel_upper_and_lower = sentinel_upper_and_lower
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert_eq!(compact_root_body(&sentinel_upper_and_lower).len(), 1);
    }

    #[test]
    fn sparse_root_lattice_owns_the_disc14_site() {
        let records = [
            flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x12, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            record(15, 0x0e, [3, 14, 1, 1, 1, 1]),
            record(20, 0x14, [1; 6]),
            record(21, 0x14, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = sparse_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one sparse-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1c_root_lattice_owns_the_disc0e_site() {
        let records = [
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            record(15, 0x10, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1c_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1c-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn direct_shell_root_lattice_owns_the_disc14_site() {
        let records = [
            flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x16, [3, 10, 12, 1, 1, 1]),
            record(12, 0x12, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x10, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x0e, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0c, [3, 14, 1, 1, 1, 1]),
            record(20, 0x14, [1; 6]),
            record(21, 0x14, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = direct_shell_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one direct-shell-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 11);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc20_root_lattice_owns_the_disc22_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            record(13, 0x18, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x16, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x14, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x12, [3, 15, 17, 1, 1, 1]),
            flo2(17, 0x10, [3, 16, 18, 1, 1, 1]),
            record(18, 0x0e, [3, 17, 1, 1, 1, 1]),
            flo4(20, 0x22, [1; 6]),
            flo4(21, 0x22, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc20_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc20-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 14);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn shifted_disc16_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x12, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            record(15, 0x0e, [3, 14, 1, 1, 1, 1]),
            record(20, 0x16, [1; 6]),
            record(21, 0x16, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = shifted_disc16_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one shifted-disc16-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn shifted_disc16_root_accepts_the_disc14_lower_branch() {
        let records = vec![
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x16, [1; 6]),
            record(21, 0x16, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = shifted_disc16_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one shifted-disc16-root body");
        };
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn shifted_disc18_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x18, [1; 6]),
            record(21, 0x18, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = shifted_disc18_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one shifted-disc18-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_root_lattice_owns_the_disc0e_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 16, 1]),
            flo2(16, 0x10, [3, 15, 1, 15, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc12_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x10, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x0e, [3, 13, 15, 1, 1, 1]),
            record(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x12, [1; 6]),
            record(21, 0x12, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc12_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            record(13, 0x18, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x0e, [3, 15, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc04_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc04_face_root_accepts_the_disc1e_prefix() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            record(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x10, [3, 15, 17, 1, 1, 1]),
            flo2(17, 0x0e, [3, 16, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc04_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn compact_disc16_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x14, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x10, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x0e, [3, 12, 14, 1, 1, 1]),
            record(14, 0x04, [3, 13, 1, 1, 1, 1]),
            record(20, 0x16, [1; 6]),
            record(21, 0x16, [1; 6]),
            record(30, 0x18, [1; 6]),
            record(31, 0x18, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = compact_disc16_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one compact-disc16-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 11);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn compact_disc12_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x12, [1; 6]),
            record(21, 0x12, [1; 6]),
            record(30, 0x1a, [1; 6]),
            record(31, 0x1a, [1; 6]),
            flo4(40, 0x22, [1; 6]),
            flo4(41, 0x22, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = compact_disc12_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one compact-disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_disc0e_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 14, 1]),
            flo2(14, 0x16, [3, 13, 15, 13, 1, 1]),
            flo2(15, 0x14, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x10, [3, 15, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x12, [1; 6]),
            record(31, 0x12, [1; 6]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc0e_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc0e-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc04_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x04, [3, 16, 1, 1, 1, 1]),
            flo2(11, 0x1c, [3, 1, 1, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 1, 1, 1, 1]),
            flo2(13, 0x18, [3, 12, 1, 1, 1, 1]),
            record(14, 0x14, [3, 13, 1, 1, 1, 1]),
            flo2(15, 0x12, [3, 14, 1, 1, 1, 1]),
            flo2(16, 0x10, [3, 15, 10, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x16, [1; 6]),
            record(31, 0x16, [1; 6]),
            flo4(40, 0x1e, [1; 6]),
            flo4(41, 0x1e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc04_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc04-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 16);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn compact_disc0e_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x1a, [1; 6]),
            record(31, 0x1a, [1; 6]),
            flo4(40, 0x22, [1; 6]),
            flo4(41, 0x22, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = compact_disc0e_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one compact-disc0e-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc22_disc12_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x1a, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 1, 1, 1, 1]),
            record(20, 0x12, [1; 6]),
            record(21, 0x12, [1; 6]),
            record(30, 0x1e, [1; 6]),
            record(31, 0x1e, [1; 6]),
            flo4(40, 0x24, [1; 6]),
            flo4(41, 0x24, [1; 6]),
            flo4(42, 0x24, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc22_disc12_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc22-disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc22_disc18_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x22, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x20, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 1, 1, 1, 1]),
            record(20, 0x18, [1; 6]),
            record(21, 0x18, [1; 6]),
            record(30, 0x1e, [1; 6]),
            record(31, 0x1e, [1; 6]),
            flo4(40, 0x24, [1; 6]),
            flo4(41, 0x24, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc22_disc18_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc22-disc18-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_disc14_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x14, [1; 6]),
            record(21, 0x14, [1; 6]),
            record(30, 0x1c, [1; 6]),
            record(31, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc14_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc14-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_disc10_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x10, [1; 6]),
            record(21, 0x10, [1; 6]),
            record(30, 0x18, [1; 6]),
            record(31, 0x18, [1; 6]),
            flo4(40, 0x20, [1; 6]),
            flo4(41, 0x20, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc10_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc10-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn direct_disc12_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x16, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x14, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x10, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x0e, [3, 13, 15, 1, 1, 1]),
            record(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x12, [1; 6]),
            record(21, 0x12, [1; 6]),
            record(30, 0x18, [1; 6]),
            record(31, 0x18, [1; 6]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
            flo4(42, 0x1c, [1; 6]),
            flo4(43, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = direct_disc12_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one direct-disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 11);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_compact_disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            record(16, 0x0e, [3, 15, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
            record(30, 0x1c, [1; 6]),
            record(31, 0x1c, [1; 6]),
            flo4(40, 0x20, [1; 6]),
            flo4(41, 0x20, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_compact_disc04_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-compact-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc20_compact_disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x18, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x16, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            record(16, 0x0e, [3, 15, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
            record(30, 0x1a, [1; 6]),
            record(31, 0x1a, [1; 6]),
            flo4(40, 0x22, [1; 6]),
            flo4(41, 0x22, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc20_compact_disc04_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc20-compact-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc20_disc12_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x12, [1; 6]),
            record(21, 0x12, [1; 6]),
            record(30, 0x1e, [1; 6]),
            record(31, 0x1e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc20_disc12_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc20-disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_direct_disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            record(15, 0x0e, [3, 14, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
            record(30, 0x18, [1; 6]),
            record(31, 0x18, [1; 6]),
            flo4(40, 0x20, [1; 6]),
            flo4(41, 0x20, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_direct_disc04_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-direct-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1c_compact_disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0e, [3, 14, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
            record(30, 0x18, [1; 6]),
            record(31, 0x18, [1; 6]),
            flo4(40, 0x1e, [1; 6]),
            flo4(41, 0x1e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1c_compact_disc04_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1c-compact-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    fn flo2(attr: u16, disc: u16, refs: [u16; 6]) -> EntityRecord {
        let mut out = record(attr, disc, refs);
        out.flags = 2;
        out
    }

    fn flo4(attr: u16, disc: u16, refs: [u16; 6]) -> EntityRecord {
        let mut out = record(attr, disc, refs);
        out.flags = 4;
        out
    }

    #[test]
    fn cluster_key_chain_heads_partition_bodies() {
        let records = vec![
            flo2(5, 0x04, [3, 32, 1, 1, 1, 1]),
            flo2(32, 0x0f, [3, 36, 5, 1, 1, 1]),
            flo2(36, 0x11, [3, 1, 32, 1, 1, 1]),
            record(57, 0x0d, [56, 59, 1, 60, 1, 61]),
            flo2(100, 0x04, [7, 132, 1, 1, 1, 1]),
            record(120, 0x0d, [119, 122, 1, 57, 1, 124]),
            flo2(132, 0x0f, [7, 136, 100, 1, 1, 1]),
            flo2(136, 0x11, [7, 1, 132, 1, 1, 1]),
        ];

        let bodies = cluster_chain_bodies(&records);
        let [first, second] = bodies.as_slice() else {
            panic!("two chain bodies, got {bodies:?}");
        };
        assert_eq!(first.attr, 32);
        assert_eq!(second.attr, 132);
        assert!(first.refs.contains(&57) && !first.refs.contains(&120));
        assert!(second.refs.contains(&120) && !second.refs.contains(&57));
        assert_eq!(first.regions[0].shells[0].attr, 32);
        assert_eq!(second.regions[0].shells[0].attr, 132);

        // A head without a mutual root produces no chain body.
        let broken = vec![
            flo2(5, 0x04, [3, 32, 1, 1, 1, 1]),
            flo2(32, 0x0f, [4, 36, 5, 1, 1, 1]),
        ];
        assert!(cluster_chain_bodies(&broken).is_empty());
    }
}
