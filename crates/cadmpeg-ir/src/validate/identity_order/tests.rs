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

#[test]
fn duplicate_identity_findings_keep_model_then_native_visit_order() {
    let mut ir = crate::CadIr::empty();
    for id in [
        "test:validation:point#a",
        "test:validation:point#a",
        "test:validation:point#b",
    ] {
        ir.model.points.push(crate::topology::Point::new(
            id.try_into().unwrap(),
            crate::features::FinitePoint3::new(crate::math::Point3::new(0.0, 0.0, 0.0)).unwrap(),
            None,
        ));
    }
    for id in ["test:validation:point#b", "test:validation:point#a"] {
        ir.native
            .namespace_mut("test")
            .arenas_mut()
            .entry("records".into())
            .or_default()
            .push(
                crate::native::NativeRecord::new(id.try_into().unwrap(), serde_json::Map::new())
                    .unwrap(),
            );
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_identity_and_order(
        crate::native::view::NativeView::new(&ir, None),
        &mut findings,
    )
    .unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| (finding.check, finding.entity.as_deref()))
            .collect::<Vec<_>>(),
        [
            (Check::ArenaOrder, Some("test:validation:point#a")),
            (Check::Identity, Some("test:validation:point#a")),
            (Check::Identity, Some("test:validation:point#b")),
            (Check::Identity, Some("test:validation:point#a")),
            (Check::ArenaOrder, Some("test:validation:point#a")),
        ]
    );
    ctx.finish_session().unwrap();
}
