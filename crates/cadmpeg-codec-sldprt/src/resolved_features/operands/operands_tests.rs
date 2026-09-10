//! Tests for the `operands` module.

use super::*;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputOperand, FeatureInputOperandKind, SketchInputEntity, SketchInputKind,
    SketchInputLink, SketchRelationKind,
};
use std::collections::{HashMap, HashSet};

#[test]
fn qualified_operand_falls_back_to_marker_family_ordinal() {
    let markers = [4, 8, 11]
        .into_iter()
        .enumerate()
        .map(|(ordinal, local_id)| {
            let marker_id: String = format!("marker-{local_id}");
            let marker_parent: String = "lane".into();
            let mut constructed_marker = SketchInputEntity::new(
                marker_id,
                marker_parent,
                ordinal as u32,
                ordinal as u64,
                SketchInputKind::LineOrCircle,
            );
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker = constructed_marker.with_test_identity(None, Some(local_id));
            constructed_marker.state_value = None;
            constructed_marker.coordinates_m = None;
            constructed_marker.links = None;
            constructed_marker
        })
        .collect::<Vec<_>>();
    let kind = FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386);
    assert_eq!(
        resolve_operand_marker(&markers, kind, 4).map(crate::records::SketchInputEntity::id),
        Some("marker-4")
    );
    assert_eq!(
        resolve_operand_marker(&markers, kind, 2).map(crate::records::SketchInputEntity::id),
        Some("marker-11")
    );
}

#[test]
fn line_distance_operand_selects_a_point_coded_linked_line_handle() {
    let endpoint = |id: &str, local_id, u| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            local_id,
            u64::from(local_id),
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(local_id));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([u, 0.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    let endpoints = [endpoint("first", 2, 1.0), endpoint("second", 3, 2.0)];
    let handle = {
        let marker_id: String = "line-handle".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 16, 16, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(16));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([9.0, 9.0]);
        constructed_marker.links = crate::records::SketchInputLinks::new(
            0x8386,
            endpoints
                .iter()
                .map(|endpoint| SketchInputLink {
                    local_id: u16::try_from(endpoint.local_id().expect("local identity"))
                        .expect("u16 local identity"),
                    entity_ref: endpoint.id().to_string(),
                })
                .collect(),
        );
        constructed_marker
    };
    let markers = [&endpoints[0], &endpoints[1], &handle];

    assert_eq!(
        resolve_operand_marker(
            markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386),
            16,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("line-handle")
    );
    assert!(resolve_operand_marker(
        markers,
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
        16,
    )
    .is_none());
}

#[test]
fn qualified_operand_selects_one_coordinate_marker_in_a_reused_local_id() {
    let marker = |id: &str, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, 0, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(7));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let markers = [
        marker("reference", None),
        marker("geometry", Some([1.0, 2.0])),
    ];
    assert_eq!(
        resolve_operand_marker(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_837B),
            7,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("geometry")
    );
}

#[test]
fn qualified_point_operand_selects_a_curve_marker_locus() {
    let marker = {
        let marker_id: String = "line-locus".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            0,
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(16));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([1.0, 2.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    for tag in [0x837b, 0xbc7c] {
        assert_eq!(
            resolve_operand_marker(
                std::slice::from_ref(&marker),
                FeatureInputOperandKind::Native(tag.try_into().unwrap()),
                16,
            )
            .map(crate::records::SketchInputEntity::id),
            Some("line-locus")
        );
    }
    let mut markers = vec![marker];
    markers.extend((0..3).map(|index| {
        let marker_id: String = format!("point-{index}");
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            index,
            u64::from(index + 1),
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(10 + index));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([f64::from(index), 0.0]);
        constructed_marker.links = None;
        constructed_marker
    }));
    markers[0] = markers[0].with_test_identity(markers[0].object_index(), Some(1));
    assert_eq!(
        resolve_operand_marker(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C),
            1
        )
        .map(crate::records::SketchInputEntity::id),
        Some("point-1")
    );
}

#[test]
fn object_indexed_bc_operands_precede_local_and_ordinal_fallbacks() {
    let marker = |id: &str, offset, object_index, kind, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, offset as u32, offset, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker =
            constructed_marker.with_test_identity(object_index, Some(100 + offset as u32));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let markers = [
        marker(
            "unrelated-point",
            0,
            Some(3),
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "indexed-curve-locus",
            1,
            Some(0),
            SketchInputKind::LineOrCircle,
            Some([1.0, 0.0]),
        ),
        marker(
            "indexed-relation",
            2,
            Some(0),
            SketchInputKind::Relation(SketchRelationKind::Distance),
            None,
        ),
        {
            let mut constructed_marker = marker(
                "local-id-curve",
                3,
                Some(2),
                SketchInputKind::LineOrCircle,
                Some([2.0, 0.0]),
            );
            constructed_marker =
                constructed_marker.with_test_identity(constructed_marker.object_index(), Some(0));
            constructed_marker
        },
    ];

    assert_eq!(
        resolve_operand_marker(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C),
            0
        )
        .map(crate::records::SketchInputEntity::id),
        Some("indexed-curve-locus")
    );
    assert_eq!(
        resolve_operand_marker(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC87),
            0
        )
        .map(crate::records::SketchInputEntity::id),
        Some("indexed-curve-locus")
    );
}

#[test]
fn roster_point_operand_uses_coordinate_point_order() {
    let marker = |id: &str, offset, kind, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, offset, u64::from(offset), kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let markers = [
        marker("first", 20, SketchInputKind::Point, Some([0.0, 0.0])),
        marker(
            "second",
            30,
            SketchInputKind::ConstrainedPoint,
            Some([1.0, 0.0]),
        ),
        marker("unaddressable", 40, SketchInputKind::LineOrCircle, None),
    ];

    assert_eq!(
        resolve_operand_marker(
            markers.iter(),
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD),
            1,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("second")
    );
    assert!(resolve_operand_marker_excluding(
        markers.iter(),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD),
        0,
        &HashSet::from([String::from("first")]),
    )
    .is_none());
    assert!(resolve_operand_marker(
        markers.iter(),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7),
        0,
    )
    .is_none());
    assert!(operand_accepts_marker(
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7),
        SketchInputKind::LineOrCircle,
    ));
}

#[test]
fn object_indexed_point_operands_precede_local_fallbacks() {
    let point = |id: &str, object_index, local_id| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, 0, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker =
            constructed_marker.with_test_identity(Some(object_index), Some(local_id));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([1.0, 2.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    let indexed = point("indexed", 7, 100);
    let local = point("local", 8, 7);
    let markers = [&indexed, &local];

    for kind in [
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152),
    ] {
        assert_eq!(
            resolve_operand_marker(markers.iter().copied(), kind, 7)
                .map(crate::records::SketchInputEntity::id),
            Some("indexed")
        );
    }

    let duplicate = point("duplicate", 7, 101);
    for kind in [
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152),
    ] {
        assert!(resolve_operand_marker([&indexed, &duplicate], kind, 7).is_none());
    }
    assert!(resolve_operand_marker(
        [&local],
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C),
        7,
    )
    .is_none());
}

#[test]
fn relation_point_operands_use_object_index_before_local_identifier() {
    let marker = |id: &str, object_index, local_id, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(marker_id, marker_parent, 0, 0, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, local_id);
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = matches!(
            kind,
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        )
        .then_some([1.0, 2.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    let indexed = marker("indexed", Some(7), Some(100), SketchInputKind::Point);
    let local = marker("local", Some(8), Some(7), SketchInputKind::Point);
    let colliding_line = marker(
        "colliding-line",
        Some(7),
        Some(7),
        SketchInputKind::LineOrCircle,
    );
    let relation = marker(
        "relation",
        Some(9),
        Some(9),
        SketchInputKind::Relation(SketchRelationKind::Distance),
    );
    let markers = [&indexed, &local, &colliding_line, &relation];

    for kind in [
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_80AC),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D5),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8138),
    ] {
        assert_eq!(
            resolve_operand_marker(markers.iter().copied(), kind, 7)
                .map(crate::records::SketchInputEntity::id),
            Some("indexed")
        );
        assert_eq!(
            resolve_operand_marker(markers.iter().copied(), kind, 9)
                .map(crate::records::SketchInputEntity::id),
            Some("relation")
        );
    }
}

#[test]
fn relation_point_operand_rejects_ambiguous_indexed_points() {
    let marker = |id: &str| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, 0, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(Some(7), None);
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([1.0, 2.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    let first = marker("first");
    let second = marker("second");
    for kind in [
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_80AC),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D5),
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8138),
    ] {
        assert!(resolve_operand_marker([&first, &second], kind, 7).is_none());
    }
}

#[test]
fn point_operand_follows_relation_handle_graph_and_excludes_its_sibling() {
    let marker = |id: &str, local_id, kind, links: &[&str]| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(marker_id, marker_parent, 0, 0, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, local_id);
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = None;
        constructed_marker.links = crate::records::SketchInputLinks::new(
            0,
            links
                .iter()
                .map(|target| SketchInputLink {
                    local_id: 0,
                    entity_ref: (*target).into(),
                })
                .collect(),
        );
        constructed_marker
    };
    let markers = [
        marker("first", Some(5), SketchInputKind::Point, &[]),
        marker("second", Some(1), SketchInputKind::Point, &[]),
        marker(
            "relation-2",
            Some(2),
            SketchInputKind::Relation(SketchRelationKind::Distance),
            &["relation-0"],
        ),
        marker(
            "relation-0",
            Some(0),
            SketchInputKind::Relation(SketchRelationKind::Distance),
            &["second"],
        ),
    ];
    let operands = [
        FeatureInputOperand {
            kind: FeatureInputOperandKind::D6,
            entity_index: 0,
            offset: 0,
            reference_ref: "first-ref".into(),
            entity_ref: None,
        },
        FeatureInputOperand {
            kind: FeatureInputOperandKind::D6,
            entity_index: 2,
            offset: 0,
            reference_ref: "second-ref".into(),
            entity_ref: None,
        },
    ];
    let resolved = resolve_scalar_operand_markers(&markers, &operands);
    assert_eq!(
        resolved[0].map(crate::records::SketchInputEntity::id),
        Some("first")
    );
    assert_eq!(
        resolved[1].map(crate::records::SketchInputEntity::id),
        Some("second")
    );

    let duplicate = [
        operands[1].clone(),
        FeatureInputOperand {
            kind: FeatureInputOperandKind::D6,
            entity_index: 1,
            offset: 0,
            reference_ref: "known-second-ref".into(),
            entity_ref: None,
        },
    ];
    let resolved = resolve_scalar_operand_markers(&markers, &duplicate);
    assert_eq!(
        resolved[0].map(crate::records::SketchInputEntity::id),
        Some("first")
    );
    assert_eq!(
        resolved[1].map(crate::records::SketchInputEntity::id),
        Some("second")
    );
}

#[test]
fn curve_operand_selects_an_arc_by_local_identifier() {
    let markers = [
        {
            let marker_id: String = "line-11".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker = SketchInputEntity::new(
                marker_id,
                marker_parent,
                0,
                0,
                SketchInputKind::LineOrCircle,
            );
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker = constructed_marker.with_test_identity(None, Some(11));
            constructed_marker.state_value = None;
            constructed_marker.coordinates_m = Some([0.0, 0.0]);
            constructed_marker.links = None;
            constructed_marker
        },
        {
            let marker_id: String = "arc-3".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker =
                SketchInputEntity::new(marker_id, marker_parent, 1, 1, SketchInputKind::Arc);
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker = constructed_marker.with_test_identity(None, Some(3));
            constructed_marker.state_value = None;
            constructed_marker.coordinates_m = Some([1.0, 1.0]);
            constructed_marker.links = None;
            constructed_marker
        },
    ];
    assert_eq!(
        resolve_operand_marker(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
            3,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("arc-3")
    );
}

#[test]
fn curve_operand_follows_a_unique_local_reference_handle() {
    let markers = [
        {
            let marker_id: String = "line-11".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker = SketchInputEntity::new(
                marker_id,
                marker_parent,
                0,
                0,
                SketchInputKind::LineOrCircle,
            );
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker = constructed_marker.with_test_identity(None, Some(11));
            constructed_marker.state_value = None;
            constructed_marker.coordinates_m = Some([0.0, 0.0]);
            constructed_marker.links = None;
            constructed_marker
        },
        {
            let marker_id: String = "arc-8".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker =
                SketchInputEntity::new(marker_id, marker_parent, 1, 1, SketchInputKind::Arc);
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker = constructed_marker.with_test_identity(None, Some(8));
            constructed_marker.state_value = None;
            constructed_marker.coordinates_m = Some([1.0, 1.0]);
            constructed_marker.links = None;
            constructed_marker
        },
        {
            let marker_id: String = "reference-3".into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker = SketchInputEntity::new(
                marker_id,
                marker_parent,
                2,
                2,
                SketchInputKind::Relation(SketchRelationKind::Angle),
            );
            constructed_marker.feature_ref = Some("feature".into());
            constructed_marker = constructed_marker.with_test_identity(None, Some(3));
            constructed_marker.state_value = None;
            constructed_marker.coordinates_m = None;
            constructed_marker.links = crate::records::SketchInputLinks::new(
                0,
                vec![crate::records::SketchInputLink {
                    local_id: 8,
                    entity_ref: "arc-8".into(),
                }],
            );
            constructed_marker
        },
    ];
    assert_eq!(
        resolve_operand_marker(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
            3,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("arc-8")
    );
}

#[test]
fn curve_operand_excludes_an_already_resolved_sibling_from_a_reference_handle() {
    let curve = |id: &str, local_id, offset| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            offset as u32,
            offset,
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(local_id));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([offset as f64, 0.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    let markers = [curve("curve-7", 7, 0), curve("curve-5", 5, 1), {
        let marker_id: String = "reference-10".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            2,
            2,
            SketchInputKind::Relation(SketchRelationKind::Distance),
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(10));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = None;
        constructed_marker.links = crate::records::SketchInputLinks::new(
            0,
            vec![
                crate::records::SketchInputLink {
                    local_id: 7,
                    entity_ref: "curve-7".into(),
                },
                crate::records::SketchInputLink {
                    local_id: 5,
                    entity_ref: "curve-5".into(),
                },
            ],
        );
        constructed_marker
    }];
    assert!(resolve_operand_marker(
        &markers,
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386),
        10
    )
    .is_none());
    assert_eq!(
        resolve_operand_marker_excluding(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386),
            10,
            &HashSet::from(["curve-7".into()]),
        )
        .map(crate::records::SketchInputEntity::id),
        Some("curve-5")
    );
}

#[test]
fn exact_local_operand_excludes_an_already_resolved_sibling() {
    let point = |id: &str, offset| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            offset as u32,
            offset,
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(3));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([offset as f64, 0.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    let markers = [point("first", 0), point("second", 1)];
    assert_eq!(
        resolve_operand_marker_excluding(
            &markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C),
            3,
            &HashSet::from(["first".into()]),
        )
        .map(crate::records::SketchInputEntity::id),
        Some("second")
    );
}

#[test]
fn e1_operand_uses_unique_native_object_index_when_local_address_is_absent() {
    let curve = {
        let marker_id: String = "curve".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, 0, SketchInputKind::Arc);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(Some(13), None);
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };
    assert_eq!(
        resolve_operand_marker(
            std::slice::from_ref(&curve),
            FeatureInputOperandKind::E1,
            13
        )
        .map(crate::records::SketchInputEntity::id),
        Some("curve")
    );
    assert_eq!(
        resolve_operand_marker(
            std::slice::from_ref(&curve),
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386),
            13,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("curve")
    );
}

#[test]
fn line_distance_810f_operand_uses_only_a_unique_line_handle() {
    let marker = |id: &str, object_index, local_id, kind, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(marker_id, marker_parent, 0, 0, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, local_id);
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let line = marker("line", Some(7), None, SketchInputKind::LineOrCircle, None);
    let colliding_point = marker(
        "point",
        Some(7),
        Some(7),
        SketchInputKind::Point,
        Some([1.0, 2.0]),
    );
    let relation = marker(
        "relation",
        Some(8),
        None,
        SketchInputKind::Relation(SketchRelationKind::Vertical),
        None,
    );
    let proxy = marker("proxy", None, Some(9), SketchInputKind::Point, None);
    let markers = [&line, &colliding_point, &relation, &proxy];

    assert_eq!(
        resolve_operand_marker(
            markers.iter().copied(),
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F),
            7,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("line")
    );
    assert_eq!(
        resolve_operand_marker(
            markers.iter().copied(),
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F),
            8,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("relation")
    );
    assert_eq!(
        resolve_operand_marker(
            markers.iter().copied(),
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F),
            9,
        )
        .map(crate::records::SketchInputEntity::id),
        Some("proxy")
    );

    let second_line = marker(
        "second-line",
        Some(7),
        None,
        SketchInputKind::LineOrCircle,
        None,
    );
    assert!(resolve_operand_marker(
        [&line, &second_line],
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F),
        7,
    )
    .is_none());
}

#[test]
fn line_distance_operand_uses_an_object_indexed_relation_line_handle() {
    let endpoint = |id: &str, offset, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            offset,
            u64::from(offset),
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, Some(offset));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let endpoints = [
        endpoint("first", 1, Some([0.0, 0.0])),
        endpoint("second", 2, Some([1.0, 0.0])),
    ];
    let handle = {
        let marker_id: String = "relation-line-handle".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            16,
            16,
            SketchInputKind::Relation(SketchRelationKind::Radius),
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(Some(5), Some(6));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = None;
        constructed_marker.links = crate::records::SketchInputLinks::new(
            0,
            endpoints
                .iter()
                .map(|endpoint| SketchInputLink {
                    local_id: u16::try_from(endpoint.local_id().expect("local identity"))
                        .expect("u16 local identity"),
                    entity_ref: endpoint.id().to_string(),
                })
                .collect(),
        );
        constructed_marker
    };
    let markers = [&endpoints[0], &endpoints[1], &handle];

    assert_eq!(
        resolve_operand_marker(
            markers,
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386),
            5
        )
        .map(crate::records::SketchInputEntity::id),
        Some("relation-line-handle")
    );
}

#[test]
fn coordinate_line_handle_uses_its_own_coordinate_and_one_point_link() {
    let point = {
        let marker_id: String = "point".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 1, 1, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(Some(2), Some(2));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([2.0, 0.0]);
        constructed_marker.links = None;
        constructed_marker
    };
    let relation = {
        let marker_id: String = "relation".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            2,
            2,
            SketchInputKind::Relation(SketchRelationKind::Angle),
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(Some(3), Some(3));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };
    let marker = {
        let marker_id: String = "line-handle".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, 0, SketchInputKind::Arc);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(Some(1), Some(1));
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some([1.0, 0.0]);
        constructed_marker.links = crate::records::SketchInputLinks::new(
            0,
            vec![
                SketchInputLink {
                    local_id: 3,
                    entity_ref: relation.id().to_string(),
                },
                SketchInputLink {
                    local_id: 2,
                    entity_ref: point.id().to_string(),
                },
            ],
        );
        constructed_marker
    };
    let markers = HashMap::from([
        (marker.id(), &marker),
        (point.id(), &point),
        (relation.id(), &relation),
    ]);

    assert_eq!(
        coordinate_line_endpoints_with_linked_point(&marker, &markers)
            .map(|endpoints| endpoints.map(crate::records::SketchInputEntity::id)),
        Some(["line-handle", "point"])
    );
}
