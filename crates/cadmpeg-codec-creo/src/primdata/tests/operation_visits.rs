// SPDX-License-Identifier: Apache-2.0
use super::super::PrimitiveTriangleStrip;
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const RECORD: &[u8] = b"value(prim_tristripsetwithatt)\0";
const BOUNDARY: &[u8] = b"\xe0\x00value(";
const ACCUM: &[u8] = b"\xe0\x01p_accum_set_size\0";

#[test]
fn fixed_short_primitive_searches_and_empty_geometry_are_free_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for bytes in [&[][..], &[0xff; 4][..]] {
        assert!(scalar_arrays(&ctx, bytes)
            .expect("no complete field window")
            .is_empty());
        let strips = triangle_strips(&ctx, bytes).expect("no complete record window");
        assert!(strips.strips.is_empty());
        assert_eq!(strips.conflicting_representation_count, 0);
    }
    assert!(matches!(
        triangle_strip_geometry(&ctx, &[], 3),
        Err(TriangleStripGeometryError::Missing)
    ));
    let original = ctx
        .charge_work_limit(1, "seed empty primitive refusal")
        .expect_err("zero cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(
        matches!(scalar_arrays(&ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(triangle_strips(&ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(matches!(triangle_strip_geometry(&ctx, &[], 3),
        Err(TriangleStripGeometryError::Resource(CodecError::ResourceLimit(actual))) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn primitive_misses_admit_each_present_schema_window_without_an_exhausted_probe() {
    let data = [0xff; 64];
    for strips in [false, true] {
        let length = super::work_output(|ctx| {
            if strips {
                triangle_strips(ctx, &data).map(|scan| {
                    assert_eq!(scan.conflicting_representation_count, 0);
                    scan.strips.len()
                })
            } else {
                scalar_arrays(ctx, &data).map(|arrays| arrays.len())
            }
        });
        assert_eq!(length, 0);
    }
}

#[test]
fn primitive_scalar_parsing_admits_actual_token_groups_and_leaves_absent_slots_free() {
    for (body, expected) in [
        (&[0xff, 0, 0][..], None),
        (&[0x00, 0x28, 0x00][..], Some([0.0, 1.0, 0.0])),
        (&[0x00, 0x00, 0x00][..], Some([0.0, 0.0, 0.0])),
        (&[0x46, 0x80, 0, 0][..], None),
    ] {
        let bytes = named("p1", body, 3);
        let arrays = super::work_output(|ctx| scalar_arrays(ctx, &bytes));
        match expected {
            None => assert!(arrays.is_empty()),
            Some(values) => {
                let [array] = arrays.as_slice() else {
                    panic!("one complete scalar array");
                };
                assert_eq!(array.field, PrimitiveArrayField::P1);
                assert_eq!(array.offset, 0);
                assert_eq!(
                    array
                        .values
                        .iter()
                        .map(|value| value.get())
                        .collect::<Vec<_>>(),
                    values
                );
            }
        }
    }
}

#[test]
fn primitive_strip_boundary_scan_stops_before_the_following_record_body() {
    let mut bytes = RECORD.to_vec();
    bytes.extend_from_slice(BOUNDARY);
    bytes.extend([0xff; 128]);
    let scan = super::work_output(|ctx| triangle_strips(ctx, &bytes));
    assert!(scan.strips.is_empty());
    assert_eq!(scan.conflicting_representation_count, 0);
}

#[test]
fn primitive_cumulative_walks_stop_at_absent_counts_and_invalid_lengths() {
    for counts in [&[2, 5, 8][..], &[0x80, 0x03, 0x80][..]] {
        let mut bytes = RECORD.to_vec();
        bytes.extend_from_slice(ACCUM);
        bytes.extend([0xf8, 3]);
        bytes.extend_from_slice(counts);
        let scan = super::work_output(|ctx| triangle_strips(ctx, &bytes));
        assert!(scan.strips.is_empty());
        assert_eq!(scan.conflicting_representation_count, 0);
    }
}

#[test]
fn primitive_geometry_projects_present_points_and_stops_at_first_representation_conflict() {
    for (shaded, conflict) in [(false, false), (false, true), (true, false), (true, true)] {
        let field = if shaded {
            PrimitiveArrayField::VertexNormalsAndPositions
        } else {
            PrimitiveArrayField::VertexPositions
        };
        let values = if shaded {
            vec![
                0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                1.0, 0.0,
            ]
        } else {
            vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        };
        let mut other = values.clone();
        if conflict {
            other[0] = 1.0;
        }
        let mut arrays = vec![
            PrimitiveScalarArray {
                field,
                offset: 0,
                values: finite_values(values),
            },
            PrimitiveScalarArray {
                field,
                offset: 17,
                values: finite_values(other),
            },
        ];
        if conflict {
            arrays.push(PrimitiveScalarArray {
                field: PrimitiveArrayField::P1,
                offset: 42,
                values: finite_values(vec![0.0; 128]),
            });
        }
        let geometry = super::work_output(|ctx| match triangle_strip_geometry(ctx, &arrays, 3) {
            Ok(geometry) => Ok(Some(geometry)),
            Err(TriangleStripGeometryError::Conflicting) if conflict => Ok(None),
            Err(TriangleStripGeometryError::Resource(error)) => Err(error),
            other => panic!("unexpected geometry result: {other:?}"),
        });
        if conflict {
            assert!(geometry.is_none());
        } else {
            let geometry = geometry.expect("agreeing geometry");
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
        }
    }
}

#[test]
fn primitive_shaded_pairing_refuses_at_the_first_actual_pair_without_a_second_bulk_fee() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo primitive shaded vertices",
        |ctx| {
            PrimitiveTriangleStrip::new(
                ctx,
                17,
                finite_points(vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
                Some(finite_points(vec![[0.0, 0.0, 1.0]; 3])),
                vec![3],
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::WorkUnits && refusal.operation == "creo primitive shaded vertices"));
}
