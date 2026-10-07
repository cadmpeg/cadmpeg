// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity metadata records.

use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_ir::topology::Color;
use std::collections::{BTreeSet, HashMap};

use super::attrib::{Declaration, Family};
use crate::layout::entity_common_header as entity_hdr;

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

/// The u16 references of one entity record, read in place from the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct References {
    /// Offset of the first reference value.
    at: usize,
    count: usize,
    /// Bytes from one reference value to the next.
    stride: usize,
}

impl References {
    fn get(self, body: &[u8], index: usize) -> Option<u16> {
        if index >= self.count {
            return None;
        }
        View::u16_be_at(body, index.checked_mul(self.stride)?.checked_add(self.at)?)
    }
}

#[derive(Debug, Clone, Copy)]
struct EntityRecord {
    attr: u16,
    seq: u32,
    disc: u16,
    refs: References,
    end: usize,
}

/// Number of u16 fields in one bare XT ATTRIBUTE node.
///
/// The low flag byte is the value count. Every attribute has five pointer
/// fields before its values, regardless of the definition node it references.
fn attribute_slot_count(flo: u8) -> Option<usize> {
    (1..=0x20).contains(&flo).then_some(5 + usize::from(flo))
}

/// Locate an entity record's references and the offset after them. A prefixed
/// record stores each reference after a `1` byte and ends the run with `0`; a
/// bare record stores `count` contiguous references.
fn refs(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    at: usize,
    count: usize,
    prefixed: bool,
) -> Result<Option<(References, usize)>, cadmpeg_core::CodecError> {
    if prefixed {
        if body.get(at) != Some(&1) {
            return Ok(None);
        }
        let mut found = 0_usize;
        let mut p = at;
        while body.get(p) == Some(&1) {
            ctx.charge_work(1, "scan prefixed Parasolid entity references")?;
            if p.checked_add(1)
                .and_then(|value| View::u16_be_at(body, value))
                .is_none()
            {
                return Ok(None);
            }
            found += 1;
            let Some(next) = p.checked_add(3) else {
                return Ok(None);
            };
            p = next;
        }
        if body.get(p) == Some(&0) {
            return Ok(p.checked_add(1).map(|end| {
                (
                    References {
                        at: at + 1,
                        count: found,
                        stride: 3,
                    },
                    end,
                )
            }));
        }
    }
    let Some(end) = count.checked_mul(2).and_then(|size| at.checked_add(size)) else {
        return Ok(None);
    };
    if count > 0 && View::u16_be_at(body, end - 2).is_none() {
        return Ok(None);
    }
    Ok(Some((
        References {
            at,
            count,
            stride: 2,
        },
        end,
    )))
}

/// Scan the framed entity records of one stream. The records are decode
/// scratch held under the returned reservation.
fn scan_entities<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    body: &[u8],
    prefixed: bool,
) -> Result<(Vec<EntityRecord>, ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold Parasolid entity records")?;
    let mut out = Vec::new();
    let starts = 0..body.len().checked_sub(25).map_or(0, |end| end);
    for off in ctx.admit_iter(starts, "scan Parasolid entity records")? {
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
        let Some((refs, end)) = refs(ctx, body, p + entity_hdr::LEN, count, prefixed)? else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut out,
            EntityRecord {
                attr,
                seq,
                disc,
                refs,
                end,
            },
            "collect Parasolid entity records",
        )?;
    }
    Ok((out, storage))
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
    let color = Color::new(f32_from_f64(r)?, f32_from_f64(g)?, f32_from_f64(b)?, 1.0)?;
    Some((attr, color, p + 30))
}

#[derive(Debug, Clone, Copy)]
struct FramedColor {
    color: Color,
    offset: usize,
    parent_seq: u32,
}

/// The current color framed for one face and color attribute: the first color
/// framed by a parent of the highest sequence, and whether every color framed
/// at that sequence agrees with it.
#[derive(Debug, Clone, Copy)]
struct LinkedColor {
    current: FramedColor,
    agreed: bool,
}

impl LinkedColor {
    fn frame(&mut self, framed: FramedColor) {
        if framed.parent_seq > self.current.parent_seq {
            *self = Self {
                current: framed,
                agreed: true,
            };
        } else if framed.parent_seq == self.current.parent_seq {
            self.agreed &= framed.color == self.current.color;
        }
    }
}

type LinkedColors = HashMap<(u16, u16), LinkedColor>;

/// Reduce the colors each entity frames in the record run after it to the
/// current color of every linked face. The table is decode scratch held under
/// the returned reservation.
fn linked_colors<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    body: &[u8],
    entities: &[EntityRecord],
) -> Result<(LinkedColors, ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold Parasolid linked colors")?;
    let mut colors = LinkedColors::new();
    for parent in ctx.admit_iter(entities, "scan Parasolid linked color parents")? {
        if color_record(body, parent.end).is_none() {
            continue;
        }
        let mut linked_faces_storage = ctx.reserve_scoped(0, "Parasolid temporary linked faces")?;
        let mut linked_faces = BTreeSet::new();
        for index in ctx.admit_iter(
            0..parent.refs.count,
            "collect Parasolid linked face references",
        )? {
            let Some(face) = parent.refs.get(body, index) else {
                continue;
            };
            linked_faces_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut linked_faces,
                    face,
                    "collect Parasolid linked face references",
                )
            })?;
        }
        linked_faces_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut linked_faces,
                parent.attr,
                "collect Parasolid parent face reference",
            )
        })?;
        let mut at = parent.end;
        while let Some((color_attr, color, end)) = color_record(body, at) {
            ctx.charge_work(1, "scan Parasolid linked colors")?;
            let framed = FramedColor {
                color,
                offset: at,
                parent_seq: parent.seq,
            };
            for &face_attr in ctx.admit_iter(&linked_faces, "frame Parasolid linked colors")? {
                if face_attr <= 1 {
                    continue;
                }
                storage
                    .with_storage(|| {
                        ctx.entry_hash_map(
                            &mut colors,
                            (face_attr, color_attr),
                            "collect Parasolid linked colors",
                        )
                    })?
                    .and_modify(|linked| linked.frame(framed))
                    .or_insert(LinkedColor {
                        current: framed,
                        agreed: true,
                    });
            }
            at = end;
        }
    }
    Ok((colors, storage))
}

/// Scan non-topology entity metadata without interpreting attribute chains as
/// body membership. Typed Parasolid BODY/SHELL/REGION nodes own that relation.
pub(crate) fn scan_metadata(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    prefixed: bool,
) -> Result<Facts, cadmpeg_core::CodecError> {
    let (entities, _entity_storage) = scan_entities(ctx, body, prefixed)?;
    let dictionary = super::attrib::dictionary(ctx, body)?;
    let (linked_colors, _color_storage) = linked_colors(ctx, body, &entities)?;
    let mut face_colors = Vec::new();
    let mut face_color_versions = Vec::new();
    let mut unresolved_face_colors = 0;
    for face in ctx.admit_iter(&entities, "resolve Parasolid face colors")? {
        let declared = dictionary.family(ctx, face.disc)?;
        let linked_attr = face.refs.get(body, 5).filter(|attr| *attr > 1);
        let linked = match linked_attr {
            Some(color_attr) => ctx
                .get_hash_map(
                    &linked_colors,
                    &(face.attr, color_attr),
                    "look up Parasolid linked colors",
                )?
                .copied(),
            None => None,
        };
        let named_face_color = declared == Declaration::Family(Family::FaceColor);
        let unnamed_face_color = declared == Declaration::Undeclared
            && (linked.is_some() || color_record(body, face.end).is_some());
        if !named_face_color && !unnamed_face_color {
            continue;
        }
        ctx.push_vec(
            &mut face_color_versions,
            FaceColorVersion {
                face_attr: face.attr,
                seq: face.seq,
                stream_order: 0,
            },
            "collect Parasolid face color versions",
        )?;
        let framed = if let Some(color_attr) = linked_attr {
            match linked {
                Some(LinkedColor {
                    current,
                    agreed: true,
                }) => Some((color_attr, current)),
                Some(_) => {
                    unresolved_face_colors += 1;
                    None
                }
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
        ctx.push_vec(
            &mut face_colors,
            FaceColor {
                face_attr: face.attr,
                color_attr,
                face_seq: face.seq,
                stream_order: 0,
                color: framed.color,
                offset: framed.offset,
                target: None,
            },
            "collect Parasolid face colors",
        )?;
    }
    let bindings = super::attrib::bindings(ctx, body, &dictionary)?;
    Ok(Facts {
        entity_count: entities.len(),
        unresolved_face_colors,
        face_color_versions,
        face_colors,
        face_atoms: bindings.face_atoms,
        body_modifiers: bindings.body_modifiers,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        attribute_slot_count, linked_colors, refs, scan_entities, scan_metadata, EntityRecord,
        References,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    const FACE_COLOR_FAMILY: &str = "SDL/TYSA_COLOUR";

    fn values(body: &[u8], refs: References) -> Vec<u16> {
        (0..refs.count)
            .map(|index| refs.get(body, index).expect("reference in range"))
            .collect()
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
        bytes.extend_from_slice(
            &u32::try_from(name.len())
                .expect("name length fits u32")
                .to_be_bytes(),
        );
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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        for reference in 2_u16..=8 {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0);

        let (found, end) = refs(&ctx, &bytes, 0, 6, true)
            .expect("references")
            .expect("terminated run");
        assert_eq!(values(&bytes, found), (2_u16..=8).collect::<Vec<_>>());
        assert_eq!(end, 22);
    }

    #[test]
    fn bare_entity_references_need_every_cell() {
        let bytes = [0_u8; 12];
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
        let (found, end) = refs(&ctx, &bytes, 0, 6, false)
            .expect("references")
            .expect("six cells");
        assert_eq!((found.count, end), (6, 12));
        assert_eq!(refs(&ctx, &bytes, 2, 6, false).expect("references"), None);
    }

    #[test]
    fn prefixed_parasolid_entity_references_refuse_work_before_each_step() {
        let bytes = prefixed_entity(700, 1, 16, &[2, 3]);
        crate::test_support::work_refusal_at("scan prefixed Parasolid entity references", |ctx| {
            refs(ctx, &bytes, 14, 0, true)
        });
    }

    #[test]
    fn parasolid_entity_records_refuse_collection_limit_before_insertion() {
        let bytes = bare_entity_slots(700, 1, 16, 1, &[0; 6]);
        collection_refusal("collect Parasolid entity records", &bytes, |ctx| {
            scan_entities(ctx, &bytes, false).map(|(records, _)| records.len())
        });
    }

    #[test]
    fn parasolid_entity_metadata_refuses_work_limit() {
        let bytes = bare_entity_slots(700, 1, 16, 1, &[0; 6]);
        for operation in [
            "scan Parasolid entity records",
            "resolve Parasolid face colors",
        ] {
            crate::test_support::work_refusal_at(operation, |ctx| {
                scan_metadata(ctx, &bytes, false)
            });
        }
    }

    #[test]
    fn parasolid_entity_records_are_scoped() {
        let bytes = bare_entity_slots(700, 1, 16, 1, &[0; 6]);
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "collect Parasolid entity records",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                scan_entities(&ctx, &bytes, false).map(|(records, _)| records.len())
            },
        );
    }

    fn collection_refusal<T>(
        operation: &'static str,
        bytes: &[u8],
        run: impl Fn(&DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
    ) {
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)?;
                let result = run(&ctx);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            },
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    fn linked_color_refusal(operation: &'static str) {
        let mut bytes = 900_u16.to_be_bytes().to_vec();
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));
        let entities = [EntityRecord {
            attr: 700,
            seq: 1,
            disc: 16,
            refs: References {
                at: 0,
                count: 1,
                stride: 2,
            },
            end: 2,
        }];
        collection_refusal(operation, &bytes, |ctx| {
            linked_colors(ctx, &bytes, &entities).map(|(colors, _)| colors.len())
        });
    }

    #[test]
    fn parasolid_linked_face_references_refuse_before_collection() {
        linked_color_refusal("collect Parasolid linked face references");
    }

    #[test]
    fn parasolid_parent_face_reference_refuses_before_collection() {
        linked_color_refusal("collect Parasolid parent face reference");
    }

    #[test]
    fn parasolid_linked_colors_refuse_before_insertion() {
        linked_color_refusal("collect Parasolid linked colors");
    }

    fn face_color_refusal(operation: &'static str) {
        let mut bytes = bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));
        collection_refusal(operation, &bytes, |ctx| scan_metadata(ctx, &bytes, false));
    }

    #[test]
    fn parasolid_face_color_versions_refuse_before_insertion() {
        face_color_refusal("collect Parasolid face color versions");
    }

    #[test]
    fn parasolid_face_colors_refuse_before_insertion() {
        face_color_refusal("collect Parasolid face colors");
    }

    #[test]
    fn face_color_requires_the_adjacent_record_boundary() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&ctx, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn prefixed_face_color_uses_the_terminated_face_boundary() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = face_color_definition(16);
        bytes.extend(prefixed_entity(700, 4, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], true));

        let facts = scan_metadata(&ctx, &bytes, true).expect("metadata scan");

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
        assert_eq!(facts.face_colors[0].face_seq, 4);
    }

    #[test]
    fn named_face_color_uses_inline_record_when_link_is_null_like() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 1]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&ctx, &bytes, false).expect("metadata scan");

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].color_attr, 900);
    }

    #[test]
    fn named_non_face_definition_does_not_select_a_face_color_family() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = attribute_definition("OTHER_FAMILY", 0x15);
        bytes.extend(bare_entity(700, 1, 0x15, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&ctx, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert!(facts.face_color_versions.is_empty());
    }

    #[test]
    fn conflicting_definition_does_not_use_structural_color_fallback() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = face_color_definition(16);
        bytes.extend(attribute_definition("OTHER_FAMILY", 16));
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&ctx, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert!(facts.face_color_versions.is_empty());
    }

    #[test]
    fn unrelated_adjacent_color_does_not_replace_the_referenced_color() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(color(901, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&ctx, &bytes, false).expect("metadata scan");

        assert!(facts.face_colors.is_empty());
        assert_eq!(facts.unresolved_face_colors, 0);
    }

    #[test]
    fn referenced_color_uses_a_framed_inline_face_link() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(bare_entity(701, 2, 16, &[0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));

        let facts = scan_metadata(&ctx, &bytes, false).expect("metadata scan");

        assert_eq!(facts.face_colors.len(), 1);
        assert_eq!(facts.face_colors[0].face_attr, 700);
        assert_eq!(facts.face_colors[0].color_attr, 900);
    }

    #[test]
    fn inline_face_link_frames_a_contiguous_color_run() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut bytes = face_color_definition(16);
        bytes.extend(bare_entity(700, 1, 16, &[0, 0, 0, 0, 0, 900]));
        bytes.extend(bare_entity(701, 2, 16, &[0, 0, 0, 0, 700, 901]));
        bytes.extend(color(900, [0.25, 0.5, 0.75], false));
        bytes.extend(color(901, [0.75, 0.5, 0.25], false));

        let facts = scan_metadata(&ctx, &bytes, false).expect("metadata scan");

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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let seven = bare_entity_slots(700, 1, 0x9999, 2, &[2, 3, 4, 5, 6, 7, 8]);
        let nine = bare_entity_slots(701, 2, 0x1a, 4, &[9, 10, 11, 12, 13, 14, 15, 16, 17]);
        let terminal = bare_entity_slots(702, 3, 0x06, 2, &[18, 19, 1, 1, 1, 1, 1]);
        let ten = bare_entity_slots(703, 4, 0x1c, 5, &[20, 21, 22, 1, 1, 23, 1, 1, 1, 1]);
        let mut bytes = seven.clone();
        bytes.extend_from_slice(&nine);
        bytes.extend_from_slice(&terminal);
        bytes.extend_from_slice(&ten);
        bytes.extend([0; 16]);

        let records = scan_entities(&ctx, &bytes, false).expect("entity scan");
        let (records, _storage) = records;
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].refs.count, 7);
        assert_eq!(records[0].end, seven.len());
        assert_eq!(records[1].refs.count, 9);
        assert_eq!(records[1].end, seven.len() + nine.len());
        assert_eq!(values(&bytes, records[2].refs), [18, 19, 1, 1, 1, 1, 1]);
        assert_eq!(records[2].end, seven.len() + nine.len() + terminal.len());
        assert_eq!(
            values(&bytes, records[3].refs),
            [20, 21, 22, 1, 1, 23, 1, 1, 1, 1]
        );
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
