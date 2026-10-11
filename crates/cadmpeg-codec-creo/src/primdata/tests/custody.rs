// SPDX-License-Identifier: Apache-2.0

use super::super::{
    scalar_arrays, triangle_strips, PrimitiveScalarArray, PrimitiveShadedVertex,
    PrimitiveTriangleStrip, PrimitiveTriangleStripScan, PrimitiveVertices,
};
use super::{finite_points, minimal_strip, named};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const CAP: u64 = 16 * 1024;

fn assert_live_output<T>(
    retained_cap: u64,
    call: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
    output_bytes: impl Fn(&T) -> u64,
) {
    for scoped in [false, true] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::RetainedBytes,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = CAP;
            policy.limits.max_retained_bytes = retained_cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let parts = if scoped {
                let parts = ctx
                    .with_scoped_storage("primitive custody parent", || call(&ctx))
                    .expect("temporary output");
                (parts.0, Some(parts.1))
            } else {
                (call(&ctx).expect("retained output"), None)
            };
            let storage = parts.1;
            let output = parts.0;
            let bytes = output_bytes(&output);
            let (refusal, used) = match dimension {
                ResourceDimension::MaterializedBytes => (
                    ctx.reserve_scoped_limit(CAP + 1, "after primitive custody")
                        .expect_err("probe live materialization"),
                    if scoped { bytes } else { 0 },
                ),
                ResourceDimension::RetainedBytes => (
                    ctx.charge_retained_limit(retained_cap + 1, "after primitive custody")
                        .expect_err("probe retained output"),
                    if scoped { 0 } else { bytes },
                ),
                _ => unreachable!("two storage dimensions"),
            };
            assert_eq!(refusal.dimension, dimension);
            assert_eq!(refusal.used, used);
            assert_eq!(ctx.resource_refusal(), Some(refusal));
            drop(output);
            drop(storage);
        }
    }
}

#[test]
fn rejected_primitive_scalar_candidates_release_their_value_buffers() {
    let bytes = named("pts", &[0x46, 0x80, 0, 0], 3);
    assert_live_output(
        0,
        |ctx| {
            for _ in 0..8 {
                assert!(scalar_arrays(ctx, &bytes)?.is_empty());
            }
            scalar_arrays(ctx, &bytes)
        },
        |arrays| {
            assert!(arrays.is_empty());
            0
        },
    );
}

#[test]
fn primitive_scalar_output_retains_only_its_record_and_value_backing() {
    let bytes = named("p1", &[0, 0, 0], 3);
    assert_live_output(
        CAP,
        |ctx| scalar_arrays(ctx, &bytes),
        |arrays| {
            let [array] = arrays.as_slice() else {
                panic!("one complete array");
            };
            assert_eq!(array.offset, 0);
            assert_eq!(array.field.as_str(), "p1");
            assert_eq!(
                array
                    .values
                    .iter()
                    .map(|value| value.get())
                    .collect::<Vec<_>>(),
                [0.0; 3]
            );
            u64::try_from(
                arrays.capacity() * std::mem::size_of::<PrimitiveScalarArray>()
                    + array.values.capacity()
                        * std::mem::size_of::<cadmpeg_ir::scalar::FiniteReal>(),
            )
            .expect("actual scalar output backing")
        },
    );
}

#[test]
fn rejected_primitive_strip_producers_leave_no_retained_or_live_scratch_bytes() {
    const PREFIX: &[u8] = b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8";
    let mut invalid_lengths = PREFIX.to_vec();
    invalid_lengths.extend_from_slice(&[3, 2, 5, 8]);
    let mut missing_geometry = PREFIX.to_vec();
    missing_geometry.extend_from_slice(&[1, 3]);
    let mut conflict = minimal_strip();
    let mut normal_xyz = vec![0, 0x28, 0, 0x46, 0x80, 0, 0, 0, 0];
    normal_xyz.extend_from_slice(&[0, 0x28, 0, 0, 0, 0, 0, 0x28, 0, 0, 0, 0]);
    conflict.extend(named("mv_p_NxNyNzxyz", &normal_xyz, 18));
    for (bytes, expected_conflicts) in [(invalid_lengths, 0), (missing_geometry, 0), (conflict, 1)]
    {
        assert_live_output(
            0,
            |ctx| triangle_strips(ctx, &bytes),
            |scan| {
                assert!(scan.strips.is_empty());
                assert_eq!(scan.conflicting_representation_count, expected_conflicts);
                0
            },
        );
    }
}

fn strip_bytes(scan: &PrimitiveTriangleStripScan) -> u64 {
    let [strip] = scan.strips.as_slice() else {
        panic!("one complete strip");
    };
    let vertices = match strip.vertices() {
        PrimitiveVertices::Unshaded(rows) => {
            rows.capacity() * std::mem::size_of::<cadmpeg_ir::units::FiniteVector<3>>()
        }
        PrimitiveVertices::Shaded(rows) => {
            rows.capacity() * std::mem::size_of::<PrimitiveShadedVertex>()
        }
    };
    u64::try_from(
        scan.strips.capacity() * std::mem::size_of::<PrimitiveTriangleStrip>()
            + strip.strip_lengths.capacity() * std::mem::size_of::<u32>()
            + vertices,
    )
    .expect("actual strip output backing")
}

#[test]
fn primitive_strip_output_keeps_only_surviving_shaded_or_unshaded_backing() {
    for shaded in [false, true] {
        let bytes = if shaded {
            let mut bytes =
                b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
            bytes.extend(named("mv_p_NxNyNzxyz", &[0; 18], 18));
            bytes
        } else {
            minimal_strip()
        };
        assert_live_output(
            CAP,
            |ctx| triangle_strips(ctx, &bytes),
            |scan| {
                assert_eq!(scan.conflicting_representation_count, 0);
                let [strip] = scan.strips.as_slice() else {
                    panic!("one complete strip");
                };
                assert_eq!(strip.offset, 0);
                assert_eq!(strip.strip_lengths(), [3]);
                assert_eq!(
                    strip.positions().copied().collect::<Vec<_>>(),
                    finite_points(vec![[0.0; 3]; 3])
                );
                if shaded {
                    assert_eq!(
                        strip
                            .normals()
                            .expect("paired normals")
                            .copied()
                            .collect::<Vec<_>>(),
                        finite_points(vec![[0.0; 3]; 3])
                    );
                } else {
                    assert!(strip.normals().is_none());
                }
                strip_bytes(scan)
            },
        );
    }
}

#[test]
fn shaded_strip_projection_refuses_work_before_length_promotion() {
    let bytes = super::strip_visits::strip_geometry_input(17, &[3], &[[0.0; 3]; 3], Some(&[[0.0; 3]; 3]));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let CodecError::ResourceLimit(boundary) = crate::test_support::last_refusal_at(
        &bytes,
        ResourceDimension::WorkUnits,
        "creo primitive shaded vertex projection",
        |ctx| triangle_strips(ctx, &bytes),
    ) else { panic!("work boundary"); };
    assert_eq!(boundary.additional, 3);
    policy.limits.max_work_units = boundary.limit;
    policy.limits.max_materialized_bytes = CAP;
    // Records and the shaded output buffer are admitted before projection.
    // Lengths remain scoped until projection succeeds.
    policy.limits.max_retained_bytes = u64::try_from(
        4 * std::mem::size_of::<PrimitiveTriangleStrip>() + 3 * std::mem::size_of::<PrimitiveShadedVertex>()
    ).expect("backing before length promotion");
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(refusal)) = triangle_strips(&ctx, &bytes) else {
        panic!("present projection must refuse");
    };
    assert_eq!(refusal, boundary);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
