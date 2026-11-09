// SPDX-License-Identifier: Apache-2.0
//! Synthetic Protein instance-property payloads.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::index_from_u32;

use crate::test_support::lp_ascii;
use cadmpeg_protein::{
    CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER,
};

/// One member of a synthetic schema-driven Protein record.
enum Member {
    Color([f64; 4]),
    Float(f64),
}

/// The members a synthetic record of `schema` stores, in schema identifier
/// order, as the schema-driven reader consumes them.
fn generated_members(schema: &str) -> Vec<(&'static str, Member)> {
    let color = Member::Color([0.1, 0.2, 0.3, 1.0]);
    match schema {
        "GenericSchema" => vec![
            ("generic_diffuse", color),
            ("generic_reflectivity_at_0deg", Member::Float(0.25)),
            ("generic_refraction_index", Member::Float(1.5)),
        ],
        "PrismOpaqueSchema" => vec![
            ("opaque_albedo", color),
            ("surface_roughness", Member::Float(0.25)),
        ],
        "PrismTransparentSchema" => vec![
            ("transparent_color", color),
            ("transparent_refraction_index", Member::Float(1.5)),
        ],
        _ => panic!("unsupported generated Protein schema"),
    }
}

/// The schema document that declares the members of a synthetic record.
pub(crate) fn generated_protein_schema(schema: &str) -> String {
    let mut document = format!("<?xml version=\"1.0\"?>\n<Schema>\n  <UID val=\"{schema}\"/>\n");
    for (id, member) in generated_members(schema) {
        let tag = match member {
            Member::Color(_) => "Color",
            Member::Float(_) => "Float",
        };
        document.push_str(&format!("  <{tag} id=\"{id}\"/>\n"));
    }
    document.push_str("</Schema>\n");
    document
}

fn generated_record(schema: &str, guid: &str) -> Vec<u8> {
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, schema);
    lp_ascii(&mut logical, guid);
    lp_ascii(&mut logical, "Prism-001");
    lp_ascii(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    for (_, member) in generated_members(schema) {
        match member {
            Member::Color(rgba) => {
                for value in rgba {
                    logical.extend_from_slice(&value.to_le_bytes());
                }
            }
            Member::Float(value) => logical.extend_from_slice(&value.to_le_bytes()),
        }
    }
    paged_instance_properties(&logical)
}

pub(crate) fn generated_instance_properties_for(guid: &str) -> Vec<u8> {
    generated_record("GenericSchema", guid)
}

pub(crate) fn generated_prism_instance_properties(schema: &str, guid: &str) -> Vec<u8> {
    generated_record(schema, guid)
}

fn paged_instance_properties(logical: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        &(u32::try_from(PAGE_SIZE).expect("fixture value fits u32")).to_le_bytes(),
    );
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let first = logical.len().min(PAGE_SIZE - 4);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    bytes.resize(STREAM_HEADER_LEN + PAGE_SIZE, 0);
    let mut rest = &logical[first..];
    while rest.len() > PAGE_SIZE - 8 {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(CONTINUATION_MARKER);
        bytes.extend_from_slice(&rest[..PAGE_SIZE - 8]);
        rest = &rest[PAGE_SIZE - 8..];
    }
    if !rest.is_empty() {
        bytes.extend_from_slice(TERMINAL_MARKER);
        bytes.extend_from_slice(
            &(u16::try_from(rest.len()).expect("fixture value fits u16")).to_le_bytes(),
        );
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        let page_end =
            STREAM_HEADER_LEN + (bytes.len() - STREAM_HEADER_LEN).next_multiple_of(PAGE_SIZE);
        bytes.resize(page_end, 0);
    }
    bytes
}

pub(super) fn generated_schema_from_paged(properties: &[u8]) -> &str {
    let length = index_from_u32(u32::from_le_bytes(properties[24..28].try_into().unwrap()));
    std::str::from_utf8(&properties[28..28 + length]).unwrap()
}

pub(super) fn generated_definition_catalog_for(schema: &str) -> Vec<u8> {
    let mut out = RECORD_MARKER.to_vec();
    lp_ascii(&mut out, schema);
    out.push(0);
    lp_ascii(&mut out, "Prism-001");
    lp_ascii(&mut out, "Prism-001");
    out.extend_from_slice(&2_u32.to_le_bytes());
    for value in ["Plastic/Thermoplastic", "Default", "Generated appearance"] {
        lp_ascii(&mut out, value);
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    lp_ascii(&mut out, "");
    paged_instance_properties(&out)
}
