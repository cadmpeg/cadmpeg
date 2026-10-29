// SPDX-License-Identifier: Apache-2.0

use super::{inventory_with_record, references};
use crate::feature::{PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily};
use crate::pmdc::{PmDcListMetadata, PmDcReference};
use crate::test_support::test_fixtures::{content_at, parse};

#[test]
fn body_selector_frames_empty_and_ordered_versioned_lists() {
    for (version, stamps) in [
        (16, vec![]),
        (23, vec![-1]),
        (25, vec![-1, 2500]),
        (31, vec![-1, 3100]),
    ] {
        for selected in [vec![], vec![0x8000_0007], vec![8, 0x8000_0007]] {
            let mut bytes = content_at(9, version, &stamps);
            bytes.extend(references(&selected));
            let property = parse(&bytes, |ctx, view| {
                super::super::feature_property_parser(super::super::motion::BODY_SELECTION_TYPE)
                    .expect("body selector parser")(ctx, view, version)
                .expect("body selector")
            });
            assert_eq!(property.header.stamps, stamps);
            let PmDcFeaturePropertyKind::References { family, items } = property.kind else {
                panic!("body selector list");
            };
            assert_eq!(family, PmDcFeatureReferenceFamily::BodySelection);
            assert_eq!(
                items.references(),
                selected
                    .iter()
                    .copied()
                    .map(PmDcReference::from_packed)
                    .collect::<Vec<_>>()
            );
            for end in 0..bytes.len() {
                parse(&bytes[..end], |ctx, view| {
                    assert!(super::super::feature_property_parser(
                        super::super::motion::BODY_SELECTION_TYPE
                    )
                    .expect("body selector parser")(ctx, view, version)
                    .is_err());
                });
            }
            bytes.push(0);
            parse(&bytes, |ctx, view| {
                assert!(super::super::feature_property_parser(
                    super::super::motion::BODY_SELECTION_TYPE
                )
                .expect("body selector parser")(ctx, view, version)
                .is_err());
            });
        }
    }
}

#[test]
fn body_selector_rejects_wrong_marker_and_unbounded_count() {
    for (list, reason) in [
        (vec![8, 0, 0, 0x30, 0, 0, 0, 0], "marker"),
        (
            vec![
                2, 0, 0, 0x30, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            "count exceeds remaining payload",
        ),
    ] {
        let mut bytes = content_at(9, 16, &[]);
        bytes.extend(list);
        parse(&bytes, |ctx, view| {
            let error =
                super::super::feature_property_parser(super::super::motion::BODY_SELECTION_TYPE)
                    .expect("body selector parser")(ctx, view, 16)
                .expect_err("invalid body selector");
            assert!(error.to_string().contains(reason));
        });
    }
}

#[test]
fn body_selector_preserves_nonempty_list_metadata() {
    let mut bytes = content_at(0, 16, &[]);
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(0x3000u16.to_le_bytes());
    for value in [1u32, 17, 19, 0x8000_0007] {
        bytes.extend(value.to_le_bytes());
    }
    let property = parse(&bytes, |ctx, view| {
        super::super::feature_property_parser(super::super::motion::BODY_SELECTION_TYPE)
            .expect("body selector parser")(ctx, view, 16)
        .expect("synthetic body selector")
    });
    let PmDcFeaturePropertyKind::References { items, .. } = property.kind else {
        panic!("body selector list");
    };
    assert_eq!(
        items.into_parts(),
        (
            2,
            Some(PmDcListMetadata::U32([17, 19])),
            vec![PmDcReference::from_packed(0x8000_0007)],
        )
    );
}

#[test]
fn body_selector_type_enters_the_feature_property_inventory() {
    let mut bytes = content_at(9, 16, &[]);
    bytes.extend(references(&[0x8000_0007]));
    let inventory = inventory_with_record(
        super::super::motion::BODY_SELECTION_TYPE,
        &bytes,
        cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("synthetic body selector");
    assert!(inventory.issues.is_empty());
    assert_eq!(inventory.properties.len(), 1);
    assert!(matches!(
        inventory.properties[0].kind,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            ..
        }
    ));
}
