use super::native_feature_kind;
use cadmpeg_core::decode::{ResourceDimension, ResourceFailure};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::NativeFeatureKind;

#[test]
fn native_feature_kind_releases_canonical_tag_storage() {
    for (text, expected) in [
        ("Canvas", NativeFeatureKind::Canvas),
        ("Decal", NativeFeatureKind::Decal),
        ("Draft", NativeFeatureKind::Draft),
        ("Fillet", NativeFeatureKind::Fillet),
        ("Chamfer", NativeFeatureKind::Chamfer),
        ("Extrude", NativeFeatureKind::Extrude),
        ("DeleteFace", NativeFeatureKind::DeleteFace),
        ("SurfaceDeleteFace", NativeFeatureKind::SurfaceDeleteFace),
    ] {
        let bytes = cadmpeg_core::decode::u64_from_index(text.len());
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_materialized_bytes = bytes;
            },
            |ctx| {
                let kind = native_feature_kind(ctx, text).unwrap();
                assert_eq!(kind, expected);
                assert_eq!(serde_json::to_string(&kind).unwrap(), serde_json::to_string(text).unwrap());
                let storage = ctx.reserve_scoped(bytes, "canonical tag storage released").unwrap();
                drop(storage);
            },
        );
    }
}

#[test]
fn native_feature_kind_retains_unknown_tag_bytes_once() {
    let text = "CUSTOM μ";
    let bytes = cadmpeg_core::decode::u64_from_index(text.len());
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = bytes,
        |ctx| {
            let kind = native_feature_kind(ctx, text).unwrap();
            assert_eq!(kind, NativeFeatureKind::Other(text.to_owned()));
            assert_eq!(serde_json::to_string(&kind).unwrap(), serde_json::to_string(text).unwrap());
            let CodecError::ResourceLimit(limit) = ctx.charge_retained(1, "retained tag probe").unwrap_err() else {
                panic!("the retained tag occupies the byte limit");
            };
            // Retained bytes count the unknown tag's UTF-8 payload.
            assert_eq!(limit.used, bytes);
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        },
    );
}

fn native_feature_kind_refusal(dimension: ResourceDimension) {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("tag copies use work, scoped bytes and retained bytes"),
        },
        |ctx| {
            let CodecError::ResourceLimit(limit) = native_feature_kind(ctx, "BLEND").unwrap_err() else {
                panic!("tag copies must propagate resource refusals");
            };
            assert_eq!(limit.operation, "NX native feature kind");
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.reason, ResourceFailure::BudgetExceeded);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        },
    );
}

#[test]
fn native_feature_kind_copy_refuses_work() {
    native_feature_kind_refusal(ResourceDimension::WorkUnits);
}

#[test]
fn native_feature_kind_copy_refuses_scoped_bytes() {
    native_feature_kind_refusal(ResourceDimension::MaterializedBytes);
}

#[test]
fn native_feature_kind_copy_refuses_retained_bytes() {
    native_feature_kind_refusal(ResourceDimension::RetainedBytes);
}
