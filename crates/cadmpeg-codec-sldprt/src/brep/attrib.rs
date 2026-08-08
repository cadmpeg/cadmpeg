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

use super::{u16_be, u32_be};
use std::collections::HashMap;

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
pub struct FaceAtom {
    /// Attribute id of the face bridge record owning the attribute.
    pub face_attr: u16,
    /// Native source id of the history feature that produced the face.
    pub feature_source_id: u32,
    /// Feature-local face identity within that producer.
    pub local_face_id: u32,
    /// Byte offset of the attribute-instance record.
    pub offset: usize,
    /// Emitted face identity, resolved once the graph retains its faces.
    pub target: Option<String>,
}

/// One body's last modifying history ordinal.
#[derive(Debug, Clone)]
pub struct BodyModifier {
    /// Attribute id of the body carrying the attribute.
    pub body_attr: u16,
    /// One-based ordinal in the ordered Keywords modeling-feature records.
    pub history_ordinal: u32,
    /// Byte offset of the attribute-instance record.
    pub offset: usize,
    /// Emitted body identity, resolved once the graph retains its bodies.
    pub target: Option<String>,
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

/// Read the attribute-family names keyed by their name-record node id, with the
/// end offset of each name record.
fn names(buf: &[u8]) -> HashMap<u16, (String, usize)> {
    let mut out = HashMap::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x4f) else {
            continue;
        };
        let Some(len) = u32_be(buf, p) else { continue };
        let Some(node) = u16_be(buf, p + 4) else {
            continue;
        };
        let len = len as usize;
        if !(1..64).contains(&len) {
            continue;
        }
        let Some(text) = buf.get(p + 6..p + 6 + len) else {
            continue;
        };
        if !text.iter().all(|byte| (0x20..0x7f).contains(byte)) {
            continue;
        }
        let Ok(name) = std::str::from_utf8(text) else {
            continue;
        };
        out.entry(node)
            .or_insert_with(|| (name.to_owned(), p + 6 + len));
    }
    out
}

/// Map definition-record node ids to their family name. A definition record
/// follows its name record with no gap.
fn definitions(buf: &[u8]) -> HashMap<u16, String> {
    let mut out = HashMap::new();
    for (name, end) in names(buf).into_values() {
        let Some(p) = record_body(buf, end, 0x50) else {
            continue;
        };
        let Some(node) = u16_be(buf, p + 4) else {
            continue;
        };
        out.entry(node).or_insert(name);
    }
    out
}

/// Read integer payload lists keyed by their node id.
fn integer_lists(buf: &[u8]) -> HashMap<u16, Vec<u32>> {
    let mut out: HashMap<u16, Vec<u32>> = HashMap::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x52) else {
            continue;
        };
        let Some(count) = u32_be(buf, p) else {
            continue;
        };
        let Some(node) = u16_be(buf, p + 4) else {
            continue;
        };
        let count = count as usize;
        if !(1..64).contains(&count) {
            continue;
        }
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let Some(value) = u32_be(buf, p + 6 + index * 4) else {
                values.clear();
                break;
            };
            values.push(value);
        }
        if values.len() == count {
            out.entry(node).or_insert(values);
        }
    }
    out
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
        let node = u16_be(buf, at)?;
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
) -> Option<&'a [u32]> {
    referenced_payload(buf, from, lists, |values| {
        ATOM_WIDTHS.contains(&values.len()) && values[ATOM_GUARD] == 0
    })
}

/// Decode every `ATOM_ID_2001` binding carried by one stream body.
pub fn scan(buf: &[u8]) -> Vec<FaceAtom> {
    let definitions = definitions(buf);
    if !definitions.values().any(|name| name == ATOM_ID) {
        return Vec::new();
    }
    let lists = integer_lists(buf);
    let mut out: Vec<FaceAtom> = Vec::new();
    let mut seen = HashMap::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x51) else {
            continue;
        };
        if u16_be(buf, p + 6) != Some(0) {
            continue;
        }
        let Some(definition) = u16_be(buf, p + 10) else {
            continue;
        };
        if definitions.get(&definition).map(String::as_str) != Some(ATOM_ID) {
            continue;
        }
        let Some(face_attr) = u16_be(buf, p + 12) else {
            continue;
        };
        if face_attr <= 1 {
            continue;
        }
        let Some(values) = atom_payload(buf, p + 14, &lists) else {
            continue;
        };
        if seen.insert(face_attr, off).is_some() {
            continue;
        }
        out.push(FaceAtom {
            face_attr,
            feature_source_id: values[ATOM_FEATURE],
            local_face_id: values[ATOM_LOCAL],
            offset: off,
            target: None,
        });
    }
    out.sort_by_key(|atom| atom.face_attr);
    out
}

/// Decode every body-level last-modifier binding carried by one stream body.
pub fn scan_body_modifiers(buf: &[u8]) -> Vec<BodyModifier> {
    let definitions = definitions(buf);
    if !definitions.values().any(|name| name == LAST_BODY_MODIFIER) {
        return Vec::new();
    }
    let lists = integer_lists(buf);
    let mut found = HashMap::<u16, Option<BodyModifier>>::new();
    for off in 0..buf.len() {
        let Some(p) = record_body(buf, off, 0x51) else {
            continue;
        };
        if u16_be(buf, p + 6) != Some(0) {
            continue;
        }
        let Some(definition) = u16_be(buf, p + 10) else {
            continue;
        };
        if definitions.get(&definition).map(String::as_str) != Some(LAST_BODY_MODIFIER) {
            continue;
        }
        let Some(body_attr) = u16_be(buf, p + 12).filter(|attr| *attr > 1) else {
            continue;
        };
        let Some(values) = referenced_payload(buf, p + 14, &lists, |values| {
            values.len() == 1 && values[0] > 0
        }) else {
            found.insert(body_attr, None);
            continue;
        };
        let modifier = BodyModifier {
            body_attr,
            history_ordinal: values[0],
            offset: off,
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
    let mut out = found.into_values().flatten().collect::<Vec<_>>();
    out.sort_by_key(|modifier| modifier.body_attr);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize one attribute family, one payload list, and one instance.
    fn stream(payload: &[u32], face_attr: u16) -> Vec<u8> {
        let mut out = vec![0x00, 0x4f];
        out.extend((ATOM_ID.len() as u32).to_be_bytes());
        out.extend(15_u16.to_be_bytes());
        out.extend(ATOM_ID.as_bytes());
        out.extend([0x00, 0x50]);
        out.extend(2_u32.to_be_bytes());
        out.extend(16_u16.to_be_bytes());
        out.extend([0x00, 0x52]);
        out.extend((payload.len() as u32).to_be_bytes());
        out.extend(300_u16.to_be_bytes());
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
        out.extend(300_u16.to_be_bytes());
        out
    }

    /// Serialize one body-modifier family, one scalar list, and one instance.
    fn body_modifier_stream(payloads: &[&[u32]], body_attr: u16) -> Vec<u8> {
        let mut out = vec![0x00, 0x4f];
        out.extend((LAST_BODY_MODIFIER.len() as u32).to_be_bytes());
        out.extend(15_u16.to_be_bytes());
        out.extend(LAST_BODY_MODIFIER.as_bytes());
        out.extend([0x00, 0x50]);
        out.extend(2_u32.to_be_bytes());
        out.extend(16_u16.to_be_bytes());
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
    fn instance_binds_face_to_producing_feature() {
        let atoms = scan(&stream(&[74, 75, 1_390_698_820, 0, 3], 333));
        assert_eq!(atoms.len(), 1);
        assert_eq!(atoms[0].face_attr, 333);
        assert_eq!(atoms[0].feature_source_id, 75);
        assert_eq!(atoms[0].local_face_id, 3);
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
