// SPDX-License-Identifier: Apache-2.0

use crate::records::act::{
    ActChannelGroup, ActEntity, ActGuid, ActRegistryChannel, ActRegistryFlag, ActRootComponent,
    ActRootLayout, ActTableReference,
};
use crate::records::{
    entity_header::{DesignFeatureTimeline, DesignTimelineFrame},
    identity::Located,
};
use crate::test_support::native_test::reject_changed_id;
use std::collections::BTreeMap;
use std::num::NonZeroU64;

const GUID: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn group() -> ActChannelGroup {
    ActChannelGroup::try_new(
        100,
        Some(200),
        "261".to_owned().try_into().unwrap(),
        BTreeMap::from([(
            "Appearance".into(),
            Located {
                value: GUID.to_owned().try_into().unwrap(),
                offset: 120,
            },
        )]),
        None,
    )
    .unwrap()
}

#[test]
fn native_act_ids_bind_kind_and_key_at_each_admission_route() {
    for id in [
        "",
        "act-entity#7",
        "stream:act-guid#7",
        "stream:act-entity#8",
        "stream:act-entity#07",
    ] {
        assert!(ActEntity::try_new(id.into(), 7, "0_1".into(), None, group()).is_err());
    }
    reject_changed_id(
        ActEntity::try_new("stream:act-entity#7".into(), 7, "0_1".into(), None, group()).unwrap(),
        &[
            "",
            "act-entity#7",
            "stream:act-guid#7",
            "stream:act-entity#8",
            "stream:act-entity#07",
        ],
    );
    for id in ["id", "stream:act-guid#21", "stream:act-entity#20"] {
        assert!(ActGuid::new(id.into(), 20, 0, GUID.into()).is_err());
    }
    reject_changed_id(
        ActGuid::new("stream:act-guid#20".into(), 20, 0, GUID.into()).unwrap(),
        &["id", "stream:act-guid#21", "stream:act-entity#20"],
    );
    for id in ["id", "stream:act-table-reference#21", "stream:act-guid#20"] {
        assert!(ActTableReference::new(id.into(), 0, 20, 7).is_err());
    }
    reject_changed_id(
        ActTableReference::new("stream:act-table-reference#20".into(), 0, 20, 7).unwrap(),
        &["id", "stream:act-table-reference#21", "stream:act-guid#20"],
    );
    for id in ["id", "stream:act-registry-channel#21", "stream:act-guid#20"] {
        assert!(
            ActRegistryChannel::new(id.into(), 0, 20, "Appearance".into(), GUID.into()).is_err()
        );
    }
    reject_changed_id(
        ActRegistryChannel::new(
            "stream:act-registry-channel#20".into(),
            0,
            20,
            "Appearance".into(),
            GUID.into(),
        )
        .unwrap(),
        &["id", "stream:act-registry-channel#21", "stream:act-guid#20"],
    );
    for id in ["id", "stream:act-root-component#21", "stream:act-guid#20"] {
        assert!(ActRootComponent::try_new(
            id.into(),
            1,
            "261".to_owned().try_into().unwrap(),
            2,
            4,
            ActRegistryFlag::Off,
            ActRootLayout::new(20, "0_3".into(), String::new(), 1).unwrap()
        )
        .is_err());
    }
    let mut root = ActRootComponent::try_new(
        "stream:act-root-component#20".into(),
        1,
        "261".to_owned().try_into().unwrap(),
        2,
        4,
        ActRegistryFlag::Off,
        ActRootLayout::new(20, "0_3".into(), String::new(), 1).unwrap(),
    )
    .unwrap();
    root.try_set_strings("10_3".into(), "Renamed".into())
        .unwrap();
    assert_eq!(root.id(), "stream:act-root-component#20");
    assert_eq!(root.layout().byte_offset(), 20);
    let before = root.clone();
    assert!(root
        .try_set_strings("invalid".into(), "Name".into())
        .is_err());
    assert_eq!(root, before);
    reject_changed_id(
        root,
        &["id", "stream:act-root-component#21", "stream:act-guid#20"],
    );
}

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
