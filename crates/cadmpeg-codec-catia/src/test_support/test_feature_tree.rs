// SPDX-License-Identifier: Apache-2.0
//! Synthetic object graphs of compact feature-tree records: a part root, its
//! sets, and their features.

use crate::test_support::test_object_graph::{
    catalog_stream, entity_table_record_with_definition_and_value, object_graph_from_records,
    object_graph_record,
};

/// Zero-based catalog ordinals, so each compact tag is `ordinal + 0x34`.
pub(crate) const CATALOG: [&str; 15] = [
    "CATCatalogManager",
    "catalogManager",
    "catalogLinks",
    "",
    "MechanicalPart",
    "Part1",
    "_FtrList",
    "CurrentFeature",
    "ShowNoShowStamp",
    "MainTool",
    "MechanicalTool",
    "HybridBody",
    "Pad",
    "GSMPlane",
    "_ReferencePlanes",
];
pub(crate) const MECHANICAL_PART: u8 = 0x38;
pub(crate) const FTR_LIST: u8 = 0x3a;
pub(crate) const CURRENT_FEATURE: u8 = 0x3b;
pub(crate) const MAIN_TOOL: u8 = 0x3d;
pub(crate) const MECHANICAL_TOOL: u8 = 0x3e;
pub(crate) const HYBRID_BODY: u8 = 0x3f;
pub(crate) const PAD: u8 = 0x40;
pub(crate) const GSM_PLANE: u8 = 0x41;
pub(crate) const REFERENCE_PLANES: u8 = 0x42;

pub(crate) fn escaped(value: u32) -> Vec<u8> {
    let mut bytes = vec![0x80];
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// One graph of `(entity id, definition prefix, payload)` records followed by
/// its catalog.
pub(crate) fn stream(records: &[(u32, Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (entity_id, prefix, _) in records {
        bytes.extend(entity_table_record_with_definition_and_value(
            *entity_id,
            prefix,
            &[],
        ));
    }
    bytes.push(0xde);
    let objects = records
        .iter()
        .map(|(_, _, payload)| object_graph_record(&[0x12, 0x82, 0x84], payload))
        .collect::<Vec<_>>();
    bytes.extend(object_graph_from_records(&objects));
    bytes.extend(catalog_stream(&CATALOG));
    bytes
}

pub(crate) fn compact(flags: u8, tag: u8) -> Vec<u8> {
    vec![0x00, flags, tag, tag]
}

/// Compact prefix whose class is a wide one-based selector.
pub(crate) fn wide(flags: u8, ordinal: u32) -> Vec<u8> {
    let mut prefix = vec![0x00, flags, 0x32];
    prefix.extend_from_slice(&ordinal.to_le_bytes());
    prefix.push(0x32);
    prefix.extend_from_slice(&ordinal.to_le_bytes());
    prefix
}

/// A part root listing a plane, a hybrid body, and an empty body, with its
/// in-work object and main body; the hybrid body lists two pads.
pub(crate) fn part() -> Vec<(u32, Vec<u8>, Vec<u8>)> {
    let mut root = vec![FTR_LIST, 0x84, 0x81, 0x85];
    root.extend(escaped(0x1000));
    root.push(0x84);
    root.extend(escaped(0x1001));
    root.extend([0x88, 0xd1, 0x7f, CURRENT_FEATURE, 0x86, MAIN_TOOL, 0x84]);
    root.extend([REFERENCE_PLANES, 0x82, 0x81, 0x85, 0x81, 0xfe]);
    let mut body = vec![FTR_LIST, 0x83, 0x81, 0x86];
    body.extend(escaped(0x2000));
    body.push(0x87);
    body.extend(escaped(0x2001));
    body.push(0xfe);
    vec![
        (2, vec![0x00, 0x00, MECHANICAL_PART, 0x39], root),
        (3, compact(0x08, HYBRID_BODY), body),
        (4, compact(0x0c, GSM_PLANE), vec![0xfe]),
        (5, compact(0x08, PAD), vec![0xfe]),
        (6, wide(0x08, 13), vec![0xfe]),
        (7, compact(0x08, MECHANICAL_TOOL), vec![0xfe]),
    ]
}
