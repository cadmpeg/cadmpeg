// SPDX-License-Identifier: Apache-2.0
//! Synthetic Design streams with local component definitions, with and
//! without their occurrence placement records.

use cadmpeg_core::decode::u64_from_index;

use crate::test_support::component_body_test::{append_body_owners, body_type, container_type};
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::{indexed_header, lp_ascii, lp_utf16};

/// A repeated local module with two children and one existing body-map binding.
pub(crate) fn nested_local_component_design(body_owners: &[u64]) -> (Vec<u8>, Vec<u8>) {
    component_design(body_owners, true)
}

/// The nested local module design with its occurrence placement records
/// removed: three component definitions, one body-map binding, no occurrence.
pub(crate) fn flat_component_design(body_owners: &[u64]) -> (Vec<u8>, Vec<u8>) {
    component_design(body_owners, false)
}

fn component_design(body_owners: &[u64], placements: bool) -> (Vec<u8>, Vec<u8>) {
    let mut bulk = Vec::new();
    let mut records = Vec::new();
    for (record, root, context) in [
        (100, 101_u64, "00000000-0000-4000-8000-000000000100"),
        (200, 201, "00000000-0000-4000-8000-000000000200"),
        (300, 301, "00000000-0000-4000-8000-000000000300"),
    ] {
        records.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"256", record);
        bulk.push(1);
        bulk.extend_from_slice(&root.to_le_bytes());
        bulk.extend_from_slice(&[0; 2]);
        lp_utf16(&mut bulk, context);
    }
    let placement_records: Vec<u64> = if placements {
        (400..=404).collect()
    } else {
        Vec::new()
    };
    if placements {
        records.push((400, u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"257", 400);
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        lp_ascii(&mut bulk, "rootInstance");
        bulk.push(1);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        bulk.push(1);
        bulk.extend_from_slice(&100_u64.to_le_bytes());
        bulk.extend_from_slice(&[0; 7]);
        let parent = [
            [0.0, -1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let translated = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (record, definition, owner, ordinal, transform) in [
            (401, 200_u64, 101_u64, 1_u32, Some(parent)),
            (402, 300, 201, 1, None),
            (403, 300, 201, 2, Some(translated)),
            (404, 200, 101, 2, Some(parent)),
        ] {
            records.push((u64::from(record), u64_from_index(bulk.len())));
            indexed_header(&mut bulk, *b"257", record);
            bulk.extend_from_slice(&[0; 8]);
            bulk.push(1);
            bulk.extend_from_slice(&1_u32.to_le_bytes());
            bulk.push(1);
            bulk.extend_from_slice(&definition.to_le_bytes());
            bulk.extend_from_slice(&[0; 2]);
            bulk.extend_from_slice(&0_u32.to_le_bytes());
            bulk.push(1);
            bulk.extend_from_slice(&1_u32.to_le_bytes());
            lp_ascii(&mut bulk, "SelfGated");
            lp_ascii(&mut bulk, "IntrinsicMetaTypebool");
            bulk.push(0);
            bulk.extend_from_slice(&[0; 2]);
            bulk.extend_from_slice(&ordinal.to_le_bytes());
            lp_utf16(&mut bulk, "00000000-0000-4000-8000-000000000999");
            lp_utf16(&mut bulk, &format!("00000000-0000-4000-8001-{record:012}"));
            bulk.push(1);
            bulk.extend_from_slice(&500_u64.to_le_bytes());
            bulk.extend_from_slice(&[0, 1, 1, 0, 0, 0, 0]);
            bulk.push(1);
            bulk.extend_from_slice(&definition.to_le_bytes());
            bulk.extend_from_slice(&[0; 2]);
            bulk.push(u8::from(transform.is_none()));
            if let Some(transform) = transform {
                for value in transform.into_iter().flatten() {
                    bulk.extend_from_slice(&f64::to_le_bytes(value));
                }
            }
            bulk.extend_from_slice(&[0; 9]);
            bulk.push(1);
            bulk.extend_from_slice(&owner.to_le_bytes());
            bulk.extend_from_slice(&[0; 2]);
        }
    }
    records.push((500, u64_from_index(bulk.len())));
    indexed_header(&mut bulk, *b"258", 500);
    bulk.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    bulk.extend_from_slice(&42_u64.to_le_bytes());
    bulk.extend_from_slice(&350_u64.to_le_bytes());
    bulk.extend_from_slice(&1793_u64.to_le_bytes());
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bulk, "BREP.module.smbh");
    let containers = append_body_owners(&mut bulk, &mut records, body_owners);
    let meta = design_metastream_with_records(
        &[
            (
                crate::records::body_ownership::COMPONENT_GUID,
                "",
                2,
                "Component",
                &[100, 200, 300],
            ),
            (
                crate::records::feature::assembly_features::OCCURRENCE_PLACEMENT_TYPE_GUID,
                "",
                2,
                "Component",
                &placement_records,
            ),
            (
                crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                "Body",
                &[500],
            ),
            (
                "00000000-0000-4000-8000-000000000011",
                "21F379C8-CAFD-4985-B461-767673A4C502",
                2,
                "Component",
                &[101, 201, 301],
            ),
            container_type(&containers),
            body_type(),
        ],
        &records,
    );
    (bulk, meta)
}
