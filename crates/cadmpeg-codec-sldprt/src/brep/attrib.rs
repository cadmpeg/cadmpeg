// SPDX-License-Identifier: Apache-2.0
//! Parasolid attribute dictionary and the per-face producing-feature identity
//! it carries ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
//!
//! A partition stream declares its attribute families inline. Each family is a
//! definition record `00 50` whose identifier points to a name record `00 4f`.
//! Attribute instances `00 51` name their definition and the entity they hang
//! on. Integer payloads live in separate `00 52` list records the instance
//! references by node id.
//!
//! The `FACE_ID_2001` family binds a face to the history feature that produced
//! it. The `LAST_BODY_MODIFYING_FEATURE_ID` family binds a body to the last
//! modeling-history ordinal that wrote it. The `SWEntUnchanged` family binds a
//! face to the key of its `DisplayLists` table. Deltas streams carry no attribute
//! dictionary, so a deltas body yields no bindings.

use cadmpeg_core::decode::View;
use std::collections::HashMap;

use super::feature_source::FeatureSourceId;
use crate::layout::attribute_definition_short_refs as attr_def;
use crate::layout::attribute_instance_00_51 as attr_inst;
use crate::layout::attribute_short_references as attr_refs;

/// Attribute family binding a face to its producing feature.
const FACE_ID: &str = "FACE_ID_2001";
const BODY_ID: &str = "ATOM_ID_2001";

/// Attribute family carrying a body's last native feature source identity.
const LAST_BODY_MODIFIER: &str = "LAST_BODY_MODIFYING_FEATURE_ID";

/// Attribute family carrying a face's `DisplayLists` table key.
const FACE_KEY: &str = "SWEntUnchanged";

const FACE_FIELDS: &[u8] = &[9, 9, 1, 1];
const BODY_FIELDS: &[u8] = &[9, 1];
const FACE_KEY_FIELDS: &[u8] = &[1];

/// Widths of an `FACE_ID_2001` payload list.
const FACE_ID_WIDTHS: std::ops::RangeInclusive<usize> = 5..=7;
const NATIVE_FACE_ID_WIDTHS: std::ops::RangeInclusive<usize> = 4..=7;

/// Payload position of the producing feature's native source id.
const FACE_ID_FEATURE: usize = 1;

/// Payload position that is zero on a face-identity payload.
const FACE_ID_GUARD: usize = 3;

/// Payload position of the feature-local face identity.
const FACE_ID_LOCAL: usize = 4;

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
    /// Native source identity of the last modifying history feature.
    pub(crate) feature_source_id: FeatureSourceId,
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

/// Stream-local attribute definitions with unique names or withheld conflicts.
type DefinitionTable = HashMap<u16, Option<String>>;

fn consistent_insert<T: PartialEq>(found: &mut HashMap<u16, Option<T>>, node: u16, value: T) {
    match found.entry(node) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry
                .get()
                .as_ref()
                .is_some_and(|previous| previous != &value)
            {
                *entry.get_mut() = None;
            }
        }
    }
}

fn short_index(buf: &[u8], at: usize) -> Option<u16> {
    View::u16_be_at(buf, at).filter(|node| *node > 1 && *node <= i16::MAX.unsigned_abs())
}

fn definition_frame(buf: &[u8], p: usize) -> Option<(u16, Vec<u8>)> {
    let identifier = short_index(buf, p + attr_def::IDENTIFIER)?;
    if [attr_def::NEXT_DEFINITION, attr_def::FIELD_NAMES]
        .into_iter()
        .any(|offset| {
            View::u16_be_at(buf, p + offset).is_none_or(|index| index > i16::MAX.unsigned_abs())
        })
    {
        return None;
    }
    let count = View::u32_be_at(buf, p + attr_def::FIELD_COUNT)?;
    let fields_start = p.checked_add(attr_def::LEN)?;
    let available = buf.len().checked_sub(fields_start)?;
    let count = cadmpeg_core::decode::bounded_len(u64::from(count), 1, available)?;
    let owners = buf.get(p + attr_def::LEGAL_OWNERS..fields_start)?;
    let fields = &buf[fields_start..fields_start + count];
    if owners.iter().any(|value| *value > 1) || fields.iter().any(|value| *value > 10) {
        return None;
    }
    Some((identifier, buf[p..fields_start + count].to_vec()))
}

struct NamedDefinition {
    name: String,
    fields: Vec<u8>,
}

fn definition_candidates(buf: &[u8]) -> HashMap<u16, Option<NamedDefinition>> {
    let mut names = HashMap::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x4f) else {
            continue;
        };
        let Some(len) = View::u32_be_at(buf, p) else {
            continue;
        };
        let Some(node) = short_index(buf, p + 4) else {
            continue;
        };
        let Some(data) = p.checked_add(6) else {
            continue;
        };
        let Some(available) = buf.len().checked_sub(data) else {
            continue;
        };
        let Some(len) = cadmpeg_core::decode::bounded_len(u64::from(len), 1, available) else {
            continue;
        };
        let Some(text) = buf.get(data..data + len) else {
            continue;
        };
        if text.is_empty() || text.iter().any(|byte| !byte.is_ascii_graphic()) {
            continue;
        }
        consistent_insert(&mut names, node, text.to_vec());
    }
    let mut definitions = HashMap::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x50) else {
            continue;
        };
        let Some(node) = short_index(buf, p + attr_def::NODE_INDEX) else {
            continue;
        };
        if let Some(frame) = definition_frame(buf, p) {
            consistent_insert(&mut definitions, node, frame);
        } else if short_index(buf, p + attr_def::IDENTIFIER)
            .is_some_and(|identifier| names.contains_key(&identifier))
            && View::u16_be_at(buf, p + attr_def::NEXT_DEFINITION)
                .is_some_and(|next| next <= i16::MAX.unsigned_abs())
            && View::u32_be_at(buf, p + attr_def::FIELD_COUNT).is_some_and(|count| {
                u64::from(count) <= cadmpeg_core::decode::u64_from_index(buf.len() - p)
            })
            && buf
                .get(p + attr_def::ACTIONS..p + attr_def::FIELD_NAMES)
                .is_some_and(|actions| actions.iter().all(|action| *action <= 6))
        {
            definitions.insert(node, None);
        }
    }
    definitions
        .into_iter()
        .map(|(node, schema)| {
            let definition = schema.and_then(|(identifier, body)| {
                Some(NamedDefinition {
                    name: String::from_utf8(names.get(&identifier)?.clone()?).ok()?,
                    fields: body[attr_def::LEN..].to_vec(),
                })
            });
            (node, definition)
        })
        .collect()
}

/// Resolve the stream-local attribute-definition table.
pub(super) fn definition_table(buf: &[u8]) -> DefinitionTable {
    definition_candidates(buf)
        .into_iter()
        .map(|(node, family)| (node, family.map(|family| family.name)))
        .collect()
}

/// Map definition-record node ids to the two supported native attribute
/// families whose payload consumers are implemented here.
fn definitions(buf: &[u8]) -> HashMap<u16, &'static str> {
    let mut supported = HashMap::new();
    for (node, family) in definition_candidates(buf) {
        let Some(family) = family else {
            continue;
        };
        let family = match family.name.as_str() {
            FACE_ID if family.fields == FACE_FIELDS => FACE_ID,
            LAST_BODY_MODIFIER if family.fields == BODY_FIELDS => LAST_BODY_MODIFIER,
            BODY_ID if family.fields == BODY_FIELDS => BODY_ID,
            FACE_KEY if family.fields == FACE_KEY_FIELDS => FACE_KEY,
            _ => continue,
        };
        supported.insert(node, family);
    }
    supported
}

/// Read integer payload lists keyed by their node id.
fn integer_lists(buf: &[u8]) -> HashMap<u16, Vec<u32>> {
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
        let Some(available) = buf.len().checked_sub(data) else {
            continue;
        };
        let Some(count) = cadmpeg_core::decode::bounded_len(u64::from(count), 4, available) else {
            continue;
        };
        if count != 1 && !NATIVE_FACE_ID_WIDTHS.contains(&count) {
            continue;
        }
        let mut values = Vec::new();
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
    found
        .into_iter()
        .filter_map(|(node, values)| values.map(|values| (node, values)))
        .collect()
}

fn named_integer_payload<'a>(
    buf: &[u8],
    p: usize,
    lists: &'a HashMap<u16, Vec<u32>>,
    fields: &[u8],
) -> Option<&'a [u32]> {
    let count = u32::try_from(fields.len()).ok()?;
    if View::u32_be_at(buf, p + attr_refs::VALUE_COUNT) != Some(count)
        || short_index(buf, p + attr_refs::NODE_INDEX).is_none()
        || View::u32_be_at(buf, p + attr_refs::SEQUENCE).is_none_or(|sequence| sequence == 0)
    {
        return None;
    }
    let start = p.checked_add(attr_refs::LEN)?;
    let end = start.checked_add(fields.len().checked_mul(2)?)?;
    buf.get(p..end)?;
    for at in (p + attr_refs::OWNER..end).step_by(2) {
        if View::u16_be_at(buf, at).is_none_or(|node| node > i16::MAX.unsigned_abs()) {
            return None;
        }
    }
    let field = fields.iter().position(|kind| *kind == 1)?;
    let node = View::u16_be_at(buf, start + field * 2)?;
    lists.get(&node).map(Vec::as_slice)
}

/// The first integer field's supported persistent face payload.
struct FacePayload<'a> {
    values: &'a [u32; 5],
    trailing_fields: &'a [u32],
}

fn face_payload<'a>(
    buf: &[u8],
    p: usize,
    lists: &'a HashMap<u16, Vec<u32>>,
) -> Option<FacePayload<'a>> {
    named_integer_payload(buf, p, lists, FACE_FIELDS)
        .filter(|values| {
            FACE_ID_WIDTHS.contains(&values.len()) && values.get(FACE_ID_GUARD) == Some(&0)
        })
        .and_then(|values| values.split_first_chunk::<5>())
        .map(|(values, trailing_fields)| FacePayload {
            values,
            trailing_fields,
        })
}

/// Decode every `FACE_ID_2001` binding carried by one stream body.
pub(super) fn scan(buf: &[u8]) -> Vec<RawFaceAtom> {
    let definitions = definitions(buf);
    if !definitions.values().any(|name| *name == FACE_ID) {
        return Vec::new();
    }
    let lists = integer_lists(buf);
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
        if definitions.get(&definition).copied() != Some(FACE_ID) {
            continue;
        }
        let Some(face_attr) = View::u16_be_at(buf, p + attr_inst::OWNER_ATTRIBUTE_ID) else {
            continue;
        };
        if face_attr <= 1 {
            continue;
        }
        let Some(FacePayload {
            values,
            trailing_fields,
        }) = face_payload(buf, p, &lists)
        else {
            continue;
        };
        let identity = if let Ok(feature_source_id) =
            super::feature_source::FeatureSourceId::try_from(values[FACE_ID_FEATURE])
        {
            Some(super::PersistentFaceIdentity {
                feature_source_id,
                local_id: values[FACE_ID_LOCAL],
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
    let mut out: Vec<_> = found.into_values().flatten().collect();
    out.sort_by_key(|atom| atom.face_attr);
    out
}

/// A complete native identity attached to one emitted topology entity.
#[derive(Debug, Clone)]
pub(crate) struct NativeIdentity<T> {
    pub(crate) target: T,
    pub(crate) values: Vec<u32>,
}

fn scan_identity_payloads(buf: &[u8], family: &str, fields: &[u8]) -> Vec<(u16, Vec<u32>)> {
    let definitions = definitions(buf);
    let lists = integer_lists(buf);
    let mut found = HashMap::<u16, Option<Vec<u32>>>::new();
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
        if definitions.get(&definition).copied() != Some(family) {
            continue;
        }
        let Some(owner) = short_index(buf, p + attr_inst::OWNER_ATTRIBUTE_ID) else {
            continue;
        };
        let Some(values) = named_integer_payload(buf, p, &lists, fields) else {
            continue;
        };
        let valid = if family == FACE_ID {
            NATIVE_FACE_ID_WIDTHS.contains(&values.len())
                && values.get(FACE_ID_GUARD) == Some(&0)
                && (values.len() != 4 || values[0] == 36)
                && FeatureSourceId::try_from(values[FACE_ID_FEATURE]).is_ok()
                && values[2] != 0
                && values[2] != u32::MAX
        } else {
            values.len() == 1 && values[0] != 0 && values[0] != u32::MAX
        };
        if valid {
            consistent_insert(&mut found, owner, values.to_vec());
        }
    }
    let mut out: Vec<_> = found
        .into_iter()
        .filter_map(|(owner, values)| Some((owner, values?)))
        .collect();
    out.sort_by_key(|(owner, _)| *owner);
    out
}

pub(super) fn scan_face_payloads(buf: &[u8]) -> Vec<(u16, Vec<u32>)> {
    scan_identity_payloads(buf, FACE_ID, FACE_FIELDS)
}

pub(super) fn scan_body_payloads(buf: &[u8]) -> Vec<(u16, Vec<u32>)> {
    scan_identity_payloads(buf, BODY_ID, BODY_FIELDS)
}

pub(super) fn scan_face_keys(buf: &[u8]) -> Vec<(u16, Vec<u32>)> {
    scan_identity_payloads(buf, FACE_KEY, FACE_KEY_FIELDS)
}

pub(super) fn unique_payloads(payloads: Vec<(u16, Vec<u32>)>) -> Vec<(u16, Vec<u32>)> {
    let mut found = HashMap::new();
    for (owner, values) in payloads {
        consistent_insert(&mut found, owner, values);
    }
    let mut out: Vec<_> = found
        .into_iter()
        .filter_map(|(owner, values)| Some((owner, values?)))
        .collect();
    out.sort_by_key(|(owner, _)| *owner);
    out
}

/// Decode every body-level last-modifier binding carried by one stream body.
pub(super) fn scan_body_modifiers(buf: &[u8]) -> Vec<BodyModifier> {
    let definitions = definitions(buf);
    if !definitions.values().any(|name| *name == LAST_BODY_MODIFIER) {
        return Vec::new();
    }
    let lists = integer_lists(buf);
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
        let Some(feature_source_id) = named_integer_payload(buf, p, &lists, BODY_FIELDS)
            .filter(|values| values.len() == 1)
            .and_then(|values| FeatureSourceId::try_from(values[0]).ok())
        else {
            found.insert(body_attr, None);
            continue;
        };
        let modifier = BodyModifier {
            body_attr,
            feature_source_id,
            target: None,
        };
        match found.get_mut(&body_attr) {
            Some(slot)
                if slot.as_ref().is_some_and(|previous| {
                    previous.feature_source_id != modifier.feature_source_id
                }) =>
            {
                *slot = None;
            }
            Some(None) => {}
            Some(Some(_)) => {}
            None => {
                found.insert(body_attr, Some(modifier));
            }
        }
    }
    let mut out: Vec<_> = found.into_values().flatten().collect();
    out.sort_by_key(|modifier| modifier.body_attr);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        definition_table, definitions, integer_lists, scan, scan_body_modifiers, scan_face_keys,
        FACE_ID, LAST_BODY_MODIFIER,
    };
    use crate::test_support::parasolid::{
        attribute_definition, attribute_name, int_attribute, int_value,
    };

    fn append_definition(out: &mut Vec<u8>, family: &str, name_node: u16, definition: u16) {
        out.extend([0x00, 0x4f]);
        out.extend(
            u32::try_from(family.len())
                .expect("family length fits u32")
                .to_be_bytes(),
        );
        out.extend(name_node.to_be_bytes());
        out.extend(family.as_bytes());
        out.extend([0x00, 0x50]);
        let fields: &[u8] = if family == FACE_ID {
            &[9, 9, 1, 1]
        } else {
            &[9, 1]
        };
        out.extend(
            u32::try_from(fields.len())
                .expect("field count")
                .to_be_bytes(),
        );
        out.extend(definition.to_be_bytes());
        out.extend(1_u16.to_be_bytes());
        out.extend(name_node.to_be_bytes());
        out.extend(9000_u32.to_be_bytes());
        out.extend([0; 8]);
        out.extend(1_u16.to_be_bytes());
        out.extend([0; 14]);
        out.extend(fields);
    }

    /// Serialize one attribute family, one payload list, and one instance.
    fn stream(payload: &[u32], face_attr: u16) -> Vec<u8> {
        stream_with_list_node(payload, face_attr, 300)
    }

    fn stream_with_list_node(payload: &[u32], face_attr: u16, list_node: u16) -> Vec<u8> {
        let mut out = Vec::new();
        append_definition(&mut out, FACE_ID, 15, 16);
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
        for value in [1_u16, 1, 1, 1, 1, 1, list_node, 1] {
            out.extend(value.to_be_bytes());
        }
        out
    }

    #[test]
    fn four_word_face_payload_keeps_every_feature_scoped_owner_without_a_local_selector() {
        let payload = [36, 47, 0x6523_4178, 0];
        for owner in [13, 29, 53] {
            let bytes = stream(&payload, owner);
            assert!(scan(&bytes).is_empty());
            assert_eq!(
                super::scan_face_payloads(&bytes),
                vec![(owner, payload.to_vec())]
            );
        }
    }

    #[test]
    fn native_face_scope_rejects_unknown_four_word_forms_and_invalid_producer_identity() {
        for payload in [
            [37, 47, 0x6523_4178, 0],
            [36, 0, 0x6523_4178, 0],
            [36, 47, 0, 0],
            [36, 47, u32::MAX, 0],
            [36, 47, 0x6523_4178, 1],
        ] {
            assert!(super::scan_face_payloads(&stream(&payload, 13)).is_empty());
        }
    }

    #[test]
    fn merged_identity_payloads_withhold_conflicting_owners_and_keep_distinct_owners() {
        let first = vec![36, 47, 0x6523_4178, 0];
        let second = vec![72, 53, 0x6523_4180, 0, 9];
        assert_eq!(
            super::unique_payloads(vec![
                (13, first.clone()),
                (13, second.clone()),
                (29, first.clone()),
                (29, first.clone()),
                (31, second.clone()),
            ]),
            vec![(29, first), (31, second)]
        );
    }

    fn body_modifier_stream(payloads: &[&[u32]], body_attr: u16) -> Vec<u8> {
        let mut out = Vec::new();
        append_definition(&mut out, LAST_BODY_MODIFIER, 15, 16);
        for (index, payload) in payloads.iter().enumerate() {
            out.extend([0x00, 0x52]);
            out.extend(
                u32::try_from(payload.len())
                    .expect("payload length fits u32")
                    .to_be_bytes(),
            );
            out.extend((300 + u16::try_from(index).expect("index fits u16")).to_be_bytes());
            for value in *payload {
                out.extend(value.to_be_bytes());
            }
        }
        for index in 0..payloads.len() {
            out.extend([0x00, 0x51]);
            out.extend(2_u32.to_be_bytes());
            out.extend((400 + u16::try_from(index).expect("index")).to_be_bytes());
            out.extend(302_u32.to_be_bytes());
            out.extend(16_u16.to_be_bytes());
            let list = 300 + u16::try_from(index).expect("index");
            for value in [body_attr, 1, 1, 1, 1, 1, list] {
                out.extend(value.to_be_bytes());
            }
        }
        out
    }

    #[test]
    fn parasolid_attribute_values_decode_a_five_value_list() {
        let bytes = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        assert_eq!(integer_lists(&bytes).get(&300).map(Vec::len), Some(5));
    }

    #[test]
    fn parasolid_supported_definitions_admit_the_face_family() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, FACE_ID, 15, 16);
        assert_eq!(definitions(&bytes).get(&16), Some(&FACE_ID));
    }

    #[test]
    fn definition_names_follow_identifier_pointers_across_record_order() {
        let mut first = Vec::new();
        append_definition(&mut first, FACE_ID, 41, 91);
        let mut second = Vec::new();
        append_definition(&mut second, "OTHER_FAMILY", 43, 93);
        let first_end = 8 + FACE_ID.len();
        let second_end = 8 + "OTHER_FAMILY".len();
        let mut bytes = first[..first_end].to_vec();
        bytes.extend(&second[..second_end]);
        bytes.extend(&first[first_end..]);
        bytes.extend(&second[second_end..]);
        let names = definition_table(&bytes);
        assert_eq!(names.get(&91).and_then(Option::as_deref), Some(FACE_ID));
        assert_eq!(
            names.get(&93).and_then(Option::as_deref),
            Some("OTHER_FAMILY")
        );
    }

    #[test]
    fn an_adjacent_name_does_not_replace_a_missing_identifier() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, FACE_ID, 41, 91);
        let identifier = 8 + FACE_ID.len() + 10;
        bytes[identifier..identifier + 2].copy_from_slice(&1234_u16.to_be_bytes());
        assert!(definition_table(&bytes)
            .get(&91)
            .is_some_and(Option::is_none));
    }

    #[test]
    fn conflicting_key_names_withhold_every_dependent_definition() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, FACE_ID, 41, 91);
        append_definition(&mut bytes, "OTHER_FAMILY", 41, 93);
        let names = definition_table(&bytes);
        assert!(names.get(&91).is_some_and(Option::is_none));
        assert!(names.get(&93).is_some_and(Option::is_none));
    }

    #[test]
    fn conflicting_definition_payloads_are_not_a_named_family() {
        let mut first = Vec::new();
        append_definition(&mut first, FACE_ID, 41, 91);
        let mut second = first.clone();
        let type_id = 8 + FACE_ID.len() + 12;
        second[type_id..type_id + 4].copy_from_slice(&8001_u32.to_be_bytes());
        first.extend(second);
        assert!(definition_table(&first)
            .get(&91)
            .is_some_and(Option::is_none));
    }

    #[test]
    fn incomplete_or_invalid_definition_schemas_do_not_bind_names() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, FACE_ID, 41, 91);
        let mut truncated = bytes.clone();
        truncated.pop();
        assert!(!definition_table(&truncated).values().any(Option::is_some));
        let mut bad_owner = bytes.clone();
        let legal_owners = 8 + FACE_ID.len() + 26;
        bad_owner[legal_owners] = 2;
        assert!(!definition_table(&bad_owner).values().any(Option::is_some));
        let mut bad_field = bytes;
        *bad_field.last_mut().expect("field type") = 11;
        assert!(!definition_table(&bad_field).values().any(Option::is_some));
    }

    #[test]
    fn unframed_definition_tag_in_character_values_does_not_replace_a_name() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, FACE_ID, 41, 91);
        bytes.extend([0, 0x54]);
        bytes.extend(8_u32.to_be_bytes());
        bytes.extend(99_u16.to_be_bytes());
        bytes.extend([0, 0x50, 0, 0, 0, 1, 0, 91]);
        assert_eq!(
            definition_table(&bytes).get(&91).and_then(Option::as_deref),
            Some(FACE_ID)
        );
    }

    #[test]
    fn extended_field_name_references_do_not_use_short_header_offsets() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, FACE_ID, 41, 91);
        let field_names = 8 + FACE_ID.len() + 24;
        bytes.splice(field_names..field_names + 2, [0xff, 0xff, 0, 1]);
        assert!(definition_table(&bytes)
            .get(&91)
            .is_some_and(Option::is_none));
    }

    #[test]
    fn unchanged_first_type_markers_preserve_definition_identifiers() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, FACE_ID, 41, 91);
        bytes.insert(8 + FACE_ID.len() + 2, 0xff);
        bytes.insert(2, 0xff);
        assert_eq!(
            definition_table(&bytes).get(&91).and_then(Option::as_deref),
            Some(FACE_ID)
        );
    }

    #[test]
    fn face_source_sentinels_have_no_identity() {
        for source in [0, u32::MAX] {
            let atoms = scan(&stream(&[74, source, 1_390_698_820, 0, 3], 333));
            assert_eq!(atoms.len(), 1);
            assert_eq!(atoms[0].face_attr, 333);
            assert!(atoms[0].identity.is_none());
        }
    }

    #[test]
    fn instance_binds_face_to_producing_feature() {
        let atoms = scan(&stream(&[74, 75, 1_390_698_820, 0, 3], 333));
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
        let atoms = scan(&stream(&[49, 266, 1_704_609_508, 0, 2, 10, 8], 333));
        assert_eq!(atoms.len(), 1);
        assert_eq!(
            atoms[0].identity.as_ref().unwrap().trailing_fields,
            vec![10, 8]
        );
    }

    #[test]
    fn payload_with_a_nonzero_guard_position_is_not_a_face_identity() {
        assert!(scan(&stream(&[74, 75, 1_390_698_820, 9, 3], 333)).is_empty());
    }

    #[test]
    fn stream_without_the_family_declaration_yields_nothing() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        body[8] = b'X';
        assert!(scan(&body).is_empty());
    }

    #[test]
    fn conflicting_definition_identity_is_withheld() {
        let mut body = Vec::new();
        append_definition(&mut body, FACE_ID, 15, 16);
        append_definition(&mut body, LAST_BODY_MODIFIER, 17, 16);
        assert!(!definitions(&body).contains_key(&16));
        let table = definition_table(&body);
        assert!(table.contains_key(&16));
        assert!(table.get(&16).is_some_and(Option::is_none));
    }

    #[test]
    fn definition_table_retains_unsupported_families() {
        let mut body = Vec::new();
        append_definition(&mut body, "SDL/TYSA_COLOUR", 15, 16);

        assert_eq!(
            definition_table(&body).get(&16).and_then(Option::as_deref),
            Some("SDL/TYSA_COLOUR")
        );
        assert!(!definitions(&body).contains_key(&16));
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
        assert!(integer_lists(&body).is_empty());
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
        assert!(!integer_lists(&body).contains_key(&300));
    }

    #[test]
    fn conflicting_face_identity_is_withheld() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        body.extend(stream_with_list_node(
            &[74, 76, 1_390_698_820, 0, 4],
            333,
            310,
        ));
        assert!(scan(&body).is_empty());
    }

    #[test]
    fn conflicting_persistent_tail_is_withheld() {
        let mut body = stream(&[74, 75, 1_390_698_820, 0, 3, 10], 333);
        body.extend(stream_with_list_node(
            &[74, 75, 1_390_698_820, 0, 3, 11],
            333,
            310,
        ));
        assert!(scan(&body).is_empty());
    }

    #[test]
    fn a_different_native_family_does_not_supply_a_face_identity() {
        let mut bytes = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        bytes[8..8 + FACE_ID.len()].copy_from_slice(b"ATOM_ID_2001");
        assert!(scan(&bytes).is_empty());
    }

    #[test]
    fn a_named_face_definition_requires_its_field_schema() {
        let mut bytes = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        let fields = 8 + FACE_ID.len() + 2 + super::attr_def::LEN;
        bytes[fields] = 1;
        assert_eq!(
            definition_table(&bytes).get(&16).and_then(Option::as_deref),
            Some(FACE_ID)
        );
        assert!(scan(&bytes).is_empty());
    }

    #[test]
    fn chain_pointer_and_second_integer_slots_do_not_supply_the_first_integer_field() {
        for slot in [14, 22, 28] {
            let mut bytes = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
            let p = bytes.len() - 30;
            bytes[p + 26..p + 28].copy_from_slice(&1_u16.to_be_bytes());
            bytes[p + slot..p + slot + 2].copy_from_slice(&300_u16.to_be_bytes());
            assert!(scan(&bytes).is_empty());
        }
    }

    #[test]
    fn truncated_and_extended_attribute_references_do_not_use_short_slots() {
        let bytes = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        let mut truncated = bytes.clone();
        truncated.pop();
        assert!(scan(&truncated).is_empty());
        let p = bytes.len() - 30;
        let mut extended = bytes;
        extended.splice(p + 14..p + 16, [0xff, 0xff, 0, 1]);
        assert!(scan(&extended).is_empty());
    }

    #[test]
    fn a_body_pointer_field_does_not_supply_its_integer_value() {
        let mut bytes = body_modifier_stream(&[&[75]], 333);
        let p = bytes.len() - 26;
        bytes[p + 22..p + 24].copy_from_slice(&300_u16.to_be_bytes());
        bytes[p + 24..p + 26].copy_from_slice(&1_u16.to_be_bytes());
        assert!(scan_body_modifiers(&bytes).is_empty());
    }

    #[test]
    fn body_modifier_binds_one_native_feature_source_id() {
        let modifiers = scan_body_modifiers(&body_modifier_stream(&[&[2]], 333));
        assert_eq!(modifiers.len(), 1);
        assert_eq!(modifiers[0].body_attr, 333);
        assert_eq!(modifiers[0].feature_source_id.value(), 2);
    }

    #[test]
    fn body_modifier_rejects_non_scalar_and_conflicting_payloads() {
        assert!(scan_body_modifiers(&body_modifier_stream(&[&[0]], 333)).is_empty());
        assert!(scan_body_modifiers(&body_modifier_stream(&[&[u32::MAX]], 333)).is_empty());
        assert!(scan_body_modifiers(&body_modifier_stream(&[&[2, 3]], 333)).is_empty());
        assert!(scan_body_modifiers(&body_modifier_stream(&[&[2], &[3]], 333)).is_empty());
    }

    /// The KEY and definition order of a native dictionary: a KEY is followed
    /// by the definition of another family.
    fn face_key_dictionary(key_fields: &[u8], key: u32) -> Vec<u8> {
        let mut bytes = attribute_name(25, "SDL/TYSA_DENSITY");
        bytes.extend(attribute_definition(26, 29, key_fields));
        bytes.extend(attribute_name(29, "SWEntUnchanged"));
        bytes.extend(attribute_definition(22, 25, &[2, 3]));
        bytes.extend(int_value(300, key));
        bytes.extend(int_attribute(301, 26, 13, 300));
        bytes
    }

    #[test]
    fn face_key_family_is_named_by_identifier_not_by_the_next_definition() {
        assert_eq!(
            scan_face_keys(&face_key_dictionary(&[1], 0x4173_00a6)),
            vec![(13, vec![0x4173_00a6])]
        );
        assert!(scan_face_keys(&face_key_dictionary(&[1], 0)).is_empty());
        assert!(scan_face_keys(&face_key_dictionary(&[9, 1], 0x4173_00a6)).is_empty());
    }
}
