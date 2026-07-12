// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity records needed for body membership.

use super::{u16_be, u32_be};
use cadmpeg_ir::be::f64_at as f64_be;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Color;
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
    pub bodies: Vec<BodyRecord>,
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
        bodies: bodies(&entities),
        face_colors: face_colors.into_values().collect(),
    }
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
    bind_schema_33103_faces(entities, &mut out);
    if out.is_empty() {
        out.extend(disc14_bodies(&by_attr));
    }
    out.sort_by_key(|record| record.attr);
    out
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
    let first = shell
        .refs
        .iter()
        .filter_map(|reference| by_attr.get(reference))
        .find(|record| record.disc == 0x0020)?;
    let mut current = first.attr;
    let mut seen = HashSet::new();
    let mut faces = Vec::new();
    while seen.insert(current) {
        let face_use = by_attr.get(&current)?;
        if face_use.disc != 0x0020 {
            return None;
        }
        let geometry = by_attr.get(face_use.refs.get(2)?)?;
        if geometry.disc != 0x0018 {
            return None;
        }
        let face = by_attr.get(geometry.refs.get(2)?)?;
        if face.disc != 0x0014 {
            return None;
        }
        faces.push(face.attr);
        let next = *face_use.refs.get(3)?;
        if next == first.attr {
            break;
        }
        current = next;
    }
    (!faces.is_empty()).then_some(faces)
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
