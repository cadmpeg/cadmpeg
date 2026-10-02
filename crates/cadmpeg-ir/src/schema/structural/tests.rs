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
fn structural_projection_preserves_nonfinite_values_and_admits_each_dimension() {
    let shape = Shape { text: "source text".into(), values: vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0], optional: Some(Box::new((true, 17))) };
    let expected = serde_value::to_value(&shape).unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let projected = project(&ctx, &shape, "project structural fixture").unwrap();
    assert_eq!(*projected, expected);
    drop(projected);
    ctx.finish_session().unwrap();
    for dimension in [ResourceDimension::CollectionItems, ResourceDimension::MaterializedBytes, ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("projection uses scoped dimensions"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project(&ctx, &shape, "project structural fixture").unwrap_err();
        let CodecError::ResourceLimit(limit) = error else { panic!("resource refusal must stay outside serde"); };
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn structural_projection_holds_and_releases_its_storage_reservation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 5;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = project(&ctx, "abcde", "project first text").unwrap();
    assert_eq!(*first, serde_value::Value::String("abcde".into()));
    drop(first);
    let second = project(&ctx, "abcde", "project second text").unwrap();
    drop(second);
    ctx.finish_session().unwrap();

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = project(&ctx, "abcde", "project first text").unwrap();
    let error = project(&ctx, "x", "project second text").unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes));
    drop(first);
}

#[test]
fn structural_projection_counts_both_variant_containers() {
    #[derive(Serialize)]
    enum Shape { Tuple(), Struct {} }
    for shape in [Shape::Tuple(), Shape::Struct {}] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = project(&ctx, &shape, "project variant containers").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth && limit.used == 1 && limit.additional == 1));
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
fn structural_display_text_uses_only_scoped_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 5;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let projected = project(&ctx, &DisplayText, "project display text").unwrap();
    assert_eq!(*projected, serde_value::Value::String("abcde".into()));
    drop(projected);
    let storage = ctx.reserve_scoped(5, "display text released").unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn structural_display_text_preserves_formatting_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = project(&ctx, &DisplayText, "project display text").unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("display formatting must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "project display text");
    assert_eq!(limit.additional, 5);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
}
