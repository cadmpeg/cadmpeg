// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn arrays(shaded: bool) -> [PrimitiveScalarArray; 2] {
    let values = if shaded {
        vec![
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0,
            0.0,
        ]
    } else {
        vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    };
    let first = PrimitiveScalarArray {
        field: if shaded {
            PrimitiveArrayField::VertexNormalsAndPositions
        } else {
            PrimitiveArrayField::VertexPositions
        },
        offset: 17,
        values: finite_values(values),
    };
    let mut second = first.clone();
    second.offset = 42;
    [first, second]
}

#[test]
fn primitive_conflicts_compare_borrowed_lanes_without_allocating_candidates() {
    for shaded in [false, true] {
        let mut arrays = arrays(shaded);
        arrays[1].values[0] = FiniteReal::ONE;
        let need =
            crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                match triangle_strip_geometry(&ctx, &arrays, 3) {
                    Err(TriangleStripGeometryError::Conflicting) => Ok(()),
                    Err(TriangleStripGeometryError::Resource(error)) => Err(error),
                    other => panic!("expected conflict: {other:?}"),
                }
            });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = need;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(
            triangle_strip_geometry(&ctx, &arrays, 3),
            Err(TriangleStripGeometryError::Conflicting)
        ));
        let original = ctx
            .charge_work_limit(1, "after primitive conflict")
            .expect_err("exact visits");
        assert_eq!((original.used, original.additional), (need, 1));
        assert!(matches!(triangle_strip_geometry(&ctx, &arrays, 3),
            Err(TriangleStripGeometryError::Resource(CodecError::ResourceLimit(actual)))
                if actual == original));
    }
}

#[test]
fn primitive_geometry_retains_only_selected_output_buffers() {
    const CAP: u64 = 16 * 1024;
    for shaded in [false, true] {
        for scoped in [false, true] {
            let arrays = arrays(shaded);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = CAP;
            policy.limits.max_retained_bytes = CAP;
            // One selected three-vertex positions buffer, plus normals when present.
            policy.limits.max_collection_items = 3 * (1 + u64::from(shaded));
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let parse = || {
                triangle_strip_geometry(&ctx, &arrays, 3).map_err(|error| {
                    let TriangleStripGeometryError::Resource(error) = error else {
                        panic!("valid agreeing representations");
                    };
                    error
                })
            };
            let parts = if scoped {
                let parts = ctx
                    .with_scoped_storage("primitive geometry parent", &parse)
                    .expect("scoped geometry");
                (parts.0, Some(parts.1))
            } else {
                (parse().expect("retained geometry"), None)
            };
            let storage = parts.1;
            let geometry = parts.0;
            assert_eq!(
                geometry.positions,
                finite_points(vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            );
            assert_eq!(
                geometry.normals,
                if shaded {
                    Some(finite_points(vec![[0.0, 0.0, 1.0]; 3]))
                } else {
                    None
                }
            );
            let expected_bytes = u64::try_from(
                (geometry.positions.capacity()
                    + geometry.normals.as_ref().map_or(0, Vec::capacity))
                    * std::mem::size_of::<FiniteVector<3>>(),
            )
            .expect("live geometry backing");
            let original = if scoped {
                ctx.reserve_scoped_limit(CAP + 1, "after selected geometry")
                    .expect_err("probe live selected backing")
            } else {
                ctx.charge_retained_limit(CAP + 1, "after selected geometry")
                    .expect_err("probe retained selected backing")
            };
            assert_eq!(
                original.dimension,
                if scoped {
                    ResourceDimension::MaterializedBytes
                } else {
                    ResourceDimension::RetainedBytes
                }
            );
            assert_eq!(
                (original.used, original.additional),
                (expected_bytes, CAP + 1)
            );
            assert!(
                matches!(parse(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
            drop(geometry);
            drop(storage);
        }
    }
}

#[test]
fn primitive_geometry_allocation_and_promotion_preserve_original_refusal() {
    let arrays = arrays(false);
    // Core amortized growth starts a nonempty vector with four slots.
    let bytes = u64::try_from(4 * std::mem::size_of::<FiniteVector<3>>()).expect("four slots");
    for retained in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if retained {
            policy.limits.max_retained_bytes = 0;
        } else {
            policy.limits.max_materialized_bytes = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(TriangleStripGeometryError::Resource(CodecError::ResourceLimit(original))) =
            triangle_strip_geometry(&ctx, &arrays[..1], 3)
        else {
            panic!("selected backing needs admission");
        };
        assert_eq!(
            original.dimension,
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::MaterializedBytes
            }
        );
        assert_eq!(
            original.operation,
            if retained {
                "creo triangle strip geometry storage"
            } else {
                "creo triangle strip positions"
            }
        );
        assert_eq!((original.used, original.additional), (0, bytes));
        assert!(matches!(triangle_strip_geometry(&ctx, &[], 3),
            Err(TriangleStripGeometryError::Resource(CodecError::ResourceLimit(actual)))
                if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}
