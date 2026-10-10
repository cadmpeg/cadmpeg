// SPDX-License-Identifier: Apache-2.0
//! Fixed-width list validation and material cache storage.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

fn validate_cases(
    parse: impl Fn(&DecodeContext<'_>, View<'_>, &str) -> Result<(), CodecError>,
    label: &str,
    record_size: usize,
    scalar_offset: usize,
    scalar_name: &str,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let mut bytes = 64_u32.to_le_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(0_u8, 64 * record_size));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    parse(&ctx, View::over_retained(&bytes), "values")
        .expect("validation retains no values or scratch collection");

    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    let error = parse(&ctx, View::over_retained(&bytes), "values")
        .expect_err("record work must be admitted");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
    );

    for (count, nonfinite, trailing, expected) in [
        (2_u32, true, false, "count exceeds its payload".to_owned()),
        (1, true, true, format!("has a non-finite {scalar_name}")),
        (1, false, true, "has trailing bytes".to_owned()),
    ] {
        let mut bytes = count.to_le_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(0_u8, record_size));
        if nonfinite {
            bytes[4 + scalar_offset..12 + scalar_offset].copy_from_slice(&f64::NAN.to_le_bytes());
        }
        if trailing {
            bytes.push(0);
        }
        crate::test_support::with_service_context(&[], |ctx| {
            let error =
                parse(ctx, View::over_retained(&bytes), "values").expect_err("invalid list");
            assert!(matches!(error, CodecError::Malformed(message)
                if message == format!("{label} entry values {expected}")));
        });
    }
}

#[test]
fn float_list_streaming_preserves_validation_and_error_order() {
    validate_cases(super::super::parse_float_list, "float-list", 8, 0, "value");
}

#[test]
fn vector_list_streaming_preserves_validation_and_error_order() {
    validate_cases(
        super::super::parse_vector_list,
        "vector-list",
        24,
        0,
        "value",
    );
}

#[test]
fn placement_list_streaming_preserves_validation_and_error_order() {
    validate_cases(
        super::super::parse_placement_list,
        "placement-list",
        56,
        0,
        "value",
    );
}

#[test]
fn fillet_list_streaming_preserves_validation_and_error_order() {
    validate_cases(
        super::super::parse_fillet_edges,
        "fillet-edges",
        20,
        4,
        "radius",
    );
}

#[test]
fn material_list_builds_one_cache_and_reads_version_three_strings() {
    for version in 0..=3 {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        for color in [0x1122_3300_u32, 0x4455_6640, 0x7788_9980, 0xaabb_ccff] {
            bytes.extend(color.to_le_bytes());
        }
        bytes.extend(0.5_f32.to_le_bytes());
        bytes.extend(0.25_f32.to_le_bytes());
        if version == 3 {
            for text in ["name", "description", "material-id"] {
                bytes.extend(
                    u32::try_from(text.len())
                        .expect("text length")
                        .to_le_bytes(),
                );
                bytes.extend(text.as_bytes());
            }
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
        let (materials, _storage) = ctx
            .with_scoped_storage("material cache", || {
                super::super::parse_material_list(
                    &ctx,
                    View::over_retained(&bytes),
                    version,
                    "material",
                    true,
                )
            })
            .expect("material cache");
        assert_eq!(materials.len(), 1);
        let material = &materials[0];
        assert_eq!(material.ambient, 0x1122_33ff);
        assert_eq!(material.diffuse, 0x4455_66bf);
        assert_eq!(material.specular, 0x7788_997f);
        assert_eq!(material.emissive, 0xaabb_cc00);
        assert_eq!(material.shininess.get(), 0.5);
        assert_eq!(material.transparency.get(), 0.25);
        assert_eq!(material.uuid, if version == 3 { "material-id" } else { "" });
    }
}

#[test]
fn material_list_refuses_real_cache_storage() {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend([0_u8; 24]);
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
    ] {
        let error =
            crate::test_support::refusal_at(dimension, &[], "FCStd GUI material entries", |ctx| {
                ctx.with_scoped_storage("material cache", || {
                    super::super::parse_material_list(
                        ctx,
                        View::over_retained(&bytes),
                        2,
                        "material",
                        false,
                    )
                })
                .map(|_| ())
            });
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }
}

#[test]
fn material_list_preserves_bounds_and_scalar_error_order() {
    for (count, nonfinite, trailing, expected) in [
        (2_u32, true, false, "count exceeds its payload"),
        (1, true, true, "has non-finite scalars"),
        (1, false, true, "has trailing bytes"),
    ] {
        let mut bytes = count.to_le_bytes().to_vec();
        bytes.extend([0_u8; 24]);
        if nonfinite {
            bytes[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
        }
        if trailing {
            bytes.push(0);
        }
        crate::test_support::with_service_context(&[], |ctx| {
            let error = match super::super::parse_material_list(
                ctx,
                View::over_retained(&bytes),
                2,
                "material",
                false,
            ) {
                Ok(_) => panic!("invalid material list must be refused"),
                Err(error) => error,
            };
            assert!(matches!(error, CodecError::Malformed(message)
                if message == format!("GUI material list material {expected}")));
        });
    }
}
