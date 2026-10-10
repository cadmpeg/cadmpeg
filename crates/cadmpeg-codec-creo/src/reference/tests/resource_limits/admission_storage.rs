// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::reference::{ConicType, ReferenceConic, ReferenceLineKind};

fn check_live_output_storage<T>(
    data: &[u8],
    parse: impl Fn(&DecodeContext<'_>) -> Result<Vec<T>, CodecError>,
    check: impl Fn(&[T]) -> usize,
) {
    for scoped in [false, true] {
        let dimension = if scoped {
            ResourceDimension::MaterializedBytes
        } else {
            ResourceDimension::RetainedBytes
        };
        let cap = crate::test_support::allocation_limit_at(dimension, None, |allowed| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if scoped {
                policy.limits.max_materialized_bytes = allowed;
            } else {
                policy.limits.max_retained_bytes = allowed;
            }
            let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy).expect("root");
            if scoped {
                ctx.with_scoped_storage("reference output parent", || parse(&ctx))
                    .map(drop)
            } else {
                parse(&ctx).map(drop)
            }
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if scoped {
            policy.limits.max_materialized_bytes = cap;
        } else {
            policy.limits.max_retained_bytes = cap;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy).expect("root");
        let parts = if scoped {
            let parts = ctx
                .with_scoped_storage("reference output parent", || parse(&ctx))
                .expect("scoped reference output");
            (parts.0, Some(parts.1))
        } else {
            (parse(&ctx).expect("retained reference output"), None)
        };
        let storage = parts.1;
        let records = parts.0;
        let expected_bytes =
            u64::try_from(records.capacity() * std::mem::size_of::<T>() + check(&records))
                .expect("actual surviving backing bytes");
        let refusal = if scoped {
            ctx.reserve_scoped_limit(u64::MAX, "after reference cache scratch")
                .expect_err("probe live output only")
        } else {
            ctx.charge_retained_limit(u64::MAX, "after reference cache scratch")
                .expect_err("probe retained output only")
        };
        assert_eq!(
            refusal.dimension,
            if scoped {
                ResourceDimension::MaterializedBytes
            } else {
                ResourceDimension::RetainedBytes
            }
        );
        assert_eq!(
            (refusal.used, refusal.additional),
            (expected_bytes, u64::MAX)
        );
        drop(records);
        drop(storage);
    }
}

#[test]
fn unused_reference_scalar_caches_release_their_actual_backing() {
    macro_rules! check_empty {
        ($parse:ident) => {
            check_live_output_storage(
                SCALAR_IMAGE,
                |ctx| crate::reference::$parse(ctx, SCALAR_IMAGE),
                |records| {
                    assert!(records.is_empty());
                    0
                },
            );
        };
    }
    check_empty!(named_conics);
    check_empty!(positional_conics);
    check_empty!(lines);
    check_empty!(line3d_lines);
    check_empty!(arc_z_circles);
}

#[test]
fn reference_fixed_records_outlive_only_their_output_backing() {
    let mut line = SCALAR_IMAGE.to_vec();
    line.extend_from_slice(LINE3D);
    check_live_output_storage(
        &line,
        |ctx| crate::reference::line3d_lines(ctx, &line),
        |records| {
            let [record] = records else {
                panic!("one spatial line");
            };
            assert!(matches!(record.kind(), ReferenceLineKind::Line3d {
            entity_id: 35, original_length,
        } if original_length.get() == 1.0));
            assert_eq!(<[f64; 3]>::from(record.start().get()), [0.0; 3]);
            assert_eq!(<[f64; 3]>::from(record.end().get()), [1.0, 0.0, 0.0]);
            assert_eq!(
                record.offset,
                SCALAR_IMAGE.len() + b"ent_list(line3d)\0\x23\xe3".len()
            );
            0
        },
    );
    let mut circle = SCALAR_IMAGE.to_vec();
    circle.extend_from_slice(ARC_Z);
    check_live_output_storage(
        &circle,
        |ctx| crate::reference::arc_z_circles(ctx, &circle),
        |records| {
            let [record] = records else {
                panic!("one circle");
            };
            assert_eq!(record.entity_id, 45);
            assert_eq!(<[f64; 3]>::from(record.center().get()), [0.0; 3]);
            assert!(!record.center_stored());
            assert_eq!(record.radius().get(), 1.0);
            assert_eq!(record.axis(), cadmpeg_ir::units::UnitVector3::Z_AXIS);
            assert_eq!(<[f64; 3]>::from(record.start().get()), [1.0, 0.0, 0.0]);
            assert_eq!(<[f64; 3]>::from(record.end().get()), [-1.0, 0.0, 0.0]);
            assert_eq!(
                record.offset,
                SCALAR_IMAGE.len() + b"ent_list(arc_z)\0\xe2\x2d\xe3".len()
            );
            0
        },
    );
}

#[test]
fn reference_conic_bodies_remain_live_after_the_cache_drops() {
    for (source, named) in [(NAMED_CONIC, true), (POSITIONAL_CONIC, false)] {
        let mut data = SCALAR_IMAGE.to_vec();
        data.extend_from_slice(source);
        check_live_output_storage(
            &data,
            |ctx| {
                if named {
                    crate::reference::named_conics(ctx, &data)
                } else {
                    crate::reference::positional_conics(ctx, &data)
                }
            },
            |records: &[ReferenceConic]| {
                let [record] = records else {
                    panic!("one conic");
                };
                assert_eq!(record.entity_id, if named { 42 } else { 43 });
                assert_eq!(record.type_id, ConicType::Ellipse);
                assert_eq!(record.flip, 1);
                assert_eq!(<[f64; 3]>::from(record.start.get()), [1.0, 0.0, 0.0]);
                assert_eq!(<[f64; 3]>::from(record.end.get()), [-1.0, 0.0, 0.0]);
                assert_eq!(
                    record
                        .parameter_start
                        .map(cadmpeg_ir::scalar::FiniteReal::get),
                    Some(0.0)
                );
                assert_eq!(
                    record
                        .parameter_end
                        .map(cadmpeg_ir::scalar::FiniteReal::get),
                    Some(std::f64::consts::PI)
                );
                assert_eq!(
                    [record.coefficient_1.get(), record.coefficient_2.get()],
                    [-1.0, 1.0]
                );
                if named {
                    assert_eq!(record.offset, SCALAR_IMAGE.len());
                    assert_eq!(
                        record
                            .local_system
                            .map(cadmpeg_ir::units::FiniteVector::get),
                        Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                    );
                    assert_eq!(
                        record.body,
                        NAMED_CONIC[b"ent_list(conic)\0\xe0\x01id\0".len()..NAMED_CONIC.len() - 4]
                    );
                } else {
                    assert_eq!(
                        record.offset,
                        SCALAR_IMAGE.len() + b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3".len()
                    );
                    assert_eq!(
                        record
                            .local_system
                            .map(cadmpeg_ir::units::FiniteVector::get),
                        Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0, 0.0])
                    );
                    assert_eq!(
                        record.body,
                        POSITIONAL_CONIC[b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\x2b\x1e\xe2"
                            .len()
                            ..POSITIONAL_CONIC.len()
                                - b"\xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0".len()]
                    );
                }
                record.body.capacity()
            },
        );
    }
}
