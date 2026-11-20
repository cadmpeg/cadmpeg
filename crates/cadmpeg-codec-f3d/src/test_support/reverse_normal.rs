// SPDX-License-Identifier: Apache-2.0
//! Linked Reverse Normal scopes and Design timelines.

use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::feature::body_ops::DesignReverseNormalOperation;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::Located;
use crate::records::references::DesignClassTag;

pub(crate) const STREAM: &str = "f3d:native/BulkStream.dat";
const GROUP: u32 = 704;

/// A Reverse Normal scope of `record_index` with an optional link GUID.
pub(crate) fn linked(record_index: u32, link: Option<&str>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{record_index}"),
        DesignFeatureKind::ReverseNormal,
        record_index,
    );
    if let DesignScopePayloadMut::ReverseNormal(slot) = scope.payload_mut() {
        *slot = Some(DesignReverseNormalOperation {
            body_group: Located {
                value: GROUP,
                offset: 26,
            },
            link: link.map(|guid| Located {
                value: crate::records::mesh::DesignRelaxedGuidText::try_from(guid.to_owned())
                    .unwrap(),
                offset: 40,
            }),
        });
    }
    scope
}

/// A Design timeline of `items` in [`STREAM`].
pub(crate) fn timeline(items: &[u64]) -> DesignFeatureTimeline {
    DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(STREAM, 10),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            10,
            items
                .iter()
                .map(|&value| Located { value, offset: 0 })
                .collect(),
        ),
        DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(35).unwrap(),
        0,
        std::num::NonZeroU64::new(17).unwrap(),
    )
    .unwrap()
}
