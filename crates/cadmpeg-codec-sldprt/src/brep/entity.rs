// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity records needed for body membership.

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Color;
use std::collections::{HashMap, HashSet};

use crate::layout::class_root_directory_prefix as class_root;
use crate::layout::entity_common_header as entity_hdr;

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
    pub face_seq: u32,
    pub stream_order: usize,
    pub color: Color,
    pub offset: usize,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct FaceColorVersion {
    pub face_attr: u16,
    pub seq: u32,
    pub stream_order: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Number of framed top-level model entity records in the stream.
    pub entity_count: usize,
    pub bodies: Vec<BodyRecord>,
    /// Cluster-key bodies selected by the stream's class-root index.
    pub class_root_bodies: Vec<BodyRecord>,
    /// Cluster-key chain bodies ([spec §6]); consulted when `bodies` binds no face.
    pub cluster_bodies: Vec<BodyRecord>,
    /// Schema-33103 body heads whose maximum face-component overlap was tied.
    pub ambiguous_body_assignments: usize,
    /// Face-color links whose current framed records conflict.
    pub unresolved_face_colors: usize,
    /// Version of every face record that can carry a linked color.
    pub face_color_versions: Vec<FaceColorVersion>,
    pub face_colors: Vec<FaceColor>,
    /// Per-face producing-feature identities carried by Parasolid attributes.
    pub face_atoms: Vec<super::attrib::FaceAtom>,
    /// Body-to-history ordinals carried by Parasolid attributes.
    pub body_modifiers: Vec<super::attrib::BodyModifier>,
}

#[derive(Debug, Clone)]
struct EntityRecord {
    attr: u16,
    flags: u32,
    seq: u32,
    disc: u16,
    refs: Vec<u16>,
    offset: usize,
    end: usize,
}

const CLASS_ROOT_INDEX_PREFIX: &[u8] = b"CI\x10index_map_offset\0\0\0\x01\x01dCCZ\0\0\0\x14";

impl EntityRecord {
    fn flo(&self) -> u8 {
        (self.flags & 0xff) as u8
    }
}

fn slot_count(schema: &str, disc: u16, flo: u8) -> Option<usize> {
    let revision = schema.split('_').nth(2)?.parse::<u32>().ok()?;
    if !matches!(
        revision,
        17_106
            | 18_106
            | 19_008
            | 20_000
            | 25_001
            | 26_105
            | 28_002
            | 28_101
            | 30_000
            | 31_001
            | 31_100
            | 32_001
            | 33_103
            | 34_101
            | 35_102
            | 36_001
    ) {
        return None;
    }
    if !matches!(
        disc,
        0x0004
            | 0x000c
            | 0x000e
            | 0x000f
            | 0x0010
            | 0x0011
            | 0x0012
            | 0x0013
            | 0x0014
            | 0x0015
            | 0x0016
            | 0x0017
            | 0x0018
            | 0x0019
            | 0x001a
            | 0x001b
            | 0x001c
            | 0x001d
            | 0x001e
            | 0x001f
            | 0x0020
            | 0x0021
            | 0x0022
            | 0x0023
            | 0x0024
            | 0x0025
            | 0x0026
            | 0x0027
            | 0x0028
            | 0x002a
            | 0x002c
            | 0x002e
    ) {
        return None;
    }
    match (disc, flo) {
        (0x0026, 3) => Some(6),
        (_, 1) => Some(6),
        (_, 2) => Some(7),
        (_, 4) => Some(9),
        _ => None,
    }
}

fn refs(body: &[u8], at: usize, count: usize, prefixed: bool) -> Option<(Vec<u16>, usize)> {
    if prefixed {
        if body.get(at) != Some(&1) {
            return None;
        }
        let mut out = Vec::new();
        let mut p = at;
        while body.get(p) == Some(&1) {
            out.push(View::u16_be_at(body, p.checked_add(1)?)?);
            p = p.checked_add(3)?;
        }
        if !out.is_empty() && body.get(p) == Some(&0) {
            return Some((out, p.checked_add(1)?));
        }
    }
    let refs = (0..count)
        .map(|index| View::u16_be_at(body, at + index * 2))
        .collect::<Option<Vec<_>>>()?;
    Some((refs, at.checked_add(count.checked_mul(2)?)?))
}

fn scan_entities(body: &[u8], schema: &str, prefixed: bool) -> Vec<EntityRecord> {
    let mut out = Vec::new();
    for off in 0..body.len().saturating_sub(25) {
        if body.get(off..off + 2) != Some(&[0x00, 0x51]) {
            continue;
        }
        let mut p = off + 2;
        if body.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(flags) = View::u32_be_at(body, p + entity_hdr::FLAGS) else {
            continue;
        };
        let Some(attr) = View::u16_be_at(body, p + entity_hdr::ATTR) else {
            continue;
        };
        let Some(seq) = View::u32_be_at(body, p + entity_hdr::SEQ) else {
            continue;
        };
        let Some(disc) = View::u16_be_at(body, p + entity_hdr::DISC) else {
            continue;
        };
        let flo = (flags & 0xff) as u8;
        if attr <= 1 || seq == 0 || !(1..=0x20).contains(&flo) {
            continue;
        }
        let count = if prefixed {
            0
        } else {
            let Some(count) = slot_count(schema, disc, flo) else {
                continue;
            };
            count
        };
        let Some((refs, end)) = refs(body, p + entity_hdr::LEN, count, prefixed) else {
            continue;
        };
        out.push(EntityRecord {
            attr,
            flags,
            seq,
            disc,
            refs,
            offset: off,
            end,
        });
    }
    out
}

fn class_root_attrs_at(body: &[u8], offset: usize) -> Option<Vec<u16>> {
    let token_at = offset.checked_add(class_root::CLASS_TOKEN)?;
    let token = View::u16_be_at(body, token_at)?;
    let count = View::u32_be_at(body, offset.checked_add(class_root::ROOT_COUNT)?)?;
    let preamble_at = offset.checked_add(class_root::ROOTS_PREAMBLE)?;
    let roots_at = offset.checked_add(class_root::LEN)?;
    if token <= 1 || body.get(preamble_at..roots_at) != Some(&[0, 0, 0, 0, 0, 1]) {
        return None;
    }
    let remaining = body.len().saturating_sub(roots_at);
    let count = cadmpeg_core::decode::bounded_len(u64::from(count), 2, remaining)?;
    if count == 0 {
        return None;
    }
    let mut roots = Vec::with_capacity(count);
    let mut distinct = HashSet::new();
    for index in 0..count {
        let attr = View::u16_be_at(body, roots_at.checked_add(index.checked_mul(2)?)?)?;
        if attr <= 1 || !distinct.insert(attr) {
            return None;
        }
        roots.push(attr);
    }
    Some(roots)
}

fn class_root_attrs(body: &[u8], entity_attrs: &HashSet<u16>) -> Option<HashSet<u16>> {
    let mut candidates = body
        .windows(CLASS_ROOT_INDEX_PREFIX.len())
        .enumerate()
        .filter(|(_, window)| *window == CLASS_ROOT_INDEX_PREFIX)
        .filter_map(|(offset, _)| class_root_attrs_at(body, offset))
        .filter(|roots| roots.iter().all(|attr| entity_attrs.contains(attr)))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    let [roots] = candidates.as_slice() else {
        return None;
    };
    Some(roots.iter().copied().collect())
}

fn color_record(body: &[u8], off: usize) -> Option<(u16, Color, usize)> {
    if body.get(off..off + 2) != Some(&[0x00, 0x53]) {
        return None;
    }
    let mut p = off + 2;
    if body.get(p) == Some(&0xff) {
        p += 1;
    }
    if View::u32_be_at(body, p)? & 0xff != 3 {
        return None;
    }
    let attr = View::u16_be_at(body, p + 4)?;
    let [r, g, b] = [
        View::f64_be_at(body, p + 6)?,
        View::f64_be_at(body, p + 14)?,
        View::f64_be_at(body, p + 22)?,
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

#[derive(Clone, Copy)]
struct FramedColor {
    color: Color,
    offset: usize,
    parent_seq: u32,
}

fn linked_colors(body: &[u8], entities: &[EntityRecord]) -> HashMap<(u16, u16), Vec<FramedColor>> {
    let mut colors = HashMap::<(u16, u16), Vec<FramedColor>>::new();
    for parent in entities {
        let mut linked_faces = parent.refs.iter().copied().collect::<HashSet<_>>();
        linked_faces.insert(parent.attr);
        let mut at = parent.end;
        while let Some((color_attr, color, end)) = color_record(body, at) {
            let framed = FramedColor {
                color,
                offset: at,
                parent_seq: parent.seq,
            };
            for face_attr in linked_faces.iter().copied().filter(|attr| *attr > 1) {
                colors
                    .entry((face_attr, color_attr))
                    .or_default()
                    .push(framed);
            }
            at = end;
        }
    }
    colors
}

fn current_linked_color(candidates: &[FramedColor]) -> Option<FramedColor> {
    let current_seq = candidates
        .iter()
        .map(|candidate| candidate.parent_seq)
        .max()?;
    let mut current = candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.parent_seq == current_seq);
    let first = current.next()?;
    current
        .all(|candidate| candidate.color == first.color)
        .then_some(first)
}

pub fn scan(body: &[u8], schema: &str) -> Facts {
    scan_with_framing(body, schema, false)
}

pub fn scan_deltas(body: &[u8], schema: &str) -> Facts {
    scan_with_framing(body, schema, true)
}

fn scan_with_framing(body: &[u8], schema: &str, prefixed: bool) -> Facts {
    let entities = scan_entities(body, schema, prefixed);
    let entity_attrs = entities.iter().map(|record| record.attr).collect();
    let class_roots = class_root_attrs(body, &entity_attrs);
    let linked_colors = linked_colors(body, &entities);
    let mut face_colors = Vec::new();
    let mut face_color_versions = Vec::new();
    let mut unresolved_face_colors = 0;
    for face in &entities {
        let framed = match face.disc {
            0x0014 => {
                face_color_versions.push(FaceColorVersion {
                    face_attr: face.attr,
                    seq: face.seq,
                    stream_order: 0,
                });
                color_record(body, face.end).map(|(color_attr, color, _end)| {
                    (
                        color_attr,
                        FramedColor {
                            color,
                            offset: face.end,
                            parent_seq: face.seq,
                        },
                    )
                })
            }
            0x0015 | 0x001f => {
                face_color_versions.push(FaceColorVersion {
                    face_attr: face.attr,
                    seq: face.seq,
                    stream_order: 0,
                });
                let Some(color_attr) = face.refs.get(5).copied().filter(|attr| *attr > 1) else {
                    continue;
                };
                match linked_colors.get(&(face.attr, color_attr)) {
                    Some(candidates) => match current_linked_color(candidates) {
                        Some(color) => Some((color_attr, color)),
                        None => {
                            unresolved_face_colors += 1;
                            None
                        }
                    },
                    None => None,
                }
            }
            _ => continue,
        };
        let Some((color_attr, framed)) = framed else {
            continue;
        };
        face_colors.push(FaceColor {
            face_attr: face.attr,
            color_attr,
            face_seq: face.seq,
            stream_order: 0,
            color: framed.color,
            offset: framed.offset,
            target: None,
        });
    }
    let (bodies, ambiguous_body_assignments) = bodies(&entities);
    Facts {
        entity_count: entities.len(),
        bodies,
        class_root_bodies: class_roots.as_ref().map_or_else(Vec::new, |roots| {
            cluster_chain_bodies(&entities, Some(roots))
        }),
        cluster_bodies: cluster_chain_bodies(&entities, None),
        ambiguous_body_assignments,
        unresolved_face_colors,
        face_color_versions,
        face_colors,
        face_atoms: super::attrib::scan(body),
        body_modifiers: super::attrib::scan_body_modifiers(body),
    }
}

/// Reconstruct the bridge selector carried by explicit deltas body relations.
///
/// Deltas entity records are ordered after partition records. Equal-sequence
/// records therefore select the deltas framing while references can still
/// resolve to unchanged partition records.
pub fn scan_final_bridge_selector(streams: &[(&[u8], &str, bool)]) -> Option<HashSet<u16>> {
    let (entities, has_deltas_body_root) = scan_stream_entities(streams);
    has_deltas_body_root.then(|| {
        bodies(&entities)
            .0
            .into_iter()
            .flat_map(|body| body.refs)
            .collect()
    })
}

pub(crate) fn scan_combined_bodies(streams: &[(&[u8], &str, bool)]) -> (Vec<BodyRecord>, usize) {
    let (entities, _) = scan_stream_entities(streams);
    bodies(&entities)
}

fn scan_stream_entities(streams: &[(&[u8], &str, bool)]) -> (Vec<EntityRecord>, bool) {
    let mut entities = Vec::new();
    let mut has_deltas_body_root = false;
    for (body, schema, is_deltas) in streams {
        let scanned = scan_entities(body, schema, *is_deltas);
        has_deltas_body_root |= *is_deltas && scanned.iter().any(is_explicit_body_root);
        entities.extend(scanned);
    }
    (entities, has_deltas_body_root)
}

/// Decode cluster-key chain bodies ([spec §6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#6-body-records)).
///
/// A body list head is a `flo == 2` record shaped `[key, root, 1, 1, ...]`
/// whose root is a record with `slot0 == key` and `slot2` naming the
/// head back. The root begins a descending chain of records sharing
/// `slot0 == key` linked through `slot1`; each valid chain is one stored body.
/// The entity records between one head and the next, in stream order, form the
/// body's section interval; a body owns the face entities in its interval.
fn cluster_chain_bodies(
    entities: &[EntityRecord],
    selected_heads: Option<&HashSet<u16>>,
) -> Vec<BodyRecord> {
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
            heads.push((head.offset, head.attr, key, root, chain));
        }
    }
    heads.sort_by_key(|(offset, ..)| *offset);
    let mut out = Vec::new();
    for (index, (offset, head_attr, _key, root, chain)) in heads.iter().enumerate() {
        let start = if index == 0 { 0 } else { *offset };
        let end = heads
            .get(index + 1)
            .map_or(usize::MAX, |(next_offset, ..)| *next_offset);
        if selected_heads.is_some_and(|roots| !roots.contains(head_attr)) {
            continue;
        }
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

fn is_explicit_body_root(record: &EntityRecord) -> bool {
    (record.flags == 2 || record.flags & 0xff00_0000 == 0xff00_0000) && record.disc == 0x0017
}

/// Decode explicit `MANIFOLD_SOLID_BREP` entity-51 records.
fn bodies(entities: &[EntityRecord]) -> (Vec<BodyRecord>, usize) {
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
    for root in by_attr
        .values()
        .copied()
        .filter(|record| is_explicit_body_root(record))
    {
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
    let ambiguous_body_assignments = bind_schema_33103_faces(entities, &mut out);
    if out.is_empty() {
        out.extend(keyed_disc14_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(keyed_disc1a_disc18_disc14_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1c_disc1a_disc16_disc14_disc12_face_root_body(
            &by_attr,
        ));
    }
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
        out.extend(disc1c_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc16_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc16_disc14_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc16_disc14_linked_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc14_disc12_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc1c_disc14_disc0e_linked_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc12_terminal_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc0c_terminal_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc1c_disc16_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc16_disc0e_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc16_disc12_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc16_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1c_disc1a_disc18_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1a_disc16_disc14_disc12_disc0e_disc04_face_root_body(
            &by_attr,
        ));
    }
    if out.is_empty() {
        out.extend(disc1a_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1a_disc14_disc0c_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1a_disc12_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1a_disc14_disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1a_disc18_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc16_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc18_disc14_disc12_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(direct_shell_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc1a_disc04_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc20_disc1a_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(
            disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc04_face_root_body(&by_attr),
        );
    }
    if out.is_empty() {
        out.extend(disc20_disc1c_disc1a_disc16_disc12_disc10_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc12_disc1e_disc1c_disc18_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc18_disc12_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc20_disc18_disc14_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc20_disc1a_disc18_disc16_disc14_disc04_face_root_body(
            &by_attr,
        ));
    }
    if out.is_empty() {
        out.extend(disc20_disc1a_disc18_disc16_disc14_disc12_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(
            disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc0e_face_root_body(&by_attr),
        );
    }
    if out.is_empty() {
        out.extend(
            disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc04_face_root_body(&by_attr),
        );
    }
    if out.is_empty() {
        out.extend(disc20_disc1e_disc1c_disc18_disc16_disc04_face_root_body(
            &by_attr,
        ));
    }
    if out.is_empty() {
        out.extend(disc20_disc1e_disc1c_disc18_disc16_disc12_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc1e_disc1c_disc16_disc14_disc10_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(
            disc20_disc1e_disc1c_disc16_disc14_disc12_disc10_disc04_face_root_body(&by_attr),
        );
    }
    if out.is_empty() {
        out.extend(disc20_disc1e_disc1c_disc18_disc16_disc10_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc1e_disc1c_disc16_disc14_disc12_face_root_body(
            &by_attr,
        ));
    }
    if out.is_empty() {
        out.extend(disc20_disc1e_disc1c_disc14_disc12_disc10_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(
            disc22_disc20_disc1e_disc1a_disc18_disc12_disc10_disc04_face_root_body(&by_attr),
        );
    }
    if out.is_empty() {
        out.extend(shifted_disc16_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(shifted_disc18_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc18_disc04_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc18_disc0e_disc04_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc1e_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc12_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc12_direct_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc12_disc0e_flo1_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc04_terminal_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc14_terminal_face_root_body(&by_attr));
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
        out.extend(disc1e_direct_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_direct_disc0e_auxiliary_face_root_body(
            &by_attr, entities,
        ));
    }
    if out.is_empty() {
        out.extend(disc1e_disc12_flo1_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1c_disc14_face_root_body(&by_attr, entities));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1c_disc14_linked_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1c_disc16_disc14_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc16_disc1c_disc1a_disc14_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc10_disc1c_disc1a_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc20_disc16_disc26_disc1e_disc14_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc26_disc1e_disc24_disc22_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1c_disc14_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1a_disc18_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1a_disc18_disc14_disc12_disc04_face_root_body(
            &by_attr,
        ));
    }
    if out.is_empty() {
        out.extend(disc1e_disc18_disc16_disc14_disc04_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc1c_disc16_disc14_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc12_terminal_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc1e_disc12_disc0e_face_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc04_root_body(&by_attr));
    }
    if out.is_empty() {
        out.extend(disc04_disc12_flo1_face_root_body(&by_attr));
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
        out.extend(disc22_disc04_face_root_body(&by_attr));
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
    (out, ambiguous_body_assignments)
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

fn disc22_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0022 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let key = root.refs.first().copied().filter(|key| *key > 1);
    let Some(key) = key else {
        return Vec::new();
    };
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        let next = record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)?;
        (next.refs.first() == Some(&key) && next.refs.get(1) == Some(&record.attr)).then_some(next)
    };
    let Some(disc_1e) = follows(root, 0x001e, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_1e, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1c, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }

    let canonical_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0004 && record.flo() == 1)
        .collect::<Vec<_>>();
    if canonical_faces.is_empty() {
        return Vec::new();
    }
    let mut companions = HashSet::new();
    let mut use_nodes = HashSet::new();
    for face in canonical_faces {
        let Some(companion) = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == 0x001a && record.flo() == 1 && record.refs.get(2) == Some(&face.attr)
            })
        else {
            return Vec::new();
        };
        let Some(use_node) = companion
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == 0x0020
                    && record.flo() == 4
                    && record.refs.get(2) == Some(&companion.attr)
            })
        else {
            return Vec::new();
        };
        if !companions.insert(companion.attr) || !use_nodes.insert(use_node.attr) {
            return Vec::new();
        }
    }
    if companions.len() != use_nodes.len()
        || companions.len()
            != by_attr
                .values()
                .filter(|record| record.disc == 0x001a && record.flo() == 1)
                .count()
        || use_nodes.len()
            != by_attr
                .values()
                .filter(|record| record.disc == 0x0020 && record.flo() == 4)
                .count()
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

fn disc04_disc12_flo1_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let regions = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0004 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [region] = regions.as_slice() else {
        return Vec::new();
    };
    let Some(key) = region.refs.first().copied().filter(|key| *key > 1) else {
        return Vec::new();
    };
    if region.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
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
    if shell.refs.first() != Some(&key) || shell.refs.get(2) != Some(&region.attr) {
        return Vec::new();
    }
    let Some(disc_12) = linked(shell, 1, 0x0012, 1) else {
        return Vec::new();
    };
    if disc_12.refs.first() != Some(&key) || disc_12.refs.get(2) != Some(&shell.attr) {
        return Vec::new();
    }
    let Some(disc_1a) = linked(disc_12, 1, 0x001a, 2) else {
        return Vec::new();
    };
    if disc_1a.refs.first() != Some(&key) || disc_1a.refs.get(2) != Some(&disc_12.attr) {
        return Vec::new();
    }
    let Some(disc_1c) = linked(disc_1a, 1, 0x001c, 2) else {
        return Vec::new();
    };
    if disc_1c.refs.first() != Some(&key) || disc_1c.refs.get(2) != Some(&disc_1a.attr) {
        return Vec::new();
    }
    let Some(terminal) = linked(disc_1c, 1, 0x001e, 2) else {
        return Vec::new();
    };
    if terminal.refs.first() != Some(&key)
        || terminal.refs.get(1).is_some_and(|attr| *attr > 1)
        || terminal.refs.get(2) != Some(&disc_1c.attr)
    {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
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

fn disc1e_direct_disc0e_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let Some(disc_14) = follows(shell, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0016, 1) || faces != count(0x001c, 4) {
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

fn disc1e_direct_disc0e_auxiliary_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
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
    let Some(disc_14) = follows(shell, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    let face_uses = count(0x0016, 1);
    if faces == 0 || faces != face_uses || count(0x001c, 4) < faces {
        return Vec::new();
    }
    let face_use_attrs = entities
        .iter()
        .filter(|record| record.disc == 0x0016 && record.flo() == 1)
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    let use_node_attrs = entities
        .iter()
        .filter(|record| record.disc == 0x001c && record.flo() == 4)
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    if entities
        .iter()
        .filter(|record| record.disc == 0x000e && record.flo() == 1)
        .any(|face| {
            face.refs
                .get(1)
                .is_none_or(|attr| !face_use_attrs.contains(attr))
        })
        || entities
            .iter()
            .filter(|record| record.disc == 0x0016 && record.flo() == 1)
            .any(|face_use| {
                face_use
                    .refs
                    .get(1)
                    .is_none_or(|attr| !use_node_attrs.contains(attr))
            })
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

fn disc1e_disc12_flo1_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
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
    let Some(disc_1c) = follows(root, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 1) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0018, 1) || faces != count(0x0020, 4) {
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

fn disc1e_disc1c_disc14_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
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
    let Some(disc_1c) = follows(root, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_12, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0010, 1) || faces != count(0x0018, 1) {
        return Vec::new();
    }
    if faces != count(0x0020, 4) {
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

fn disc1e_disc1c_disc14_linked_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    let matching_chains = keyed_forward_chain_candidates(by_attr)
        .into_iter()
        .filter(|chain| {
            chain.len() == 7
                && matches!(
                    chain.as_slice(),
                    [root, disc_1c, shell, disc_14, disc_12, disc_10, terminal]
                        if root.disc == 0x001e
                            && root.flo() == 2
                            && disc_1c.disc == 0x001c
                            && disc_1c.flo() == 2
                            && shell.disc == 0x001a
                            && shell.flo() == 2
                            && disc_14.disc == 0x0014
                            && disc_14.flo() == 1
                            && disc_12.disc == 0x0012
                            && disc_12.flo() == 2
                            && disc_10.disc == 0x0010
                            && disc_10.flo() == 2
                            && terminal.disc == 0x0004
                            && terminal.flo() == 2
                )
        })
        .collect::<Vec<_>>();
    let [chain] = matching_chains.as_slice() else {
        return Vec::new();
    };
    let canonical_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x000e && record.flo() == 1)
        .collect::<Vec<_>>();
    if canonical_faces.is_empty() {
        return Vec::new();
    }
    let direct_face_use = |face: &EntityRecord| {
        let companion = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        if companion.disc != 0x0018
            || companion.flo() != 1
            || companion.refs.get(2) != Some(&face.attr)
        {
            return None;
        }
        Some(companion)
    };
    let intermediate_face_use = |face: &EntityRecord| {
        let intermediate = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        if intermediate.disc != 0x0016
            || intermediate.flo() != 2
            || intermediate.refs.get(2) != Some(&face.attr)
        {
            return None;
        }
        let companion = intermediate
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        (companion.disc == 0x0018
            && companion.flo() == 1
            && companion.refs.get(2) == Some(&intermediate.attr))
        .then_some(companion)
    };
    let mut companions = HashSet::new();
    let mut use_nodes = HashSet::new();
    for face in canonical_faces {
        let companion = direct_face_use(face).or_else(|| intermediate_face_use(face));
        let Some(companion) = companion else {
            return Vec::new();
        };
        let Some(use_node) = companion
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == 0x0020
                    && record.flo() == 4
                    && record.refs.get(2) == Some(&companion.attr)
            })
        else {
            return Vec::new();
        };
        if !companions.insert(companion.attr) || !use_nodes.insert(use_node.attr) {
            return Vec::new();
        }
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    let root = chain[0];
    let shell = chain[2];
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

fn disc1e_disc12_terminal_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let Some(disc_14) = follows(shell, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_12, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0016, 1) || faces != count(0x001c, 4) {
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

fn disc1e_disc12_disc0e_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let Some(disc_14) = follows(shell, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x000e, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0004, 1);
    if faces == 0 || faces != count(0x0016, 1) || faces != count(0x001c, 4) {
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

fn disc1e_disc04_terminal_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
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
    let Some(disc_1c) = follows(root, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(disc_1a) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0016, 1) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let terminal = follows(disc_14, 0x0004, 2).or_else(|| {
        let disc_12 = follows(disc_14, 0x0012, 2)?;
        let disc_10 = follows(disc_12, 0x0010, 2)?;
        follows(disc_10, 0x0004, 2)
    });
    let Some(terminal) = terminal else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0018, 1) || faces != count(0x0020, 4) {
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

fn disc1e_disc14_terminal_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x001e && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
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
    let Some(disc_1a) = follows(root, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 1) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_14, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0
        || faces != count(0x0010, 1)
        || faces != count(0x001c, 1)
        || faces != count(0x0020, 4)
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

fn disc12_direct_disc0e_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let Some(disc_18) = follows(region, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012, 1) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0014, 1) || faces != count(0x001c, 4) {
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

fn disc12_disc0e_flo1_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let Some(disc_18) = follows(region, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 1) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0014, 1) || faces != count(0x001c, 4) {
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

fn disc18_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0018 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
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
    let Some(disc_14) = follows(root, 0x0014, 2) else {
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
    let Some(terminal) = follows(disc_0e, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        // Population counts describe framed records in this site. Do not let
        // a later overlapping record hide a canonical face from the lattice.
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000c, 1);
    if faces == 0 || faces != count(0x0016, 1) || faces != count(0x001a, 4) {
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
                attr: disc_14.attr,
                offset: disc_14.offset,
                refs,
            }],
        }],
    }]
}

fn disc18_disc0e_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    let roots = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x0018 && record.flo() == 2)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Vec::new();
    };
    if root.refs.get(1).is_some_and(|attr| *attr > 1) {
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
    let Some(shell) = follows(root, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_14, 0x0010, 1) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_0e, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        // Population counts describe framed records in this site. Do not let
        // a later overlapping record hide a canonical face from the lattice.
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000c, 1);
    if faces == 0 || faces != count(0x0012, 1) || faces != count(0x001a, 4) {
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
    let (shell, direct_shell) = if let Some(disc_18) = follows(disc_1c, 0x0018) {
        let Some(shell) = follows(disc_18, 0x0016) else {
            return Vec::new();
        };
        (shell, false)
    } else {
        let Some(shell) = follows(disc_1c, 0x0016).filter(|shell| shell.flo() == 1) else {
            return Vec::new();
        };
        (shell, true)
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
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let complete = if direct_shell {
        let faces = count(0x0004, 1);
        disc_10.refs.get(2).is_none_or(|attr| *attr <= 1)
            && faces > 0
            && faces == count(0x000e, 2)
            && faces == count(0x001a, 1)
            && faces == count(0x0022, 4)
    } else {
        follows(disc_10, 0x000e).is_some() && count(0x0022, 4) > 0
    };
    if !complete {
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

fn disc20_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1e) = follows(root, 0x001e, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_1e, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1c, 0x0018, 1) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_12, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
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

fn disc20_disc1a_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1c) = follows(root, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(disc_1a) = follows(disc_1c, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1a, 0x0018, 1) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_12, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        // Population counts describe framed records in this site. Do not let
        // a later overlapping record hide a canonical face from the lattice.
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0010, 1) || faces != count(0x001e, 1) {
        return Vec::new();
    }
    let face_use_attrs = entities
        .iter()
        .filter(|record| record.disc == 0x001e && record.flo() == 1)
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    let linked_face_use_attrs = entities
        .iter()
        .filter(|record| record.disc == 0x0022 && record.flo() == 4)
        .filter_map(|record| record.refs.get(2).copied())
        .filter(|attr| face_use_attrs.contains(attr))
        .collect::<HashSet<_>>();
    if face_use_attrs
        .iter()
        .any(|attr| !linked_face_use_attrs.contains(attr))
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

fn disc20_disc18_disc12_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    disc20_disc18_terminal_face_root_body(by_attr, entities, 0x0012)
}

fn disc20_disc18_disc14_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    disc20_disc18_terminal_face_root_body(by_attr, entities, 0x0014)
}

fn disc20_disc18_terminal_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
    terminal_disc: u16,
) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1e) = follows(root, 0x001e, 2) else {
        return Vec::new();
    };
    let Some(disc_1c) = follows(disc_1e, 0x001c, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1c, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(face_chain) = follows(shell, terminal_disc, 1) else {
        return Vec::new();
    };
    let Some(suffix) = face_chain
        .refs
        .get(2)
        .and_then(|attr| by_attr.get(attr))
        .copied()
    else {
        return Vec::new();
    };
    let terminal = if suffix.disc == 0x0004 && suffix.flo() == 2 {
        suffix
    } else if matches!(suffix.disc, 0x0010 | 0x0012) && suffix.flo() == 2 {
        let Some(terminal) = suffix
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| record.disc == 0x0004 && record.flo() == 2)
        else {
            return Vec::new();
        };
        terminal
    } else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        // Population counts describe framed records in this site. Do not let
        // a later overlapping record hide a canonical face from the lattice.
        entities
            .iter()
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

fn disc20_disc1a_disc18_disc16_disc14_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x0020, 2),
            (0x001a, 2),
            (0x0018, 2),
            (0x0016, 1),
            (0x0014, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x0010,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: Some(KeyedFaceUse {
                disc: 0x001e,
                bridge: None,
                by_key: true,
            }),
            shell_index: 2,
            require_exact_use_population: false,
        },
    )
}

fn disc20_disc1a_disc18_disc16_disc14_disc12_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_companion_use_bridge(
        by_attr,
        &[
            (0x0020, 2),
            (0x001a, 2),
            (0x0018, 2),
            (0x0016, 1),
            (0x0014, 2),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001e,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x001c,
                flo: 2,
            }),
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: false,
        },
        Some(KeyedFaceBridge {
            disc: 0x001c,
            flo: 2,
        }),
    )
}

fn disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc0e_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0018, 2),
            (0x0016, 1),
            (0x0014, 2),
            (0x0012, 2),
            (0x000e, 2),
        ],
        0x0004,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 4,
            require_exact_use_population: false,
        },
        false,
    )
}

fn disc20_disc1e_disc1c_disc18_disc16_disc14_disc12_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0018, 2),
            (0x0016, 1),
            (0x0014, 2),
            (0x0012, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 4,
            require_exact_use_population: false,
        },
        false,
    )
}

fn disc20_disc1e_disc1c_disc18_disc16_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0018, 1),
            (0x0016, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0010,
                flo: 1,
            }),
            face_use_shape: None,
            shell_index: 4,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc20_disc1e_disc1c_disc18_disc16_disc12_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0018, 1),
            (0x0016, 2),
            (0x0012, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0010,
                flo: 1,
            }),
            face_use_shape: None,
            shell_index: 4,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc20_disc1e_disc1c_disc16_disc14_disc10_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_and_companion_use_bridge(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0016, 1),
            (0x0014, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: false,
        },
        Some(KeyedFaceBridge {
            disc: 0x0018,
            flo: 2,
        }),
        true,
    )
}

fn disc20_disc1e_disc1c_disc16_disc14_disc12_disc10_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_and_companion_use_bridge(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0016, 1),
            (0x0014, 2),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: false,
        },
        Some(KeyedFaceBridge {
            disc: 0x0018,
            flo: 2,
        }),
        true,
    )
}

fn disc20_disc1e_disc1c_disc18_disc16_disc10_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0018, 1),
            (0x0016, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 4,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc20_disc1e_disc1c_disc16_disc14_disc12_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_and_companion_use_bridge(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0016, 2),
            (0x0014, 2),
            (0x0012, 2),
        ],
        0x0010,
        0x0018,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: true,
        },
        Some(KeyedFaceBridge {
            disc: 0x001a,
            flo: 2,
        }),
        false,
    )
}

fn disc20_disc1e_disc1c_disc14_disc12_disc10_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: false,
        },
        true,
    )
}

fn keyed_disc1a_disc18_disc14_disc12_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x001a, 2),
            (0x0018, 2),
            (0x0014, 2),
            (0x0012, 1),
            (0x0004, 2),
        ],
        0x000e,
        0x0016,
        0x001c,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc1e_disc1c_disc1a_disc16_disc14_disc12_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x001e, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0016, 2),
            (0x0014, 2),
            (0x0012, 2),
            (0x000e, 1),
        ],
        0x0004,
        0x0018,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0010,
                flo: 2,
            }),
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
        false,
    )
}

fn disc22_disc20_disc1e_disc1a_disc18_disc12_disc10_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0022, 2),
            (0x0020, 2),
            (0x001e, 2),
            (0x001a, 1),
            (0x0018, 2),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001c,
        0x0024,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: false,
        },
        true,
    )
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

fn disc1c_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(root, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012, 1) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x001e, 4) {
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

fn disc1c_disc16_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(shell) = follows(root, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(disc_16) = follows(shell, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_16, 0x0012, 1) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0 || faces != count(0x0014, 1) || faces != count(0x001a, 4) {
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

fn disc1c_disc16_disc14_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_18) = follows(root, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_12, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0
        || faces != count(0x0010, 1)
        || faces != count(0x001a, 1)
        || faces != count(0x001e, 4)
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

fn disc1c_disc16_disc14_linked_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    let matching_chains = keyed_forward_chain_candidates(by_attr)
        .into_iter()
        .filter(|chain| {
            chain.len() == 6
                && matches!(
                    chain.as_slice(),
                    [root, disc_18, shell, disc_14, disc_12, terminal]
                        if root.disc == 0x001c
                            && root.flo() == 2
                            && disc_18.disc == 0x0018
                            && disc_18.flo() == 2
                            && shell.disc == 0x0016
                            && shell.flo() == 2
                            && disc_14.disc == 0x0014
                            && disc_14.flo() == 1
                            && disc_12.disc == 0x0012
                            && disc_12.flo() == 2
                            && terminal.disc == 0x0004
                            && terminal.flo() == 2
                )
        })
        .collect::<Vec<_>>();
    let [chain] = matching_chains.as_slice() else {
        return Vec::new();
    };
    let canonical_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x000e && record.flo() == 1)
        .collect::<Vec<_>>();
    if canonical_faces.is_empty() {
        return Vec::new();
    }
    let direct_face_use = |face: &EntityRecord| {
        let companion = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        if companion.disc != 0x001a
            || companion.flo() != 1
            || companion.refs.get(2) != Some(&face.attr)
        {
            return None;
        }
        Some(companion)
    };
    let intermediate_face_use = |face: &EntityRecord| {
        let intermediate = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        if intermediate.disc != 0x0010
            || intermediate.flo() != 1
            || intermediate.refs.get(2) != Some(&face.attr)
        {
            return None;
        }
        let companion = intermediate
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        (companion.disc == 0x001a
            && companion.flo() == 1
            && companion.refs.get(2) == Some(&intermediate.attr))
        .then_some(companion)
    };
    let mut companions = HashSet::new();
    let mut use_nodes = HashSet::new();
    for face in canonical_faces {
        let companion = direct_face_use(face).or_else(|| intermediate_face_use(face));
        let Some(companion) = companion else {
            return Vec::new();
        };
        let Some(use_node) = companion
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == 0x001e
                    && record.flo() == 4
                    && record.refs.get(2) == Some(&companion.attr)
            })
        else {
            return Vec::new();
        };
        if !companions.insert(companion.attr) || !use_nodes.insert(use_node.attr) {
            return Vec::new();
        }
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    let root = chain[0];
    let shell = chain[2];
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

fn disc1c_disc14_disc12_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(root, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(disc_14) = follows(shell, 0x0014, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(disc_14, 0x0012, 1) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_10, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let disc_0e_faces = count(0x000e, 1);
    let disc_0c_faces = count(0x000c, 1);
    let (face_disc, faces) = match (disc_0e_faces, disc_0c_faces) {
        (faces, 0) if faces > 0 => (0x000e, faces),
        (0, faces) if faces > 0 => (0x000c, faces),
        _ => return Vec::new(),
    };
    if faces != count(0x0016, 1) || count(0x001e, 4) < faces {
        return Vec::new();
    }
    let face_use_attrs = entities
        .iter()
        .filter(|record| record.disc == 0x0016 && record.flo() == 1)
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    let use_node_attrs = entities
        .iter()
        .filter(|record| record.disc == 0x001e && record.flo() == 4)
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    let face_use_attr = |face: &EntityRecord| {
        let linked = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        match (linked.disc, linked.flo()) {
            (0x0016, 1) => Some(linked.attr),
            (0x0014 | 0x000e, 2) => {
                if linked.refs.get(2) != Some(&face.attr) {
                    return None;
                }
                let face_use_attr = linked.refs.get(1).copied()?;
                by_attr
                    .get(&face_use_attr)
                    .copied()
                    .filter(|record| record.disc == 0x0016 && record.flo() == 1)
                    .map(|_| face_use_attr)
            }
            _ => None,
        }
    };
    let resolved_face_uses = entities
        .iter()
        .filter(|record| record.disc == face_disc && record.flo() == 1)
        .filter_map(face_use_attr)
        .collect::<HashSet<_>>();
    if resolved_face_uses != face_use_attrs
        || entities
            .iter()
            .filter(|record| record.disc == 0x0016 && record.flo() == 1)
            .any(|face_use| {
                face_use
                    .refs
                    .get(1)
                    .is_none_or(|attr| !use_node_attrs.contains(attr))
            })
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

fn disc1c_disc14_disc0e_linked_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    let matching_chains = keyed_forward_chain_candidates(by_attr)
        .into_iter()
        .filter(|chain| {
            chain.len() == 6
                && matches!(
                    chain.as_slice(),
                    [root, disc_1a, shell, disc_14, disc_12, terminal]
                        if root.disc == 0x001c
                            && root.flo() == 2
                            && root.refs.get(1) == Some(&1)
                            && disc_1a.disc == 0x001a
                            && disc_1a.flo() == 2
                            && shell.disc == 0x0018
                            && shell.flo() == 2
                            && disc_14.disc == 0x0014
                            && disc_14.flo() == 1
                            && disc_12.disc == 0x0012
                            && disc_12.flo() == 2
                            && terminal.disc == 0x0004
                            && terminal.flo() == 2
                            && terminal.refs.get(2) == Some(&1)
                )
        })
        .collect::<Vec<_>>();
    let [chain] = matching_chains.as_slice() else {
        return Vec::new();
    };
    let canonical_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x000c && record.flo() == 1)
        .collect::<Vec<_>>();
    if canonical_faces.is_empty() {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = canonical_faces.len();
    if faces != count(0x000e, 1) || faces != count(0x0016, 1) || faces != count(0x001e, 4) {
        return Vec::new();
    }
    let companion_of = |face: &EntityRecord| {
        let intermediate = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        if intermediate.disc != 0x000e
            || intermediate.flo() != 1
            || intermediate.refs.get(2) != Some(&face.attr)
        {
            return None;
        }
        let linked = intermediate
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        if linked.disc == 0x0016
            && linked.flo() == 1
            && linked.refs.get(2) == Some(&intermediate.attr)
        {
            return Some(linked);
        }
        if linked.disc != 0x0010
            || linked.flo() != 2
            || linked.refs.get(2) != Some(&intermediate.attr)
        {
            return None;
        }
        let companion = linked
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()?;
        (companion.disc == 0x0016
            && companion.flo() == 1
            && companion.refs.get(2) == Some(&linked.attr))
        .then_some(companion)
    };
    let mut companions = HashSet::new();
    let mut use_nodes = HashSet::new();
    for face in canonical_faces {
        let Some(companion) = companion_of(face) else {
            return Vec::new();
        };
        let Some(use_node) = companion
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == 0x001e
                    && record.flo() == 4
                    && record.refs.get(2) == Some(&companion.attr)
            })
        else {
            return Vec::new();
        };
        if !companions.insert(companion.attr) || !use_nodes.insert(use_node.attr) {
            return Vec::new();
        }
    }
    if companions.len() != faces || use_nodes.len() != faces {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    let root = chain[0];
    let shell = chain[2];
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

fn disc1c_disc12_terminal_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_1a) = follows(root, 0x001a, 2) else {
        return Vec::new();
    };
    let Some(disc_18) = follows(disc_1a, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0014, 1) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_12, 0x0004, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        by_attr
            .values()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x000e, 1);
    if faces == 0
        || faces != count(0x0010, 1)
        || faces != count(0x0016, 1)
        || faces != count(0x001e, 4)
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

fn disc1c_disc0c_terminal_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
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
    let follows = |record: &EntityRecord, disc: u16, flo: u8| {
        record
            .refs
            .get(2)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|next| next.disc == disc && next.flo() == flo)
    };
    let Some(disc_18) = follows(root, 0x0018, 2) else {
        return Vec::new();
    };
    let Some(shell) = follows(disc_18, 0x0016, 2) else {
        return Vec::new();
    };
    let Some(disc_12) = follows(shell, 0x0012, 1) else {
        return Vec::new();
    };
    let Some(disc_10) = follows(disc_12, 0x0010, 2) else {
        return Vec::new();
    };
    let Some(disc_0e) = follows(disc_10, 0x000e, 2) else {
        return Vec::new();
    };
    let Some(terminal) = follows(disc_0e, 0x000c, 2) else {
        return Vec::new();
    };
    if terminal.refs.get(2).is_some_and(|attr| *attr > 1) {
        return Vec::new();
    }
    let count = |disc: u16, flo: u8| {
        // Keep population counts independent of the latest-record index. An
        // overlapping scan can shadow a canonical face attribute with a
        // later non-face record while the framed face remains in the stream.
        entities
            .iter()
            .filter(|record| record.disc == disc && record.flo() == flo)
            .count()
    };
    let faces = count(0x0004, 1);
    if faces == 0 || faces != count(0x0014, 1) || faces != count(0x001a, 4) {
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

fn disc1c_disc16_disc0e_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001c, 2),
            (0x0018, 2),
            (0x0016, 2),
            (0x0012, 1),
            (0x0010, 2),
            (0x000e, 2),
        ],
        0x0004,
        0x0014,
        0x001a,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1c_disc16_disc0e_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001c, 2),
            (0x0018, 2),
            (0x0016, 2),
            (0x0012, 1),
            (0x0010, 2),
            (0x000e, 2),
            (0x0004, 2),
        ],
        0x000c,
        0x0014,
        0x001a,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1c_disc16_disc12_disc0e_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001c, 2),
            (0x0018, 2),
            (0x0016, 2),
            (0x0012, 2),
            (0x0010, 1),
            (0x000e, 2),
        ],
        0x0004,
        0x0014,
        0x001a,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1c_disc16_disc14_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001c, 2),
            (0x0018, 2),
            (0x0016, 2),
            (0x0014, 2),
            (0x0010, 1),
            (0x000e, 2),
            (0x0004, 2),
        ],
        0x000c,
        0x0012,
        0x001a,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1c_disc1a_disc18_disc14_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001c, 2),
            (0x001a, 2),
            (0x0018, 2),
            (0x0014, 1),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x0016,
        0x001e,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0012,
                flo: 2,
            }),
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1a_disc14_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001a, 2),
            (0x0016, 2),
            (0x0014, 2),
            (0x0010, 1),
            (0x000e, 2),
            (0x0004, 2),
        ],
        0x000c,
        0x0012,
        0x0018,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
    )
}

fn disc1a_disc14_disc0c_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001a, 2),
            (0x0016, 2),
            (0x0014, 2),
            (0x0010, 1),
            (0x000e, 2),
            (0x000c, 2),
        ],
        0x0004,
        0x0012,
        0x0018,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
    )
}

fn disc1a_disc16_disc14_disc12_disc0e_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001a, 2),
            (0x0016, 2),
            (0x0014, 2),
            (0x0012, 1),
            (0x0010, 2),
            (0x000e, 2),
            (0x0004, 2),
        ],
        0x000c,
        0x0018,
        0x001c,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
    )
}

fn disc1a_disc12_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001a, 2),
            (0x0016, 2),
            (0x0012, 1),
            (0x0010, 2),
            (0x000e, 2),
            (0x0004, 2),
        ],
        0x000c,
        0x0014,
        0x0018,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
    )
}

fn disc1a_disc14_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001a, 2),
            (0x0016, 2),
            (0x0012, 2),
            (0x0010, 1),
            (0x0004, 2),
        ],
        0x000e,
        0x0014,
        0x0018,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
    )
}

fn disc1a_disc18_disc14_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001a, 2),
            (0x0018, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x0016,
        0x001c,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
    )
}

fn disc16_disc14_disc04_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x0016, 2),
            (0x0014, 2),
            (0x0012, 2),
            (0x0010, 1),
            (0x0004, 2),
        ],
        0x000e,
        0x0018,
        0x001a,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: false,
        },
    )
}

fn disc18_disc14_disc12_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x0018, 2),
            (0x0014, 2),
            (0x0012, 2),
            (0x0010, 1),
            (0x000e, 2),
            (0x0004, 2),
        ],
        0x000c,
        0x0016,
        0x001a,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
    )
}

fn disc20_disc1a_disc14_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x0020, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x0010,
        0x001e,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: Some(KeyedFaceUse {
                disc: 0x0018,
                bridge: Some(KeyedFaceBridge {
                    disc: 0x0016,
                    flo: 2,
                }),
                by_key: false,
            }),
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0016, 1),
            (0x0014, 2),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x0018,
        0x001e,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc20_disc1c_disc1a_disc16_disc12_disc10_disc0e_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links(
        by_attr,
        &[
            (0x0020, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0016, 1),
            (0x0012, 2),
            (0x0010, 2),
            (0x000e, 2),
        ],
        0x0004,
        0x0018,
        0x001e,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0016,
                flo: 2,
            }),
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc20_disc12_disc1e_disc1c_disc18_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_with_unselected_companions(
        by_attr,
        &[
            (0x0020, 2),
            (0x0012, 2),
            (0x001e, 2),
            (0x001c, 2),
            (0x0018, 1),
        ],
        0x0004,
        0x001a,
        0x0022,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 4,
            require_exact_use_population: true,
        },
        true,
    )
}

fn disc1e_disc1c_disc16_disc14_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001e, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0016, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0010, 2),
        ],
        0x000e,
        0x0018,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1e_disc16_disc1c_disc1a_disc14_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_with_unselected_companions(
        by_attr,
        &[
            (0x001e, 2),
            (0x0016, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0014, 1),
        ],
        0x000c,
        0x0018,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
        true,
    )
}

fn disc1e_disc10_disc1c_disc1a_disc0e_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_with_unselected_companions(
        by_attr,
        &[
            (0x001e, 2),
            (0x0010, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x000e, 1),
        ],
        0x0014,
        0x0018,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: true,
        },
        true,
    )
}

fn disc20_disc16_disc26_disc1e_disc14_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_with_unselected_companions(
        by_attr,
        &[
            (0x0020, 2),
            (0x0016, 2),
            (0x0026, 2),
            (0x001e, 2),
            (0x0014, 1),
        ],
        0x0006,
        0x0024,
        0x0028,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc26_disc1e_disc24_disc22_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_with_unselected_companions(
        by_attr,
        &[
            (0x0026, 2),
            (0x001e, 2),
            (0x0024, 2),
            (0x0022, 2),
            (0x0004, 1),
        ],
        0x0010,
        0x0020,
        0x0028,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 1,
            require_exact_use_population: false,
        },
        true,
    )
}

fn disc1e_disc1c_disc16_disc14_disc0e_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001e, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0016, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0010, 2),
            (0x000e, 2),
        ],
        0x0004,
        0x0018,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: None,
            face_use_shape: None,
            shell_index: 3,
            require_exact_use_population: true,
        },
    )
}

fn disc1e_disc1c_disc14_disc0e_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001e, 2),
            (0x001c, 2),
            (0x001a, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0010, 2),
            (0x000e, 2),
        ],
        0x0004,
        0x0018,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0016,
                flo: 2,
            }),
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1e_disc1a_disc18_disc14_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001e, 2),
            (0x001a, 2),
            (0x0018, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001c,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0016,
                flo: 2,
            }),
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: false,
        },
    )
}

fn disc1e_disc18_disc16_disc14_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001e, 2),
            (0x0018, 2),
            (0x0016, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0010, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x001c,
        0x0020,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x001a,
                flo: 2,
            }),
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: true,
        },
    )
}

fn disc1e_disc1a_disc18_disc14_disc12_disc04_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
) -> Vec<BodyRecord> {
    keyed_face_root_body(
        by_attr,
        &[
            (0x001e, 2),
            (0x001a, 2),
            (0x0018, 2),
            (0x0014, 1),
            (0x0012, 2),
            (0x0004, 2),
        ],
        0x000e,
        0x0016,
        0x001c,
        KeyedFaceRootOptions {
            canonical_face_bridge: Some(KeyedFaceBridge {
                disc: 0x0010,
                flo: 1,
            }),
            face_use_shape: None,
            shell_index: 2,
            require_exact_use_population: false,
        },
    )
}

#[derive(Clone, Copy)]
struct KeyedFaceBridge {
    disc: u16,
    flo: u8,
}

#[derive(Clone, Copy)]
struct KeyedFaceUse {
    disc: u16,
    bridge: Option<KeyedFaceBridge>,
    by_key: bool,
}

#[derive(Clone, Copy)]
struct KeyedFaceRootOptions {
    canonical_face_bridge: Option<KeyedFaceBridge>,
    face_use_shape: Option<KeyedFaceUse>,
    shell_index: usize,
    require_exact_use_population: bool,
}

#[derive(Clone, Copy)]
enum KeyedLinkPolicy {
    Reciprocal,
    ForwardKeyed,
}

fn keyed_companion_by_key<'a>(
    by_attr: &HashMap<u16, &'a EntityRecord>,
    face: &EntityRecord,
    companion_disc: u16,
) -> Option<&'a EntityRecord> {
    let key = face.refs.first().copied().filter(|key| *key > 1)?;
    let candidates = by_attr
        .values()
        .copied()
        .filter(|record| {
            record.disc == companion_disc
                && record.flo() == 1
                && record.refs.first() == Some(&key)
                && record.refs.get(1).is_some_and(|attr| *attr > 1)
        })
        .collect::<Vec<_>>();
    let [companion] = candidates.as_slice() else {
        return None;
    };
    Some(*companion)
}

fn keyed_face_companion<'a>(
    by_attr: &HashMap<u16, &'a EntityRecord>,
    face: &EntityRecord,
    companion_disc: u16,
    canonical_face_bridge: Option<KeyedFaceBridge>,
    link_policy: KeyedLinkPolicy,
    allow_keyed_companion_fallback: bool,
) -> Option<(&'a EntityRecord, Option<&'a EntityRecord>)> {
    let direct = face
        .refs
        .get(1)
        .and_then(|attr| by_attr.get(attr))
        .copied()
        .filter(|record| {
            record.disc == companion_disc
                && record.flo() == 1
                && record.refs.get(2) == Some(&face.attr)
        })
        .map(|companion| (companion, None));
    let bridged = canonical_face_bridge.and_then(|bridge_shape| {
        let bridge = face
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == bridge_shape.disc
                    && record.flo() == bridge_shape.flo
                    && record.refs.get(2) == Some(&face.attr)
            })?;
        let companion = bridge
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == companion_disc
                    && record.flo() == 1
                    && record.refs.get(2) == Some(&bridge.attr)
            })?;
        Some((companion, Some(bridge)))
    });
    let forward_keyed = if direct.is_none()
        && bridged.is_none()
        && matches!(link_policy, KeyedLinkPolicy::ForwardKeyed)
    {
        let key = face.refs.first().copied().filter(|key| *key > 1)?;
        face.refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == companion_disc
                    && record.flo() == 1
                    && record.refs.first() == Some(&key)
                    && record.refs.get(1).is_some_and(|attr| *attr > 1)
            })
            .map(|companion| (companion, None))
    } else {
        None
    };
    let keyed = if direct.is_none()
        && bridged.is_none()
        && forward_keyed.is_none()
        && allow_keyed_companion_fallback
    {
        keyed_companion_by_key(by_attr, face, companion_disc).map(|companion| (companion, None))
    } else {
        None
    };
    let mut selected = None;
    for candidate in [direct, bridged, forward_keyed, keyed]
        .into_iter()
        .flatten()
    {
        if selected.is_some() {
            return None;
        }
        selected = Some(candidate);
    }
    selected
}

fn keyed_companion_use<'a>(
    by_attr: &HashMap<u16, &'a EntityRecord>,
    companion: &EntityRecord,
    use_disc: u16,
    bridge_shape: Option<KeyedFaceBridge>,
    link_policy: KeyedLinkPolicy,
) -> Option<(&'a EntityRecord, Option<&'a EntityRecord>)> {
    let direct = companion
        .refs
        .get(1)
        .and_then(|attr| by_attr.get(attr))
        .copied()
        .filter(|record| {
            record.disc == use_disc
                && record.flo() == 4
                && record.refs.get(2) == Some(&companion.attr)
        })
        .map(|use_node| (use_node, None));
    let bridged = bridge_shape.and_then(|bridge_shape| {
        let bridge = companion
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == bridge_shape.disc
                    && record.flo() == bridge_shape.flo
                    && record.refs.get(2) == Some(&companion.attr)
            })?;
        let use_node = bridge
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == use_disc
                    && record.flo() == 4
                    && record.refs.get(2) == Some(&bridge.attr)
            })?;
        Some((use_node, Some(bridge)))
    });
    let forward_keyed = if direct.is_none()
        && bridged.is_none()
        && matches!(link_policy, KeyedLinkPolicy::ForwardKeyed)
    {
        let key = companion.refs.first().copied().filter(|key| *key > 1)?;
        companion
            .refs
            .get(1)
            .and_then(|attr| by_attr.get(attr))
            .copied()
            .filter(|record| {
                record.disc == use_disc && record.flo() == 4 && record.refs.first() == Some(&key)
            })
            .map(|use_node| (use_node, None))
    } else {
        None
    };
    let mut selected = None;
    for candidate in [direct, bridged, forward_keyed].into_iter().flatten() {
        if selected.is_some() {
            return None;
        }
        selected = Some(candidate);
    }
    selected
}

fn keyed_face_root_body_with_keyed_face_links(
    by_attr: &HashMap<u16, &EntityRecord>,
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    options: KeyedFaceRootOptions,
    allow_keyed_companion_fallback: bool,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_and_companion_use_bridge(
        by_attr,
        chain_shape,
        canonical_disc,
        companion_disc,
        use_disc,
        options,
        None,
        allow_keyed_companion_fallback,
    )
}

fn keyed_face_root_body_with_keyed_face_links_with_unselected_companions(
    by_attr: &HashMap<u16, &EntityRecord>,
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    options: KeyedFaceRootOptions,
    allow_keyed_companion_fallback: bool,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_with_population_policy(
        by_attr,
        chain_shape,
        canonical_disc,
        companion_disc,
        use_disc,
        options,
        None,
        KeyedLinkPolicy::ForwardKeyed,
        allow_keyed_companion_fallback,
        true,
    )
}

#[allow(clippy::too_many_arguments)] // Each argument identifies an independent layout role or link policy.
fn keyed_face_root_body_with_keyed_face_links_and_companion_use_bridge(
    by_attr: &HashMap<u16, &EntityRecord>,
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    options: KeyedFaceRootOptions,
    companion_use_bridge: Option<KeyedFaceBridge>,
    allow_keyed_companion_fallback: bool,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_keyed_face_links_with_population_policy(
        by_attr,
        chain_shape,
        canonical_disc,
        companion_disc,
        use_disc,
        options,
        companion_use_bridge,
        KeyedLinkPolicy::Reciprocal,
        allow_keyed_companion_fallback,
        false,
    )
}

#[allow(clippy::too_many_arguments)] // Each argument identifies an independent layout role or link policy.
fn keyed_face_root_body_with_keyed_face_links_with_population_policy(
    by_attr: &HashMap<u16, &EntityRecord>,
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    options: KeyedFaceRootOptions,
    companion_use_bridge: Option<KeyedFaceBridge>,
    link_policy: KeyedLinkPolicy,
    allow_keyed_companion_fallback: bool,
    allow_unselected_companions: bool,
) -> Vec<BodyRecord> {
    let bodies = keyed_face_root_body_with_companion_use_bridge_and_fallback(
        by_attr,
        chain_shape,
        canonical_disc,
        companion_disc,
        use_disc,
        options,
        companion_use_bridge,
        link_policy,
        allow_keyed_companion_fallback,
        allow_unselected_companions,
    );
    if bodies.is_empty() {
        return bodies;
    }
    let keyed_links = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == canonical_disc && record.flo() == 1)
        .all(|face| {
            let Some((companion, bridge)) = keyed_face_companion(
                by_attr,
                face,
                companion_disc,
                options.canonical_face_bridge,
                link_policy,
                allow_keyed_companion_fallback,
            ) else {
                return false;
            };
            let Some((use_node, use_bridge)) = keyed_companion_use(
                by_attr,
                companion,
                use_disc,
                companion_use_bridge,
                link_policy,
            ) else {
                return false;
            };
            let direct_selected = face
                .refs
                .get(1)
                .and_then(|attr| by_attr.get(attr))
                .is_some_and(|record| {
                    record.attr == companion.attr
                        && record.disc == companion_disc
                        && record.flo() == 1
                        && record.refs.get(2) == Some(&face.attr)
                });
            let keyed_fallback_selected = allow_keyed_companion_fallback
                && !direct_selected
                && keyed_companion_by_key(by_attr, face, companion_disc)
                    .is_some_and(|record| record.attr == companion.attr);
            let companion_link_matches = match bridge {
                Some(bridge) => {
                    bridge.refs.get(2) == Some(&face.attr)
                        && companion.refs.get(2) == Some(&bridge.attr)
                }
                None => {
                    let forward_keyed_selected =
                        matches!(link_policy, KeyedLinkPolicy::ForwardKeyed)
                            && !direct_selected
                            && face.refs.get(1) == Some(&companion.attr)
                            && companion.refs.first() == face.refs.first()
                            && companion.refs.get(1).is_some_and(|attr| *attr > 1);
                    direct_selected || forward_keyed_selected || keyed_fallback_selected
                }
            };
            let use_link_matches = match use_bridge {
                Some(bridge) => {
                    bridge.refs.get(2) == Some(&companion.attr)
                        && use_node.refs.get(2) == Some(&bridge.attr)
                }
                None => {
                    let forward_keyed_selected =
                        matches!(link_policy, KeyedLinkPolicy::ForwardKeyed)
                            && companion.refs.get(1) == Some(&use_node.attr)
                            && use_node.refs.first() == companion.refs.first();
                    use_node.refs.get(2) == Some(&companion.attr) || forward_keyed_selected
                }
            };
            companion_link_matches
                && use_link_matches
                && face.refs.first() == companion.refs.first()
                && companion.refs.first() == use_node.refs.first()
                && bridge.is_none_or(|bridge| bridge.refs.first() == face.refs.first())
                && use_bridge.is_none_or(|bridge| bridge.refs.first() == companion.refs.first())
        });
    if keyed_links {
        bodies
    } else {
        Vec::new()
    }
}

fn keyed_face_root_body(
    by_attr: &HashMap<u16, &EntityRecord>,
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    options: KeyedFaceRootOptions,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_companion_use_bridge(
        by_attr,
        chain_shape,
        canonical_disc,
        companion_disc,
        use_disc,
        options,
        None,
    )
}

fn keyed_face_root_body_with_companion_use_bridge(
    by_attr: &HashMap<u16, &EntityRecord>,
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    options: KeyedFaceRootOptions,
    companion_use_bridge: Option<KeyedFaceBridge>,
) -> Vec<BodyRecord> {
    keyed_face_root_body_with_companion_use_bridge_and_fallback(
        by_attr,
        chain_shape,
        canonical_disc,
        companion_disc,
        use_disc,
        options,
        companion_use_bridge,
        KeyedLinkPolicy::Reciprocal,
        false,
        false,
    )
}

#[allow(clippy::too_many_arguments)] // Each argument identifies an independent layout role or link policy.
fn keyed_face_root_body_with_companion_use_bridge_and_fallback(
    by_attr: &HashMap<u16, &EntityRecord>,
    chain_shape: &[(u16, u8)],
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    options: KeyedFaceRootOptions,
    companion_use_bridge: Option<KeyedFaceBridge>,
    link_policy: KeyedLinkPolicy,
    allow_keyed_companion_fallback: bool,
    allow_unselected_companions: bool,
) -> Vec<BodyRecord> {
    let matching_chains = keyed_forward_chain_candidates(by_attr)
        .into_iter()
        .filter(|chain| {
            chain.len() == chain_shape.len()
                && chain
                    .first()
                    .is_some_and(|root| root.refs.get(1) == Some(&1))
                && chain
                    .last()
                    .is_some_and(|terminal| terminal.refs.get(2) == Some(&1))
                && chain_shape
                    .iter()
                    .copied()
                    .zip(chain.iter())
                    .all(|((disc, flo), record)| record.disc == disc && record.flo() == flo)
        })
        .collect::<Vec<_>>();
    let [chain] = matching_chains.as_slice() else {
        return Vec::new();
    };
    let canonical_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == canonical_disc && record.flo() == 1)
        .collect::<Vec<_>>();
    if canonical_faces.is_empty() {
        return Vec::new();
    }
    let companion_of = |face: &EntityRecord| {
        keyed_face_companion(
            by_attr,
            face,
            companion_disc,
            options.canonical_face_bridge,
            link_policy,
            allow_keyed_companion_fallback,
        )
    };
    let mut companions = HashSet::new();
    let mut face_uses = HashSet::new();
    let mut canonical_face_bridges = HashSet::new();
    let mut face_use_bridges = HashSet::new();
    let mut companion_use_bridges = HashSet::new();
    let mut use_nodes = HashSet::new();
    for face in canonical_faces.iter().copied() {
        let Some((companion, canonical_face_bridge)) = companion_of(face) else {
            return Vec::new();
        };
        let (face_use, face_use_bridge) = if let Some(face_use_shape) = options.face_use_shape {
            if face_use_shape.by_key {
                let Some(key) = face.refs.first().copied().filter(|key| *key > 1) else {
                    return Vec::new();
                };
                let candidates = by_attr
                    .values()
                    .copied()
                    .filter(|record| {
                        record.disc == face_use_shape.disc
                            && record.flo() == 1
                            && record.refs.first() == Some(&key)
                            && record
                                .refs
                                .get(1)
                                .and_then(|attr| by_attr.get(attr))
                                .is_some_and(|use_node| {
                                    use_node.disc == use_disc
                                        && use_node.flo() == 4
                                        && use_node.refs.get(2) == Some(&record.attr)
                                })
                    })
                    .collect::<Vec<_>>();
                let [face_use] = candidates.as_slice() else {
                    return Vec::new();
                };
                (*face_use, None)
            } else {
                let direct = companion
                    .refs
                    .get(1)
                    .and_then(|attr| by_attr.get(attr))
                    .copied()
                    .filter(|record| {
                        record.disc == face_use_shape.disc
                            && record.flo() == 1
                            && record.refs.get(2) == Some(&companion.attr)
                    });
                let bridged = face_use_shape.bridge.and_then(|bridge_shape| {
                    let bridge = companion
                        .refs
                        .get(1)
                        .and_then(|attr| by_attr.get(attr))
                        .copied()
                        .filter(|record| {
                            record.disc == bridge_shape.disc
                                && record.flo() == bridge_shape.flo
                                && record.refs.get(2) == Some(&companion.attr)
                        })?;
                    let face_use = bridge
                        .refs
                        .get(1)
                        .and_then(|attr| by_attr.get(attr))
                        .copied()
                        .filter(|record| {
                            record.disc == face_use_shape.disc
                                && record.flo() == 1
                                && record.refs.get(2) == Some(&bridge.attr)
                        })?;
                    Some((face_use, bridge))
                });
                match (direct, bridged) {
                    (Some(_), Some(_)) => return Vec::new(),
                    (Some(face_use), None) => (face_use, None),
                    (None, Some((face_use, bridge))) => (face_use, Some(bridge)),
                    (None, None) => return Vec::new(),
                }
            }
        } else {
            (companion, None)
        };
        let use_owner = companion_use_bridge.map_or(face_use, |_| companion);
        let Some((use_node, selected_companion_use_bridge)) = keyed_companion_use(
            by_attr,
            use_owner,
            use_disc,
            companion_use_bridge,
            link_policy,
        ) else {
            return Vec::new();
        };
        if !companions.insert(companion.attr) || !use_nodes.insert(use_node.attr) {
            return Vec::new();
        }
        if options.face_use_shape.is_some() && !face_uses.insert(face_use.attr) {
            return Vec::new();
        }
        if let Some(bridge) = canonical_face_bridge {
            if !canonical_face_bridges.insert(bridge.attr) {
                return Vec::new();
            }
        }
        if let Some(bridge) = face_use_bridge {
            if !face_use_bridges.insert(bridge.attr) {
                return Vec::new();
            }
        }
        if let Some(bridge) = selected_companion_use_bridge {
            if !companion_use_bridges.insert(bridge.attr) {
                return Vec::new();
            }
        }
    }
    let companion_count = by_attr
        .values()
        .filter(|record| record.disc == companion_disc && record.flo() == 1)
        .count();
    let use_node_count = by_attr
        .values()
        .filter(|record| record.disc == use_disc && record.flo() == 4)
        .count();
    let face_use_count = options.face_use_shape.map(|face_use_shape| {
        by_attr
            .values()
            .filter(|record| record.disc == face_use_shape.disc && record.flo() == 1)
            .count()
    });
    let require_exact_companion_population = !allow_unselected_companions
        && companion_use_bridge.is_none()
        && !options
            .face_use_shape
            .is_some_and(|shape| shape.by_key && shape.disc == companion_disc);
    if (require_exact_companion_population && companions.len() != companion_count)
        || (options.face_use_shape.is_some_and(|shape| !shape.by_key)
            && face_use_count.is_some_and(|count| face_uses.len() != count))
        || (options.require_exact_use_population && use_nodes.len() != use_node_count)
        || (!options.require_exact_use_population && use_nodes.len() > use_node_count)
    {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    let root = chain[0];
    let Some(shell) = chain.get(options.shell_index).copied() else {
        return Vec::new();
    };
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

fn keyed_disc14_disc12_face_root_body(by_attr: &HashMap<u16, &EntityRecord>) -> Vec<BodyRecord> {
    let chains = keyed_forward_chain_candidates(by_attr);
    let matching_chains = chains
        .iter()
        .filter_map(|chain| {
            chain
                .windows(4)
                .find(|window| {
                    matches!(
                        window,
                        [first, second, third, fourth]
                            if first.disc == 0x0014
                                && first.flo() == 2
                                && second.disc == 0x0012
                                && second.flo() == 1
                                && third.disc == 0x0010
                                && third.flo() == 2
                                && fourth.disc == 0x0004
                                && fourth.flo() == 2
                    )
                })
                .map(|window| (chain, window))
        })
        .collect::<Vec<_>>();
    let [(chain, window)] = matching_chains.as_slice() else {
        return Vec::new();
    };
    let canonical_faces = by_attr
        .values()
        .copied()
        .filter(|record| record.disc == 0x000e && record.flo() == 1)
        .collect::<Vec<_>>();
    if canonical_faces.is_empty() {
        return Vec::new();
    }
    let is_companion = |record: &EntityRecord| {
        record.flo() == 1
            && matches!(
                record.disc,
                0x0010 | 0x0012 | 0x0014 | 0x0016 | 0x0018 | 0x001a | 0x001c | 0x001e
            )
    };
    let is_use_node = |record: &EntityRecord| {
        record.flo() == 4
            && matches!(
                record.disc,
                0x001a | 0x001c | 0x001e | 0x0020 | 0x0022 | 0x0024
            )
    };
    let has_companion_link =
        |record: &EntityRecord| record.refs.get(1).is_some_and(|attr| *attr > 1);
    let keyed_companion = |face: &EntityRecord| {
        let key = face.refs.first().copied().filter(|attr| *attr > 1)?;
        let candidates = by_attr
            .values()
            .copied()
            .filter(|record| {
                is_companion(record)
                    && record.refs.first() == Some(&key)
                    && has_companion_link(record)
            })
            .collect::<Vec<_>>();
        let [companion] = candidates.as_slice() else {
            return None;
        };
        Some(*companion)
    };
    let face_use = |face: &EntityRecord| {
        let linked = face.refs.get(1).and_then(|attr| by_attr.get(attr)).copied();
        if let Some(linked) = linked {
            if is_companion(linked) && has_companion_link(linked) {
                return Some(linked);
            }
            if linked.flo() == 2
                && matches!(linked.disc, 0x000e | 0x0014 | 0x001a | 0x001c)
                && linked.refs.get(2) == Some(&face.attr)
            {
                let companion = linked
                    .refs
                    .get(1)
                    .and_then(|attr| by_attr.get(attr))
                    .copied();
                if companion
                    .is_some_and(|record| is_companion(record) && has_companion_link(record))
                {
                    return companion;
                }
            }
        }
        keyed_companion(face)
    };
    let face_use_attrs = canonical_faces
        .iter()
        .filter_map(|face| face_use(face).map(|record| record.attr))
        .collect::<HashSet<_>>();
    let use_nodes = by_attr
        .values()
        .filter(|record| is_use_node(record))
        .count();
    if face_use_attrs.len() != canonical_faces.len() || use_nodes < canonical_faces.len() {
        return Vec::new();
    }
    let mut refs = by_attr.keys().copied().collect::<Vec<_>>();
    refs.sort_unstable();
    let root = chain[0];
    let shell = window[0];
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

fn keyed_forward_chain_candidates<'a>(
    by_attr: &HashMap<u16, &'a EntityRecord>,
) -> Vec<Vec<&'a EntityRecord>> {
    let mut candidates = Vec::new();
    for root in by_attr.values().copied().filter(|record| {
        record.flo() == 2
            && record.refs.first().is_some_and(|key| *key > 1)
            && record.refs.get(1).is_none_or(|attr| *attr <= 1)
            && record.refs.get(2).is_some_and(|attr| *attr > 1)
    }) {
        let key = root.refs[0];
        let mut chain = vec![root];
        let mut previous = root.attr;
        let mut next = root.refs[2];
        let mut seen = HashSet::from([root.attr]);
        let mut valid = true;
        while next > 1 {
            let Some(record) = by_attr.get(&next).copied() else {
                valid = false;
                break;
            };
            if !seen.insert(record.attr)
                || record.refs.first() != Some(&key)
                || record.refs.get(1) != Some(&previous)
            {
                valid = false;
                break;
            }
            chain.push(record);
            previous = record.attr;
            next = record.refs.get(2).copied().unwrap_or(0);
        }
        if valid && chain.len() >= 5 {
            candidates.push(chain);
        }
    }
    candidates
}

#[cfg(test)]
mod keyed_tests {
    use super::{keyed_disc14_disc12_face_root_body, EntityRecord};
    use std::collections::HashMap;

    fn record(attr: u16, disc: u16, flags: u32, refs: [u16; 6]) -> EntityRecord {
        EntityRecord {
            attr,
            flags,
            seq: u32::from(attr),
            disc,
            refs: refs.to_vec(),
            offset: usize::from(attr),
            end: usize::from(attr) + 26,
        }
    }

    fn index(records: &[EntityRecord]) -> HashMap<u16, &EntityRecord> {
        records.iter().map(|record| (record.attr, record)).collect()
    }

    #[test]
    fn disc14_disc12_keyed_lattice_owns_direct_intermediate_and_keyed_faces() {
        let mut records = vec![
            record(10, 0x1e, 2, [3, 1, 11, 1, 1, 1]),
            record(11, 0x1c, 2, [3, 10, 12, 1, 1, 1]),
            record(12, 0x18, 2, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, 2, [3, 12, 14, 1, 1, 1]),
            record(14, 0x12, 1, [3, 13, 15, 1, 1, 1]),
            record(15, 0x10, 2, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, 2, [3, 15, 1, 1, 1, 1]),
            record(20, 0x0e, 1, [1, 30, 1, 1, 1, 1]),
            record(30, 0x16, 1, [1, 40, 20, 1, 1, 1]),
            record(21, 0x0e, 1, [1, 31, 1, 1, 1, 1]),
            record(31, 0x0e, 2, [1, 32, 21, 1, 1, 1]),
            record(32, 0x1a, 1, [1, 41, 31, 1, 1, 1]),
            record(22, 0x0e, 1, [50, 99, 1, 1, 1, 1]),
            record(33, 0x1c, 1, [50, 42, 1, 1, 1, 1]),
            record(40, 0x1c, 4, [1; 6]),
            record(41, 0x1e, 4, [1; 6]),
            record(42, 0x22, 4, [1; 6]),
            record(60, 0x20, 2, [7, 1, 61, 1, 1, 1]),
            record(61, 0x1e, 2, [7, 60, 62, 1, 1, 1]),
            record(62, 0x1c, 2, [7, 61, 63, 1, 1, 1]),
            record(63, 0x18, 2, [7, 62, 64, 1, 1, 1]),
            record(64, 0x16, 2, [7, 63, 1, 1, 1, 1]),
        ];
        let bodies = keyed_disc14_disc12_face_root_body(&index(&records));
        let [body] = bodies.as_slice() else {
            panic!("one keyed disc14-disc12 body");
        };
        assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 13));
        assert!(body.refs.contains(&20) && body.refs.contains(&42));

        records[10].refs[2] = 1;
        assert!(keyed_disc14_disc12_face_root_body(&index(&records)).is_empty());
    }
}

#[cfg(test)]
mod linked_disc1c_disc16_disc14_tests {
    use super::{disc1c_disc16_disc14_linked_face_root_body, EntityRecord};
    use std::collections::HashMap;

    fn record(attr: u16, disc: u16, flags: u32, refs: [u16; 6]) -> EntityRecord {
        EntityRecord {
            attr,
            flags,
            seq: u32::from(attr),
            disc,
            refs: refs.to_vec(),
            offset: usize::from(attr),
            end: usize::from(attr) + 26,
        }
    }

    fn index(records: &[EntityRecord]) -> HashMap<u16, &EntityRecord> {
        records.iter().map(|record| (record.attr, record)).collect()
    }

    #[test]
    fn linked_lattice_owns_direct_and_intermediate_faces() {
        let records = vec![
            record(10, 0x1c, 2, [3, 1, 11, 1, 1, 1]),
            record(11, 0x18, 2, [3, 10, 12, 1, 1, 1]),
            record(12, 0x16, 2, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, 1, [3, 12, 14, 1, 1, 1]),
            record(14, 0x12, 2, [3, 13, 15, 1, 1, 1]),
            record(15, 0x04, 2, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, 1, [1, 30, 1, 1, 1, 1]),
            record(30, 0x1a, 1, [1, 40, 20, 1, 1, 1]),
            record(21, 0x0e, 1, [1, 31, 1, 1, 1, 1]),
            record(31, 0x10, 1, [1, 32, 21, 1, 1, 1]),
            record(32, 0x1a, 1, [1, 41, 31, 1, 1, 1]),
            record(40, 0x1e, 4, [1, 1, 30, 1, 1, 1]),
            record(41, 0x1e, 4, [1, 1, 32, 1, 1, 1]),
            record(50, 0x1e, 4, [1; 6]),
        ];
        let bodies = disc1c_disc16_disc14_linked_face_root_body(&index(&records));
        let [body] = bodies.as_slice() else {
            panic!("one linked disc1c-disc16-disc14 body");
        };
        assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 12));
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
        assert!(body.refs.contains(&50));
    }

    #[test]
    fn linked_lattice_rejects_nonreciprocal_face_use() {
        let mut records = vec![
            record(10, 0x1c, 2, [3, 1, 11, 1, 1, 1]),
            record(11, 0x18, 2, [3, 10, 12, 1, 1, 1]),
            record(12, 0x16, 2, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, 1, [3, 12, 14, 1, 1, 1]),
            record(14, 0x12, 2, [3, 13, 15, 1, 1, 1]),
            record(15, 0x04, 2, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, 1, [1, 30, 1, 1, 1, 1]),
            record(30, 0x1a, 1, [1, 40, 21, 1, 1, 1]),
            record(40, 0x1e, 4, [1, 1, 30, 1, 1, 1]),
        ];
        assert!(disc1c_disc16_disc14_linked_face_root_body(&index(&records)).is_empty());

        records[7].refs[2] = 20;
        records[8].refs[2] = 21;
        assert!(disc1c_disc16_disc14_linked_face_root_body(&index(&records)).is_empty());
    }
}

#[cfg(test)]
mod linked_disc1e_disc1c_disc14_tests {
    use super::{disc1e_disc1c_disc14_linked_face_root_body, EntityRecord};
    use std::collections::HashMap;

    fn record(attr: u16, disc: u16, flags: u32, refs: [u16; 6]) -> EntityRecord {
        EntityRecord {
            attr,
            flags,
            seq: u32::from(attr),
            disc,
            refs: refs.to_vec(),
            offset: usize::from(attr),
            end: usize::from(attr) + 26,
        }
    }

    fn index(records: &[EntityRecord]) -> HashMap<u16, &EntityRecord> {
        records.iter().map(|record| (record.attr, record)).collect()
    }

    #[test]
    fn linked_lattice_owns_direct_and_intermediate_faces() {
        let records = vec![
            record(10, 0x1e, 2, [3, 1, 11, 1, 1, 1]),
            record(11, 0x1c, 2, [3, 10, 12, 1, 1, 1]),
            record(12, 0x1a, 2, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, 1, [3, 12, 14, 1, 1, 1]),
            record(14, 0x12, 2, [3, 13, 15, 1, 1, 1]),
            record(15, 0x10, 2, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, 2, [3, 15, 1, 1, 1, 1]),
            record(20, 0x0e, 1, [1, 30, 1, 1, 1, 1]),
            record(30, 0x18, 1, [1, 40, 20, 1, 1, 1]),
            record(21, 0x0e, 1, [1, 31, 1, 1, 1, 1]),
            record(31, 0x16, 2, [1, 32, 21, 1, 1, 1]),
            record(32, 0x18, 1, [1, 41, 31, 1, 1, 1]),
            record(40, 0x20, 4, [1, 1, 30, 1, 1, 1]),
            record(41, 0x20, 4, [1, 1, 32, 1, 1, 1]),
            record(50, 0x20, 4, [1; 6]),
        ];
        let bodies = disc1e_disc1c_disc14_linked_face_root_body(&index(&records));
        let [body] = bodies.as_slice() else {
            panic!("one linked disc1e-disc1c-disc14 body");
        };
        assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 12));
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
        assert!(body.refs.contains(&50));
    }

    #[test]
    fn linked_lattice_rejects_nonreciprocal_face_use() {
        let mut records = vec![
            record(10, 0x1e, 2, [3, 1, 11, 1, 1, 1]),
            record(11, 0x1c, 2, [3, 10, 12, 1, 1, 1]),
            record(12, 0x1a, 2, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, 1, [3, 12, 14, 1, 1, 1]),
            record(14, 0x12, 2, [3, 13, 15, 1, 1, 1]),
            record(15, 0x10, 2, [3, 14, 16, 1, 1, 1]),
            record(16, 0x04, 2, [3, 15, 1, 1, 1, 1]),
            record(20, 0x0e, 1, [1, 30, 1, 1, 1, 1]),
            record(30, 0x18, 1, [1, 40, 21, 1, 1, 1]),
            record(40, 0x20, 4, [1, 1, 30, 1, 1, 1]),
        ];
        assert!(disc1e_disc1c_disc14_linked_face_root_body(&index(&records)).is_empty());

        records[8].refs[2] = 20;
        records[9].refs[2] = 21;
        assert!(disc1e_disc1c_disc14_linked_face_root_body(&index(&records)).is_empty());
    }
}

#[cfg(test)]
mod linked_disc04_disc12_tests {
    use super::{disc04_disc12_flo1_face_root_body, EntityRecord};
    use std::collections::HashMap;

    fn record(attr: u16, disc: u16, flags: u32, refs: [u16; 6]) -> EntityRecord {
        EntityRecord {
            attr,
            flags,
            seq: u32::from(attr),
            disc,
            refs: refs.to_vec(),
            offset: usize::from(attr),
            end: usize::from(attr) + 26,
        }
    }

    fn index(records: &[EntityRecord]) -> HashMap<u16, &EntityRecord> {
        records.iter().map(|record| (record.attr, record)).collect()
    }

    #[test]
    fn linked_lattice_owns_the_disc12_flo1_site() {
        let records = vec![
            record(10, 0x04, 2, [3, 11, 1, 1, 1, 1]),
            record(11, 0x10, 2, [3, 12, 10, 1, 1, 1]),
            record(12, 0x12, 1, [3, 13, 11, 1, 1, 1]),
            record(13, 0x1a, 2, [3, 14, 12, 1, 1, 1]),
            record(14, 0x1c, 2, [3, 15, 13, 1, 1, 1]),
            record(15, 0x1e, 2, [3, 1, 14, 1, 1, 1]),
            record(20, 0x0e, 1, [1; 6]),
            record(21, 0x0e, 1, [1; 6]),
            record(30, 0x18, 1, [1, 31, 1, 1, 1, 1]),
            record(31, 0x18, 1, [1; 6]),
            record(40, 0x20, 4, [1, 1, 30, 1, 1, 1]),
            record(41, 0x20, 4, [1, 1, 31, 1, 1, 1]),
        ];
        let bodies = disc04_disc12_flo1_face_root_body(&index(&records));
        let [body] = bodies.as_slice() else {
            panic!("one disc04-disc12-flo1 body");
        };
        assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 11));
        assert!(body.refs.contains(&20) && body.refs.contains(&41));
    }

    #[test]
    fn linked_lattice_rejects_a_broken_backlink() {
        let mut records = vec![
            record(10, 0x04, 2, [3, 11, 1, 1, 1, 1]),
            record(11, 0x10, 2, [3, 12, 10, 1, 1, 1]),
            record(12, 0x12, 1, [3, 13, 11, 1, 1, 1]),
            record(13, 0x1a, 2, [3, 14, 12, 1, 1, 1]),
            record(14, 0x1c, 2, [3, 15, 13, 1, 1, 1]),
            record(15, 0x1e, 2, [3, 1, 14, 1, 1, 1]),
            record(20, 0x0e, 1, [1; 6]),
            record(30, 0x18, 1, [1; 6]),
            record(40, 0x20, 4, [1, 1, 30, 1, 1, 1]),
        ];
        records[4].refs[2] = 99;
        assert!(disc04_disc12_flo1_face_root_body(&index(&records)).is_empty());
    }
}

#[cfg(test)]
mod disc22_disc04_tests {
    use super::{disc22_disc04_face_root_body, EntityRecord};
    use std::collections::HashMap;

    fn record(attr: u16, disc: u16, flags: u32, refs: [u16; 6]) -> EntityRecord {
        EntityRecord {
            attr,
            flags,
            seq: u32::from(attr),
            disc,
            refs: refs.to_vec(),
            offset: usize::from(attr),
            end: usize::from(attr) + 26,
        }
    }

    fn index(records: &[EntityRecord]) -> HashMap<u16, &EntityRecord> {
        records.iter().map(|record| (record.attr, record)).collect()
    }

    fn lattice() -> Vec<EntityRecord> {
        vec![
            record(10, 0x22, 2, [3, 1, 11, 1, 1, 1]),
            record(11, 0x1e, 2, [3, 10, 12, 1, 1, 1]),
            record(12, 0x1c, 2, [3, 11, 13, 1, 1, 1]),
            record(13, 0x18, 2, [3, 12, 14, 1, 1, 1]),
            record(14, 0x16, 2, [3, 13, 15, 1, 1, 1]),
            record(15, 0x14, 2, [3, 14, 1, 1, 1, 1]),
            record(20, 0x04, 1, [1, 30, 1, 1, 1, 1]),
            record(30, 0x1a, 1, [1, 40, 20, 1, 1, 1]),
            record(40, 0x20, 4, [1, 1, 30, 1, 1, 1]),
        ]
    }

    #[test]
    fn reciprocal_lattice_owns_the_disc04_site() {
        let records = lattice();
        let bodies = disc22_disc04_face_root_body(&index(&records));
        let [body] = bodies.as_slice() else {
            panic!("one disc22-disc04 body");
        };
        assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 14));
        assert!(body.refs.contains(&20) && body.refs.contains(&40));
    }

    #[test]
    fn reciprocal_lattice_rejects_a_broken_backlink() {
        let mut records = lattice();
        records[1].refs[1] = 99;
        assert!(disc22_disc04_face_root_body(&index(&records)).is_empty());

        let mut records = lattice();
        records[7].refs[2] = 21;
        assert!(disc22_disc04_face_root_body(&index(&records)).is_empty());
    }
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
    for region in regions {
        let shells = reachable_records(by_attr, region, 0x0016)
            .into_iter()
            .filter_map(|shell| {
                let face_attrs = shell_face_ring(by_attr, shell)?;
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
    region_records.sort_by_key(|region| (region.attr, region.offset));
    region_records
        .into_iter()
        .map(|region| {
            let mut refs = region
                .shells
                .iter()
                .flat_map(|shell| shell.refs.iter().copied())
                .collect::<Vec<_>>();
            refs.sort_unstable();
            refs.dedup();
            BodyRecord {
                attr: region.attr,
                kind: BodyKind::Solid,
                refs,
                offset: region.offset,
                regions: vec![region],
            }
        })
        .collect()
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

fn bind_schema_33103_faces(entities: &[EntityRecord], bodies: &mut [BodyRecord]) -> usize {
    let faces = entities
        .iter()
        .filter(|record| record.disc == 0x0015 && record.flo() == 1)
        .collect::<Vec<_>>();
    let face_attrs = faces
        .iter()
        .map(|record| record.attr)
        .collect::<HashSet<_>>();
    if face_attrs.is_empty() {
        return 0;
    }

    let by_attr = faces
        .iter()
        .map(|record| (record.attr, *record))
        .collect::<HashMap<_, _>>();
    let mut unseen = face_attrs.clone();
    let mut components = Vec::new();
    while let Some(start) = unseen.iter().min().copied() {
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
    let mut ambiguous = 0;
    for (index, head) in heads.iter().enumerate() {
        let Some(cluster) = head.refs.first() else {
            continue;
        };
        if *cluster <= 1 {
            continue;
        }
        let Some(body_index) = bodies.iter().position(|body| {
            entities
                .iter()
                .any(|record| record.attr == body.attr && record.refs.first() == Some(cluster))
        }) else {
            continue;
        };
        let interval_end = heads.get(index + 1).map_or(usize::MAX, |next| next.offset);
        let candidates = components
            .iter()
            .enumerate()
            .filter(|(component_index, _)| !assigned.contains(component_index))
            .map(|(component_index, component)| {
                let overlap = component
                    .iter()
                    .filter_map(|attr| by_attr.get(attr))
                    .filter(|face| face.offset >= head.offset && face.offset < interval_end)
                    .count();
                (component_index, overlap)
            })
            .collect::<Vec<_>>();
        let Some(max_overlap) = candidates.iter().map(|(_, overlap)| *overlap).max() else {
            continue;
        };
        if max_overlap == 0 {
            continue;
        }
        let best = candidates
            .iter()
            .filter(|(_, overlap)| *overlap == max_overlap)
            .collect::<Vec<_>>();
        let [candidate] = best.as_slice() else {
            ambiguous += 1;
            continue;
        };
        let component_index = candidate.0;
        let component = &components[component_index];
        assigned.insert(component_index);
        let body = &mut bodies[body_index];
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
    ambiguous
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
    mod disc1a_disc18_disc14_disc12;
    mod disc1a_linked;
    mod disc1c_disc14_linked;
    mod disc1c_disc16_disc0e;
    mod disc1e_disc04;
    mod disc1e_disc1c_disc1a_disc16_disc14_disc12;
    mod disc20_disc12_disc1e_disc1c_disc18;
    mod disc20_disc18;
    mod disc20_disc1a_disc18;
    mod disc20_disc1c_disc1a_disc16_disc12_disc10_disc0e;
    mod disc20_disc1c_disc1a_disc16_disc14_disc12_disc10_disc04;
    mod disc20_disc1e_disc1c;
    mod disc20_disc1e_disc1c_disc14_disc12_disc10_disc04;
    mod disc20_disc1e_disc1c_disc16_disc14_disc10_disc04;
    mod disc20_disc1e_disc1c_disc16_disc14_disc12;
    mod disc20_disc1e_disc1c_disc16_disc14_disc12_disc10_disc04;
    mod disc20_disc1e_disc1c_disc18_disc16;
    mod disc20_disc1e_disc1c_disc18_disc16_disc10;
    mod disc20_disc1e_disc1c_disc18_disc16_disc12;
    mod disc22_disc20_disc1e_disc1a;
    mod merged_stream_bodies;
    const TEST_SCHEMA: &str = "SCH_SW_33103_11000";
    fn bare_entity(attr: u16, seq: u32, disc: u16, refs: [u16; 6]) -> Vec<u8> {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&seq.to_be_bytes());
        bytes.extend_from_slice(&disc.to_be_bytes());
        for reference in refs {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes
    }
    fn bare_entity_slots(attr: u16, seq: u32, disc: u16, flo: u8, refs: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&u32::from(flo).to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&seq.to_be_bytes());
        bytes.extend_from_slice(&disc.to_be_bytes());
        for reference in refs {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes
    }
    fn prefixed_entity(attr: u16, seq: u32, disc: u16, refs: [u16; 6]) -> Vec<u8> {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&seq.to_be_bytes());
        bytes.extend_from_slice(&disc.to_be_bytes());
        for reference in refs {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0);
        bytes
    }
    fn color(attr: u16, rgb: [f64; 3], prefixed: bool) -> Vec<u8> {
        let mut bytes = vec![0, 0x53];
        if prefixed {
            bytes.push(0xff);
        }
        bytes.extend_from_slice(&3_u32.to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        for channel in rgb {
            bytes.extend_from_slice(&channel.to_be_bytes());
        }
        bytes
    }
    fn record(attr: u16, disc: u16, refs: [u16; 6]) -> EntityRecord {
        EntityRecord {
            attr,
            flags: 1,
            seq: u32::from(attr),
            disc,
            refs: refs.to_vec(),
            offset: usize::from(attr),
            end: usize::from(attr) + 26,
        }
    }
    #[test]
    fn schema_33103_tied_component_overlap_remains_unassigned() {
        let mut head = record(20, 0x13, [7, 1, 1, 1, 1, 1]);
        head.flags = 2;
        let entities = vec![
            record(10, 0x17, [7, 1, 1, 1, 1, 1]),
            head,
            record(100, 0x15, [101, 1, 1, 1, 1, 1]),
            record(101, 0x15, [100, 1, 1, 1, 1, 1]),
            record(200, 0x15, [201, 1, 1, 1, 1, 1]),
            record(201, 0x15, [200, 1, 1, 1, 1, 1]),
        ];
        let mut bodies = vec![BodyRecord {
            attr: 10,
            kind: BodyKind::Solid,
            refs: vec![10],
            offset: 10,
            regions: vec![RegionRecord {
                attr: 10,
                offset: 10,
                shells: vec![ShellRecord {
                    attr: 10,
                    offset: 10,
                    refs: vec![10],
                }],
            }],
        }];
        assert_eq!(bind_schema_33103_faces(&entities, &mut bodies), 1);
        assert_eq!(bodies[0].refs, [10]);
        assert_eq!(bodies[0].regions[0].shells[0].refs, [10]);
    }
    #[test]
    fn disc14_regions_form_distinct_stored_bodies() {
        let records = [
            record(90, 0x001a, [500, 1, 1, 1, 1, 1]),
            record(10, 0x001a, [400, 1, 1, 1, 1, 1]),
            record(500, 0x0016, [550, 1, 1, 1, 1, 1]),
            record(400, 0x0016, [450, 1, 1, 1, 1, 1]),
            record(550, 0x0020, [1, 1, 700, 550, 1, 1]),
            record(450, 0x0020, [1, 1, 800, 450, 1, 1]),
            record(700, 0x0014, [1; 6]),
            record(800, 0x0014, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        let bodies = disc14_bodies(&by_attr);
        assert_eq!(bodies.len(), 2);
        assert_eq!(
            bodies
                .iter()
                .map(|body| (body.attr, body.refs.clone()))
                .collect::<Vec<_>>(),
            vec![(10, vec![800]), (90, vec![700])]
        );
        assert_eq!(bodies[0].regions.len(), 1);
        assert_eq!(bodies[0].regions[0].attr, 10);
        assert_eq!(bodies[0].regions[0].shells[0].attr, 400);
        assert_eq!(bodies[1].regions.len(), 1);
        assert_eq!(bodies[1].regions[0].attr, 90);
        assert_eq!(bodies[1].regions[0].shells[0].attr, 500);
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
    fn disc1c_disc12_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            record(13, 0x12, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            flo4(30, 0x1e, [1; 6]),
            flo4(31, 0x1e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1c_disc12_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1c-disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1c_disc16_disc12_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            record(13, 0x12, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x14, [1; 6]),
            record(31, 0x14, [1; 6]),
            flo4(40, 0x1a, [1; 6]),
            flo4(41, 0x1a, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1c_disc16_disc12_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1c-disc16-disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 11);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1c_disc16_disc14_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x10, [1; 6]),
            record(31, 0x10, [1; 6]),
            record(40, 0x1a, [1; 6]),
            record(41, 0x1a, [1; 6]),
            flo4(50, 0x1e, [1; 6]),
            flo4(51, 0x1e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1c_disc16_disc14_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1c-disc16-disc14-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1c_disc14_disc12_face_root_lattice_owns_both_face_families() {
        let mut records = vec![
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x14, [3, 12, 14, 1, 1, 1]),
            record(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x0e, [1, 60, 1, 1, 1, 1]),
            record(30, 0x16, [1, 40, 1, 1, 1, 1]),
            flo2(60, 0x14, [1, 30, 20, 1, 1, 1]),
            flo4(40, 0x1e, [1; 6]),
            flo4(42, 0x1e, [1; 6]),
        ];
        let by_attr = index_records(&records);

        let bodies = disc1c_disc14_disc12_face_root_body(&by_attr, &records);
        let body = bodies.first().expect("body");
        assert_eq!((body.attr, body.regions[0].shells[0].attr), (10, 12));
        assert!(body.refs.contains(&20) && body.refs.contains(&42));

        records[7].disc = 0x000c;
        records[9].disc = 0x000e;
        let alternate = index_records(&records);
        assert!(!disc1c_disc14_disc12_face_root_body(&alternate, &records).is_empty());

        let mut invalid_records = records.clone();
        invalid_records[9].refs[2] = 1;
        let invalid = index_records(&invalid_records);
        assert!(disc1c_disc14_disc12_face_root_body(&invalid, &invalid_records).is_empty());
    }

    #[test]
    fn disc1c_disc12_terminal_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x10, [1; 6]),
            record(31, 0x10, [1; 6]),
            record(40, 0x16, [1; 6]),
            record(41, 0x16, [1; 6]),
            flo4(50, 0x1e, [1; 6]),
            flo4(51, 0x1e, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1c_disc12_terminal_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1c-disc12-terminal-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1c_disc0c_terminal_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1c, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            record(13, 0x12, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x0e, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x0c, [3, 15, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
            record(20, 0x0f, [1; 6]),
            record(30, 0x14, [1; 6]),
            record(31, 0x14, [1; 6]),
            flo4(40, 0x1a, [1; 6]),
            flo4(41, 0x1a, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1c_disc0c_terminal_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc1c-disc0c-terminal-face-root body");
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
    fn disc20_root_lattice_accepts_the_direct_disc16_shell_branch() {
        let mut records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            record(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x10, [3, 15, 1, 1, 1, 1]),
        ];
        for index in 0..2 {
            records.extend([
                record(20 + index, 0x04, [1; 6]),
                flo2(30 + index, 0x0e, [1; 6]),
                record(40 + index, 0x1a, [1; 6]),
                flo4(50 + index, 0x22, [1; 6]),
            ]);
        }
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc20_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one direct-disc16 disc20-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&51));
    }

    #[test]
    fn disc20_disc04_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            record(13, 0x18, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x16, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x14, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x12, [3, 15, 17, 1, 1, 1]),
            flo2(17, 0x04, [3, 16, 1, 1, 1, 1]),
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

        let bodies = disc20_disc04_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc20-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 14);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }
    #[test]
    fn disc20_disc1a_disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            record(13, 0x18, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x16, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x14, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x12, [3, 15, 17, 1, 1, 1]),
            flo2(17, 0x04, [3, 16, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x10, [1; 6]),
            record(31, 0x10, [1; 6]),
            record(40, 0x1e, [1; 6]),
            record(41, 0x1e, [1; 6]),
            flo4(50, 0x22, [1, 1, 40, 1, 1, 1]),
            flo4(51, 0x22, [1, 1, 41, 1, 1, 1]),
            flo4(52, 0x22, [1; 6]),
            flo4(53, 0x22, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc20_disc1a_disc04_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc20-disc1a-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 14);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
        assert!(body.refs.contains(&50) && body.refs.contains(&51));
    }
    #[test]
    fn disc20_disc18_disc12_face_root_lattice_owns_the_site() {
        let mut records = vec![
            flo2(10, 0x20, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1e, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1c, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x18, [3, 12, 14, 1, 1, 1]),
            record(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
        ];
        for index in 0..2 {
            records.extend([
                record(20 + index, 0x0e, [1; 6]),
                record(30 + index, 0x1a, [1; 6]),
                flo4(40 + index, 0x22, [1; 6]),
            ]);
        }
        records.push(record(20, 0x0f, [1; 6]));
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc20_disc18_disc12_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc20-disc18-disc12-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
        assert!(body.refs.contains(&40) && body.refs.contains(&41));

        let mismatched_records = records
            .iter()
            .filter(|record| record.attr != 41)
            .cloned()
            .collect::<Vec<_>>();
        let mismatched = mismatched_records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert!(disc20_disc18_disc12_face_root_body(&mismatched, &mismatched_records).is_empty());
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
    fn disc18_disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x18, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x14, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x12, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x10, [3, 12, 14, 1, 1, 1]),
            record(14, 0x0e, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0c, [1; 6]),
            record(21, 0x0c, [1; 6]),
            record(20, 0x0f, [1; 6]),
            record(30, 0x16, [1; 6]),
            record(31, 0x16, [1; 6]),
            flo4(40, 0x1a, [1; 6]),
            flo4(41, 0x1a, [1; 6]),
            record(50, 0x10, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc18_disc04_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc18-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 11);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
        assert!(body.refs.contains(&30) && body.refs.contains(&31));
    }

    #[test]
    fn disc18_disc0e_disc04_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x18, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x16, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x14, [3, 11, 13, 1, 1, 1]),
            record(13, 0x10, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x0e, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0c, [1; 6]),
            record(21, 0x0c, [1; 6]),
            record(30, 0x12, [1; 6]),
            record(31, 0x12, [1; 6]),
            flo4(40, 0x1a, [1; 6]),
            flo4(41, 0x1a, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc18_disc0e_disc04_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc18-disc0e-disc04-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 11);
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
    fn disc12_direct_disc0e_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            record(13, 0x12, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x14, [1; 6]),
            record(31, 0x14, [1; 6]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc12_direct_disc0e_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc12-direct-disc0e-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc12_disc0e_flo1_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1a, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x18, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x16, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x12, [3, 12, 14, 1, 1, 1]),
            record(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x14, [1; 6]),
            record(31, 0x14, [1; 6]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc12_disc0e_flo1_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc12-disc0e-flo1-face-root body");
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
    fn disc1e_direct_disc0e_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x16, [1; 6]),
            record(31, 0x16, [1; 6]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_direct_disc0e_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-direct-disc0e-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_direct_disc0e_auxiliary_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x04, [3, 15, 1, 1, 1, 1]),
            record(20, 0x0e, [1, 30, 1, 1, 1, 1]),
            record(21, 0x0e, [1, 31, 1, 1, 1, 1]),
            record(30, 0x16, [1, 40, 1, 1, 1, 1]),
            record(31, 0x16, [1, 41, 1, 1, 1, 1]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
            flo4(42, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_direct_disc0e_auxiliary_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-direct-disc0e-auxiliary-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));

        let mut invalid_records = records.clone();
        invalid_records
            .iter_mut()
            .find(|record| record.attr == 30)
            .expect("face-use record")
            .refs[1] = 1;
        let invalid = invalid_records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert!(
            disc1e_direct_disc0e_auxiliary_face_root_body(&invalid, &invalid_records).is_empty()
        );
    }

    #[test]
    fn disc1e_disc12_flo1_face_root_lattice_owns_the_site() {
        let mut records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            flo2(13, 0x12, [3, 12, 14, 1, 1, 1]),
            record(14, 0x10, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
        ];
        for index in 0..2 {
            records.extend([
                record(20 + index, 0x0e, [1; 6]),
                record(30 + index, 0x18, [1; 6]),
                flo4(40 + index, 0x20, [1; 6]),
            ]);
        }
        records.extend([flo2(50, 0x16, [1; 6]), flo2(51, 0x14, [1; 6])]);
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc12_flo1_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc12-flo1-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
        assert!(body.refs.contains(&40) && body.refs.contains(&41));

        let mismatched_records = records
            .iter()
            .filter(|record| record.attr != 41)
            .cloned()
            .collect::<Vec<_>>();
        let mismatched = mismatched_records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert!(disc1e_disc12_flo1_face_root_body(&mismatched, &mismatched_records).is_empty());
    }

    #[test]
    fn disc1e_disc1c_disc14_face_root_lattice_owns_the_site() {
        let mut records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
        ];
        for index in 0..2 {
            records.extend([
                record(20 + index, 0x0e, [1; 6]),
                record(30 + index, 0x10, [1; 6]),
                record(40 + index, 0x18, [1; 6]),
                flo4(50 + index, 0x20, [1; 6]),
            ]);
        }
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc1c_disc14_face_root_body(&by_attr, &records);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc1c-disc14-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
        assert!(body.refs.contains(&50) && body.refs.contains(&51));

        let mismatched_records = records
            .iter()
            .filter(|record| record.attr != 51)
            .cloned()
            .collect::<Vec<_>>();
        let mismatched = mismatched_records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();
        assert!(disc1e_disc1c_disc14_face_root_body(&mismatched, &mismatched_records).is_empty());
    }

    #[test]
    fn disc1e_disc12_terminal_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x16, [1; 6]),
            record(31, 0x16, [1; 6]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc12_terminal_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc12-terminal-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 12);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_disc12_disc0e_face_root_lattice_owns_the_site() {
        let records = vec![
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            record(13, 0x14, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x12, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x10, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x0e, [3, 15, 1, 1, 1, 1]),
            record(20, 0x04, [1; 6]),
            record(21, 0x04, [1; 6]),
            record(30, 0x16, [1; 6]),
            record(31, 0x16, [1; 6]),
            flo4(40, 0x1c, [1; 6]),
            flo4(41, 0x1c, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc12_disc0e_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc12-disc0e-face-root body");
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
    fn disc1e_disc04_terminal_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1c, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x1a, [3, 11, 13, 1, 1, 1]),
            record(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x12, [3, 14, 16, 1, 1, 1]),
            flo2(16, 0x10, [3, 15, 17, 1, 1, 1]),
            flo2(17, 0x04, [3, 16, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x18, [1; 6]),
            record(31, 0x18, [1; 6]),
            flo4(40, 0x20, [1; 6]),
            flo4(41, 0x20, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc04_terminal_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc04-terminal-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
        assert!(body.refs.contains(&20) && body.refs.contains(&21));
    }

    #[test]
    fn disc1e_disc14_terminal_face_root_lattice_owns_the_site() {
        let records = [
            flo2(10, 0x1e, [3, 1, 11, 1, 1, 1]),
            flo2(11, 0x1a, [3, 10, 12, 1, 1, 1]),
            flo2(12, 0x18, [3, 11, 13, 1, 1, 1]),
            record(13, 0x16, [3, 12, 14, 1, 1, 1]),
            flo2(14, 0x14, [3, 13, 15, 1, 1, 1]),
            flo2(15, 0x04, [3, 14, 1, 1, 1, 1]),
            record(20, 0x0e, [1; 6]),
            record(21, 0x0e, [1; 6]),
            record(30, 0x10, [1; 6]),
            record(31, 0x10, [1; 6]),
            record(40, 0x1c, [1; 6]),
            record(41, 0x1c, [1; 6]),
            flo4(50, 0x20, [1; 6]),
            flo4(51, 0x20, [1; 6]),
        ];
        let by_attr = records
            .iter()
            .map(|record| (record.attr, record))
            .collect::<HashMap<_, _>>();

        let bodies = disc1e_disc14_terminal_face_root_body(&by_attr);
        let [body] = bodies.as_slice() else {
            panic!("one disc1e-disc14-terminal-face-root body");
        };
        assert_eq!(body.attr, 10);
        assert_eq!(body.regions[0].shells[0].attr, 13);
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

    fn index_records(records: &[EntityRecord]) -> HashMap<u16, &EntityRecord> {
        records.iter().map(|record| (record.attr, record)).collect()
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

    fn class_root_index(attrs: &[u16]) -> Vec<u8> {
        let mut bytes = CLASS_ROOT_INDEX_PREFIX.to_vec();
        bytes.extend_from_slice(&0x0042_u16.to_be_bytes());
        bytes.extend_from_slice(&(attrs.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 1]);
        for attr in attrs {
            bytes.extend_from_slice(&attr.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn class_root_index_requires_one_complete_distinct_vector() {
        let entity_attrs = HashSet::from([5, 32, 36, 100, 132, 136]);
        let bytes = class_root_index(&[5, 32, 36]);
        assert_eq!(
            class_root_attrs(&bytes, &entity_attrs),
            Some(HashSet::from([5, 32, 36]))
        );

        let mut truncated = bytes.clone();
        truncated.pop();
        assert_eq!(class_root_attrs(&truncated, &entity_attrs), None);

        truncated.extend(class_root_index(&[5, 32, 36]));
        assert_eq!(
            class_root_attrs(&truncated, &entity_attrs),
            Some(HashSet::from([5, 32, 36]))
        );

        let mut ambiguous = bytes;
        ambiguous.extend(class_root_index(&[100, 132, 136]));
        assert_eq!(class_root_attrs(&ambiguous, &entity_attrs), None);

        let unknown_root = class_root_index(&[5, 32, 200]);
        assert_eq!(class_root_attrs(&unknown_root, &entity_attrs), None);
    }

    #[test]
    fn prefixed_entity_refs_end_at_the_zero_terminator() {
        let mut bytes = Vec::new();
        for reference in 2_u16..=8 {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0);

        assert_eq!(refs(&bytes, 0, 6, true), Some(((2_u16..=8).collect(), 22)));
    }

    #[test]
    fn face_color_requires_the_adjacent_record_boundary() {
        let mut bytes = bare_entity(700, 1, 0x15, [0, 0, 0, 0, 0, 900]);
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan(&bytes, TEST_SCHEMA);

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn prefixed_face_color_uses_the_terminated_face_boundary() {
        let mut bytes = prefixed_entity(700, 4, 0x15, [0, 0, 0, 0, 0, 900]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], true));

        let facts = scan_deltas(&bytes, TEST_SCHEMA);

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
        assert_eq!(facts.face_colors[0].face_seq, 4);
    }

    #[test]
    fn unrelated_adjacent_color_does_not_replace_the_referenced_color() {
        let mut bytes = bare_entity(700, 1, 0x15, [0, 0, 0, 0, 0, 900]);
        bytes.extend(color(901, [0.25, 0.5, 0.75], false));

        let facts = scan(&bytes, TEST_SCHEMA);

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn referenced_color_uses_a_framed_inline_face_link() {
        let mut bytes = bare_entity(700, 1, 0x15, [0, 0, 0, 0, 0, 900]);
        bytes.extend(bare_entity(701, 2, 0x15, [0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan(&bytes, TEST_SCHEMA);

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
    }

    #[test]
    fn inline_face_link_frames_a_contiguous_color_run() {
        let mut bytes = bare_entity(700, 1, 0x15, [0, 0, 0, 0, 0, 900]);
        bytes.extend(bare_entity(701, 2, 0x15, [0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));
        bytes.extend(color(901, [0.75, 0.5, 0.25], false));

        let facts = scan(&bytes, TEST_SCHEMA);

        assert_eq!(facts.face_colors.len(), 2);
        assert!(facts
            .face_colors
            .iter()
            .any(|color| color.face_attr == 700 && color.color_attr == 900));
        assert!(facts
            .face_colors
            .iter()
            .any(|color| color.face_attr == 701 && color.color_attr == 901));
    }

    #[test]
    fn unknown_bare_entity_families_do_not_invent_six_reference_slots() {
        let mut header = vec![0x00, 0x51];
        header.extend(3u32.to_be_bytes());
        header.extend(2u16.to_be_bytes());
        header.extend(1u32.to_be_bytes());
        header.extend(0x9999u16.to_be_bytes());

        let mut bare = header.clone();
        for reference in 2u16..=7 {
            bare.extend(reference.to_be_bytes());
        }
        bare.extend([0; 16]);
        assert!(scan_entities(&bare, TEST_SCHEMA, false).is_empty());

        let mut prefixed = header;
        for reference in 2u16..=8 {
            prefixed.push(1);
            prefixed.extend(reference.to_be_bytes());
        }
        prefixed.push(0);
        prefixed.extend([0; 16]);
        assert_eq!(
            scan_entities(&prefixed, "SCH_UNKNOWN_99999", true)[0].refs,
            (2u16..=8).collect::<Vec<_>>()
        );
    }

    #[test]
    fn bare_entity_slot_counts_use_schema_disc_and_flo() {
        let seven = bare_entity_slots(700, 1, 0x16, 2, &[2, 3, 4, 5, 6, 7, 8]);
        let nine = bare_entity_slots(701, 2, 0x1a, 4, &[9, 10, 11, 12, 13, 14, 15, 16, 17]);
        let mut bytes = seven.clone();
        bytes.extend_from_slice(&nine);
        bytes.extend([0; 16]);

        let records = scan_entities(&bytes, "SCH_2400201_20000_13006", false);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].refs.len(), 7);
        assert_eq!(records[0].end, seven.len());
        assert_eq!(records[1].refs.len(), 9);
        assert_eq!(records[1].end, seven.len() + nine.len());
        assert!(scan_entities(&bytes, "SCH_UNKNOWN_99999_13006", false).is_empty());
        assert_eq!(slot_count(TEST_SCHEMA, 0x26, 3), Some(6));
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

        let bodies = cluster_chain_bodies(&records, None);
        let [first, second] = bodies.as_slice() else {
            panic!("two chain bodies, got {bodies:?}");
        };
        assert_eq!(first.attr, 32);
        assert_eq!(second.attr, 132);
        assert!(first.refs.contains(&57) && !first.refs.contains(&120));
        assert!(second.refs.contains(&120) && !second.refs.contains(&57));
        assert_eq!(first.regions[0].shells[0].attr, 32);
        assert_eq!(second.regions[0].shells[0].attr, 132);

        let selected_heads = HashSet::from([5, 32, 36]);
        let selected = cluster_chain_bodies(&records, Some(&selected_heads));
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].attr, 32);

        let selected_heads = HashSet::from([100]);
        let selected = cluster_chain_bodies(&records, Some(&selected_heads));
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].attr, 132);
        assert!(selected[0].refs.contains(&120) && !selected[0].refs.contains(&57));

        let broken = vec![
            flo2(5, 0x04, [3, 32, 1, 1, 1, 1]),
            flo2(32, 0x0f, [4, 36, 5, 1, 1, 1]),
        ];
        assert!(cluster_chain_bodies(&broken, None).is_empty());
    }
}
