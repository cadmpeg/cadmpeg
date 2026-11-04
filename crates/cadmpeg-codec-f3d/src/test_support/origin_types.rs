// SPDX-License-Identifier: Apache-2.0

use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
use crate::records::identity::{Located, ReferenceRun};

/// Type-table rows of one Design Geometry origin in `meta_stream`: axes at
/// `first..first + 3` and planes at `first + 3..first + 6`.
pub(crate) fn origin_design_types(meta_stream: &str, first: u64) -> Vec<SegmentType> {
    [
        ("19031004-3AB6-4D13-ABBF-C51EE214688D", first, 100),
        ("8D424E29-DE23-4B1F-A3AD-84C52A497D1A", first + 3, 200),
    ]
    .into_iter()
    .map(|(guid, start, byte_offset)| {
        SegmentType::try_new(
            format!("{meta_stream}:design-type#{byte_offset}"),
            SegmentTypeData {
                byte_offset,
                type_guid: guid.to_owned().try_into().unwrap(),
                type_guid_offset: 0,
                base_type_guid: BaseTypeGuid::Absent,
                version: 0,
                version_offset: 0,
                module: "Geometry".to_owned(),
                entities: ReferenceRun::located(
                    (start..start + 3)
                        .map(|value| Located { value, offset: 0 })
                        .collect(),
                ),
            },
        )
        .unwrap()
    })
    .collect()
}
