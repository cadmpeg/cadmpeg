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

use cadmpeg_core::decode::View;
use std::collections::HashMap;

use crate::layout::attribute_definition_short_refs as attr_def;
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

fn definition_candidates(buf: &[u8]) -> HashMap<u16, Option<Vec<u8>>> {
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
        } else {
            definitions.insert(node, None);
        }
    }
    definitions
        .into_iter()
        .map(|(node, schema)| {
            let name = schema.and_then(|(identifier, _)| names.get(&identifier)?.clone());
            (node, name)
        })
        .collect()
}

/// Resolve the stream-local attribute-definition table.
pub(super) fn definition_table(buf: &[u8]) -> DefinitionTable {
    definition_candidates(buf)
        .into_iter()
        .map(|(node, family)| {
            (
                node,
                family.and_then(|family| String::from_utf8(family).ok()),
            )
        })
        .collect()
}

/// Map definition-record node ids to the two supported native attribute
/// families whose payload consumers are implemented here.
fn definitions(buf: &[u8]) -> HashMap<u16, &'static str> {
    let mut supported = HashMap::new();
    for (node, family) in definition_table(buf) {
        let Some(family) = family else {
            continue;
        };
        let family = match family.as_str() {
            ATOM_ID => ATOM_ID,
            LAST_BODY_MODIFIER => LAST_BODY_MODIFIER,
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
        if count != 1 && !ATOM_WIDTHS.contains(&count) {
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
struct AtomPayload<'a> {
    values: &'a [u32; 5],
    trailing_fields: &'a [u32],
}

fn atom_payload<'a>(
    buf: &[u8],
    from: usize,
    lists: &'a HashMap<u16, Vec<u32>>,
) -> Option<AtomPayload<'a>> {
    referenced_payload(buf, from, lists, |values| {
        ATOM_WIDTHS.contains(&values.len()) && values.get(ATOM_GUARD) == Some(&0)
    })
    .and_then(|values| values.split_first_chunk::<5>())
    .map(|(values, trailing_fields)| AtomPayload {
        values,
        trailing_fields,
    })
}

/// Decode every `ATOM_ID_2001` binding carried by one stream body.
pub(super) fn scan(buf: &[u8]) -> Vec<RawFaceAtom> {
    let definitions = definitions(buf);
    if !definitions.values().any(|name| *name == ATOM_ID) {
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
        if definitions.get(&definition).copied() != Some(ATOM_ID) {
            continue;
        }
        let Some(face_attr) = View::u16_be_at(buf, p + attr_inst::OWNER_ATTRIBUTE_ID) else {
            continue;
        };
        if face_attr <= 1 {
            continue;
        }
        let Some(AtomPayload {
            values,
            trailing_fields,
        }) = atom_payload(buf, p + attr_inst::LEN, &lists)
        else {
            continue;
        };
        let identity = if let Ok(feature_source_id) =
            super::feature_source::FeatureSourceId::try_from(values[ATOM_FEATURE])
        {
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
        let Some(values) = referenced_payload(buf, p + attr_inst::LEN, &lists, |values| {
            values.len() == 1 && values[0] > 0
        }) else {
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
        definition_table, definitions, integer_lists, scan, scan_body_modifiers, ATOM_ID,
        LAST_BODY_MODIFIER,
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
        out.extend(2_u32.to_be_bytes());
        out.extend(definition.to_be_bytes());
        out.extend(1_u16.to_be_bytes());
        out.extend(name_node.to_be_bytes());
        out.extend(9000_u32.to_be_bytes());
        out.extend([0; 8]);
        out.extend(1_u16.to_be_bytes());
        out.extend([0; 14]);
        out.extend([9, 1]);
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

    /// Serialize one body-modifier family, one scalar list, and one instance.
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
        out.extend([0x00, 0x51]);
        out.extend(4_u32.to_be_bytes());
        out.extend(301_u16.to_be_bytes());
        out.extend(0_u16.to_be_bytes());
        out.extend(302_u16.to_be_bytes());
        out.extend(16_u16.to_be_bytes());
        out.extend(body_attr.to_be_bytes());
        for index in 0..payloads.len() {
            out.extend((300 + u16::try_from(index).expect("index fits u16")).to_be_bytes());
        }
        out
    }

    #[test]
    fn parasolid_attribute_values_decode_a_five_value_list() {
        let bytes = stream(&[74, 75, 1_390_698_820, 0, 3], 333);
        assert_eq!(integer_lists(&bytes).get(&300).map(Vec::len), Some(5));
    }

    #[test]
    fn parasolid_supported_definitions_admit_the_atom_family() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 15, 16);
        assert_eq!(definitions(&bytes).get(&16), Some(&ATOM_ID));
    }

    #[test]
    fn definition_names_follow_identifier_pointers_across_record_order() {
        let mut first = Vec::new();
        append_definition(&mut first, ATOM_ID, 41, 91);
        let mut second = Vec::new();
        append_definition(&mut second, "OTHER_FAMILY", 43, 93);
        let first_end = 8 + ATOM_ID.len();
        let second_end = 8 + "OTHER_FAMILY".len();
        let mut bytes = first[..first_end].to_vec();
        bytes.extend(&second[..second_end]);
        bytes.extend(&first[first_end..]);
        bytes.extend(&second[second_end..]);
        let names = definition_table(&bytes);
        assert_eq!(names.get(&91).and_then(Option::as_deref), Some(ATOM_ID));
        assert_eq!(
            names.get(&93).and_then(Option::as_deref),
            Some("OTHER_FAMILY")
        );
    }

    #[test]
    fn an_adjacent_name_does_not_replace_a_missing_identifier() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 41, 91);
        let identifier = 8 + ATOM_ID.len() + 10;
        bytes[identifier..identifier + 2].copy_from_slice(&1234_u16.to_be_bytes());
        assert!(definition_table(&bytes)
            .get(&91)
            .is_some_and(Option::is_none));
    }

    #[test]
    fn conflicting_key_names_withhold_every_dependent_definition() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 41, 91);
        append_definition(&mut bytes, "OTHER_FAMILY", 41, 93);
        let names = definition_table(&bytes);
        assert!(names.get(&91).is_some_and(Option::is_none));
        assert!(names.get(&93).is_some_and(Option::is_none));
    }

    #[test]
    fn conflicting_definition_payloads_are_not_a_named_family() {
        let mut first = Vec::new();
        append_definition(&mut first, ATOM_ID, 41, 91);
        let mut second = first.clone();
        let type_id = 8 + ATOM_ID.len() + 12;
        second[type_id..type_id + 4].copy_from_slice(&8001_u32.to_be_bytes());
        first.extend(second);
        assert!(definition_table(&first)
            .get(&91)
            .is_some_and(Option::is_none));
    }

    #[test]
    fn incomplete_or_invalid_definition_schemas_do_not_bind_names() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 41, 91);
        let mut truncated = bytes.clone();
        truncated.pop();
        assert!(!definition_table(&truncated).values().any(Option::is_some));
        let mut bad_owner = bytes.clone();
        let legal_owners = 8 + ATOM_ID.len() + 26;
        bad_owner[legal_owners] = 2;
        assert!(!definition_table(&bad_owner).values().any(Option::is_some));
        let mut bad_field = bytes;
        *bad_field.last_mut().expect("field type") = 11;
        assert!(!definition_table(&bad_field).values().any(Option::is_some));
    }

    #[test]
    fn extended_field_name_references_do_not_use_short_header_offsets() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 41, 91);
        let field_names = 8 + ATOM_ID.len() + 24;
        bytes.splice(field_names..field_names + 2, [0xff, 0xff, 0, 1]);
        assert!(definition_table(&bytes)
            .get(&91)
            .is_some_and(Option::is_none));
    }

    #[test]
    fn unchanged_first_type_markers_preserve_definition_identifiers() {
        let mut bytes = Vec::new();
        append_definition(&mut bytes, ATOM_ID, 41, 91);
        bytes.insert(8 + ATOM_ID.len() + 2, 0xff);
        bytes.insert(2, 0xff);
        assert_eq!(
            definition_table(&bytes).get(&91).and_then(Option::as_deref),
            Some(ATOM_ID)
        );
    }

    #[test]
    fn atom_source_sentinels_have_no_identity() {
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
        append_definition(&mut body, ATOM_ID, 15, 16);
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
    fn body_modifier_binds_one_history_ordinal() {
        let modifiers = scan_body_modifiers(&body_modifier_stream(&[&[2]], 333));
        assert_eq!(modifiers.len(), 1);
        assert_eq!(modifiers[0].body_attr, 333);
        assert_eq!(modifiers[0].history_ordinal, 2);
    }

    #[test]
    fn body_modifier_rejects_non_scalar_and_conflicting_payloads() {
        assert!(scan_body_modifiers(&body_modifier_stream(&[&[0]], 333)).is_empty());
        assert!(scan_body_modifiers(&body_modifier_stream(&[&[2, 3]], 333)).is_empty());
        assert!(scan_body_modifiers(&body_modifier_stream(&[&[2], &[3]], 333)).is_empty());
    }
}
