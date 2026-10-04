// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::report::check::Check;
use crate::validate::validate_neutral;

#[test]
fn ids_are_globally_unique_across_arenas() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.points[0].id = crate::ids::PointId::mint(ir.model.vertices[0].id.as_str()).unwrap();
    assert!(validate_neutral(&ir, Vec::new())
        .expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::Identity));
}

#[test]
fn arena_ids_must_be_sorted() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.points.swap(0, 1);
    assert!(validate_neutral(&ir, Vec::new())
        .expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::ArenaOrder));
}

#[test]
fn identity_validation_borrows_long_identities_and_releases_group_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let identity = format!("test:validation:point#{}", "p".repeat(1024));
    let mut ir = crate::CadIr::empty();
    ir.model.points.push(crate::topology::Point::new(
        identity.as_str().try_into().unwrap(),
        crate::features::FinitePoint3::new(crate::math::Point3::new(0.0, 0.0, 0.0)).unwrap(),
        None,
    ));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check_identity_and_order(
        &ctx,
        crate::native::view::NativeView::new(&ir, None),
        &mut findings,
    )
    .unwrap();
    assert!(findings.is_empty());
    let reservation = ctx.reserve_scoped(4096, "identity scope released").unwrap();
    drop(reservation);
    ctx.finish_session().unwrap();
}

#[test]
fn identity_validation_preserves_original_refusals_before_scans_and_findings() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut ir = unit_cube().unwrap();
    ir.model.points.push(ir.model.points[0].clone());
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::CollectionItems,
        ResourceDimension::RetainedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = super::check_identity_and_order(
            &ctx,
            crate::native::view::NativeView::new(&ir, None),
            &mut findings,
        ) else {
            panic!("identity validation must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}

#[test]
fn native_order_groups_preserve_combined_name_and_finding_order() {
    let mut ir = crate::CadIr::empty();
    for (format, arena, ids) in [
        ("a", "z", ["test:native:record#2", "test:native:record#1"]),
        ("a.b", "a", ["test:native:record#4", "test:native:record#3"]),
        ("a", "b.a", ["test:native:record#6", "test:native:record#5"]),
    ] {
        for id in ids {
            ir.native
                .namespace_mut(format)
                .arenas_mut()
                .entry(arena.into())
                .or_default()
                .push(
                    crate::native::NativeRecord::new(
                        id.try_into().unwrap(),
                        serde_json::Map::new(),
                    )
                    .unwrap(),
                );
        }
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_identity_and_order(
        &ctx,
        crate::native::view::NativeView::new(&ir, None),
        &mut findings,
    )
    .unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| (
                finding.check,
                finding.entity.as_deref(),
                finding.message.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                Check::ArenaOrder,
                Some("test:native:record#5"),
                "arena `native.a.b.a` is not strictly sorted by id"
            ),
            (
                Check::ArenaOrder,
                Some("test:native:record#1"),
                "arena `native.a.z` is not strictly sorted by id"
            ),
        ]
    );
    ctx.finish_session().unwrap();
}
