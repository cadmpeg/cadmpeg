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
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result =
        super::non_resource_error(CodecError::Malformed("invalid source".into()), Some(&ctx));
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "iges diagnostic error text")
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::non_resource_error(CodecError::Malformed("invalid source".into()), Some(&ctx))
            .unwrap(),
        "malformed container: invalid source"
    );
}

fn assert_entity_loss_limit(bytes: &[u8], operation: &str, retained: bool) {
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        if retained {
            policy.limits.max_retained_bytes = cap;
        } else {
            policy.limits.max_collection_items = cap;
        }
        match IgesCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        ) {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(
                    limit.dimension,
                    if retained {
                        ResourceDimension::RetainedBytes
                    } else {
                        ResourceDimension::CollectionItems
                    }
                );
                if limit.operation == operation {
                    return;
                }
                let next = limit.used.checked_add(limit.additional).unwrap();
                assert!(next > cap, "limit did not advance from {cap}: {limit:?}");
                cap = next;
            }
            other => panic!("did not reach {operation} at cap {cap}: {other:?}"),
        }
    }
    panic!("did not reach {operation} within 4096 admission boundaries");
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
        "brep",
        "structure",
        "offsets",
        "surfaces",
        "trimming",
        "splines",
        "composite",
        "annotation",
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
    let mut graph = (1..=100_000_u32)
        .map(|sequence| (sequence, vec![sequence + 1]))
        .collect::<BTreeMap<_, _>>();
    graph.entry(50_000).or_default().push(100_001);
    let mut visited = std::collections::BTreeSet::new();

    assert!(
        !crate::entities::directed_cycle(1, &mut visited, None, |sequence| graph
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
        None,
        |sequence| graph.get(&sequence).into_iter().flatten().copied()
    )
    .unwrap());
}

#[test]
fn directed_cycle_refuses_stack_and_tree_nodes_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let graph = [(1_u32, vec![2_u32]), (2, Vec::new())]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    for (cap, operation) in [
        (0, "iges cycle stack"),
        (1, "iges cycle active"),
        (6, "iges cycle visited"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error =
            crate::entities::directed_cycle(1, &mut BTreeSet::new(), Some(&ctx), |sequence| {
                graph.get(&sequence).into_iter().flatten().copied()
            })
            .unwrap_err();
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut visited = BTreeSet::new();
    assert!(
        !crate::entities::directed_cycle(1, &mut visited, Some(&ctx), |sequence| graph
            .get(&sequence)
            .into_iter()
            .flatten()
            .copied(),)
        .unwrap()
    );
    assert_eq!(visited, [1, 2].into());
}
