// SPDX-License-Identifier: Apache-2.0

use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
use crate::records::identity::{Located, ReferenceRun};

/// Type-table rows of one Design Geometry origin in `meta_stream`: axes at
/// `first..first + 3` and planes at `first + 3..first + 6`.
pub(crate) fn origin_design_types(meta_stream: &str, first: u64) -> Vec<SegmentType> {
    vec![
        type_row(meta_stream, AXIS_TYPE_GUID, 100, first..first + 3),
        type_row(
            meta_stream,
            "8D424E29-DE23-4B1F-A3AD-84C52A497D1A",
            200,
            first + 3..first + 6,
        ),
    ]
}

/// One axis-type row that lists only `identity`.
pub(crate) fn axis_row(meta_stream: &str, identity: u64) -> SegmentType {
    type_row(meta_stream, AXIS_TYPE_GUID, 100, [identity])
}

/// One plane-type row that lists only `identity`.
pub(crate) fn plane_row(meta_stream: &str, identity: u64) -> SegmentType {
    type_row(
        meta_stream,
        "8D424E29-DE23-4B1F-A3AD-84C52A497D1A",
        200,
        [identity],
    )
}

/// One row of a type other than the origin types that lists only `identity`.
pub(crate) fn other_type_row(meta_stream: &str, identity: u64) -> SegmentType {
    type_row(
        meta_stream,
        "00112233-4455-4677-8899-AABBCCDDEEFF",
        300,
        [identity],
    )
}

/// One component naming-space row that registers `identities`.
pub(crate) fn component_row(
    meta_stream: &str,
    identities: impl IntoIterator<Item = u64>,
) -> SegmentType {
    registered_row(
        meta_stream,
        "0F1E2D3C-4B5A-4978-8695-A4B3C2D1E0F0",
        400,
        "Component",
        BaseTypeGuid::Guid {
            value: "21F379C8-CAFD-4985-B461-767673A4C502"
                .to_owned()
                .try_into()
                .unwrap(),
            offset: 0,
        },
        identities,
    )
}

const AXIS_TYPE_GUID: &str = "19031004-3AB6-4D13-ABBF-C51EE214688D";

fn type_row(
    meta_stream: &str,
    guid: &str,
    byte_offset: u64,
    identities: impl IntoIterator<Item = u64>,
) -> SegmentType {
    registered_row(
        meta_stream,
        guid,
        byte_offset,
        "Geometry",
        BaseTypeGuid::Absent,
        identities,
    )
}

fn registered_row(
    meta_stream: &str,
    guid: &str,
    byte_offset: u64,
    module: &str,
    base_type_guid: BaseTypeGuid,
    identities: impl IntoIterator<Item = u64>,
) -> SegmentType {
    SegmentType::try_new(
        format!("{meta_stream}:design-type#{byte_offset}"),
        SegmentTypeData {
            byte_offset,
            type_guid: guid.to_owned().try_into().unwrap(),
            type_guid_offset: 0,
            base_type_guid,
            version: 0,
            version_offset: 0,
            module: module.to_owned(),
            entities: ReferenceRun::located(
                identities
                    .into_iter()
                    .map(|value| Located { value, offset: 0 })
                    .collect(),
            ),
        },
    )
    .unwrap()
}
