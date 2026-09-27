// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::ReferenceOrigin;
use crate::graph::expectation::{ExpectationLabel, ReferenceExpectation};
use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    build, cyclic_transform_nodes, ParameterResolver, ReferenceEdge, ReferenceKind, Resolution,
    MAX_POINTER_SEQUENCE,
};
use crate::loss::IgesLossCode;
use crate::test_support::directory_target;
use crate::test_support::test_cards::{
    card, directory_card, fixed_ascii_with_global, global_card_count, parameter_card,
};
use crate::test_support::test_curves_and_surfaces::point_file;
use crate::IgesCodec;

#[test]
fn reference_summary_refuses_note_limit_without_heap_group_index() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let graph = BTreeMap::from([(
        1,
        vec![ReferenceEdge {
            origin: ReferenceOrigin::Directory(ReferenceKind::Transform),
            raw_pointer: 3,
            resolution: Resolution::Dangling,
            expected: ReferenceExpectation::Named(ExpectationLabel::Type124Transformation),
        }],
    )]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::summary_notes(&graph, &ctx);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges reference summary notes"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::summary_notes(&graph, &ctx).unwrap(),
        ["references.dangling=1"]
    );
}

#[test]
fn parameter_resolver_directory_index_refuses_collection_limit_before_insert() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let directory = [directory_target(1, 116)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = ParameterResolver::new(&directory, &ctx);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges parameter resolver directory index"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(ParameterResolver::new(&directory, &ctx).is_ok());
}

#[test]
fn parameter_resolver_edges_refuse_each_collection_limit_before_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let directory = [directory_target(1, 116)];
    for (cap, operation) in [
        (1, "iges parameter resolver edge groups"),
        (2, "iges parameter resolver edges"),
        (3, "iges parameter resolver graph groups"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
        let resolution = resolver.resolve(
            1,
            0,
            3,
            ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
            |_| true,
        );
        let result = if cap == 3 {
            assert_eq!(resolution.unwrap(), None);
            resolver.append_to(&mut BTreeMap::new())
        } else {
            resolution.map(|_| ())
        };
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == cap
                    && limit.additional == 1
                    && limit.operation == operation
        ));
    }

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(
        resolver
            .resolve(
                1,
                0,
                3,
                ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                |_| true,
            )
            .unwrap(),
        None
    );
    let mut graph = BTreeMap::new();
    resolver.append_to(&mut graph).unwrap();
    assert_eq!(graph[&1].len(), 1);
}

#[test]
fn parameter_resolver_expected_forms_refuse_collection_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let directory = [directory_target(1, 116)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    let result = resolver.resolve_type(1, 0, 1, 116, &[0]);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 1
                && limit.additional == 1
                && limit.operation == "iges parameter resolver expected forms"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(resolver.resolve_type(1, 0, 1, 116, &[0]).unwrap(), Some(1));
}

#[test]
fn parameter_resolver_append_refuses_existing_graph_edge_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let directory = [directory_target(1, 116)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(
        resolver
            .resolve(
                1,
                0,
                3,
                ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                |_| true,
            )
            .unwrap(),
        None
    );
    let mut graph = BTreeMap::from([(1, Vec::new())]);
    let result = resolver.append_to(&mut graph);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 3
                && limit.additional == 1
                && limit.operation == "iges appended parameter reference edges"
    ));
    assert!(graph[&1].is_empty());

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(
        resolver
            .resolve(
                1,
                0,
                3,
                ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                |_| true,
            )
            .unwrap(),
        None
    );
    let mut graph = BTreeMap::from([(1, Vec::new())]);
    resolver.append_to(&mut graph).unwrap();
    assert_eq!(graph[&1].len(), 1);
}

#[test]
fn parameter_resolver_expected_types_refuse_collection_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let directory = [directory_target(1, 116)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    let result = resolver.resolve_any_of(1, 0, 1, (212, 312, &[402]), |_| false);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 1
                && limit.additional == 1
                && limit.operation == "iges parameter resolver expected types"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(
        resolver
            .resolve_any_of(1, 0, 1, (212, 312, &[402]), |_| false)
            .unwrap(),
        None
    );
}

#[test]
fn parameter_pointers_enforce_the_seven_digit_sequence_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let maximum = u32::try_from(MAX_POINTER_SEQUENCE).unwrap();
    let directory = [directory_target(maximum, 116)];
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();

    assert_eq!(
        resolver
            .resolve(
                1,
                0,
                i64::from(maximum),
                ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                |_| true
            )
            .unwrap(),
        Some(maximum)
    );
    assert_eq!(
        resolver
            .resolve(
                1,
                1,
                i64::from(maximum) + 1,
                ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                |_| true
            )
            .unwrap(),
        None
    );
    assert_eq!(
        resolver
            .resolve_negative(
                2,
                0,
                -i64::from(maximum),
                ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                |_| true
            )
            .unwrap(),
        Some(maximum)
    );
    assert_eq!(
        resolver
            .resolve_negative(
                2,
                1,
                -i64::from(maximum) - 1,
                ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                |_| true
            )
            .unwrap(),
        None
    );

    let mut graph = BTreeMap::new();
    resolver.append_to(&mut graph).unwrap();
    assert_eq!(
        graph[&1]
            .iter()
            .map(|edge| edge.resolution)
            .collect::<Vec<_>>(),
        vec![Resolution::Resolved(maximum), Resolution::OutOfRange]
    );
    assert_eq!(
        graph[&2]
            .iter()
            .map(|edge| edge.resolution)
            .collect::<Vec<_>>(),
        vec![Resolution::Resolved(maximum), Resolution::OutOfRange]
    );
}

#[test]
fn semantic_expectation_labels_are_preserved_in_pointer_losses() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let directory = [directory_target(1, 116)];
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(
        resolver
            .resolve(
                1,
                1,
                3,
                ReferenceExpectation::Named(ExpectationLabel::Type124Transformation),
                |target| target.entity_type == 124,
            )
            .unwrap(),
        None
    );
    assert_eq!(
        resolver
            .resolve_negative(
                1,
                2,
                -3,
                ReferenceExpectation::Named(ExpectationLabel::Type310Form0FontDefinition),
                |target| target.entity_type == 310 && target.form == 0,
            )
            .unwrap(),
        None
    );
    let mut graph = BTreeMap::new();
    resolver.append_to(&mut graph).unwrap();
    let source = point_file();
    let scan = crate::card::scan(&source).unwrap();
    let messages = super::losses(&graph, &scan, &[])
        .into_iter()
        .map(|note| note.message)
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            "IGES Directory Entry D1 Parameter pointer 3 has dangling resolution; expected type-124-transformation",
            "IGES Directory Entry D1 Parameter pointer -3 has dangling resolution; expected type-310-form-0-font-definition",
        ]
    );
}

#[test]
fn directory_pointers_enforce_the_seven_digit_sequence_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let maximum = u32::try_from(MAX_POINTER_SEQUENCE).unwrap();
    let mut source = directory_target(1, 116);
    source.transform = i64::from(maximum);
    let graph = build(&[source, directory_target(maximum, 124)], &ctx).unwrap();
    let edge = graph[&1]
        .iter()
        .find(|edge| edge.origin == ReferenceOrigin::Directory(ReferenceKind::Transform))
        .unwrap();
    assert_eq!(edge.resolution, Resolution::Resolved(maximum));

    let mut source = directory_target(1, 116);
    source.transform = i64::from(maximum) + 1;
    let graph = build(&[source], &ctx).unwrap();
    let edge = graph[&1]
        .iter()
        .find(|edge| edge.origin == ReferenceOrigin::Directory(ReferenceKind::Transform))
        .unwrap();
    assert_eq!(edge.resolution, Resolution::OutOfRange);
    assert!(edge.resolution.target_sequence().is_none());
}

#[test]
fn directory_reference_edge_refuses_collection_limit_before_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut source = directory_target(1, 116);
    source.transform = 3;
    let directory = [source];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = build(&directory, &ctx);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 1
                && limit.additional == 1
                && limit.operation == "iges directory reference edges"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(build(&directory, &ctx).is_ok());
}

#[test]
fn transform_cycle_detection_does_not_rewalk_a_long_acyclic_prefix() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let chain_length = 100_000_u32;
    let edges = (1..=chain_length)
        .map(|source| {
            let target = source + 1;
            (
                source,
                vec![ReferenceEdge {
                    origin: ReferenceOrigin::Directory(ReferenceKind::Transform),
                    raw_pointer: i64::from(target),
                    resolution: Resolution::Resolved(target),
                    expected: ReferenceExpectation::Type {
                        entity_type: 124,
                        forms: vec![],
                    },
                }],
            )
        })
        .collect::<BTreeMap<_, _>>();

    assert!(cyclic_transform_nodes(&edges, &ctx).unwrap().is_empty());
}

#[test]
fn inspect_preserves_transform_cycles_as_named_reference_states() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["124", "1", "0", "0", "0", "0", "3", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "1"],
        2,
    ));
    bytes.extend(directory_card(
        ["124", "2", "0", "0", "0", "0", "1", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "2"],
        4,
    ));
    let matrix = b"124,1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.;";
    bytes.extend(parameter_card(matrix, 1, 1));
    bytes.extend(parameter_card(matrix, 3, 2));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.as_slice()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"references.cyclic=2".into()));

    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cycle_losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::PointerUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(cycle_losses.len(), 2);
    assert_eq!(
        cycle_losses
            .iter()
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>(),
        [
            "IGES Directory Entry D1 Transform pointer 3 has cyclic resolution; expected type-124",
            "IGES Directory Entry D3 Transform pointer 1 has cyclic resolution; expected type-124",
        ],
    );
}

#[test]
fn zero_pointer_absence_creates_no_reference_edge() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let directory = [directory_target(1, 116)];
    let mut graph = build(&directory, &ctx).unwrap();
    assert!(graph[&1].is_empty());
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    let expectation = ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry);
    assert_eq!(
        resolver
            .resolve(1, 1, 0, expectation.clone(), |_| panic!("absent target"))
            .unwrap(),
        None
    );
    assert_eq!(
        resolver
            .resolve_negative(1, 2, 0, expectation, |_| panic!("absent target"))
            .unwrap(),
        None
    );
    resolver.append_to(&mut graph).unwrap();
    assert!(graph[&1].is_empty());
    assert!(super::summary_notes(&graph, &ctx).unwrap().is_empty());
}

#[test]
fn reference_target_id_streams_once_with_native_retained_limit() {
    use cadmpeg_test_support::native_serialization::assert_native_limit;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Row<'a> {
        id: &'static str,
        edge: &'a super::ReferenceEdge,
    }

    let edge = super::ReferenceEdge {
        origin: super::ReferenceOrigin::Directory(super::ReferenceKind::Transform),
        raw_pointer: 1,
        resolution: super::Resolution::Resolved(1),
        expected: ReferenceExpectation::Named(ExpectationLabel::Type124Transformation),
    };
    let record = Row {
        id: "iges:reference:edge#1",
        edge: &edge,
    };
    assert_native_limit(
        &record,
        serde_json::json!({
            "id": "iges:reference:edge#1",
            "edge": {
                "kind": "transform", "raw_pointer": 1,
                "target": "iges:entity:directory#1", "resolution": "resolved",
                "expected": "type-124-transformation"
            }
        }),
    );
}
