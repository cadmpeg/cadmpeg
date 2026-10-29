// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::{indexed_header, lp_ascii};

pub(crate) fn append_body_owners(
    bulk: &mut Vec<u8>,
    records: &mut Vec<(u64, u64)>,
    owners: &[u64],
) -> Vec<u64> {
    records.push((350, u64_from_index(bulk.len())));
    indexed_header(bulk, *b"261", 350);
    let mut containers = Vec::new();
    for (ordinal, owner) in owners.iter().enumerate() {
        let record = 501 + u32::try_from(ordinal).unwrap();
        containers.push(u64::from(record));
        records.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(bulk, *b"260", record);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        for (target, guid) in [
            (
                350_u64,
                crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            ),
            (*owner, crate::records::body_ownership::COMPONENT_GUID),
        ] {
            bulk.push(1);
            bulk.extend_from_slice(&target.to_le_bytes());
            lp_ascii(bulk, guid);
            bulk.extend_from_slice(&[0, 0]);
        }
    }
    containers
}

type TypeRegistration<'a> = (&'a str, &'a str, u32, &'a str, &'a [u64]);

pub(crate) fn container_type(containers: &[u64]) -> TypeRegistration<'_> {
    (
        crate::records::body_ownership::CONTAINER_GUID,
        crate::records::body_ownership::CONTAINER_BASE_GUID,
        4,
        "Body",
        containers,
    )
}

pub(crate) fn body_type() -> TypeRegistration<'static> {
    (
        crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
        crate::design::presentation::BODY_PRESENTATION_BASE_TYPE_GUID,
        15,
        "Body",
        &[350],
    )
}
