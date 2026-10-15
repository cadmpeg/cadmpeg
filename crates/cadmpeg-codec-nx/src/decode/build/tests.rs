// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use super::retain_live_annotations;

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
    crate::test_support::with_decode_context(|ctx| {
        retain_live_annotations(ctx, &ir, &[], &mut annotations)
            .expect("live annotations are retained");
    });
    assert_eq!(annotations.exactness().len(), 4000);
    assert!(!annotations
        .exactness()
        .contains_key("nx:model:point#absent"));
}

#[test]
fn annotation_identity_retention_walks_ordered_keys_once() {
    let names: Vec<_> = (0..10_000).map(|i| format!("id{i:05}")).collect();
    let ids: std::collections::BTreeSet<_> = names.iter().map(String::as_str).collect();
    let mut cursor = ids.iter().peekable();
    for id in &names {
        assert!(super::ordered_live_identity_contains(&mut cursor, id));
    }
    assert!(!super::ordered_live_identity_contains(
        &mut cursor,
        "id99999"
    ));
}

#[test]
fn annotation_identity_cursor_preserves_sorted_keep_decisions() {
    let ids: std::collections::BTreeSet<_> = ["b", "d", "f"].into_iter().collect();
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
            super::ordered_live_identity_contains(&mut cursor, id),
            expected,
            "{id}"
        );
    }
}
