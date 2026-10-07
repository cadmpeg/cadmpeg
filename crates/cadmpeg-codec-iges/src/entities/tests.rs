// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::loss::IgesLossCode;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

#[test]
fn diagnostic_error_text_refuses_before_retained_copy() {
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, "iges diagnostic error text", |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::non_resource_error(CodecError::Malformed("invalid source".into()), &ctx)
    });
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::non_resource_error(CodecError::Malformed("invalid source".into()), &ctx).unwrap(),
        "malformed container: invalid source"
    );
}

fn assert_entity_loss_limit(bytes: &[u8], operation: &str, retained: bool) {
    let dimension = if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems };
    cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut policy = DecodePolicy::service();
        if retained { policy.limits.max_retained_bytes = cap; } else { policy.limits.max_collection_items = cap; }
        IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions { policy, ..DecodeOptions::default() })
            .map_err(|failure| match failure { DecodeFailure::Codec(error) => error, other => panic!("unexpected decode failure: {other:?}") })
    });
}

#[test]
fn entity_projection_losses_refuse_slots_and_messages() {
    let cases = [
        (110, 0, "110,0;", "endpoint coordinate"),
        (150, 0, "150,0;", "primitive dimensions"),
        (502, 1, "502,0;", "vertex-list count"),
        (308, 0, "308,0;", "subfigure"),
        (130, 0, "130,0;", "offset distance flag"),
        (108, 0, "108,0;", "plane coefficients"),
        (142, 0, "142,0;", "curve-on-surface"),
        (112, 0, "112,0;", "spline header"),
        (102, 0, "102,0;", "child count"),
        (212, 0, "212,0;", "text count"),
    ];
    for (entity_type, form, parameters, reason) in cases {
        let bytes = owned_test_file(&[OwnedTestEntity {
            entity_type,
            form,
            label: "INVALID".into(),
            status: "00000000",
            parameters: parameters.into(),
        }]);
        let service = IgesCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        assert!(
            service
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()
                    && loss.message.contains(reason)),
            "{entity_type}: {:#?}",
            service.report().losses
        );
        assert_entity_loss_limit(&bytes, "iges entity loss slots", false);
        assert_entity_loss_limit(&bytes, "iges entity loss message", true);
    }
}

#[test]
fn entity_projector_indexes_refuse_collection_limits() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 110,
        form: 0,
        label: "LINE".into(),
        status: "00000000",
        parameters: "110,0;".into(),
    }]);
    for name in [
        "csg",
        "structure",
        "offsets",
        "surfaces",
        "trimming",
        "splines",
        "composite",
    ] {
        for index in ["parameter index", "directory index"] {
            let operation = format!("iges {name} {index}");
            assert_entity_loss_limit(&bytes, &operation, false);
        }
    }
}

#[test]
fn affine_parameter_map_retains_finite_ratio_of_overflowing_span() {
    let (scale, offset) = crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [0.0, 1.0])
        .expect("finite affine map");
    assert!((scale * f64::MAX - 0.5).abs() < 8.0 * f64::EPSILON);
    assert!((offset - 0.5).abs() < 8.0 * f64::EPSILON);
}

#[test]
fn affine_parameter_map_retains_identity_between_overflowing_spans() {
    assert_eq!(
        crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [-f64::MAX, f64::MAX]),
        Some((1.0, 0.0))
    );
}

#[test]
fn directed_cycle_detection_handles_long_branching_graphs_iteratively() {
    {
        let mut graph = (1..=100_000_u32)
            .map(|sequence| (sequence, vec![sequence + 1]))
            .collect::<BTreeMap<_, _>>();
        graph.entry(50_000).or_default().push(100_001);
        // The unchanged graph supplies the input-dependent byte envelope for its node storage.
        let input = serde_json::to_vec(&graph).unwrap();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // The graph traversal assertion admits all complete-key and node-movement work.
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&input, &arena, &policy).unwrap();
        let decode_ctx = &ctx;
        let mut visited = std::collections::BTreeSet::new();

        assert!(
            !crate::entities::directed_cycle(1, &mut visited, decode_ctx, |sequence| graph
                .get(&sequence)
                .into_iter()
                .flatten()
                .copied())
            .unwrap()
        );
        assert_eq!(visited.len(), 100_001);

        graph.insert(100_001, vec![50_000]);
        assert!(crate::entities::directed_cycle(
            1,
            &mut std::collections::BTreeSet::new(),
            decode_ctx,
            |sequence| graph.get(&sequence).into_iter().flatten().copied()
        )
        .unwrap());
    }
}

#[test]
fn directed_cycle_refuses_stack_and_tree_nodes_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let graph = [(1_u32, vec![2_u32]), (2, Vec::new())]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    for operation in ["iges cycle stack", "iges cycle active", "iges cycle visited"] {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::CollectionItems, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            crate::entities::directed_cycle(1, &mut BTreeSet::new(), &ctx, |sequence| graph.get(&sequence).into_iter().flatten().copied())
        });
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut visited = BTreeSet::new();
    assert!(
        !crate::entities::directed_cycle(1, &mut visited, &ctx, |sequence| graph
            .get(&sequence)
            .into_iter()
            .flatten()
            .copied(),)
        .unwrap()
    );
    assert_eq!(visited, [1, 2].into());
}

#[test]
fn directed_cycle_work_refusal_reaches_caller() {
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "iges cycle work", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::directed_cycle(1, &mut BTreeSet::new(), &ctx, |sequence| (sequence == 1).then_some(2).into_iter())
    });
}
