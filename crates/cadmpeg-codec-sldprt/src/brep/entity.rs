// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity metadata records.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::topology::Color;
use std::collections::{HashMap, HashSet};

use crate::layout::entity_common_header as entity_hdr;

/// Named ATTRIBUTE family whose inline `REAL_VALUES` child carries face color.
const FACE_COLOR_FAMILY: &str = "SDL/TYSA_COLOUR";

#[derive(Debug, Clone)]
pub(crate) struct FaceColor {
    pub(crate) face_attr: u16,
    pub(crate) color_attr: u16,
    pub(super) face_seq: u32,
    pub(super) stream_order: usize,
    pub(crate) color: Color,
    pub(crate) offset: usize,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FaceColorVersion {
    pub(super) face_attr: u16,
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
    /// Body-to-history ordinals carried by Parasolid attributes.
    pub(super) body_modifiers: Vec<super::attrib::BodyModifier>,
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

fn refs(
    ctx: Option<&DecodeContext<'_>>,
    body: &[u8],
    at: usize,
    count: usize,
    prefixed: bool,
) -> Result<Option<(Vec<u16>, usize)>, cadmpeg_core::CodecError> {
    if prefixed {
        if body.get(at) != Some(&1) {
            return Ok(None);
        }
        let mut out = Vec::new();
        let mut p = at;
        while body.get(p) == Some(&1) {
            let Some(value) = p.checked_add(1).and_then(|at| View::u16_be_at(body, at)) else {
                return Ok(None);
            };
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "decode prefixed Parasolid entity references")?;
            }
            out.push(value);
            let Some(next) = p.checked_add(3) else {
                return Ok(None);
            };
            p = next;
        }
        if !out.is_empty() && body.get(p) == Some(&0) {
            return Ok(p.checked_add(1).map(|end| (out, end)));
        }
    }
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, "decode Parasolid entity references")?;
    }
    let mut refs = Vec::with_capacity(count);
    for index in 0..count {
        let Some(cell) = index.checked_mul(2).and_then(|delta| at.checked_add(delta)) else {
            return Ok(None);
        };
        let Some(value) = View::u16_be_at(body, cell) else {
            return Ok(None);
        };
        refs.push(value);
    }
    Ok(count
        .checked_mul(2)
        .and_then(|size| at.checked_add(size))
        .map(|end| (refs, end)))
}

fn scan_entities(
    ctx: Option<&DecodeContext<'_>>,
    body: &[u8],
    prefixed: bool,
) -> Result<Vec<EntityRecord>, cadmpeg_core::CodecError> {
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
        let Some((refs, end)) = refs(ctx, body, p + entity_hdr::LEN, count, prefixed)? else {
            continue;
        };
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "collect Parasolid entity records")?;
        }
        out.push(EntityRecord {
            attr,
            seq,
            disc,
            refs,
            end,
        });
    }
    Ok(out)
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
    Some((attr, Color::new(r as f32, g as f32, b as f32, 1.0)?, p + 30))
}

#[derive(Debug, Clone, Copy)]
struct FramedColor {
    color: Color,
    offset: usize,
    parent_seq: u32,
}

fn linked_colors(
    ctx: Option<&DecodeContext<'_>>,
    body: &[u8],
    entities: &[EntityRecord],
) -> Result<HashMap<(u16, u16), Vec<FramedColor>>, cadmpeg_core::CodecError> {
    let mut colors = HashMap::<(u16, u16), Vec<FramedColor>>::new();
    for parent in entities {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(
                parent.refs.len() as u64,
                "collect Parasolid linked face references",
            )?;
            ctx.charge_collection_items(1, "collect Parasolid parent face reference")?;
        }
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
                if let Some(ctx) = ctx {
                    if !colors.contains_key(&(face_attr, color_attr)) {
                        ctx.charge_collection_items(1, "collect Parasolid linked color groups")?;
                    }
                    ctx.charge_collection_items(1, "collect Parasolid linked colors")?;
                }
                colors
                    .entry((face_attr, color_attr))
                    .or_default()
                    .push(framed);
            }
            at = end;
        }
    }
    Ok(colors)
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
pub(crate) fn scan_metadata(
    ctx: Option<&DecodeContext<'_>>,
    body: &[u8],
    prefixed: bool,
) -> Result<Facts, cadmpeg_core::CodecError> {
    let entities = scan_entities(ctx, body, prefixed)?;
    let definitions = super::attrib::definition_table(ctx, body)?;
    let linked_colors = linked_colors(ctx, body, &entities)?;
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
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "collect Parasolid face color versions")?;
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
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "collect Parasolid face colors")?;
        }
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
    Ok(Facts {
        entity_count: entities.len(),
        unresolved_face_colors,
        face_color_versions,
        face_colors,
        face_atoms: super::attrib::scan(ctx, body)?,
        body_modifiers: super::attrib::scan_body_modifiers(ctx, body)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        attribute_slot_count, linked_colors, refs, scan_entities, scan_metadata, EntityRecord,
        FACE_COLOR_FAMILY,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

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

    fn attribute_definition(family: &str, definition: u16) -> Vec<u8> {
        let name = family.as_bytes();
        let mut bytes = vec![0, 0x4f];
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&15_u16.to_be_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&[0, 0x50]);
        bytes.extend_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(&definition.to_be_bytes());
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

        assert_eq!(
            refs(None, &bytes, 0, 6, true).expect("references"),
            Some(((2_u16..=8).collect(), 22))
        );
    }

    #[test]
    fn parasolid_entity_references_refuse_collection_limit_before_allocation() {
        let bytes = [0_u8; 12];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error =
            refs(Some(&ctx), &bytes, 0, 6, false).expect_err("six references exceed five items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid entity references"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            refs(Some(&ctx), &bytes, 0, 6, false)
                .expect("service scan")
                .map(|(values, _)| values.len()),
            Some(6)
        );
    }

    #[test]
    fn prefixed_parasolid_entity_references_refuse_collection_limit_before_insertion() {
        let bytes = prefixed_entity(700, 1, 16, &[2, 3]);
        let at = 14;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error =
            refs(Some(&ctx), &bytes, at, 0, true).expect_err("second reference exceeds one item");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode prefixed Parasolid entity references"));
    }

    #[test]
    fn parasolid_entity_records_refuse_collection_limit_before_insertion() {
        let bytes = bare_entity_slots(700, 1, 16, 1, &[0; 6]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_entities(Some(&ctx), &bytes, false)
            .expect_err("entity insertion exceeds the limit");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect Parasolid entity records"));
    }

    fn linked_color_refusal(max_items: u64, operation: &'static str) {
        let bytes = color(900, [0.25, 0.5, 0.75], false);
        let entities = [EntityRecord {
            attr: 700,
            seq: 1,
            disc: 16,
            refs: vec![900],
            end: 0,
        }];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = linked_colors(Some(&ctx), &bytes, &entities)
            .expect_err("linked color collection exceeds the limit");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn parasolid_linked_face_references_refuse_before_collection() {
        linked_color_refusal(0, "collect Parasolid linked face references");
    }

    #[test]
    fn parasolid_parent_face_reference_refuses_before_collection() {
        linked_color_refusal(1, "collect Parasolid parent face reference");
    }

    #[test]
    fn parasolid_linked_color_groups_refuse_before_insertion() {
        linked_color_refusal(2, "collect Parasolid linked color groups");
    }

    #[test]
    fn parasolid_linked_colors_refuse_before_insertion() {
        linked_color_refusal(3, "collect Parasolid linked colors");
    }

    fn face_color_refusal(max_items: u64, operation: &'static str) {
        let mut bytes = bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan_metadata(Some(&ctx), &bytes, false)
            .expect_err("face color collection exceeds the limit");
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn parasolid_face_color_versions_refuse_before_insertion() {
        face_color_refusal(18, "collect Parasolid face color versions");
    }

    #[test]
    fn parasolid_face_colors_refuse_before_insertion() {
        face_color_refusal(19, "collect Parasolid face colors");
    }

    #[test]
    fn face_color_requires_the_adjacent_record_boundary() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(None, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn prefixed_face_color_uses_the_terminated_face_boundary() {
        let mut bytes = face_color_definition(16);
        bytes.extend(prefixed_entity(700, 4, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], true));

        let facts = scan_metadata(None, &bytes, true).expect("metadata scan");

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

        let facts = scan_metadata(None, &bytes, false).expect("metadata scan");

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].color_attr, 900);
    }

    #[test]
    fn named_non_face_definition_does_not_select_a_face_color_family() {
        let mut bytes = attribute_definition("OTHER_FAMILY", 0x15);
        bytes.extend(bare_entity(700, 1, 0x15, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(None, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert!(facts.face_color_versions.is_empty());
    }

    #[test]
    fn conflicting_definition_does_not_use_structural_color_fallback() {
        let mut bytes = face_color_definition(16);
        bytes.extend(attribute_definition("OTHER_FAMILY", 16));
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(None, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert!(facts.face_color_versions.is_empty());
    }

    #[test]
    fn unrelated_adjacent_color_does_not_replace_the_referenced_color() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(901, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(None, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn referenced_color_uses_a_framed_inline_face_link() {
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(bare_entity(701, 2, 16, &[0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(None, &bytes, false).expect("metadata scan");

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

        let facts = scan_metadata(None, &bytes, false).expect("metadata scan");

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

        let records = scan_entities(None, &bytes, false).expect("entity scan");
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
