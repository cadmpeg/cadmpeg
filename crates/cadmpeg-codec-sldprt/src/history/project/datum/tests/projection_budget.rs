// SPDX-License-Identifier: Apache-2.0
//! Composite-curve work boundaries and segment projection.

use crate::history::project::budget_tests::{limited, property};
use crate::history::tests::feature;
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

