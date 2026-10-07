// SPDX-License-Identifier: Apache-2.0
//! Stream-scope entity metadata records.

use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_ir::topology::Color;
use std::collections::{BTreeMap, BTreeSet, HashMap};

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

#[derive(Clone, Copy)]
enum ReferenceTail {
    Terminated(usize),
    Bare,
    Invalid,
}

/// A prefixed reference suffix is parsed once, including malformed endings.
struct ReferenceRuns<'ctx> {
    tails: BTreeMap<usize, ReferenceTail>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> ReferenceRuns<'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            tails: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "hold prefixed Parasolid reference tails")?,
        })
    }

    fn refs(
        &mut self,
        ctx: &DecodeContext<'_>,
        body: &[u8],
        at: usize,
        count: usize,
        prefixed: bool,
    ) -> Result<Option<(References, usize)>, cadmpeg_core::CodecError> {
        const INDEX: &str = "index prefixed Parasolid reference tails";
        if prefixed {
            if body.get(at) != Some(&1) {
                return Ok(None);
            }
            let mut path_storage = ctx.reserve_scoped(0, INDEX)?;
            let mut path = Vec::new();
            let mut p = at;
            let tail = loop {
                if let Some(&tail) = ctx.get_btree_map(&self.tails, &p, INDEX)? {
                    break tail;
                }
                if body.get(p) != Some(&1) {
                    break if body.get(p) == Some(&0) {
                        ReferenceTail::Terminated(p + 1)
                    } else {
                        ReferenceTail::Bare
                    };
                }
                ctx.charge_work(1, "scan prefixed Parasolid entity references")?;
                if View::u16_be_at(body, p + 1).is_none() {
                    break ReferenceTail::Invalid;
                }
                ctx.push_scoped_vec(&mut path_storage, &mut path, p, INDEX)?;
                p += 3;
            };
            for p in ctx.admit_iter(path, INDEX)? {
                self.storage
                    .with_storage(|| ctx.insert_btree_map(&mut self.tails, p, tail, INDEX))?;
            }
            match tail {
                ReferenceTail::Terminated(end) => {
                    return Ok(Some((
                        References {
                            at: at + 1,
                            count: (end - 1 - at) / 3,
                            stride: 3,
                        },
                        end,
                    )))
                }
                ReferenceTail::Invalid => return Ok(None),
                ReferenceTail::Bare => {}
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
    let mut runs = ReferenceRuns::new(ctx)?;
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
        let Some((refs, end)) = runs.refs(ctx, body, p + entity_hdr::LEN, count, prefixed)? else {
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
    parent_order: usize,
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
    fn frame(&mut self, framed: FramedColor, agreed: bool) {
        if framed.parent_seq > self.current.parent_seq {
            *self = Self {
                current: framed,
                agreed,
            };
        } else if framed.parent_seq == self.current.parent_seq {
            self.agreed &= agreed && framed.color == self.current.color;
            if (framed.parent_order, framed.offset)
                < (self.current.parent_order, self.current.offset)
            {
                self.current = framed;
            }
        }
    }
}

type LinkedColors = HashMap<(u16, u16), LinkedColor>;
type ColorRequests = BTreeMap<u16, BTreeSet<u16>>;

/// First color and agreement of one attribute in a record suffix.
#[derive(Clone, Copy)]
struct RunColor {
    color: Color,
    offset: usize,
    agreed: bool,
}

/// Reduce each color suffix once, then bind only requested face/color pairs.
/// The returned reservation holds the face/color index.
fn linked_colors<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    body: &[u8],
    entities: &[EntityRecord],
    requested: &ColorRequests,
) -> Result<(LinkedColors, ScopedReservation<'ctx>), cadmpeg_core::CodecError> {
    use std::cmp::Reverse;
    type Stamp = (Reverse<u32>, usize);
    type ReferenceStarts = BTreeMap<usize, (References, Stamp)>;
    const INDEX: &str = "index Parasolid color run parents";
    let mut storage = ctx.reserve_scoped(0, "hold Parasolid linked colors")?;
    if requested.is_empty() {
        return Ok((LinkedColors::new(), storage));
    }
    let mut run_storage = ctx.reserve_scoped(0, INDEX)?;
    let mut parents = BTreeMap::<usize, BTreeMap<u16, Stamp>>::new();
    let mut references = BTreeMap::<(usize, usize, usize), ReferenceStarts>::new();
    let mut records = BTreeMap::<usize, (u16, Color, usize)>::new();
    for (order, parent) in ctx
        .admit_iter(entities, "scan Parasolid linked color parents")?
        .enumerate()
    {
        let own_requested = ctx.contains_key_btree_map(
            requested,
            &parent.attr,
            "select Parasolid referenced faces",
        )?;
        if (!own_requested && parent.refs.count == 0) || color_record(body, parent.end).is_none() {
            continue;
        }
        let stamp = (Reverse(parent.seq), order);
        let faces = run_storage
            .with_storage(|| ctx.entry_btree_map(&mut parents, parent.end, INDEX))?
            .or_default();
        if own_requested {
            run_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        faces,
                        parent.attr,
                        "collect Parasolid parent face reference",
                    )
                })?
                .and_modify(|previous| *previous = (*previous).min(stamp))
                .or_insert(stamp);
        }
        if parent.refs.count > 0 {
            // Runs with the same end and stride are suffixes of one cell lane.
            let end = parent.refs.at + parent.refs.count * parent.refs.stride;
            let starts = run_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut references,
                        (parent.end, parent.refs.stride, end),
                        INDEX,
                    )
                })?
                .or_default();
            run_storage
                .with_storage(|| ctx.entry_btree_map(starts, parent.refs.at, INDEX))?
                .and_modify(|(_, previous)| *previous = (*previous).min(stamp))
                .or_insert((parent.refs, stamp));
        }
    }
    // A parent starts contributing at its first reference. Keep the best
    // stamp seen so far and visit each cell of a shared lane once.
    for (&(color_start, _, _), starts) in ctx.admit_iter(&references, INDEX)? {
        let mut events = starts.iter();
        let Some((&first, &(refs, first_stamp))) = ctx.next_charged(&mut events, INDEX)? else {
            continue;
        };
        let mut stamp = first_stamp;
        let mut next = ctx.next_charged(&mut events, INDEX)?;
        let Some(faces) = ctx.get_mut_btree_map(&mut parents, &color_start, INDEX)? else {
            continue;
        };
        for index in ctx.admit_iter(0..refs.count, "collect Parasolid linked face references")? {
            let at = first + index * refs.stride;
            while let Some((&start, &(_, parent_stamp))) = next {
                if start > at {
                    break;
                }
                stamp = stamp.min(parent_stamp);
                next = ctx.next_charged(&mut events, INDEX)?;
            }
            let Some(face) = refs.get(body, index) else {
                continue;
            };
            if !ctx.contains_key_btree_map(requested, &face, "select Parasolid referenced faces")? {
                continue;
            }
            run_storage
                .with_storage(|| {
                    ctx.entry_btree_map(faces, face, "collect Parasolid linked face references")
                })?
                .and_modify(|previous| *previous = (*previous).min(stamp))
                .or_insert(stamp);
        }
    }
    for (&start, faces) in ctx.admit_iter(&parents, "select Parasolid requested color runs")? {
        if faces.is_empty() {
            continue;
        }
        let mut at = start;
        loop {
            if ctx.contains_key_btree_map(&records, &at, INDEX)? {
                break;
            }
            ctx.charge_work(1, "scan Parasolid linked colors")?;
            let Some(record) = color_record(body, at) else {
                break;
            };
            run_storage.with_storage(|| ctx.insert_btree_map(&mut records, at, record, INDEX))?;
            at = record.2;
        }
    }
    let mut colors = LinkedColors::new();
    let mut summaries = BTreeMap::<usize, BTreeMap<u16, RunColor>>::new();
    // The escape changes record length by one byte. The tag excludes two
    // valid starts one byte apart, so each suffix has at most one predecessor.
    // Move its reduction to that predecessor instead of copying or replaying it.
    for (&at, &(attr, color, end)) in ctx
        .admit_iter(&records, "reduce Parasolid color runs")?
        .rev()
    {
        let mut summary = ctx
            .remove_btree_map(&mut summaries, &end, INDEX)?
            .unwrap_or_default();
        run_storage
            .with_storage(|| ctx.entry_btree_map(&mut summary, attr, INDEX))?
            .and_modify(|run| {
                run.agreed &= run.color == color;
                run.color = color;
                run.offset = at;
            })
            .or_insert(RunColor {
                color,
                offset: at,
                agreed: true,
            });
        if let Some(faces) = ctx.get_btree_map(&parents, &at, INDEX)? {
            for (&face_attr, &(Reverse(parent_seq), parent_order)) in
                ctx.admit_iter(faces, "frame Parasolid linked colors")?
            {
                if face_attr <= 1 {
                    continue;
                }
                let Some(attributes) = ctx.get_btree_map(requested, &face_attr, INDEX)? else {
                    continue;
                };
                for &color_attr in ctx.admit_iter(attributes, "frame Parasolid reduced colors")? {
                    let Some(run) = ctx.get_btree_map(&summary, &color_attr, INDEX)? else {
                        continue;
                    };
                    let framed = FramedColor {
                        color: run.color,
                        offset: run.offset,
                        parent_seq,
                        parent_order,
                    };
                    storage
                        .with_storage(|| {
                            ctx.entry_hash_map(
                                &mut colors,
                                (face_attr, color_attr),
                                "collect Parasolid linked colors",
                            )
                        })?
                        .and_modify(|linked| linked.frame(framed, run.agreed))
                        .or_insert(LinkedColor {
                            current: framed,
                            agreed: run.agreed,
                        });
                }
            }
        }
        run_storage.with_storage(|| ctx.insert_btree_map(&mut summaries, at, summary, INDEX))?;
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
    let mut request_storage = ctx.reserve_scoped(0, "hold Parasolid color requests")?;
    let mut requested = ColorRequests::new();
    for entity in ctx.admit_iter(&entities, "select Parasolid requested colors")? {
        if let Some(attr) = entity.refs.get(body, 5).filter(|attr| *attr > 1) {
            request_storage.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut requested,
                    entity.attr,
                    attr,
                    "index Parasolid color request faces",
                    "index Parasolid color requests",
                )
            })?;
        }
    }
    let (linked_colors, _color_storage) = linked_colors(ctx, body, &entities, &requested)?;
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
                        parent_order: 0,
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
        attribute_slot_count, linked_colors, scan_entities, scan_metadata, EntityRecord,
        ReferenceRuns, References,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    const FACE_COLOR_FAMILY: &str = "SDL/TYSA_COLOUR";

    fn refs(
        ctx: &DecodeContext<'_>,
        body: &[u8],
        at: usize,
        count: usize,
        prefixed: bool,
    ) -> Result<Option<(References, usize)>, cadmpeg_core::CodecError> {
        ReferenceRuns::new(ctx)?.refs(ctx, body, at, count, prefixed)
    }

    fn requested_colors(pairs: impl IntoIterator<Item = (u16, u16)>) -> super::ColorRequests {
        let mut requested = super::ColorRequests::new();
        for (face, color) in pairs {
            requested.entry(face).or_default().insert(color);
        }
        requested
    }

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
    fn prefixed_reference_tails_reuse_overlapping_runs() {
        let bytes = [1, 0, 2, 1, 0, 3, 1, 0, 4, 0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut runs = ReferenceRuns::new(&ctx).unwrap();
        let (head, end) = runs.refs(&ctx, &bytes, 0, 0, true).unwrap().unwrap();
        assert_eq!((values(&bytes, head), end), (vec![2, 3, 4], 10));
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan prefixed Parasolid entity references",
            None,
        );
        let (tail, end) = runs.refs(&ctx, &bytes, 3, 0, true).unwrap().unwrap();
        assert_eq!((values(&bytes, tail), end), (vec![3, 4], 10));
    }

    #[test]
    fn shared_color_run_keeps_highest_parent_sequence() {
        let bytes = color(800, [0.25, 0.5, 0.75], false);
        let parents = [1, 4, 2].map(|seq| EntityRecord {
            attr: 700,
            seq,
            disc: 16,
            refs: References {
                at: 0,
                count: 0,
                stride: 2,
            },
            end: 0,
        });
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let (colors, _storage) =
            linked_colors(&ctx, &bytes, &parents, &requested_colors([(700, 800)])).unwrap();
        let linked = colors.get(&(700, 800)).unwrap();
        assert!(linked.agreed);
        assert_eq!(linked.current.parent_seq, 4);
        assert_eq!(linked.current.offset, 0);
        assert_eq!(
            linked.current.color,
            super::color_record(&bytes, 0).unwrap().1
        );
    }

    #[test]
    fn shared_color_suffix_preserves_parent_order_and_conflicts() {
        let mut bytes = color(800, [0.25, 0.5, 0.75], false);
        let suffix = bytes.len();
        bytes.extend(color(800, [0.25, 0.5, 0.75], false));
        let parents = [suffix, 0].map(|end| EntityRecord {
            attr: 700,
            seq: 4,
            disc: 16,
            refs: References {
                at: 0,
                count: 0,
                stride: 2,
            },
            end,
        });
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let (colors, _storage) =
            linked_colors(&ctx, &bytes, &parents, &requested_colors([(700, 800)])).unwrap();
        let linked = colors.get(&(700, 800)).unwrap();
        assert!(linked.agreed);
        assert_eq!(linked.current.offset, suffix);
        bytes[suffix..].copy_from_slice(&color(800, [0.5, 0.25, 0.75], false));
        let (colors, _storage) =
            linked_colors(&ctx, &bytes, &parents, &requested_colors([(700, 800)])).unwrap();
        assert!(!colors.get(&(700, 800)).unwrap().agreed);
        assert_eq!(colors.get(&(700, 800)).unwrap().current.offset, suffix);
    }

    #[test]
    fn overlapping_parent_reference_tails_keep_the_highest_sequence() {
        let mut bytes = Vec::new();
        for _ in 0..128 {
            bytes.extend_from_slice(&[1, 0, 100]);
        }
        bytes.push(0);
        let end = bytes.len();
        bytes.extend(color(800, [0.25, 0.5, 0.75], false));
        let parents: Vec<_> = (0_u16..64)
            .map(|index| EntityRecord {
                attr: 200 + index,
                seq: u32::from(index),
                disc: 16,
                refs: References {
                    at: 1 + usize::from(index) * 3,
                    count: 128 - usize::from(index),
                    stride: 3,
                },
                end,
            })
            .collect();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let (colors, _storage) = linked_colors(
            &ctx,
            &bytes,
            &parents,
            &requested_colors(
                parents
                    .iter()
                    .map(|parent| (parent.attr, 800))
                    .chain([(100, 800)]),
            ),
        )
        .unwrap();
        assert_eq!(colors.len(), 65);
        let shared = colors.get(&(100, 800)).unwrap();
        assert!(shared.agreed);
        assert_eq!(shared.current.parent_seq, 63);
        assert_eq!(shared.current.parent_order, 63);
        for parent in &parents {
            assert_eq!(
                colors.get(&(parent.attr, 800)).unwrap().current.parent_seq,
                parent.seq
            );
        }
        crate::test_support::work_refusal_at("collect Parasolid linked face references", |ctx| {
            linked_colors(
                ctx,
                &bytes,
                &parents,
                &requested_colors(
                    parents
                        .iter()
                        .map(|parent| (parent.attr, 800))
                        .chain([(100, 800)]),
                ),
            )
            .map(|(colors, _)| colors.len())
        });
    }

    #[test]
    fn empty_color_request_set_does_not_walk_parents_or_runs() {
        let bytes = color(800, [0.25, 0.5, 0.75], false);
        let parents = [EntityRecord {
            attr: 700,
            seq: 4,
            disc: 16,
            refs: References {
                at: 0,
                count: 0,
                stride: 2,
            },
            end: 0,
        }];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            linked_colors(&ctx, &bytes, &parents, &super::ColorRequests::new())
                .unwrap()
                .0
                .is_empty()
        );
        ctx.finish_session().unwrap();
    }

    #[test]
    fn color_runs_bind_only_requested_face_attribute_pairs() {
        let mut bytes = Vec::new();
        let mut offsets = Vec::new();
        for attr in 800_u16..928 {
            offsets.push(bytes.len());
            bytes.extend(color(attr, [0.25, 0.5, 0.75], false));
        }
        let parents: Vec<_> = (200_u16..264)
            .map(|attr| EntityRecord {
                attr,
                seq: 4,
                disc: 16,
                refs: References {
                    at: 0,
                    count: 0,
                    stride: 2,
                },
                end: 0,
            })
            .collect();
        let requested = requested_colors(
            parents
                .iter()
                .map(|parent| (parent.attr, parent.attr + 600)),
        );
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let (colors, _storage) = linked_colors(&ctx, &bytes, &parents, &requested).unwrap();
        assert_eq!(colors.len(), 64);
        for parent in &parents {
            let linked = colors.get(&(parent.attr, parent.attr + 600)).unwrap();
            assert!(linked.agreed);
            assert_eq!(
                linked.current.offset,
                offsets[usize::from(parent.attr - 200)]
            );
            assert_eq!(linked.current.parent_seq, 4);
            assert!(!colors.contains_key(&(parent.attr, 927)));
        }
    }

    #[test]
    fn repeated_color_attribute_is_reduced_before_parent_binding() {
        let mut bytes = Vec::new();
        for _ in 0..128 {
            bytes.extend(color(800, [0.25, 0.5, 0.75], false));
        }
        let parents: Vec<_> = (200..264)
            .map(|attr| EntityRecord {
                attr,
                seq: 4,
                disc: 16,
                refs: References {
                    at: 0,
                    count: 0,
                    stride: 2,
                },
                end: 0,
            })
            .collect();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let (colors, _storage) = linked_colors(
            &ctx,
            &bytes,
            &parents,
            &requested_colors(parents.iter().map(|parent| (parent.attr, 800))),
        )
        .unwrap();
        assert_eq!(colors.len(), 64);
        for parent in &parents {
            let linked = colors.get(&(parent.attr, 800)).unwrap();
            assert!(linked.agreed);
            assert_eq!(linked.current.offset, 0);
            assert_eq!(linked.current.parent_seq, 4);
            assert_eq!(
                linked.current.color,
                super::color_record(&bytes, 0).unwrap().1
            );
        }
        crate::test_support::work_refusal_at("reduce Parasolid color runs", |ctx| {
            linked_colors(
                ctx,
                &bytes,
                &parents,
                &requested_colors(parents.iter().map(|parent| (parent.attr, 800))),
            )
            .map(|(colors, _)| colors.len())
        });
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
            linked_colors(
                ctx,
                &bytes,
                &entities,
                &requested_colors([(700, 900), (900, 900)]),
            )
            .map(|(colors, _)| colors.len())
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
