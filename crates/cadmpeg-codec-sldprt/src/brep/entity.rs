// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity metadata records.

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::Color;
use std::collections::{HashMap, HashSet};

use crate::layout::entity_common_header as entity_hdr;

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
    seq: u32,
    disc: u16,
    refs: Vec<u16>,
    end: usize,
}

/// Number of u16 fields in one bare XT ATTRIBUTE node.
///
/// The low flag byte is the value count. Every attribute has five pointer
/// fields before its values, regardless of the definition node it references.
fn attribute_slot_count(flo: u8) -> Option<usize> {
    (1..=0x20).contains(&flo).then_some(5 + usize::from(flo))
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
    let mut refs = Vec::with_capacity(count);
    for index in 0..count {
        let cell = at.checked_add(index.checked_mul(2)?)?;
        refs.push(View::u16_be_at(body, cell)?);
    }
    Some((refs, at.checked_add(count.checked_mul(2)?)?))
}

fn scan_entities(body: &[u8], prefixed: bool) -> Vec<EntityRecord> {
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
            let Some(count) = attribute_slot_count(flo) else {
                continue;
            };
            count
        };
        let Some((refs, end)) = refs(body, p + entity_hdr::LEN, count, prefixed) else {
            continue;
        };
        out.push(EntityRecord {
            attr,
            seq,
            disc,
            refs,
            end,
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

/// Scan non-topology entity metadata without interpreting attribute chains as
/// body membership. Typed Parasolid BODY/SHELL/REGION nodes own that relation.
pub(crate) fn scan_metadata(body: &[u8], prefixed: bool) -> Facts {
    let entities = scan_entities(body, prefixed);
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
    Facts {
        entity_count: entities.len(),
        unresolved_face_colors,
        face_color_versions,
        face_colors,
        face_atoms: super::attrib::scan(body),
        body_modifiers: super::attrib::scan_body_modifiers(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare_entity(attr: u16, seq: u32, disc: u16, refs: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&seq.to_be_bytes());
        bytes.extend_from_slice(&disc.to_be_bytes());
        bytes.extend(refs.iter().flat_map(|reference| reference.to_be_bytes()));
        bytes
    }

    fn bare_entity_slots(attr: u16, seq: u32, disc: u16, flo: u8, refs: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&u32::from(flo).to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&seq.to_be_bytes());
        bytes.extend_from_slice(&disc.to_be_bytes());
        bytes.extend(refs.iter().flat_map(|reference| reference.to_be_bytes()));
        bytes
    }

    fn prefixed_entity(attr: u16, seq: u32, disc: u16, refs: &[u16]) -> Vec<u8> {
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
        let mut bytes = bare_entity(700, 1, 0x15, &[0, 0, 0, 0, 0, 900]);
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn prefixed_face_color_uses_the_terminated_face_boundary() {
        let mut bytes = prefixed_entity(700, 4, 0x15, &[0, 0, 0, 0, 0, 900]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], true));

        let facts = scan_metadata(&bytes, true);

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
        assert_eq!(facts.face_colors[0].face_seq, 4);
    }

    #[test]
    fn unrelated_adjacent_color_does_not_replace_the_referenced_color() {
        let mut bytes = bare_entity(700, 1, 0x15, &[0, 0, 0, 0, 0, 900]);
        bytes.extend(color(901, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn referenced_color_uses_a_framed_inline_face_link() {
        let mut bytes = bare_entity(700, 1, 0x15, &[0, 0, 0, 0, 0, 900]);
        bytes.extend(bare_entity(701, 2, 0x15, &[0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
    }

    #[test]
    fn inline_face_link_frames_a_contiguous_color_run() {
        let mut bytes = bare_entity(700, 1, 0x15, &[0, 0, 0, 0, 0, 900]);
        bytes.extend(bare_entity(701, 2, 0x15, &[0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));
        bytes.extend(color(901, [0.75, 0.5, 0.25], false));

        let facts = scan_metadata(&bytes, false);

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
    fn bare_attribute_slot_counts_use_flo_only() {
        let seven = bare_entity_slots(700, 1, 0x9999, 2, &[2, 3, 4, 5, 6, 7, 8]);
        let nine = bare_entity_slots(701, 2, 0x1a, 4, &[9, 10, 11, 12, 13, 14, 15, 16, 17]);
        let terminal = bare_entity_slots(702, 3, 0x06, 2, &[18, 19, 1, 1, 1, 1, 1]);
        let ten = bare_entity_slots(703, 4, 0x1c, 5, &[20, 21, 22, 1, 1, 23, 1, 1, 1, 1]);
        let mut bytes = seven.clone();
        bytes.extend_from_slice(&nine);
        bytes.extend_from_slice(&terminal);
        bytes.extend_from_slice(&ten);
        bytes.extend([0; 16]);

        let records = scan_entities(&bytes, false);
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].refs.len(), 7);
        assert_eq!(records[0].end, seven.len());
        assert_eq!(records[1].refs.len(), 9);
        assert_eq!(records[1].end, seven.len() + nine.len());
        assert_eq!(records[2].refs, [18, 19, 1, 1, 1, 1, 1]);
        assert_eq!(records[2].end, seven.len() + nine.len() + terminal.len());
        assert_eq!(records[3].refs, [20, 21, 22, 1, 1, 23, 1, 1, 1, 1]);
        assert_eq!(
            records[3].end,
            seven.len() + nine.len() + terminal.len() + ten.len()
        );
        assert_eq!(attribute_slot_count(1), Some(6));
        assert_eq!(attribute_slot_count(3), Some(8));
        assert_eq!(attribute_slot_count(5), Some(10));
        assert_eq!(attribute_slot_count(0), None);
    }
}
