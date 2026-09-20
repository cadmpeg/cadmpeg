// SPDX-License-Identifier: Apache-2.0

use crate::records::entity_header::{DesignFeatureTimeline, DesignTimelineFrame};
use crate::records::identity::Located;
use crate::test_support::native_test::reject_changed_id;
use std::num::NonZeroU64;

#[test]
fn timeline_id_binds_a_design_stream_and_frame_offset() {
    let make = |id: &str| {
        DesignFeatureTimeline::try_new(
            id.into(),
            DesignTimelineFrame::new(
                200,
                100,
                220,
                240,
                vec![Located {
                    value: 101,
                    offset: 245,
                }],
            )
            .unwrap(),
            "256".to_owned().try_into().unwrap(),
            NonZeroU64::new(35).unwrap(),
            0,
            NonZeroU64::new(17).unwrap(),
        )
    };
    let invalid = [
        "timeline",
        "stream:design-feature-timeline#200",
        "f3d:Design/BulkStream.dat:design-feature-timeline#201",
        "f3d:Design/BulkStream.dat:act-guid#200",
    ];
    for id in invalid {
        assert!(make(id).is_err());
    }
    reject_changed_id(
        make("f3d:Design/BulkStream.dat:design-feature-timeline#200").unwrap(),
        &invalid,
    );
}
