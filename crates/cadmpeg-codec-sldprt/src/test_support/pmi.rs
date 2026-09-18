// SPDX-License-Identifier: Apache-2.0
//! Synthetic PMI semantic-map byte builders for crate tests.
#![allow(clippy::unwrap_used)]

/// Append a msgpack fixstr header and its bytes.
pub(crate) fn fixstr(bytes: &mut Vec<u8>, value: &str) {
    assert!(value.len() < 32);
    bytes.push(0xa0 | value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

/// Append a msgpack fixarray or array16 header.
pub(crate) fn push_array_header(bytes: &mut Vec<u8>, len: usize) {
    if len < 16 {
        bytes.push(0x90 | len as u8);
    } else if let Ok(len16) = u16::try_from(len) {
        bytes.push(0xdc);
        bytes.extend_from_slice(&len16.to_be_bytes());
    } else {
        panic!("dimItems length exceeds array16");
    }
}

pub(crate) fn pmi_semantic_payload() -> Vec<u8> {
    pmi_semantic_payload_for("D1@Sketch1")
}

pub(crate) fn pmi_semantic_payload_for(cad_text: &str) -> Vec<u8> {
    pmi_semantic_payload_for_with_guid(cad_text, "01234567-89ab-cdef-0123-456789abcdef")
}

pub(crate) fn pmi_semantic_payload_for_with_guid(cad_text: &str, guid: &str) -> Vec<u8> {
    pmi_semantic_payload_for_with_guid_and_value(cad_text, guid, 0.025)
}

pub(crate) fn pmi_semantic_payload_for_with_guid_and_value(
    cad_text: &str,
    guid: &str,
    value: f64,
) -> Vec<u8> {
    pmi_semantic_payload_record(cad_text, guid, "Linear", value, "25.000 mm")
}

pub(crate) fn pmi_semantic_payload_record(
    cad_text: &str,
    guid: &str,
    subtype: &str,
    value: f64,
    display_text: &str,
) -> Vec<u8> {
    pmi_semantic_payload_record_with_items(cad_text, guid, &[(subtype, value)], display_text)
}

pub(crate) fn pmi_semantic_payload_record_with_items(
    cad_text: &str,
    guid: &str,
    items: &[(&str, f64)],
    display_text: &str,
) -> Vec<u8> {
    pmi_semantic_payload_record_configured(
        cad_text,
        guid,
        items,
        display_text,
        PmiPayloadOptions::default(),
    )
}

#[derive(Clone, Copy, Default)]
pub(crate) struct PmiPayloadOptions {
    /// Place `annoType` after `cadText` and add an extra map key.
    pub(crate) reorder_and_extra_key: bool,
    /// Embed a key-like `cadText` string inside `dimText`.
    pub(crate) key_like_string_in_value: bool,
    /// Truncate after writing `dimItems` so the map fails to parse.
    pub(crate) truncate_after_dim_items_key: bool,
}

pub(crate) fn pmi_semantic_payload_record_configured(
    cad_text: &str,
    guid: &str,
    items: &[(&str, f64)],
    display_text: &str,
    options: PmiPayloadOptions,
) -> Vec<u8> {
    fn push_map_header(bytes: &mut Vec<u8>, len: usize) {
        if len < 16 {
            bytes.push(0x80 | len as u8);
        } else if let Ok(len16) = u16::try_from(len) {
            bytes.push(0xde);
            bytes.extend_from_slice(&len16.to_be_bytes());
        } else {
            panic!("map length exceeds map16");
        }
    }
    assert_eq!(guid.len(), 36);
    let mut payload = b"unqlite".to_vec();
    payload.extend_from_slice(&[0; 57]);
    payload.extend_from_slice(guid.as_bytes());
    let outer_len = if options.reorder_and_extra_key { 8 } else { 7 };
    push_map_header(&mut payload, outer_len);
    if options.reorder_and_extra_key {
        fixstr(&mut payload, "cadText");
        fixstr(&mut payload, cad_text);
        fixstr(&mut payload, "extraKey");
        fixstr(&mut payload, "ignored");
        fixstr(&mut payload, "annoType");
        payload.push(1);
    } else {
        fixstr(&mut payload, "annoType");
        payload.push(1);
        fixstr(&mut payload, "cadText");
        fixstr(&mut payload, cad_text);
    }
    fixstr(&mut payload, "dimItems");
    if options.truncate_after_dim_items_key {
        // Declare one element and stop so the outer map cannot finish.
        push_array_header(&mut payload, 1);
        return payload;
    }
    push_array_header(&mut payload, items.len());
    for (subtype, value) in items {
        payload.push(0x87);
        fixstr(&mut payload, "class");
        fixstr(&mut payload, "DimSemData");
        fixstr(&mut payload, "dimSubType");
        fixstr(&mut payload, subtype);
        fixstr(&mut payload, "isBasic");
        payload.push(0xc3);
        fixstr(&mut payload, "isInspection");
        payload.push(0xc2);
        fixstr(&mut payload, "isReferenceOnly");
        payload.push(0xc3);
        fixstr(&mut payload, "valPrecision");
        payload.push(3);
        fixstr(&mut payload, "value");
        payload.push(0xcb);
        payload.extend_from_slice(&value.to_be_bytes());
    }
    fixstr(&mut payload, "dimText");
    if options.key_like_string_in_value {
        fixstr(&mut payload, "cadText");
    } else {
        fixstr(&mut payload, display_text);
    }
    fixstr(&mut payload, "dimType");
    payload.push(0);
    fixstr(&mut payload, "iDString");
    fixstr(&mut payload, "native-id");
    fixstr(&mut payload, "reserved");
    payload.push(0xc0);
    payload
}
