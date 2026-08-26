//! Tests for the `operands` module.

use super::*;
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
        .map(|(ordinal, local_id)| SketchInputEntity {
            id: format!("marker-{local_id}"),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: ordinal as u32,
            offset: ordinal as u64,
            object_index: None,
            local_id: Some(local_id),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: None,
            links: Vec::new(),
            link_selector: None,
        })
        .collect::<Vec<_>>();
    let kind = FeatureInputOperandKind::Native(0x8386);
    assert_eq!(
        resolve_operand_marker(&markers, kind, 4).map(|marker| marker.id.as_str()),
        Some("marker-4")
    );
    assert_eq!(
        resolve_operand_marker(&markers, kind, 2).map(|marker| marker.id.as_str()),
        Some("marker-11")
    );
}

#[test]
fn line_distance_operand_selects_a_point_coded_linked_line_handle() {
    let endpoint = |id: &str, local_id, u| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: local_id,
        offset: u64::from(local_id),
        object_index: None,
        local_id: Some(local_id),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([u, 0.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let endpoints = [endpoint("first", 2, 1.0), endpoint("second", 3, 2.0)];
    let handle = SketchInputEntity {
        id: "line-handle".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 16,
        offset: 16,
        object_index: None,
        local_id: Some(16),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([9.0, 9.0]),
        links: endpoints
            .iter()
            .map(|endpoint| SketchInputLink {
                local_id: u16::try_from(endpoint.local_id.expect("local identity"))
                    .expect("u16 local identity"),
                entity_ref: endpoint.id.clone(),
            })
            .collect(),
        link_selector: Some(0x8386),
    };
    let markers = [&endpoints[0], &endpoints[1], &handle];

    assert_eq!(
        resolve_operand_marker(markers, FeatureInputOperandKind::Native(0x8386), 16,)
            .map(|marker| marker.id.as_str()),
        Some("line-handle")
    );
    assert!(
        resolve_operand_marker(markers, FeatureInputOperandKind::Native(0x8dda), 16,).is_none()
    );
}

#[test]
fn qualified_operand_selects_one_coordinate_marker_in_a_reused_local_id() {
    let marker = |id: &str, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: Some(7),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker("reference", None),
        marker("geometry", Some([1.0, 2.0])),
    ];
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x837b), 7,)
            .map(|marker| marker.id.as_str()),
        Some("geometry")
    );
}

#[test]
fn qualified_point_operand_selects_a_curve_marker_locus() {
    let marker = SketchInputEntity {
        id: "line-locus".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: Some(16),
        kind: SketchInputKind::LineOrCircle,
        state_value: None,
        coordinates_m: Some([1.0, 2.0]),
        links: Vec::new(),
        link_selector: None,
    };
    for tag in [0x837b, 0xbc7c] {
        assert_eq!(
            resolve_operand_marker(
                std::slice::from_ref(&marker),
                FeatureInputOperandKind::Native(tag),
                16,
            )
            .map(|resolved| resolved.id.as_str()),
            Some("line-locus")
        );
    }
    let mut markers = vec![marker];
    markers.extend((0..3).map(|index| SketchInputEntity {
        id: format!("point-{index}"),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: index,
        offset: u64::from(index + 1),
        object_index: None,
        local_id: Some(10 + index),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([f64::from(index), 0.0]),
        links: Vec::new(),
        link_selector: None,
    }));
    markers[0].local_id = Some(1);
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc7c), 1)
            .map(|resolved| resolved.id.as_str()),
        Some("point-1")
    );
}

#[test]
fn object_indexed_bc_operands_precede_local_and_ordinal_fallbacks() {
    let marker = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset as u32,
        offset,
        object_index,
        local_id: Some(100 + offset as u32),
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
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
        SketchInputEntity {
            local_id: Some(0),
            ..marker(
                "local-id-curve",
                3,
                Some(2),
                SketchInputKind::LineOrCircle,
                Some([2.0, 0.0]),
            )
        },
    ];

    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc7c), 0)
            .map(|marker| marker.id.as_str()),
        Some("indexed-curve-locus")
    );
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc87), 0)
            .map(|marker| marker.id.as_str()),
        Some("indexed-curve-locus")
    );
}

#[test]
fn roster_point_operand_uses_coordinate_point_order() {
    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset,
        offset: u64::from(offset),
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
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
        resolve_operand_marker(markers.iter(), FeatureInputOperandKind::Native(0x81dd), 1,)
            .map(|entity| entity.id.as_str()),
        Some("second")
    );
    assert!(resolve_operand_marker_excluding(
        markers.iter(),
        FeatureInputOperandKind::Native(0x81dd),
        0,
        &HashSet::from([String::from("first")]),
    )
    .is_none());
    assert!(
        resolve_operand_marker(markers.iter(), FeatureInputOperandKind::Native(0x81e7), 0,)
            .is_none()
    );
    assert!(operand_accepts_marker(
        FeatureInputOperandKind::Native(0x81e7),
        SketchInputKind::LineOrCircle,
    ));
}

#[test]
fn object_indexed_point_operands_precede_local_fallbacks() {
    let point = |id: &str, object_index, local_id| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: Some(object_index),
        local_id: Some(local_id),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([1.0, 2.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let indexed = point("indexed", 7, 100);
    let local = point("local", 8, 7);
    let markers = [&indexed, &local];

    for kind in [
        FeatureInputOperandKind::Native(0x814c),
        FeatureInputOperandKind::Native(0x8152),
    ] {
        assert_eq!(
            resolve_operand_marker(markers.iter().copied(), kind, 7)
                .map(|marker| marker.id.as_str()),
            Some("indexed")
        );
    }

    let duplicate = point("duplicate", 7, 101);
    for kind in [
        FeatureInputOperandKind::Native(0x814c),
        FeatureInputOperandKind::Native(0x8152),
    ] {
        assert!(resolve_operand_marker([&indexed, &duplicate], kind, 7).is_none());
    }
    assert!(
        resolve_operand_marker([&local], FeatureInputOperandKind::Native(0x814c), 7,).is_none()
    );
}

#[test]
fn relation_point_operands_use_object_index_before_local_identifier() {
    let marker = |id: &str, object_index, local_id, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id,
        kind,
        state_value: None,
        coordinates_m: matches!(
            kind,
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        )
        .then_some([1.0, 2.0]),
        links: Vec::new(),
        link_selector: None,
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
        FeatureInputOperandKind::Native(0x80ac),
        FeatureInputOperandKind::Native(0x80d5),
        FeatureInputOperandKind::Native(0x8138),
    ] {
        assert_eq!(
            resolve_operand_marker(markers.iter().copied(), kind, 7)
                .map(|resolved| resolved.id.as_str()),
            Some("indexed")
        );
        assert_eq!(
            resolve_operand_marker(markers.iter().copied(), kind, 9)
                .map(|resolved| resolved.id.as_str()),
            Some("relation")
        );
    }
}

#[test]
fn relation_point_operand_rejects_ambiguous_indexed_points() {
    let marker = |id: &str| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: Some(7),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([1.0, 2.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let first = marker("first");
    let second = marker("second");
    for kind in [
        FeatureInputOperandKind::Native(0x80ac),
        FeatureInputOperandKind::Native(0x80d5),
        FeatureInputOperandKind::Native(0x8138),
    ] {
        assert!(resolve_operand_marker([&first, &second], kind, 7).is_none());
    }
}

#[test]
fn point_operand_follows_relation_handle_graph_and_excludes_its_sibling() {
    let marker = |id: &str, local_id, kind, links: &[&str]| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id,
        kind,
        state_value: None,
        coordinates_m: None,
        links: links
            .iter()
            .map(|target| SketchInputLink {
                local_id: 0,
                entity_ref: (*target).into(),
            })
            .collect(),
        link_selector: None,
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
    assert_eq!(resolved[0].map(|marker| marker.id.as_str()), Some("first"));
    assert_eq!(resolved[1].map(|marker| marker.id.as_str()), Some("second"));

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
    assert_eq!(resolved[0].map(|marker| marker.id.as_str()), Some("first"));
    assert_eq!(resolved[1].map(|marker| marker.id.as_str()), Some("second"));
}

#[test]
fn curve_operand_selects_an_arc_by_local_identifier() {
    let markers = [
        SketchInputEntity {
            id: "line-11".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: Some(11),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: Some([0.0, 0.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "arc-3".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 1,
            offset: 1,
            object_index: None,
            local_id: Some(3),
            kind: SketchInputKind::Arc,
            state_value: None,
            coordinates_m: Some([1.0, 1.0]),
            links: Vec::new(),
            link_selector: None,
        },
    ];
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8dda), 3,)
            .map(|marker| marker.id.as_str()),
        Some("arc-3")
    );
}

#[test]
fn curve_operand_follows_a_unique_local_reference_handle() {
    let markers = [
        SketchInputEntity {
            id: "line-11".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: Some(11),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: Some([0.0, 0.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "arc-8".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 1,
            offset: 1,
            object_index: None,
            local_id: Some(8),
            kind: SketchInputKind::Arc,
            state_value: None,
            coordinates_m: Some([1.0, 1.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "reference-3".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 2,
            offset: 2,
            object_index: None,
            local_id: Some(3),
            kind: SketchInputKind::Relation(SketchRelationKind::Angle),
            state_value: None,
            coordinates_m: None,
            links: vec![crate::records::SketchInputLink {
                local_id: 8,
                entity_ref: "arc-8".into(),
            }],
            link_selector: Some(0),
        },
    ];
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8dda), 3,)
            .map(|marker| marker.id.as_str()),
        Some("arc-8")
    );
}

#[test]
fn curve_operand_excludes_an_already_resolved_sibling_from_a_reference_handle() {
    let curve = |id: &str, local_id, offset| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset as u32,
        offset,
        object_index: None,
        local_id: Some(local_id),
        kind: SketchInputKind::LineOrCircle,
        state_value: None,
        coordinates_m: Some([offset as f64, 0.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        curve("curve-7", 7, 0),
        curve("curve-5", 5, 1),
        SketchInputEntity {
            id: "reference-10".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 2,
            offset: 2,
            object_index: None,
            local_id: Some(10),
            kind: SketchInputKind::Relation(SketchRelationKind::Distance),
            state_value: None,
            coordinates_m: None,
            links: vec![
                crate::records::SketchInputLink {
                    local_id: 7,
                    entity_ref: "curve-7".into(),
                },
                crate::records::SketchInputLink {
                    local_id: 5,
                    entity_ref: "curve-5".into(),
                },
            ],
            link_selector: Some(0),
        },
    ];
    assert!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8386), 10).is_none()
    );
    assert_eq!(
        resolve_operand_marker_excluding(
            &markers,
            FeatureInputOperandKind::Native(0x8386),
            10,
            &HashSet::from(["curve-7".into()]),
        )
        .map(|marker| marker.id.as_str()),
        Some("curve-5")
    );
}

#[test]
fn exact_local_operand_excludes_an_already_resolved_sibling() {
    let point = |id: &str, offset| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset as u32,
        offset,
        object_index: None,
        local_id: Some(3),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([offset as f64, 0.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [point("first", 0), point("second", 1)];
    assert_eq!(
        resolve_operand_marker_excluding(
            &markers,
            FeatureInputOperandKind::Native(0xbc7c),
            3,
            &HashSet::from(["first".into()]),
        )
        .map(|marker| marker.id.as_str()),
        Some("second")
    );
}

#[test]
fn e1_operand_uses_unique_native_object_index_when_local_address_is_absent() {
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: Some(13),
        local_id: None,
        kind: SketchInputKind::Arc,
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    assert_eq!(
        resolve_operand_marker(
            std::slice::from_ref(&curve),
            FeatureInputOperandKind::E1,
            13
        )
        .map(|marker| marker.id.as_str()),
        Some("curve")
    );
    assert_eq!(
        resolve_operand_marker(
            std::slice::from_ref(&curve),
            FeatureInputOperandKind::Native(0x8386),
            13,
        )
        .map(|marker| marker.id.as_str()),
        Some("curve")
    );
}

#[test]
fn line_distance_810f_operand_uses_only_a_unique_line_handle() {
    let marker = |id: &str, object_index, local_id, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
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
            FeatureInputOperandKind::Native(0x810f),
            7,
        )
        .map(|marker| marker.id.as_str()),
        Some("line")
    );
    assert_eq!(
        resolve_operand_marker(
            markers.iter().copied(),
            FeatureInputOperandKind::Native(0x810f),
            8,
        )
        .map(|marker| marker.id.as_str()),
        Some("relation")
    );
    assert_eq!(
        resolve_operand_marker(
            markers.iter().copied(),
            FeatureInputOperandKind::Native(0x810f),
            9,
        )
        .map(|marker| marker.id.as_str()),
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
        FeatureInputOperandKind::Native(0x810f),
        7,
    )
    .is_none());
}

#[test]
fn line_distance_operand_uses_an_object_indexed_relation_line_handle() {
    let endpoint = |id: &str, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset,
        offset: u64::from(offset),
        object_index: None,
        local_id: Some(offset),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let endpoints = [
        endpoint("first", 1, Some([0.0, 0.0])),
        endpoint("second", 2, Some([1.0, 0.0])),
    ];
    let handle = SketchInputEntity {
        id: "relation-line-handle".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 16,
        offset: 16,
        object_index: Some(5),
        local_id: Some(6),
        kind: SketchInputKind::Relation(SketchRelationKind::Radius),
        state_value: None,
        coordinates_m: None,
        links: endpoints
            .iter()
            .map(|endpoint| SketchInputLink {
                local_id: u16::try_from(endpoint.local_id.expect("local identity"))
                    .expect("u16 local identity"),
                entity_ref: endpoint.id.clone(),
            })
            .collect(),
        link_selector: None,
    };
    let markers = [&endpoints[0], &endpoints[1], &handle];

    assert_eq!(
        resolve_operand_marker(markers, FeatureInputOperandKind::Native(0x8386), 5)
            .map(|marker| marker.id.as_str()),
        Some("relation-line-handle")
    );
}

#[test]
fn coordinate_line_handle_uses_its_own_coordinate_and_one_point_link() {
    let point = SketchInputEntity {
        id: "point".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 1,
        offset: 1,
        object_index: Some(2),
        local_id: Some(2),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([2.0, 0.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let relation = SketchInputEntity {
        id: "relation".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 2,
        offset: 2,
        object_index: Some(3),
        local_id: Some(3),
        kind: SketchInputKind::Relation(SketchRelationKind::Angle),
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let marker = SketchInputEntity {
        id: "line-handle".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: Some(1),
        local_id: Some(1),
        kind: SketchInputKind::Arc,
        state_value: None,
        coordinates_m: Some([1.0, 0.0]),
        links: vec![
            SketchInputLink {
                local_id: 3,
                entity_ref: relation.id.clone(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: point.id.clone(),
            },
        ],
        link_selector: None,
    };
    let markers = HashMap::from([
        (marker.id.as_str(), &marker),
        (point.id.as_str(), &point),
        (relation.id.as_str(), &relation),
    ]);

    assert_eq!(
        coordinate_line_endpoints_with_linked_point(&marker, &markers)
            .map(|endpoints| endpoints.map(|endpoint| endpoint.id.as_str())),
        Some(["line-handle", "point"])
    );
}
