// SPDX-License-Identifier: Apache-2.0
use super::*;
use super::super::PrimitiveTriangleStrip;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const RECORD: &[u8] = b"value(prim_tristripsetwithatt)\0";
const BOUNDARY: &[u8] = b"\xe0\x00value(";
const ACCUM: &[u8] = b"\xe0\x01p_accum_set_size\0";

fn field_windows(length: usize) -> u64 {
    ["p1", "p2", "pts", "mv_p_xyz", "mv_p_NxNyNzxyz"].into_iter()
        .map(|name| length.checked_sub(name.len() + 3).map_or(0, |last| last + 1))
        .map(|count| u64::try_from(count).expect("fixture extent")).sum()
}

fn with_work<T>(work: u64, f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    f(&ctx)
}

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
        assert!(scalar_arrays(&ctx, bytes).expect("no complete field window").is_empty());
        let strips = triangle_strips(&ctx, bytes).expect("no complete record window");
        assert!(strips.strips.is_empty());
        assert_eq!(strips.conflicting_representation_count, 0);
    }
    assert!(matches!(triangle_strip_geometry(&ctx, &[], 3), Err(TriangleStripGeometryError::Missing)));
    let original = ctx.charge_work_limit(1, "seed empty primitive refusal").expect_err("zero cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(scalar_arrays(&ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(triangle_strips(&ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(triangle_strip_geometry(&ctx, &[], 3),
        Err(TriangleStripGeometryError::Resource(CodecError::ResourceLimit(actual))) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn primitive_misses_admit_each_present_schema_window_without_an_exhausted_probe() {
    let data = [0xff; 64];
    for strips in [false, true] {
        let need = if strips { u64::try_from(data.len() - RECORD.len() + 1).expect("record windows") }
            else { field_windows(data.len()) };
        for allowed in 0..=need {
            with_work(allowed, |ctx| {
                let result = if strips {
                    triangle_strips(ctx, &data).map(|scan| {
                        assert_eq!(scan.conflicting_representation_count, 0); scan.strips.len()
                    })
                } else { scalar_arrays(ctx, &data).map(|arrays| arrays.len()) };
                let original = if allowed < need {
                    let Err(CodecError::ResourceLimit(refusal)) = result else { panic!("actual window must refuse"); };
                    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(refusal.operation, if strips { "creo primitive strip discovery" }
                        else { "creo primitive scalar discovery" });
                    assert_eq!((refusal.used, refusal.additional), (allowed, 1)); refusal
                } else {
                    assert_eq!(result.expect("exact windows"), 0);
                    let refusal = ctx.charge_work_limit(1, "seed primitive miss completion").expect_err("exact cap");
                    assert_eq!((refusal.used, refusal.additional), (need, 1)); refusal
                };
                assert!(matches!(scalar_arrays(ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!(ctx.resource_refusal(), Some(original));
            });
        }
    }
}

#[test]
fn primitive_scalar_parsing_admits_actual_token_groups_and_leaves_absent_slots_free() {
    for (body, token_groups, expected) in [
        (&[0xff, 0, 0][..], 1_u64, None),
        (&[0x00, 0x28, 0x00][..], 1, Some([0.0, 1.0, 0.0])),
        (&[0x00, 0x00, 0x00][..], 3, Some([0.0, 0.0, 0.0])),
        (&[0x46, 0x80, 0, 0][..], 1, None),
    ] {
        let bytes = named("p1", body, 3);
        let need = field_windows(bytes.len()) + token_groups;
        with_work(1, |ctx| {
            let Err(CodecError::ResourceLimit(refusal)) = scalar_arrays(ctx, &bytes)
                else { panic!("first actual token group must refuse"); };
            assert_eq!(refusal.operation, "creo primitive scalar parsing");
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!((refusal.used, refusal.additional), (1, 1));
            assert_eq!(ctx.resource_refusal(), Some(refusal));
        });
        with_work(need, |ctx| {
            let arrays = scalar_arrays(ctx, &bytes).expect("exact real token groups and windows");
            match expected {
                None => assert!(arrays.is_empty()),
                Some(values) => {
                    let [array] = arrays.as_slice() else { panic!("one complete scalar array"); };
                    assert_eq!(array.field, PrimitiveArrayField::P1);
                    assert_eq!(array.offset, 0);
                    assert_eq!(array.values.iter().map(|value| value.get()).collect::<Vec<_>>(), values);
                }
            }
            let original = ctx.charge_work_limit(1, "seed primitive token completion").expect_err("exact cap");
            assert_eq!((original.used, original.additional), (need, 1));
            assert!(matches!(scalar_arrays(ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
        });
    }
}

#[test]
fn primitive_strip_boundary_scan_stops_before_the_following_record_body() {
    let mut bytes = RECORD.to_vec();
    bytes.extend_from_slice(BOUNDARY);
    bytes.extend([0xff; 128]);
    let need = u64::try_from((bytes.len() - RECORD.len() + 1) + 1
        + (RECORD.len() - ACCUM.len() + 1)).expect("executed windows");
    with_work(1, |ctx| {
        let Err(CodecError::ResourceLimit(refusal)) = triangle_strips(ctx, &bytes)
            else { panic!("first actual boundary candidate must refuse"); };
        assert_eq!(refusal.operation, "creo primitive strip boundary scan");
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!((refusal.used, refusal.additional), (1, 1));
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
    with_work(need, |ctx| {
        let scan = triangle_strips(ctx, &bytes).expect("one boundary window, missing cumulative marker");
        assert!(scan.strips.is_empty());
        assert_eq!(scan.conflicting_representation_count, 0);
        let original = ctx.charge_work_limit(1, "seed completed primitive boundary").expect_err("exact cap");
        assert_eq!((original.used, original.additional), (need, 1));
        assert!(matches!(triangle_strips(ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
    });
}

#[test]
fn primitive_cumulative_walks_stop_at_absent_counts_and_invalid_lengths() {
    for (counts, parsed, length_visits) in [
        (&[2, 5, 8][..], 3_u64, 1_u64),
        (&[0x80, 0x03, 0x80][..], 2, 0),
    ] {
        let mut bytes = RECORD.to_vec();
        bytes.extend_from_slice(ACCUM);
        bytes.extend([0xf8, 3]);
        bytes.extend_from_slice(counts);
        let boundaries = u64::try_from(bytes.len() - RECORD.len() - BOUNDARY.len() + 1).expect("boundary windows");
        let labels = u64::try_from(RECORD.len() + 1).expect("cumulative marker at record end");
        let discovery = u64::try_from(bytes.len() - RECORD.len() + 1).expect("record windows");
        let need = discovery + boundaries + labels + parsed + length_visits;
        let before_last = 1 + boundaries + labels + if length_visits == 0 { parsed - 1 } else { parsed };
        with_work(before_last, |ctx| {
            let Err(CodecError::ResourceLimit(refusal)) = triangle_strips(ctx, &bytes)
                else { panic!("actual count or length must refuse"); };
            assert_eq!(refusal.operation, if length_visits == 0 { "creo primitive cumulative parsing" }
                else { "creo primitive strip length construction" });
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!((refusal.used, refusal.additional), (before_last, 1));
            assert_eq!(ctx.resource_refusal(), Some(refusal));
        });
        with_work(need, |ctx| {
            let scan = triangle_strips(ctx, &bytes).expect("exact present cumulative candidates");
            assert!(scan.strips.is_empty());
            assert_eq!(scan.conflicting_representation_count, 0);
            let original = ctx.charge_work_limit(1, "seed completed cumulative query").expect_err("exact cap");
            assert_eq!((original.used, original.additional), (need, 1));
            assert!(matches!(triangle_strips(ctx, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original));
        });
    }
}

#[test]
fn primitive_geometry_projects_present_points_and_stops_at_first_representation_conflict() {
    for (shaded, conflict, need) in [(false, false, 11_u64), (false, true, 9),
        (true, false, 20), (true, true, 18)] {
        let field = if shaded { PrimitiveArrayField::VertexNormalsAndPositions }
            else { PrimitiveArrayField::VertexPositions };
        let values = if shaded { vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0] }
            else { vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] };
        let mut other = values.clone();
        if conflict { other[0] = 1.0; }
        let mut arrays = vec![PrimitiveScalarArray { field, offset: 0, values: finite_values(values) },
            PrimitiveScalarArray { field, offset: 17, values: finite_values(other) }];
        if conflict {
            arrays.push(PrimitiveScalarArray { field: PrimitiveArrayField::P1, offset: 42,
                values: finite_values(vec![0.0; 128]) });
        }
        for allowed in 0..=need {
            with_work(allowed, |ctx| {
                let result = triangle_strip_geometry(ctx, &arrays, 3);
                let original = if allowed < need {
                    let Err(TriangleStripGeometryError::Resource(CodecError::ResourceLimit(refusal))) = result
                        else { panic!("next actual geometry member must refuse"); };
                    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                    assert_eq!((refusal.used, refusal.additional), (allowed, 1)); refusal
                } else {
                    if conflict { assert!(matches!(result, Err(TriangleStripGeometryError::Conflicting))); }
                    else {
                        let geometry = result.expect("exact actual projection and agreement cap");
                        assert_eq!(geometry.positions, finite_points(vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]));
                        assert_eq!(geometry.normals, if shaded { Some(finite_points(vec![[0.0, 0.0, 1.0]; 3])) } else { None });
                    }
                    let refusal = ctx.charge_work_limit(1, "seed completed primitive geometry").expect_err("exact cap");
                    assert_eq!((refusal.used, refusal.additional), (need, 1)); refusal
                };
                assert!(matches!(triangle_strip_geometry(ctx, &[], 3),
                    Err(TriangleStripGeometryError::Resource(CodecError::ResourceLimit(actual))) if actual == original));
                assert_eq!(ctx.resource_refusal(), Some(original));
            });
        }
    }
}

#[test]
fn primitive_shaded_pairing_refuses_at_the_first_actual_pair_without_a_second_bulk_fee() {
    with_work(1, |ctx| {
        let positions = finite_points(vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        let normals = finite_points(vec![[0.0, 0.0, 1.0]; 3]);
        let Err(CodecError::ResourceLimit(refusal)) =
            PrimitiveTriangleStrip::new(ctx, 17, positions, Some(normals), vec![3])
        else { panic!("the first actual pair must refuse after the strip-length visit"); };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "creo primitive shaded vertices");
        assert_eq!((refusal.used, refusal.additional), (1, 1));
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        assert!(matches!(PrimitiveTriangleStrip::new(ctx, 0, vec![], None, vec![]),
            Err(CodecError::ResourceLimit(actual)) if actual == refusal));
    });
}
