// SPDX-License-Identifier: Apache-2.0
//! Parasolid attribute dictionary and the per-face producing-feature identity
//! it carries ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
//!
//! A partition stream declares its attribute families inline. Each family is a
//! name record `00 4f` immediately followed by a definition record `00 50`.
//! Attribute instances `00 51` name their definition and the entity they hang
//! on. Integer payloads live in separate `00 52` list records the instance
//! references by node id.
//!
//! The `ATOM_ID_2001` family binds a face to the history feature that produced
//! it. The `LAST_BODY_MODIFYING_FEATURE_ID` family binds a body to the last
//! modeling-history ordinal that wrote it. Deltas streams carry no attribute
//! dictionary, so a deltas body yields no bindings.
//!
//! One pass over a stream body reads the definitions. When a definition names
//! a family whose payloads the decoder reads, one more pass reads the integer
//! lists and the instances of those families together.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use std::collections::BTreeMap;

use crate::layout::attribute_instance_00_51 as attr_inst;

/// Attribute family binding a face to its producing feature.
const ATOM_ID: &[u8] = b"ATOM_ID_2001";

/// Attribute family carrying a body's last modeling-history ordinal.
const LAST_BODY_MODIFIER: &[u8] = b"LAST_BODY_MODIFYING_FEATURE_ID";

/// Attribute family whose inline `REAL_VALUES` child carries face color.
const FACE_COLOR: &[u8] = b"SDL/TYSA_COLOUR";

/// Record tags that terminate an instance record's trailing reference run.
const NODE_TAGS: [u8; 6] = [0x4f, 0x50, 0x51, 0x52, 0x53, 0x54];

/// Widths of an `ATOM_ID_2001` payload list.
const ATOM_WIDTHS: std::ops::RangeInclusive<usize> = 5..=7;

/// Largest integer list width any supported family reads.
const MAX_LIST_WIDTH: usize = 7;

/// Payload position of the producing feature's native source id.
const ATOM_FEATURE: usize = 1;

/// Payload position that is zero on a face-identity payload.
const ATOM_GUARD: usize = 3;

/// Payload position of the feature-local face identity.
const ATOM_LOCAL: usize = 4;

/// One face's producing-feature identity.
#[derive(Debug, Clone)]
pub(super) struct RawFaceAtom {
    /// Attribute id of the face bridge record owning the attribute.
    pub(super) face_attr: u16,
    pub(super) identity: Option<super::PersistentFaceIdentity>,
}

/// A persistent identity bound to an emitted face.
#[derive(Debug, Clone)]
pub(crate) struct FaceAtom {
    pub(crate) face: cadmpeg_ir::ids::FaceId,
    pub(crate) identity: super::PersistentFaceIdentity,
}

/// One body's last modifying history ordinal.
#[derive(Debug, Clone)]
pub(crate) struct BodyModifier {
    /// Attribute id of the body carrying the attribute.
    pub(super) body_attr: u16,
    /// One-based ordinal in the ordered Keywords modeling-feature records.
    pub(crate) history_ordinal: u32,
    /// Emitted body identity, resolved once the graph retains its bodies.
    pub(crate) target: Option<String>,
}

/// The attribute families the decoder interprets, by declared name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// `ATOM_ID_2001`.
    FaceAtom,
    /// `LAST_BODY_MODIFYING_FEATURE_ID`.
    BodyModifier,
    /// `SDL/TYSA_COLOUR`.
    FaceColor,
    /// Any other printable name.
    Other,
}

impl Family {
    /// Classify a declared name. Slice equality compares lengths first, so the
    /// work is bounded by the longest known name.
    fn of(name: &[u8]) -> Self {
        match name {
            ATOM_ID => Self::FaceAtom,
            LAST_BODY_MODIFIER => Self::BodyModifier,
            FACE_COLOR => Self::FaceColor,
            _ => Self::Other,
        }
    }

    /// Whether this family's instances carry payloads the decoder reads.
    fn has_bindings(self) -> bool {
        matches!(self, Self::FaceAtom | Self::BodyModifier)
    }
}

/// Stream-local attribute definitions keyed by definition node id. A node
/// declared with names of different families maps to `None`. The table is
/// decode scratch held under its own scoped reservation.
pub(super) struct Dictionary<'ctx> {
    families: BTreeMap<u16, Option<Family>>,
    _storage: ScopedReservation<'ctx>,
}

impl Dictionary<'_> {
    /// The family declared for a definition node: `None` when no name record
    /// declares the node, `Some(None)` when its declarations conflict.
    pub(super) fn family(
        &self,
        ctx: &DecodeContext<'_>,
        node: u16,
    ) -> Result<Option<Option<Family>>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_btree_map(
                &self.families,
                &node,
                "look up Parasolid attribute definitions",
            )?
            .copied())
    }
}

/// Producing-feature identities and body modifiers carried by one stream body.
#[derive(Debug, Default)]
pub(super) struct Bindings {
    pub(super) face_atoms: Vec<RawFaceAtom>,
    pub(super) body_modifiers: Vec<BodyModifier>,
}

/// Start of a record body: the tag, then an optional `0xff` marker.
fn record_body(buf: &[u8], off: usize, tag: u8) -> Option<usize> {
    if buf.get(off) != Some(&0) || buf.get(off + 1) != Some(&tag) {
        return None;
    }
    let p = off + 2;
    Some(if buf.get(p) == Some(&0xff) { p + 1 } else { p })
}

/// Whether a record tag opens at `at`.
fn opens_record(buf: &[u8], at: usize) -> bool {
    buf.get(at) == Some(&0) && buf.get(at + 1).is_some_and(|tag| NODE_TAGS.contains(tag))
}

/// The printable-ASCII run measured last: every byte in `start..end` is an
/// ASCII graphic, and `end` is the buffer length or a byte that is not.
#[derive(Default)]
struct GraphicRun {
    measured: Option<(usize, usize)>,
}

impl GraphicRun {
    /// End of the printable-ASCII run that starts at `at`. The definition scan
    /// asks at non-decreasing positions, so a measured run answers every later
    /// position inside it and each byte is measured at most once.
    fn end(
        &mut self,
        ctx: &DecodeContext<'_>,
        buf: &[u8],
        at: usize,
    ) -> Result<usize, cadmpeg_core::CodecError> {
        if let Some((start, end)) = self.measured {
            if start <= at && at <= end {
                return Ok(end);
            }
        }
        let rest = buf.get(at..).unwrap_or_default();
        let run = ctx
            .position_by(
                rest,
                |byte| Ok(!byte.is_ascii_graphic()),
                "measure Parasolid attribute names",
            )?
            .unwrap_or(rest.len());
        let end = at + run;
        self.measured = Some((at, end));
        Ok(end)
    }
}

/// Read the stream-local attribute definitions: each printable name record
/// immediately followed by its definition record.
pub(super) fn dictionary<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    buf: &[u8],
) -> Result<Dictionary<'ctx>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold Parasolid attribute definitions")?;
    let mut families = BTreeMap::<u16, Option<Family>>::new();
    let mut names = GraphicRun::default();
    for off in ctx.admit_iter(0..buf.len(), "scan Parasolid attribute definitions")? {
        let Some(p) = record_body(buf, off, 0x4f) else {
            continue;
        };
        let (Some(len), Some(node)) = (View::u32_be_at(buf, p), View::u16_be_at(buf, p + 4)) else {
            continue;
        };
        if node <= 1 {
            continue;
        }
        let Some(data) = p.checked_add(6) else {
            continue;
        };
        let Some(len) = cadmpeg_core::decode::bounded_len(
            u64::from(len),
            1,
            buf.len().checked_sub(data).map_or(0, |len| len),
        ) else {
            continue;
        };
        let Some(end) = data.checked_add(len) else {
            continue;
        };
        // The name is a nonempty printable run that ends where the next record opens.
        if end == data || names.end(ctx, buf, data)? != end {
            continue;
        }
        let Some(p) = record_body(buf, end, 0x50) else {
            continue;
        };
        let Some(definition) = View::u16_be_at(buf, p + 4).filter(|node| *node > 1) else {
            continue;
        };
        let Some(name) = buf.get(data..end) else {
            continue;
        };
        let family = Family::of(name);
        storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut families,
                    definition,
                    "collect Parasolid attribute definitions",
                )
            })?
            .and_modify(|existing| {
                if *existing != Some(family) {
                    *existing = None;
                }
            })
            .or_insert(Some(family));
    }
    Ok(Dictionary {
        families,
        _storage: storage,
    })
}

/// One integer payload list. Supported families read at most
/// [`MAX_LIST_WIDTH`] values, so a list is held inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntegerList {
    values: [u32; MAX_LIST_WIDTH],
    len: usize,
}

impl IntegerList {
    fn as_slice(&self) -> &[u32] {
        self.values.get(..self.len).unwrap_or_default()
    }
}

/// Read a `00 52` list body: node id and values, when its width is one a
/// supported family reads.
fn integer_list(buf: &[u8], p: usize) -> Option<(u16, IntegerList)> {
    let count = View::u32_be_at(buf, p)?;
    let node = View::u16_be_at(buf, p + 4).filter(|node| *node > 1)?;
    let data = p.checked_add(6)?;
    let count = cadmpeg_core::decode::bounded_len(
        u64::from(count),
        4,
        buf.len().checked_sub(data).map_or(0, |len| len),
    )?;
    if count != 1 && !ATOM_WIDTHS.contains(&count) {
        return None;
    }
    let mut list = IntegerList {
        values: [0; MAX_LIST_WIDTH],
        len: count,
    };
    for (index, slot) in list.values.iter_mut().take(count).enumerate() {
        *slot = View::u32_be_at(buf, data.checked_add(index.checked_mul(4)?)?)?;
    }
    Some((node, list))
}

/// An attribute instance of a family with bindings, waiting for the integer
/// lists of the whole body before its references resolve.
struct Instance {
    family: Family,
    owner: u16,
    references: usize,
}

/// Integer lists keyed by node id; a node carrying different lists maps to `None`.
type IntegerLists = BTreeMap<u16, Option<IntegerList>>;

/// Return the one distinct accepted integer list an instance references.
fn referenced_payload<'a>(
    ctx: &DecodeContext<'_>,
    buf: &[u8],
    from: usize,
    lists: &'a IntegerLists,
    accepts: impl Fn(&[u32]) -> bool,
) -> Result<Option<&'a [u32]>, cadmpeg_core::CodecError> {
    let mut found: Option<&[u32]> = None;
    let mut at = from;
    while at + 2 <= buf.len() && !opens_record(buf, at) {
        ctx.charge_work(1, "scan Parasolid attribute references")?;
        let Some(node) = View::u16_be_at(buf, at) else {
            return Ok(None);
        };
        if let Some(Some(list)) =
            ctx.get_btree_map(lists, &node, "look up Parasolid attribute value lists")?
        {
            let values = list.as_slice();
            if accepts(values) {
                match found {
                    Some(previous) if previous != values => return Ok(None),
                    _ => found = Some(values),
                }
            }
        }
        at += 2;
    }
    Ok(found)
}

/// A face-identity payload: the producing feature, the feature-local face id
/// and the optional trailing fields, or `None` for an absent feature source.
type AtomIdentity<'a> = Option<(super::feature_source::FeatureSourceId, u32, &'a [u32])>;

fn atom_identity(values: &[u32]) -> Option<AtomIdentity<'_>> {
    let (head, trailing_fields) = values.split_first_chunk::<5>()?;
    Some(
        super::feature_source::FeatureSourceId::try_from(head[ATOM_FEATURE])
            .ok()
            .map(|source| (source, head[ATOM_LOCAL], trailing_fields)),
    )
}

/// Decode the `ATOM_ID_2001` face bindings and the body last-modifier
/// bindings carried by one stream body.
pub(super) fn bindings(
    ctx: &DecodeContext<'_>,
    buf: &[u8],
    dictionary: &Dictionary<'_>,
) -> Result<Bindings, cadmpeg_core::CodecError> {
    if !ctx.any_by(
        dictionary.families.values(),
        |family| Ok(family.is_some_and(Family::has_bindings)),
        "find Parasolid attribute families with bindings",
    )? {
        return Ok(Bindings::default());
    }
    let mut storage = ctx.reserve_scoped(0, "hold Parasolid attribute payloads")?;
    let mut lists = IntegerLists::new();
    let mut instances = Vec::<Instance>::new();
    for off in ctx.admit_iter(0..buf.len(), "scan Parasolid attribute payloads")? {
        if let Some(p) = record_body(buf, off, 0x52) {
            let Some((node, list)) = integer_list(buf, p) else {
                continue;
            };
            storage
                .with_storage(|| {
                    ctx.entry_btree_map(&mut lists, node, "collect Parasolid attribute value lists")
                })?
                .and_modify(|existing| {
                    if *existing != Some(list) {
                        *existing = None;
                    }
                })
                .or_insert(Some(list));
            continue;
        }
        let Some(p) = record_body(buf, off, 0x51) else {
            continue;
        };
        if View::u16_be_at(buf, p + attr_inst::ZERO_SELECTOR) != Some(0) {
            continue;
        }
        let Some(definition) = View::u16_be_at(buf, p + attr_inst::DEFINITION_NODE_ID) else {
            continue;
        };
        let Some(Some(family)) = dictionary.family(ctx, definition)? else {
            continue;
        };
        if !family.has_bindings() {
            continue;
        }
        let Some(owner) =
            View::u16_be_at(buf, p + attr_inst::OWNER_ATTRIBUTE_ID).filter(|attr| *attr > 1)
        else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut instances,
            Instance {
                family,
                owner,
                references: p + attr_inst::LEN,
            },
            "collect Parasolid attribute instances",
        )?;
    }

    let mut atoms = BTreeMap::<u16, Option<AtomIdentity<'_>>>::new();
    let mut modifiers = BTreeMap::<u16, Option<u32>>::new();
    for instance in ctx.admit_iter(&instances, "resolve Parasolid attribute instances")? {
        if instance.family == Family::FaceAtom {
            let Some(identity) =
                referenced_payload(ctx, buf, instance.references, &lists, |values| {
                    ATOM_WIDTHS.contains(&values.len()) && values.get(ATOM_GUARD) == Some(&0)
                })?
                .and_then(atom_identity)
            else {
                continue;
            };
            storage
                .with_storage(|| {
                    ctx.entry_btree_map(&mut atoms, instance.owner, "collect Parasolid face atoms")
                })?
                .and_modify(|existing| {
                    if *existing != Some(identity) {
                        *existing = None;
                    }
                })
                .or_insert(Some(identity));
        } else {
            let ordinal = referenced_payload(ctx, buf, instance.references, &lists, |values| {
                values.len() == 1 && values[0] > 0
            })?
            .and_then(<[u32]>::first)
            .copied();
            storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut modifiers,
                        instance.owner,
                        "collect Parasolid body modifiers",
                    )
                })?
                .and_modify(|existing| {
                    if *existing != ordinal {
                        *existing = None;
                    }
                })
                .or_insert(ordinal);
        }
    }

    // Both maps order their bindings by owner attribute.
    let mut face_atoms = Vec::new();
    for (face_attr, identity) in ctx.admit_iter(atoms, "retain Parasolid face atoms")? {
        let Some(identity) = identity else {
            continue;
        };
        let identity = match identity {
            Some((feature_source_id, local_id, trailing_fields)) => {
                Some(super::PersistentFaceIdentity {
                    feature_source_id,
                    local_id,
                    trailing_fields: ctx
                        .copy_slice(trailing_fields, "copy Parasolid face identity fields")?,
                })
            }
            None => None,
        };
        ctx.push_vec(
            &mut face_atoms,
            RawFaceAtom {
                face_attr,
                identity,
            },
            "retain Parasolid face atoms",
        )?;
    }
    let mut body_modifiers = Vec::new();
    for (body_attr, ordinal) in ctx.admit_iter(modifiers, "retain Parasolid body modifiers")? {
        let Some(history_ordinal) = ordinal else {
            continue;
        };
        ctx.push_vec(
            &mut body_modifiers,
            BodyModifier {
                body_attr,
                history_ordinal,
                target: None,
            },
            "retain Parasolid body modifiers",
        )?;
    }
    Ok(Bindings {
        face_atoms,
        body_modifiers,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        bindings, dictionary, integer_list, Bindings, Family, GraphicRun, ATOM_ID, FACE_COLOR,
        LAST_BODY_MODIFIER,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn append_definition(out: &mut Vec<u8>, family: &[u8], name_node: u16, definition: u16) {
        out.extend([0x00, 0x4f]);
        out.extend(
            u32::try_from(family.len())
                .expect("family length fits u32")
                .to_be_bytes(),
        );
        out.extend(name_node.to_be_bytes());
        out.extend(family);
        out.extend([0x00, 0x50]);
        out.extend(2_u32.to_be_bytes());
        out.extend(definition.to_be_bytes());
    }

    /// Serialize one attribute family, one payload list, and one instance.
    fn stream(payload: &[u32], face_attr: u16) -> Vec<u8> {
        stream_with_list_node(payload, face_attr, 300)
    }

    fn stream_with_list_node(payload: &[u32], face_attr: u16, list_node: u16) -> Vec<u8> {
        let mut out = Vec::new();
        append_definition(&mut out, ATOM_ID, 15, 16);
        out.extend([0x00, 0x52]);
        out.extend(
            u32::try_from(payload.len())
                .expect("payload length fits u32")
                .to_be_bytes(),
        );
        out.extend(list_node.to_be_bytes());
        for value in payload {
            out.extend(value.to_be_bytes());
        }
        out.extend([0x00, 0x51]);
        out.extend(4_u32.to_be_bytes());
        out.extend(301_u16.to_be_bytes());
        out.extend(0_u16.to_be_bytes());
        out.extend(302_u16.to_be_bytes());
        out.extend(16_u16.to_be_bytes());
        out.extend(face_attr.to_be_bytes());
        out.extend(list_node.to_be_bytes());
        out
    }

    /// Serialize one body-modifier family, scalar lists, and one instance.
    fn body_modifier_stream(payloads: &[&[u32]], body_attr: u16) -> Vec<u8> {
        body_modifier_stream_with_nodes(payloads, body_attr, 300, 1)
    }

    /// As [`body_modifier_stream`], with list node ids `first + index * step`.
    fn body_modifier_stream_with_nodes(
        payloads: &[&[u32]],
        body_attr: u16,
        first: u16,
        step: u16,
    ) -> Vec<u8> {
        let node = |index: usize| first + u16::try_from(index).expect("index fits u16") * step;
        let mut out = Vec::new();
        append_definition(&mut out, LAST_BODY_MODIFIER, 15, 16);
        for (index, payload) in payloads.iter().enumerate() {
            out.extend([0x00, 0x52]);
            out.extend(
                u32::try_from(payload.len())
                    .expect("payload length fits u32")
                    .to_be_bytes(),
            );
            out.extend(node(index).to_be_bytes());
            for value in *payload {
                out.extend(value.to_be_bytes());
            }
        }
        out.extend([0x00, 0x51]);
        out.extend(4_u32.to_be_bytes());
        out.extend(301_u16.to_be_bytes());
        out.extend(0_u16.to_be_bytes());
        out.extend(302_u16.to_be_bytes());
        out.extend(16_u16.to_be_bytes());
        out.extend(body_attr.to_be_bytes());
        for index in 0..payloads.len() {
            out.extend(node(index).to_be_bytes());
        }
        out
    }

    fn scan(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Bindings, cadmpeg_core::CodecError> {
        let dictionary = dictionary(ctx, bytes)?;
        bindings(ctx, bytes, &dictionary)
    }

    fn scan_service(bytes: &[u8]) -> Bindings {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
        scan(&ctx, bytes).expect("attribute scan")
    }

    fn declared(bytes: &[u8], node: u16) -> Option<Option<Family>> {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
        let dictionary = dictionary(&ctx, bytes).expect("definitions");
        let family = dictionary.family(&ctx, node).expect("definition lookup");
        family
    }

    fn refusal(dimension: ResourceDimension, operation: &'static str, bytes: &[u8]) {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => unreachable!("attribute scans charge no other dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let result = scan(&ctx, bytes);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn attribute_scans_refuse_work_before_each_pass() {
        let bytes = stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333);
        for operation in [
            "scan Parasolid attribute definitions",
            "measure Parasolid attribute names",
            "find Parasolid attribute families with bindings",
            "scan Parasolid attribute payloads",
            "look up Parasolid attribute definitions",
            "resolve Parasolid attribute instances",
            "scan Parasolid attribute references",
            "look up Parasolid attribute value lists",
            "retain Parasolid face atoms",
            "copy Parasolid face identity fields",
        ] {
            refusal(ResourceDimension::WorkUnits, operation, &bytes);
        }
        let bytes = body_modifier_stream(&[&[2]], 333);
        refusal(
            ResourceDimension::WorkUnits,
            "retain Parasolid body modifiers",
            &bytes,
        );
    }

    #[test]
    fn attribute_collections_refuse_before_growth() {
        let bytes = stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333);
        for operation in [
            "collect Parasolid attribute definitions",
            "collect Parasolid attribute value lists",
            "collect Parasolid attribute instances",
            "collect Parasolid face atoms",
            "copy Parasolid face identity fields",
            "retain Parasolid face atoms",
        ] {
            refusal(ResourceDimension::CollectionItems, operation, &bytes);
        }
        let bytes = body_modifier_stream(&[&[2]], 333);
        for operation in [
            "collect Parasolid body modifiers",
            "retain Parasolid body modifiers",
        ] {
            refusal(ResourceDimension::CollectionItems, operation, &bytes);
        }
    }

    #[test]
    fn attribute_scratch_tables_are_scoped() {
        let bytes = stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333);
        refusal(
            ResourceDimension::MaterializedBytes,
            "collect Parasolid attribute definitions",
            &bytes,
        );
        refusal(
            ResourceDimension::MaterializedBytes,
            "collect Parasolid attribute value lists",
            &bytes,
        );
        refusal(
            ResourceDimension::RetainedBytes,
            "retain Parasolid face atoms",
            &bytes,
        );
    }

    #[test]
    fn family_names_are_classified_without_retained_copies() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 15, 16);
        append_definition(&mut bytes, FACE_COLOR, 17, 18);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let dictionary = dictionary(&ctx, &bytes).expect("definitions without retained bytes");
        assert_eq!(
            dictionary.family(&ctx, 16).expect("lookup"),
            Some(Some(Family::FaceAtom))
        );
        assert_eq!(
            dictionary.family(&ctx, 18).expect("lookup"),
            Some(Some(Family::FaceColor))
        );
        assert_eq!(dictionary.family(&ctx, 19).expect("lookup"), None);
    }

    #[test]
    fn graphic_runs_answer_later_positions_inside_a_measured_run() {
        let bytes = b"\0ABC\0DE";
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
        let mut runs = GraphicRun::default();
        let ends = [0, 1, 2, 3, 4, 5, 6, 7, 9].map(|at| runs.end(&ctx, bytes, at).expect("run"));
        assert_eq!(ends, [0, 4, 4, 4, 4, 7, 7, 7, 9]);
    }

    #[test]
    fn atom_source_sentinels_have_no_identity() {
        for source in [0, u32::MAX] {
            let atoms = scan_service(&stream(&[74, source, 1_390_698_820, 0, 3], 333)).face_atoms;
            assert_eq!(atoms.len(), 1);
            assert_eq!(atoms[0].face_attr, 333);
            assert!(atoms[0].identity.is_none());
        }
    }

    #[test]
    fn instance_binds_face_to_producing_feature() {
        let atoms = scan_service(&stream(&[74, 75, 1_390_698_820, 0, 3], 333)).face_atoms;
        assert_eq!(atoms.len(), 1);
        assert_eq!(atoms[0].face_attr, 333);
        let identity = atoms[0].identity.as_ref().expect("identity");
        assert_eq!(identity.feature_source_id.value(), 75);
        assert_eq!(identity.local_id, 3);
        assert!(identity.trailing_fields.is_empty());
    }

    #[test]
    fn instance_preserves_optional_persistent_tail() {
        let atoms = scan_service(&stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333)).face_atoms;
        assert_eq!(atoms.len(), 1);
        assert_eq!(
            atoms[0]
                .identity
                .as_ref()
                .expect("identity")
                .trailing_fields,
            vec![10, 8]
        );
    }

    #[test]
    fn payload_with_a_nonzero_guard_position_is_not_a_face_identity() {
        assert!(scan_service(&stream(&[74, 75, 1_390_698_820, 9, 3], 333))
            .face_atoms
            .is_empty());
    }

    #[test]
    fn stream_without_the_family_declaration_yields_nothing() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        body[8] = b'X';
        let found = scan_service(&body);
        assert!(found.face_atoms.is_empty());
        assert!(found.body_modifiers.is_empty());
    }

    #[test]
    fn a_name_must_be_printable_up_to_the_definition_record() {
        let mut body = Vec::new();
        append_definition(&mut body, b"ATOM_ID\x012001", 15, 16);
        assert_eq!(declared(&body, 16), None);
    }

    #[test]
    fn conflicting_definition_identity_is_withheld() {
        let mut body = Vec::new();
        append_definition(&mut body, ATOM_ID, 15, 16);
        append_definition(&mut body, LAST_BODY_MODIFIER, 17, 16);
        assert_eq!(declared(&body, 16), Some(None));
    }

    #[test]
    fn unsupported_families_are_declared_as_other() {
        let mut body = Vec::new();
        append_definition(&mut body, b"OTHER_FAMILY", 15, 16);
        assert_eq!(declared(&body, 16), Some(Some(Family::Other)));
    }

    #[test]
    fn unsupported_and_truncated_integer_lists_are_not_candidates() {
        let mut body = Vec::new();
        body.extend(2_u32.to_be_bytes());
        body.extend(300_u16.to_be_bytes());
        body.extend(1_u32.to_be_bytes());
        body.extend(2_u32.to_be_bytes());
        assert!(integer_list(&body, 0).is_none());

        let mut body = Vec::new();
        body.extend(5_u32.to_be_bytes());
        body.extend(301_u16.to_be_bytes());
        body.extend(1_u32.to_be_bytes());
        assert!(integer_list(&body, 0).is_none());

        let mut body = Vec::new();
        body.extend(1_u32.to_be_bytes());
        body.extend(302_u16.to_be_bytes());
        body.extend(9_u32.to_be_bytes());
        let (node, list) = integer_list(&body, 0).expect("scalar list");
        assert_eq!((node, list.as_slice()), (302, [9].as_slice()));
    }

    #[test]
    fn conflicting_integer_list_identity_is_withheld() {
        let modifiers = scan_service(&body_modifier_stream_with_nodes(&[&[2], &[3]], 333, 300, 0))
            .body_modifiers;
        assert!(modifiers.is_empty());
    }

    #[test]
    fn conflicting_face_identity_is_withheld() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        body.extend(stream_with_list_node(
            &[74, 76, 1_390_698_820, 0, 4],
            333,
            310,
        ));
        assert!(scan_service(&body).face_atoms.is_empty());
    }

    #[test]
    fn conflicting_persistent_tail_is_withheld() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3, 10], 333);
        body.extend(stream_with_list_node(
            &[74, 75, 1_390_698_820, 0, 3, 11],
            333,
            310,
        ));
        assert!(scan_service(&body).face_atoms.is_empty());
    }

    #[test]
    fn body_modifier_binds_one_history_ordinal() {
        let modifiers = scan_service(&body_modifier_stream(&[&[2]], 333)).body_modifiers;
        assert_eq!(modifiers.len(), 1);
        assert_eq!(modifiers[0].body_attr, 333);
        assert_eq!(modifiers[0].history_ordinal, 2);
    }

    #[test]
    fn body_modifier_rejects_non_scalar_and_conflicting_payloads() {
        assert!(scan_service(&body_modifier_stream(&[&[0]], 333))
            .body_modifiers
            .is_empty());
        assert!(scan_service(&body_modifier_stream(&[&[2, 3]], 333))
            .body_modifiers
            .is_empty());
        assert!(scan_service(&body_modifier_stream(&[&[2], &[3]], 333))
            .body_modifiers
            .is_empty());
    }
}
