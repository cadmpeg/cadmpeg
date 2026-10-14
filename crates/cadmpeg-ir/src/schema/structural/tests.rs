// SPDX-License-Identifier: Apache-2.0
use super::project;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use serde::Serialize;

#[derive(Serialize)]
struct Shape {
    text: String,
    values: Vec<f64>,
    optional: Option<Box<(bool, u64)>>,
}

#[test]
fn structural_projection_counts_both_variant_containers() {
    #[derive(Serialize)]
    enum Shape {
        Tuple(),
        Struct {},
    }
    for shape in [Shape::Tuple(), Shape::Struct {}] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project(&ctx, &shape, "project variant containers").unwrap_err();
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth && limit.used == 1 && limit.additional == 1)
        );
    }
}

struct DisplayText;

impl std::fmt::Display for DisplayText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("abcde")
    }
}

impl Serialize for DisplayText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[test]
fn structural_projection_preserves_nonfinite_values_and_refuses_at_the_depth_limit() {
    let shape = Shape {
        text: "source text".into(),
        values: vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0],
        optional: Some(Box::new((true, 17))),
    };
    let expected = serde_value::to_value(&shape).unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let projected = project(&ctx, &shape, "project structural fixture").unwrap();
    assert_eq!(*projected, expected);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project(&ctx, &shape, "project structural fixture").unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal must stay outside serde");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn structural_display_text_projects_its_formatted_text() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let projected = project(&ctx, &DisplayText, "project display text").unwrap();
    assert_eq!(*projected, serde_value::Value::String("abcde".into()));
}
