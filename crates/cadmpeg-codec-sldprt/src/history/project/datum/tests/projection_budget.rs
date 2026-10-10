// SPDX-License-Identifier: Apache-2.0
//! Curve work boundaries and segment projection.

use crate::history::project::budget_tests::{limited, property};
use crate::history::tests::feature;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use std::collections::HashMap;

#[test]
fn invalid_composite_closed_flag_stops_at_first_nonblank_character() {
    let mut source = feature("composite", None, 0);
    property(
        &mut source,
        "Segments",
        format!("; \u{2003};{};second", "x".repeat(100_000)),
    );
    property(&mut source, "Closed", "invalid".into());
    assert!(
        limited(|ctx| super::super::project_composite_curve(ctx, &source, &HashMap::new()))
            .unwrap()
            .is_none()
    );
    crate::test_support::work_refusal_at("project SLDPRT composite curve segments", |ctx| {
        super::super::project_composite_curve(ctx, &source, &HashMap::new())
    });
}

#[test]
fn composite_segments_preserve_unicode_trimming_and_source_order() {
    let mut source = feature("composite", None, 0);
    property(
        &mut source,
        "Segments",
        "; \u{2003};first\u{2003} ; second ;".into(),
    );
    property(&mut source, "Closed", "true".into());
    let definition = super::super::project_composite_curve(
        &cadmpeg_test_support::service_decode_context(),
        &source,
        &HashMap::from([("first", "native-first")]),
    )
    .unwrap();
    let Some(FeatureDefinition::Operation(FeatureOperation::CompositeCurve { segments, closed })) =
        definition
    else {
        panic!("expected composite curve");
    };
    assert!(closed);
    assert_eq!(
        segments.as_slice(),
        [
            cadmpeg_ir::features::PathRef::Native("native-first".into()),
            cadmpeg_ir::features::PathRef::Native("second".into())
        ]
    );
}

#[test]
fn empty_composite_segments_do_not_read_closed_flag() {
    let mut source = feature("composite", None, 0);
    property(&mut source, "Segments", "; \u{2003}; \t;".into());
    property(&mut source, "Closed", "x".repeat(100_000));
    assert!(
        limited(|ctx| super::super::project_composite_curve(ctx, &source, &HashMap::new()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn rejected_equation_domain_does_not_read_or_copy_expressions() {
    let mut source = feature("equation", None, 0);
    source
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Start"), "NaN".into());
    source
        .properties
        .insert(cadmpeg_core::nonblank_literal!("End"), "1".into());
    for key in ["Parameter", "XEquation", "YEquation", "ZEquation"] {
        source
            .properties
            .insert(key.try_into().unwrap(), "x".repeat(100_000));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1_000;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::project_equation_curve(&ctx, &source)
        .unwrap()
        .is_none());
    ctx.finish_session().unwrap();
}
