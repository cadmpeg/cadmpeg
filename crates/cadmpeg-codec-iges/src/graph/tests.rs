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
    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges reference summary notes",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::summary_notes(&graph, &ctx)
        },
    );
    assert!(matches!(
        result,
        cadmpeg_core::CodecError::ResourceLimit(limit)
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
fn parameter_resolver_edges_refuse_each_collection_limit_before_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let directory = [directory_target(1, 116)];
    for operation in [
        "iges parameter resolver edge groups",
        "iges parameter resolver edges",
        "iges parameter resolver graph groups",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let resolver = ParameterResolver::new(&directory, &ctx)?;
                assert_eq!(
                    resolver.resolve(
                        1,
                        0,
                        3,
                        ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                        |_| true
                    )?,
                    None
                );
                resolver.append_to(&mut BTreeMap::new()).map(|_| ())
            },
        );
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
    let _storage = resolver.append_to(&mut graph).unwrap();
    assert_eq!(graph[&1].len(), 1);
}

#[test]
fn parameter_resolver_expected_forms_refuse_collection_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let directory = [directory_target(1, 116)];
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges parameter resolver expected forms",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let resolver = ParameterResolver::new(&directory, &ctx)?;
            resolver.resolve_type(1, 0, 1, 116, &[0])
        },
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(resolver.resolve_type(1, 0, 1, 116, &[0]).unwrap(), Some(1));
}

#[test]
fn parameter_resolver_append_refuses_nonempty_graph_edge_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut source = directory_target(1, 116);
    source.transform = 3;
    let directory = [source, directory_target(3, 124)];
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges appended parameter reference edges",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let (mut graph, _directory_storage) = build(&directory, &ctx)?;
            let resolver = ParameterResolver::new(&directory, &ctx)?;
            assert_eq!(resolver.resolve_any(1, 0, 3)?, Some(3));
            let result = resolver.append_to(&mut graph).map(|_| ());
            assert_eq!(graph[&1].len(), 1);
            result
        },
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let (mut graph, _directory_storage) = build(&directory, &ctx).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    assert_eq!(resolver.resolve_any(1, 0, 3).unwrap(), Some(3));
    let _storage = resolver.append_to(&mut graph).unwrap();
    assert_eq!(graph[&1].len(), 2);
    assert_eq!(
        graph[&1][0].origin,
        ReferenceOrigin::Directory(ReferenceKind::Transform)
    );
    assert_eq!(graph[&1][1].origin, ReferenceOrigin::Parameter { index: 0 });
}

#[test]
fn parameter_resolver_expected_types_refuse_collection_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let directory = [directory_target(1, 116)];
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges parameter resolver expected types",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let resolver = ParameterResolver::new(&directory, &ctx)?;
            resolver.resolve_any_of(1, 0, 1, (212, 312, &[402]), |_| false)
        },
    );

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
    let _storage = resolver.append_to(&mut graph).unwrap();
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
    let _storage = resolver.append_to(&mut graph).unwrap();
    let source = point_file();
    let scan = crate::test_support::scan(&source).unwrap();
    let messages = super::losses(&graph, &scan, &[], &ctx)
        .unwrap()
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
fn graph_losses_admit_indexes_notes_and_provenance_text() {
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
    let source = point_file();
    let scan = crate::test_support::scan(&source).unwrap();
    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges graph loss sources",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::losses(&graph, &scan, &[], &ctx)
        },
    );
    assert!(matches!(
        result,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges graph loss sources"
    ));

    {
        let (cap, operation) = (0, "iges graph loss notes");
        let result = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            operation,
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                super::losses(&graph, &scan, &[], &ctx)
            },
        );
        assert!(matches!(
            result,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == cap
                    && limit.additional == 1
                    && limit.operation == operation
        ));
    }

    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "iges graph loss tag",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::losses(&graph, &scan, &[], &ctx)
        },
    );
    assert!(matches!(
        result,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<cadmpeg_ir::report::loss::LossNote>())
                && limit.additional == 2
                && limit.operation == "iges graph loss tag"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let losses = super::losses(&graph, &scan, &[], &ctx).unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].provenance.as_ref().unwrap().tag.as_deref(),
        Some("D1")
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
    let (graph, _storage) = build(&[source, directory_target(maximum, 124)], &ctx).unwrap();
    let edge = graph[&1]
        .iter()
        .find(|edge| edge.origin == ReferenceOrigin::Directory(ReferenceKind::Transform))
        .unwrap();
    assert_eq!(edge.resolution, Resolution::Resolved(maximum));

    let mut source = directory_target(1, 116);
    source.transform = i64::from(maximum) + 1;
    let (graph, _storage) = build(&[source], &ctx).unwrap();
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
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges directory reference edges",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            build(&directory, &ctx).map(|_| ())
        },
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(build(&directory, &ctx).is_ok());
}

#[test]
fn transform_cycle_detection_does_not_rewalk_a_long_acyclic_prefix() {
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

    // The unchanged graph supplies the input-dependent envelope. Three ordered
    // indices can each charge one split path for every graph node.
    let input = serde_json::to_vec(&edges).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // The graph traversal assertion admits all complete-key and node-movement work.
    policy.limits.max_work_units = u64::MAX;
    let node_bytes = 11 * (std::mem::size_of::<u32>() + std::mem::size_of::<usize>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<usize>();
    let path_nodes = usize::try_from(chain_length.ilog2()).unwrap() + 2;
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
        3 * edges.len() * path_nodes * node_bytes + 2 * edges.len() * std::mem::size_of::<u32>(),
    );
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&input, &arena, &policy).unwrap();

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
    let (mut graph, _directory_storage) = build(&directory, &ctx).unwrap();
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
    let _storage = resolver.append_to(&mut graph).unwrap();
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

#[test]
fn native_reference_copy_refuses_nested_forms_and_types() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for (expected, operation) in [
        (
            ReferenceExpectation::Type {
                entity_type: 406,
                forms: vec![1],
            },
            "iges native reference forms",
        ),
        (
            ReferenceExpectation::AnyOf {
                first: 212,
                second: 312,
                rest: vec![402],
            },
            "iges native reference types",
        ),
    ] {
        let edge = ReferenceEdge {
            origin: ReferenceOrigin::Directory(ReferenceKind::Structure),
            raw_pointer: 3,
            resolution: Resolution::Resolved(3),
            expected,
        };
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let mut storage = ctx.reserve_scoped(0, "native reference copy test")?;
                edge.copy_for_native(&ctx, &mut storage)
            },
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut storage = ctx.reserve_scoped(0, "native reference copy test").unwrap();
        assert_eq!(edge.copy_for_native(&ctx, &mut storage).unwrap(), edge);
    }
}

#[test]
fn graph_variable_traversals_refuse_at_their_own_boundaries() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let graph = BTreeMap::from([(
        1,
        vec![ReferenceEdge {
            origin: ReferenceOrigin::Directory(ReferenceKind::Transform),
            raw_pointer: 1,
            resolution: Resolution::Resolved(1),
            expected: ReferenceExpectation::Named(ExpectationLabel::Type124Transformation),
        }],
    )]);
    for operation in [
        "iges reference summary sources",
        "iges reference summary edges",
        "iges transform cycle sources",
        "iges transform successor search",
        "iges transform cycle starts",
        "iges transform reference cycle walk",
        "iges cyclic transform nodes",
        "iges completed transform path",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                if operation.starts_with("iges reference summary") {
                    super::summary_notes(&graph, &ctx).map(|_| ())
                } else {
                    cyclic_transform_nodes(&graph, &ctx).map(|_| ())
                }
            },
        );
    }
}

#[test]
fn transform_cycle_tree_lookups_refuse_before_searching() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let graph = [1, 3].into_iter().map(|source| (source, vec![ReferenceEdge {
        origin: ReferenceOrigin::Directory(ReferenceKind::Transform),
        raw_pointer: i64::from(source),
        resolution: Resolution::Resolved(source),
        expected: ReferenceExpectation::Named(ExpectationLabel::Type124Transformation),
    }])).collect::<BTreeMap<_, _>>();
    for operation in [
        "iges completed transform lookup",
        "iges active transform lookup",
        "iges transform successor lookup",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits, operation, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                cyclic_transform_nodes(&graph, &ctx)
            },
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(cyclic_transform_nodes(&graph, &ctx).unwrap(), [1, 3].into_iter().collect());
}

#[test]
fn directory_graph_source_lookups_refuse_before_searching() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut source = directory_target(1, 124);
    source.transform = 1;
    let directory = [source];
    for operation in ["iges cyclic transform source lookup", "iges structure reference source lookup"] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits, operation, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let (graph, _storage) = build(&directory, &ctx)?;
                super::resolved_structure_sequence(&graph, 1, &ctx)
            },
        );
    }
}

#[test]
fn structure_reference_search_stops_before_unvisited_parameter_edges() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut edges = vec![ReferenceEdge {
        origin: ReferenceOrigin::Directory(ReferenceKind::Structure),
        raw_pointer: 3,
        resolution: Resolution::Resolved(3),
        expected: ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
    }];
    edges.extend((0..10_000).map(|index| ReferenceEdge {
        origin: ReferenceOrigin::Parameter { index },
        raw_pointer: 3,
        resolution: Resolution::Resolved(3),
        expected: ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
    }));
    let graph = BTreeMap::from([(1, edges)]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges structure reference search",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            super::resolved_structure_sequence(&graph, 1, &ctx)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("structure search must refuse");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used + limit.additional;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::resolved_structure_sequence(&graph, 1, &ctx).unwrap(),
        Some(3)
    );
}

#[test]
fn reference_graph_storage_is_scoped_until_the_graph_is_dropped() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut source = directory_target(1, 116);
    source.level = -3;
    let mut target = directory_target(3, 406);
    target.form = 1;
    let directory = [source, target];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges directory reference edges",
        |cap| {
            let mut limited = policy;
            limited.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &limited)?;
            build(&directory, &ctx).map(|_| ())
        },
    );
    for refuse_while_live in [true, false] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (graph, storage) = build(&directory, &ctx).unwrap();
        assert_eq!(graph[&1][0].resolution, Resolution::Resolved(3));
        if refuse_while_live {
            assert!(ctx
                .reserve_scoped(policy.limits.max_materialized_bytes, "live reference graph")
                .is_err());
        } else {
            drop(graph);
            drop(storage);
            assert!(ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "released reference graph"
                )
                .is_ok());
        }
    }
}

#[test]
fn parameter_reference_storage_stays_live_after_append() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let directory = [directory_target(1, 116), directory_target(3, 116)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    for refuse_while_live in [true, false] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut graph = BTreeMap::from([(1, Vec::new())]);
        let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
        assert_eq!(resolver.resolve_type(1, 0, 3, 116, &[0]).unwrap(), Some(3));
        let storage = resolver.append_to(&mut graph).unwrap();
        assert_eq!(graph[&1].len(), 1);
        assert_eq!(graph[&1][0].resolution, Resolution::Resolved(3));
        if refuse_while_live {
            assert!(ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "live parameter references"
                )
                .is_err());
        } else {
            drop(graph);
            drop(storage);
            assert!(ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "released parameter references"
                )
                .is_ok());
        }
    }
}

#[test]
fn parameter_expected_forms_refuse_at_the_predicate_scan() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let directory = [directory_target(1, 116), directory_target(3, 116)];
    for negative in [false, true] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "iges parameter expected form search",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let resolver = ParameterResolver::new(&directory, &ctx)?;
                if negative {
                    resolver.resolve_negative_type(1, 0, -3, 116, &[0])
                } else {
                    resolver.resolve_type(1, 0, 3, 116, &[0])
                }
            },
        );
    }
}
