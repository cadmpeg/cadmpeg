// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use cadmpeg_core::decode::ResourceDimension;

use super::{retain_live_annotations, unknown_stream_metadata};

fn geometry_route_limit_error(
    adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::topology_partition_stream(),
    );

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |scan_ctx| {
            let scan_root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(scan_ctx, scan_root).expect("valid topology container");
            let (dialects, _) = crate::dialect::classify_layers(scan_ctx, &scan)
                .expect("classified topology input")
                .into_report_parts();

            crate::test_support::with_decode_context_over(&bytes, adjust, |ctx| {
                match super::try_decode_geometry(ctx, &scan, &dialects, &[], &[], &mut 0) {
                    Err(error) => error,
                    Ok(_) => panic!("geometry route must refuse the low limit"),
                }
            })
        },
    )
}

#[test]
fn geometry_route_refuses_collection_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_collection_items = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn geometry_route_refuses_retained_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn geometry_route_refuses_scoped_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_materialized_bytes = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn geometry_route_refuses_work_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_work_units = 0;
    };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
    ));
}

fn preview_stream() -> crate::parasolid::Stream {
    crate::parasolid::Stream {
        file_offset: 0,
        consumed: 0,
        inflated: Vec::new(),
        body: crate::parasolid::StreamBody::Preview,
    }
}

fn preview_unknown() -> cadmpeg_ir::unknown::UnknownRecord {
    crate::test_support::with_decode_context(|ctx| {
        unknown_stream_metadata(ctx, 0, &preview_stream())
            .expect("preview metadata fits the service profile")
    })
}

#[test]
fn live_annotations_refuse_first_identity_at_collection_limit() {
    let unknown = preview_unknown();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = retain_live_annotations(
                ctx,
                &cadmpeg_ir::document::CadIr::empty(),
                &[unknown],
                &mut cadmpeg_ir::Annotations::default(),
            )
            .expect_err("one live identity needs one slot");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "nx live annotation identities"
            ));
        },
    );
}

#[test]
fn live_annotation_lookup_scales_with_tree_depth() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = crate::test_support::with_decode_context(|ctx| {
        let mut builder = cadmpeg_ir::annotations::AnnotationBuilder::new();
        for index in 0..4000 {
            let id = cadmpeg_ir::ids::PointId::mint(format!("nx:model:point#{index}"))
                .expect("identity");
            builder
                .exactness(ctx, &id, cadmpeg_ir::Exactness::Derived)
                .expect("annotation");
            ir.model.points.push(cadmpeg_ir::topology::Point::new(
                id,
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0., 0., 0.))
                    .expect("position"),
                None,
            ));
        }
        builder
            .exactness(ctx, "nx:model:point#absent", cadmpeg_ir::Exactness::Derived)
            .expect("absent selector");
        builder.build()
    });
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 30_000_000,
        |ctx| {
            retain_live_annotations(ctx, &ir, &[], &mut annotations)
                .expect("indexed lookups fit budget");
        },
    );
    assert_eq!(annotations.exactness().len(), 4000);
    assert!(!annotations
        .exactness()
        .contains_key("nx:model:point#absent"));
}

#[test]
fn annotation_identity_retention_walks_ordered_keys_once() {
    let names: Vec<_> = (0..10_000).map(|i| format!("id{i:05}")).collect();
    let ids: std::collections::BTreeSet<_> = names.iter().map(String::as_str).collect();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 200_000,
        |ctx| {
            let mut cursor = ids.iter().peekable();
            for id in &names {
                assert!(super::ordered_live_identity_contains(ctx, &mut cursor, id).unwrap());
            }
            assert!(!super::ordered_live_identity_contains(ctx, &mut cursor, "id99999").unwrap());
            assert!(ctx.resource_refusal().is_none());
        },
    );
}

#[test]
fn annotation_identity_cursor_preserves_sorted_keep_decisions() {
    let ids: std::collections::BTreeSet<_> = ["b", "d", "f"].into_iter().collect();
    crate::test_support::with_decode_context(|ctx| {
        let mut cursor = ids.iter().peekable();
        for (id, expected) in [
            ("a", false),
            ("b", true),
            ("b", true),
            ("c", false),
            ("d", true),
            ("e", false),
            ("f", true),
            ("g", false),
        ] {
            assert_eq!(
                super::ordered_live_identity_contains(ctx, &mut cursor, id).unwrap(),
                expected,
                "{id}"
            );
        }
    });
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let mut cursor = ids.iter().peekable();
            let error = super::ordered_live_identity_contains(ctx, &mut cursor, "b").unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "nx annotation identity lookup" && ctx.resource_refusal() == Some(limit))
            );
        },
    );
}
