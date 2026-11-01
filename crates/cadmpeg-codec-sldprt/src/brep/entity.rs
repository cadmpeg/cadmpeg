// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity metadata records.

use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::Color;
use std::collections::{HashMap, HashSet};

use crate::layout::entity_common_header as entity_hdr;
use crate::layout::integer_values_header as int_values;

/// Named ATTRIBUTE family whose inline `REAL_VALUES` child carries face color.
const FACE_COLOR_FAMILY: &str = "SDL/TYSA_COLOUR";

#[derive(Debug, Clone)]
pub(crate) struct FaceColor {
    pub(crate) face_attr: u32,
    pub(crate) color_attr: u32,
    pub(super) face_seq: u32,
    pub(super) stream_order: usize,
    pub(crate) color: Color,
    pub(crate) offset: usize,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FaceColorVersion {
    pub(super) face_attr: u32,
    pub(super) seq: u32,
    pub(super) stream_order: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Facts {
    /// Number of framed top-level model entity records in the stream.
    pub(super) entity_count: usize,
    /// Face-color links whose current framed records conflict.
    pub(super) unresolved_face_colors: usize,
    /// Version of every face record that can carry a color.
    pub(super) face_color_versions: Vec<FaceColorVersion>,
    pub(super) face_colors: Vec<FaceColor>,
    /// Per-face producing-feature identities carried by Parasolid attributes.
    pub(super) face_atoms: Vec<super::attrib::RawFaceAtom>,
    pub(super) face_payloads: Vec<(u32, Vec<u32>)>,
    pub(super) body_payloads: Vec<(u32, Vec<u32>)>,
    /// Body-to-history ordinals carried by Parasolid attributes.
    pub(super) body_modifiers: Vec<super::attrib::BodyModifier>,
}

#[derive(Debug, Clone)]
struct EntityRecord {
    attr: u32,
    seq: u32,
    disc: u32,
    refs: Vec<u32>,
    end: usize,
}

/// Number of pointer fields in one bare XT ATTRIBUTE node.
///
/// The low flag byte is the value count. Every attribute has five pointer
/// fields before its values, regardless of the definition node it references.
fn attribute_slot_count(flo: u8) -> Option<usize> {
    (1..=0x20).contains(&flo).then_some(5 + usize::from(flo))
}

fn refs(body: &[u8], at: usize, count: usize, prefixed: bool) -> Option<(Vec<u32>, usize)> {
    if prefixed {
        if body.get(at) != Some(&1) {
            return None;
        }
        let mut out = Vec::new();
        let mut p = at;
        while body.get(p) == Some(&1) {
            out.push(super::word_at(body, p.checked_add(1)?)?);
            p = p.checked_add(3)?;
        }
        if !out.is_empty() && body.get(p) == Some(&0) {
            return p.checked_add(1).map(|end| (out, end));
        }
    }
    let mut refs = Vec::new();
    let mut end = at;
    for _ in 0..count {
        refs.push(cadmpeg_parasolid::pointer::read_at(body, &mut end)?);
    }
    Some((refs, end))
}

pub(super) struct EntityHeader {
    pub(super) flags: u32,
    pub(super) attr: u32,
    pub(super) seq: u32,
    pub(super) disc: u32,
    pub(super) fields: usize,
}

pub(super) fn attribute_header(bytes: &[u8], p: usize) -> Option<EntityHeader> {
    bytes.get(p..p.checked_add(entity_hdr::LEN)?)?;
    let flags = View::u32_be_at(bytes, p + entity_hdr::FLAGS)?;
    let mut at = p + entity_hdr::ATTR;
    let attr = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
    let shifted = at.checked_sub(entity_hdr::SEQ)?;
    let seq = View::u32_be_at(bytes, shifted + entity_hdr::SEQ)?;
    at = shifted + entity_hdr::DISC;
    let disc = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
    Some(EntityHeader {
        flags,
        attr,
        seq,
        disc,
        fields: at,
    })
}

pub(super) fn integer_values_header(bytes: &[u8], p: usize) -> Option<(u32, u32, usize)> {
    bytes.get(p..p.checked_add(int_values::LEN)?)?;
    let count = View::u32_be_at(bytes, p + int_values::COUNT)?;
    let mut at = p + int_values::NODE_INDEX;
    let node = cadmpeg_parasolid::pointer::read_at(bytes, &mut at)?;
    (node > 1).then_some((count, node, at))
}

fn scan_entities(body: &[u8], prefixed: bool) -> Vec<EntityRecord> {
    // A body shorter than one entity header holds no entity record.
    let Some(scan_end) = body.len().checked_sub(25) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for off in 0..scan_end {
        if body.get(off..off + 2) != Some(&[0x00, 0x51]) {
            continue;
        }
        let mut p = off + 2;
        if body.get(p) == Some(&0xff) {
            p += 1;
        }
        let Some(EntityHeader {
            flags,
            attr,
            seq,
            disc,
            fields,
        }) = attribute_header(body, p)
        else {
            continue;
        };
        let [_, _, _, flo] = flags.to_be_bytes();
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
        let Some((refs, end)) = refs(body, fields, count, prefixed) else {
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

fn color_record(body: &[u8], off: usize) -> Option<(u32, Color, usize)> {
    if body.get(off..off + 2) != Some(&[0x00, 0x53]) {
        return None;
    }
    let mut p = off + 2;
    if body.get(p) == Some(&0xff) {
        p += 1;
    }
    let (count, attr, data) = integer_values_header(body, p)?;
    if count & 0xff != 3 {
        return None;
    }
    let [r, g, b] = [
        View::f64_be_at(body, data)?,
        View::f64_be_at(body, data + 8)?,
        View::f64_be_at(body, data + 16)?,
    ];
    if attr <= 1
        || ![r, g, b]
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        return None;
    }
    let color = Color::new(f32_from_f64(r)?, f32_from_f64(g)?, f32_from_f64(b)?, 1.0)?;
    Some((attr, color, data + 24))
}

#[derive(Debug, Clone, Copy)]
struct FramedColor {
    color: Color,
    offset: usize,
    parent_seq: u32,
}

fn linked_colors(body: &[u8], entities: &[EntityRecord]) -> HashMap<(u32, u32), Vec<FramedColor>> {
    let mut colors = HashMap::<(u32, u32), Vec<FramedColor>>::new();
    for parent in entities {
        let mut linked_faces: HashSet<u32> = parent.refs.iter().copied().collect();
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
    let definitions = super::attrib::definition_table(body);
    let linked_colors = linked_colors(body, &entities);
    let mut face_colors = Vec::new();
    let mut face_color_versions = Vec::new();
    let mut unresolved_face_colors = 0;
    for face in &entities {
        let named_face_color =
            definitions.get(&face.disc).and_then(Option::as_deref) == Some(FACE_COLOR_FAMILY);
        let unnamed_definition = !definitions.contains_key(&face.disc);
        let linked_attr = face.refs.get(5).copied().filter(|attr| *attr > 1);
        let unnamed_face_color = unnamed_definition
            && (linked_attr
                .is_some_and(|color_attr| linked_colors.contains_key(&(face.attr, color_attr)))
                || color_record(body, face.end).is_some());
        if !named_face_color && !unnamed_face_color {
            continue;
        }
        face_color_versions.push(FaceColorVersion {
            face_attr: face.attr,
            seq: face.seq,
            stream_order: 0,
        });
        let framed = if let Some(color_attr) = linked_attr {
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
        } else {
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
        face_payloads: super::attrib::scan_face_payloads(body),
        body_payloads: super::attrib::scan_body_payloads(body),
        body_modifiers: super::attrib::scan_body_modifiers(body),
    }
}

#[cfg(test)]
mod tests {
    use super::{attribute_slot_count, refs, scan_entities, scan_metadata, FACE_COLOR_FAMILY};

    fn pointer(out: &mut Vec<u8>, value: u16) {
        if value <= 0x7ffe {
            out.extend(value.to_be_bytes());
        } else {
            out.extend((value | 0x8000).to_be_bytes());
            out.extend((value >> 15).to_be_bytes());
        }
    }

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
        pointer(&mut bytes, attr);
        bytes.extend_from_slice(&seq.to_be_bytes());
        pointer(&mut bytes, disc);
        for reference in refs {
            pointer(&mut bytes, *reference);
        }
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

    fn attribute_definition(family: &str, definition: u16) -> Vec<u8> {
        let name = family.as_bytes();
        let mut bytes = vec![0, 0x4f];
        bytes.extend_from_slice(
            &u32::try_from(name.len())
                .expect("name length fits u32")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&15_u16.to_be_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&[0, 0x50]);
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&definition.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&15_u16.to_be_bytes());
        bytes.extend_from_slice(&8001_u32.to_be_bytes());
        bytes.extend([0; 8]);
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend([0; 14]);
        bytes.push(2);
        bytes
    }

    fn face_color_definition(definition: u16) -> Vec<u8> {
        attribute_definition(FACE_COLOR_FAMILY, definition)
    }

    #[test]
    fn prefixed_entity_refs_end_at_the_zero_terminator() {
        let mut bytes = Vec::new();
        for reference in 2_u16..=8 {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0);

        assert_eq!(refs(&bytes, 0, 6, true), Some(((2_u32..=8).collect(), 22)));
    }

    #[test]
    fn parasolid_entity_references_decode_bare_slots() {
        let bytes = [0_u8; 12];
        assert_eq!(
            refs(&bytes, 0, 6, false).map(|(values, _)| values.len()),
            Some(6)
        );
    }

    #[test]
    fn face_color_requires_the_adjacent_record_boundary() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn prefixed_face_color_uses_the_terminated_face_boundary() {
        let mut bytes = face_color_definition(16);
        bytes.extend(prefixed_entity(700, 4, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], true));

        let facts = scan_metadata(&bytes, true);

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
        assert_eq!(facts.face_colors[0].face_seq, 4);
    }

    #[test]
    fn named_face_color_uses_inline_record_when_link_is_null_like() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 1]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].color_attr, 900);
    }

    #[test]
    fn named_non_face_definition_does_not_select_a_face_color_family() {
        let mut bytes = attribute_definition("OTHER_FAMILY", 0x15);
        bytes.extend(bare_entity(700, 1, 0x15, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert!(facts.face_colors.is_empty());
        assert!(facts.face_color_versions.is_empty());
    }

    #[test]
    fn unsupported_definition_framing_does_not_use_structural_color_fallback() {
        let mut bytes = face_color_definition(16);
        let field_names = 8 + FACE_COLOR_FAMILY.len() + 24;
        bytes.splice(field_names..field_names + 2, [0xff, 0xff, 0, 1]);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 1]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert!(facts.face_colors.is_empty());
        assert!(facts.face_color_versions.is_empty());
    }

    #[test]
    fn conflicting_definition_does_not_use_structural_color_fallback() {
        let mut bytes = face_color_definition(16);
        bytes.extend(attribute_definition("OTHER_FAMILY", 16));
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert!(facts.face_colors.is_empty());
        assert!(facts.face_color_versions.is_empty());
    }

    #[test]
    fn unrelated_adjacent_color_does_not_replace_the_referenced_color() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(901, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn referenced_color_uses_a_framed_inline_face_link() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(bare_entity(701, 2, 16, &[0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&bytes, false);

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
    }

    #[test]
    fn inline_face_link_frames_a_contiguous_color_run() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(bare_entity(701, 2, 16, &[0, 0, 0, 0, 700, 901]));
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
