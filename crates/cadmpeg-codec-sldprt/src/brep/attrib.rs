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

use cadmpeg_core::decode::{DecodeContext, View};
use std::collections::HashMap;

use crate::layout::attribute_instance_00_51 as attr_inst;

/// Attribute family binding a face to its producing feature.
const ATOM_ID: &str = "ATOM_ID_2001";

/// Attribute family carrying a body's last modeling-history ordinal.
const LAST_BODY_MODIFIER: &str = "LAST_BODY_MODIFYING_FEATURE_ID";

/// Record tags that terminate an instance record's trailing reference run.
const NODE_TAGS: [u8; 6] = [0x4f, 0x50, 0x51, 0x52, 0x53, 0x54];

/// Widths of an `ATOM_ID_2001` payload list.
const ATOM_WIDTHS: std::ops::RangeInclusive<usize> = 5..=7;

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

/// Stream-local attribute definitions with unique names or withheld conflicts.
type DefinitionTable = HashMap<u16, Option<String>>;

fn charge_items(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, operation)?;
    }
    Ok(())
}

/// Collect valid `KEY/ATTRIB_DEF` pairings, retaining conflicts as `None`.
fn definition_candidates(
    ctx: Option<&DecodeContext<'_>>,
    buf: &[u8],
) -> Result<HashMap<u16, Option<Vec<u8>>>, cadmpeg_core::CodecError> {
    let mut found = HashMap::<u16, Option<Vec<u8>>>::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x4f) else {
            continue;
        };
        let Some(len) = View::u32_be_at(buf, p) else {
            continue;
        };
        let Some(node) = View::u16_be_at(buf, p + 4) else {
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
        let Some(text) = buf.get(data..end) else {
            continue;
        };
        if text.is_empty()
            || text
                .iter()
                .any(|byte| !byte.is_ascii() || !byte.is_ascii_graphic())
        {
            continue;
        }
        let Some(p) = record_body(buf, end, 0x50) else {
            continue;
        };
        let Some(definition) = View::u16_be_at(buf, p + 4).filter(|node| *node > 1) else {
            continue;
        };
        if let Some(ctx) = ctx {
            ctx.charge_retained(text.len() as u64, "copy Parasolid attribute family")?;
        }
        let family = text.to_vec();
        match found.entry(definition) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                charge_items(ctx, 1, "collect Parasolid attribute definitions")?;
                entry.insert(Some(family));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry
                    .get()
                    .as_ref()
                    .is_some_and(|previous| previous != &family)
                {
                    *entry.get_mut() = None;
                }
            }
        }
    }
    Ok(found)
}

/// Resolve the stream-local attribute-definition table.
pub(super) fn definition_table(
    ctx: Option<&DecodeContext<'_>>,
    buf: &[u8],
) -> Result<DefinitionTable, cadmpeg_core::CodecError> {
    let candidates = definition_candidates(ctx, buf)?;
    charge_items(
        ctx,
        candidates.len(),
        "resolve Parasolid attribute definitions",
    )?;
    Ok(candidates
        .into_iter()
        .map(|(node, family)| {
            (
                node,
                family.and_then(|family| String::from_utf8(family).ok()),
            )
        })
        .collect())
}

/// Map definition-record node ids to the two supported native attribute
/// families whose payload consumers are implemented here.
fn definitions(
    ctx: Option<&DecodeContext<'_>>,
    buf: &[u8],
) -> Result<HashMap<u16, &'static str>, cadmpeg_core::CodecError> {
    let definitions = definition_table(ctx, buf)?;
    charge_items(
        ctx,
        definitions.values().filter(|name| name.is_some()).count(),
        "collect Parasolid supported definitions",
    )?;
    Ok(definitions
        .into_iter()
        .filter_map(|(node, family)| {
            let family = family?;
            let family = match family.as_str() {
                ATOM_ID => ATOM_ID,
                LAST_BODY_MODIFIER => LAST_BODY_MODIFIER,
                _ => return None,
            };
            Some((node, family))
        })
        .collect())
}

/// Read integer payload lists keyed by their node id.
fn integer_lists(
    ctx: Option<&DecodeContext<'_>>,
    buf: &[u8],
) -> Result<HashMap<u16, Vec<u32>>, cadmpeg_core::CodecError> {
    let mut found = HashMap::<u16, Option<Vec<u32>>>::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x52) else {
            continue;
        };
        let Some(count) = View::u32_be_at(buf, p) else {
            continue;
        };
        let Some(node) = View::u16_be_at(buf, p + 4).filter(|node| *node > 1) else {
            continue;
        };
        let Some(data) = p.checked_add(6) else {
            continue;
        };
        let Some(count) = cadmpeg_core::decode::bounded_len(
            u64::from(count),
            4,
            buf.len().checked_sub(data).map_or(0, |len| len),
        ) else {
            continue;
        };
        if count != 1 && !ATOM_WIDTHS.contains(&count) {
            continue;
        }
        charge_items(ctx, count, "decode Parasolid attribute values")?;
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let Some(value) = index
                .checked_mul(4)
                .and_then(|delta| data.checked_add(delta))
                .and_then(|at| View::u32_be_at(buf, at))
            else {
                values.clear();
                break;
            };
            values.push(value);
        }
        if values.len() == count {
            match found.entry(node) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    charge_items(ctx, 1, "collect Parasolid attribute value lists")?;
                    entry.insert(Some(values));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if entry
                        .get()
                        .as_ref()
                        .is_some_and(|previous| *previous != values)
                    {
                        *entry.get_mut() = None;
                    }
                }
            }
        }
    }
    charge_items(ctx, found.len(), "retain Parasolid attribute value lists")?;
    Ok(found
        .into_iter()
        .filter_map(|(node, values)| values.map(|values| (node, values)))
        .collect())
}

/// Return one distinct integer-list payload referenced by an instance.
fn referenced_payload<'a, F>(
    buf: &[u8],
    from: usize,
    lists: &'a HashMap<u16, Vec<u32>>,
    accepts: F,
) -> Option<&'a [u32]>
where
    F: Fn(&[u32]) -> bool,
{
    let mut found: Option<&[u32]> = None;
    let mut at = from;
    while at + 2 <= buf.len() && !opens_record(buf, at) {
        let node = View::u16_be_at(buf, at)?;
        if let Some(values) = lists.get(&node) {
            if accepts(values) {
                match found {
                    Some(previous) if previous != values.as_slice() => return None,
                    _ => found = Some(values),
                }
            }
        }
        at += 2;
    }
    found
}

/// The face-identity payload an instance references, when exactly one distinct
/// payload qualifies.
fn atom_payload<'a>(
    buf: &[u8],
    from: usize,
    lists: &'a HashMap<u16, Vec<u32>>,
) -> Option<(&'a [u32; 5], &'a [u32])> {
    referenced_payload(buf, from, lists, |values| {
        ATOM_WIDTHS.contains(&values.len()) && values.get(ATOM_GUARD) == Some(&0)
    })?
    .split_first_chunk::<5>()
}

/// Decode every `ATOM_ID_2001` binding carried by one stream body.
pub(super) fn scan(
    ctx: Option<&DecodeContext<'_>>,
    buf: &[u8],
) -> Result<Vec<RawFaceAtom>, cadmpeg_core::CodecError> {
    let definitions = definitions(ctx, buf)?;
    if !definitions.values().any(|name| *name == ATOM_ID) {
        return Ok(Vec::new());
    }
    let lists = integer_lists(ctx, buf)?;
    let mut found = HashMap::<u16, Option<RawFaceAtom>>::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x51) else {
            continue;
        };
        if View::u16_be_at(buf, p + attr_inst::ZERO_SELECTOR) != Some(0) {
            continue;
        }
        let Some(definition) = View::u16_be_at(buf, p + attr_inst::DEFINITION_NODE_ID) else {
            continue;
        };
        if definitions.get(&definition).copied() != Some(ATOM_ID) {
            continue;
        }
        let Some(face_attr) = View::u16_be_at(buf, p + attr_inst::OWNER_ATTRIBUTE_ID) else {
            continue;
        };
        if face_attr <= 1 {
            continue;
        }
        let Some((values, trailing_fields)) = atom_payload(buf, p + attr_inst::LEN, &lists) else {
            continue;
        };
        let identity = if let Ok(feature_source_id) =
            super::feature_source::FeatureSourceId::try_from(values[ATOM_FEATURE])
        {
            charge_items(
                ctx,
                trailing_fields.len(),
                "copy Parasolid face identity fields",
            )?;
            Some(super::PersistentFaceIdentity {
                feature_source_id,
                local_id: values[ATOM_LOCAL],
                trailing_fields: trailing_fields.to_vec(),
            })
        } else {
            None
        };
        let atom = RawFaceAtom {
            face_attr,
            identity,
        };
        match found.entry(face_attr) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                charge_items(ctx, 1, "collect Parasolid face atoms")?;
                entry.insert(Some(atom));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry
                    .get()
                    .as_ref()
                    .is_some_and(|previous| previous.identity != atom.identity)
                {
                    *entry.get_mut() = None;
                }
            }
        }
    }
    charge_items(ctx, found.len(), "retain Parasolid face atoms")?;
    let mut out = found.into_values().flatten().collect::<Vec<_>>();
    out.sort_by_key(|atom| atom.face_attr);
    Ok(out)
}

/// Decode every body-level last-modifier binding carried by one stream body.
pub(super) fn scan_body_modifiers(
    ctx: Option<&DecodeContext<'_>>,
    buf: &[u8],
) -> Result<Vec<BodyModifier>, cadmpeg_core::CodecError> {
    let definitions = definitions(ctx, buf)?;
    if !definitions.values().any(|name| *name == LAST_BODY_MODIFIER) {
        return Ok(Vec::new());
    }
    let lists = integer_lists(ctx, buf)?;
    let mut found = HashMap::<u16, Option<BodyModifier>>::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x51) else {
            continue;
        };
        if View::u16_be_at(buf, p + attr_inst::ZERO_SELECTOR) != Some(0) {
            continue;
        }
        let Some(definition) = View::u16_be_at(buf, p + attr_inst::DEFINITION_NODE_ID) else {
            continue;
        };
        if definitions.get(&definition).copied() != Some(LAST_BODY_MODIFIER) {
            continue;
        }
        let Some(body_attr) =
            View::u16_be_at(buf, p + attr_inst::OWNER_ATTRIBUTE_ID).filter(|attr| *attr > 1)
        else {
            continue;
        };
        let Some(values) = referenced_payload(buf, p + attr_inst::LEN, &lists, |values| {
            values.len() == 1 && values[0] > 0
        }) else {
            if !found.contains_key(&body_attr) {
                charge_items(ctx, 1, "collect Parasolid body modifiers")?;
            }
            found.insert(body_attr, None);
            continue;
        };
        let modifier = BodyModifier {
            body_attr,
            history_ordinal: values[0],
            target: None,
        };
        match found.get_mut(&body_attr) {
            Some(slot)
                if slot.as_ref().is_some_and(|previous| {
                    previous.history_ordinal != modifier.history_ordinal
                }) =>
            {
                *slot = None;
            }
            Some(None) => {}
            Some(Some(_)) => {}
            None => {
                charge_items(ctx, 1, "collect Parasolid body modifiers")?;
                found.insert(body_attr, Some(modifier));
            }
        }
    }
    charge_items(ctx, found.len(), "retain Parasolid body modifiers")?;
    let mut out = found.into_values().flatten().collect::<Vec<_>>();
    out.sort_by_key(|modifier| modifier.body_attr);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        definition_table, definitions, integer_lists, scan, scan_body_modifiers, ATOM_ID,
        LAST_BODY_MODIFIER,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn append_definition(out: &mut Vec<u8>, family: &str, name_node: u16, definition: u16) {
        out.extend([0x00, 0x4f]);
        out.extend((family.len() as u32).to_be_bytes());
        out.extend(name_node.to_be_bytes());
        out.extend(family.as_bytes());
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
        out.extend((payload.len() as u32).to_be_bytes());
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

    /// Serialize one body-modifier family, one scalar list, and one instance.
    fn body_modifier_stream(payloads: &[&[u32]], body_attr: u16) -> Vec<u8> {
        let mut out = Vec::new();
        append_definition(&mut out, LAST_BODY_MODIFIER, 15, 16);
        for (index, payload) in payloads.iter().enumerate() {
            out.extend([0x00, 0x52]);
            out.extend((payload.len() as u32).to_be_bytes());
            out.extend((300 + index as u16).to_be_bytes());
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
            out.extend((300 + index as u16).to_be_bytes());
        }
        out
    }

    #[test]
    fn parasolid_attribute_values_refuse_collection_limit_before_allocation() {
        let bytes = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 4;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = integer_lists(Some(&ctx), &bytes).expect_err("five values exceed four items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid attribute values"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            integer_lists(Some(&ctx), &bytes)
                .expect("service scan")
                .get(&300)
                .map(Vec::len),
            Some(5)
        );
    }

    #[test]
    fn parasolid_attribute_family_refuses_retained_limit_before_copy() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 15, 16);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = ATOM_ID.len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error =
            definition_table(Some(&ctx), &bytes).expect_err("family copy exceeds retained limit");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "copy Parasolid attribute family"));
    }

    macro_rules! attribute_collection_boundary {
        ($name:ident, $bytes:expr, $route:ident, $limit:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let bytes = $bytes;
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = $limit;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                let error = $route(Some(&ctx), &bytes)
                    .expect_err("attribute collection exceeds the limit");
                assert!(matches!(error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::CollectionItems
                            && limit.operation == $operation), "{error:?}");
            }
        };
    }

    macro_rules! definition_boundary {
        ($name:ident, $route:ident, $limit:expr, $operation:literal) => {
            attribute_collection_boundary!(
                $name,
                {
                    let mut bytes = Vec::new();
                    append_definition(&mut bytes, ATOM_ID, 15, 16);
                    bytes
                },
                $route,
                $limit,
                $operation
            );
        };
    }

    definition_boundary!(
        parasolid_attribute_definitions_refuse_before_insertion,
        definition_table,
        0,
        "collect Parasolid attribute definitions"
    );
    definition_boundary!(
        parasolid_attribute_definition_table_refuses_before_resolution,
        definition_table,
        1,
        "resolve Parasolid attribute definitions"
    );
    definition_boundary!(
        parasolid_supported_definitions_refuse_before_collection,
        definitions,
        2,
        "collect Parasolid supported definitions"
    );
    #[test]
    fn parasolid_supported_definitions_fit_without_named_map_budget() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 15, 16);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        assert_eq!(
            definitions(Some(&ctx), &bytes)
                .expect("definition fits three charged items")
                .get(&16),
            Some(&ATOM_ID)
        );
    }
    attribute_collection_boundary!(
        parasolid_attribute_value_lists_refuse_before_insertion,
        stream(&[74, 75, 1_390_698_820, 0, 3], 333),
        integer_lists,
        5,
        "collect Parasolid attribute value lists"
    );
    attribute_collection_boundary!(
        parasolid_attribute_value_lists_refuse_before_retention,
        stream(&[74, 75, 1_390_698_820, 0, 3], 333),
        integer_lists,
        6,
        "retain Parasolid attribute value lists"
    );
    attribute_collection_boundary!(
        parasolid_face_identity_fields_refuse_before_copy,
        stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333),
        scan,
        12,
        "copy Parasolid face identity fields"
    );
    attribute_collection_boundary!(
        parasolid_face_atoms_refuse_before_insertion,
        stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333),
        scan,
        14,
        "collect Parasolid face atoms"
    );
    attribute_collection_boundary!(
        parasolid_face_atoms_refuse_before_retention,
        stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333),
        scan,
        15,
        "retain Parasolid face atoms"
    );
    attribute_collection_boundary!(
        parasolid_body_modifiers_refuse_before_insertion,
        body_modifier_stream(&[&[2]], 333),
        scan_body_modifiers,
        6,
        "collect Parasolid body modifiers"
    );
    attribute_collection_boundary!(
        parasolid_body_modifiers_refuse_before_retention,
        body_modifier_stream(&[&[2]], 333),
        scan_body_modifiers,
        7,
        "retain Parasolid body modifiers"
    );

    #[test]
    fn atom_source_sentinels_have_no_identity() {
        for source in [0, u32::MAX] {
            let atoms = scan(None, &stream(&[74, source, 1_390_698_820, 0, 3], 333))
                .expect("attribute scan");
            assert_eq!(atoms.len(), 1);
            assert_eq!(atoms[0].face_attr, 333);
            assert!(atoms[0].identity.is_none());
        }
    }

    #[test]
    fn instance_binds_face_to_producing_feature() {
        let atoms =
            scan(None, &stream(&[74, 75, 1_390_698_820, 0, 3], 333)).expect("attribute scan");
        assert_eq!(atoms.len(), 1);
        assert_eq!(atoms[0].face_attr, 333);
        assert_eq!(
            atoms[0]
                .identity
                .as_ref()
                .unwrap()
                .feature_source_id
                .value(),
            75
        );
        assert_eq!(atoms[0].identity.as_ref().unwrap().local_id, 3);
    }

    #[test]
    fn instance_preserves_optional_persistent_tail() {
        let atoms = scan(None, &stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333))
            .expect("attribute scan");
        assert_eq!(atoms.len(), 1);
        assert_eq!(
            atoms[0].identity.as_ref().unwrap().trailing_fields,
            vec![10, 8]
        );
    }

    #[test]
    fn payload_with_a_nonzero_guard_position_is_not_a_face_identity() {
        assert!(scan(None, &stream(&[74, 75, 1_390_698_820, 9, 3], 333))
            .expect("attribute scan")
            .is_empty());
    }

    #[test]
    fn stream_without_the_family_declaration_yields_nothing() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        body[8] = b'X';
        assert!(scan(None, &body).expect("attribute scan").is_empty());
    }

    #[test]
    fn conflicting_definition_identity_is_withheld() {
        let mut body = Vec::new();
        append_definition(&mut body, ATOM_ID, 15, 16);
        append_definition(&mut body, LAST_BODY_MODIFIER, 17, 16);
        assert!(!definitions(None, &body)
            .expect("definitions")
            .contains_key(&16));
        let table = definition_table(None, &body).expect("definition table");
        assert!(table.contains_key(&16));
        assert!(table.get(&16).is_some_and(Option::is_none));
    }

    #[test]
    fn definition_table_retains_unsupported_families() {
        let mut body = Vec::new();
        append_definition(&mut body, "SDL/TYSA_COLOUR", 15, 16);

        assert_eq!(
            definition_table(None, &body)
                .expect("definition table")
                .get(&16)
                .and_then(Option::as_deref),
            Some("SDL/TYSA_COLOUR")
        );
        assert!(!definitions(None, &body)
            .expect("definitions")
            .contains_key(&16));
    }

    #[test]
    fn unsupported_and_truncated_integer_lists_are_not_candidates() {
        let mut body = Vec::new();
        body.extend([0x00, 0x52]);
        body.extend(2_u32.to_be_bytes());
        body.extend(300_u16.to_be_bytes());
        body.extend(1_u32.to_be_bytes());
        body.extend(2_u32.to_be_bytes());
        body.extend([0x00, 0x52]);
        body.extend(5_u32.to_be_bytes());
        body.extend(301_u16.to_be_bytes());
        body.extend(1_u32.to_be_bytes());
        assert!(integer_lists(None, &body)
            .expect("integer lists")
            .is_empty());
    }

    #[test]
    fn conflicting_integer_list_identity_is_withheld() {
        let mut body = Vec::new();
        for value in [2_u32, 3] {
            body.extend([0x00, 0x52]);
            body.extend(1_u32.to_be_bytes());
            body.extend(300_u16.to_be_bytes());
            body.extend(value.to_be_bytes());
        }
        assert!(!integer_lists(None, &body)
            .expect("integer lists")
            .contains_key(&300));
    }

    #[test]
    fn conflicting_face_identity_is_withheld() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        body.extend(stream_with_list_node(
            &[74, 76, 1_390_698_820, 0, 4],
            333,
            310,
        ));
        assert!(scan(None, &body).expect("attribute scan").is_empty());
    }

    #[test]
    fn conflicting_persistent_tail_is_withheld() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3, 10], 333);
        body.extend(stream_with_list_node(
            &[74, 75, 1_390_698_820, 0, 3, 11],
            333,
            310,
        ));
        assert!(scan(None, &body).expect("attribute scan").is_empty());
    }

    #[test]
    fn body_modifier_binds_one_history_ordinal() {
        let modifiers =
            scan_body_modifiers(None, &body_modifier_stream(&[&[2]], 333)).expect("modifier scan");
        assert_eq!(modifiers.len(), 1);
        assert_eq!(modifiers[0].body_attr, 333);
        assert_eq!(modifiers[0].history_ordinal, 2);
    }

    #[test]
    fn body_modifier_rejects_non_scalar_and_conflicting_payloads() {
        assert!(
            scan_body_modifiers(None, &body_modifier_stream(&[&[0]], 333))
                .expect("modifier scan")
                .is_empty()
        );
        assert!(
            scan_body_modifiers(None, &body_modifier_stream(&[&[2, 3]], 333))
                .expect("modifier scan")
                .is_empty()
        );
        assert!(
            scan_body_modifiers(None, &body_modifier_stream(&[&[2], &[3]], 333))
                .expect("modifier scan")
                .is_empty()
        );
    }
}
