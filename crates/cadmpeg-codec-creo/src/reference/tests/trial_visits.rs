// SPDX-License-Identifier: Apache-2.0
use super::super::arc_z_fields;
use crate::scalar::ScalarCache;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn arc_trials_admit_two_present_offset_passes_without_a_terminal_visit() {
    let valid = b"\x01\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f".as_slice();
    for (body, valid) in [(b"\x00\x00\x00".as_slice(), false), (valid, true)] {
        let cache = ScalarCache::default();
        let circle =
            crate::test_support::assert_work_boundaries(&["creo arc-z numeric trials"], |ctx| {
                arc_z_fields(ctx, body, &cache, 7)
            });

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2 * u64::try_from(body.len()).expect("two source passes");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let exact = arc_z_fields(&ctx, body, &cache, 7).expect("two complete trial passes");
        assert_eq!(exact, circle);
        let refusal = ctx
            .charge_work_limit(1, "after exact owner work")
            .expect_err("exact cap exhausted");
        assert_eq!(
            (refusal.dimension, refusal.used, refusal.additional),
            (
                ResourceDimension::WorkUnits,
                2 * u64::try_from(body.len()).expect("two source passes"),
                1
            )
        );
        if valid {
            let circle = circle.expect("diameter image");
            assert_eq!(circle.entity_id, 7);
            assert_eq!(circle.offset, 1);
            assert_eq!(<[f64; 3]>::from(circle.center().get()), [0.0; 3]);
            assert_eq!(circle.radius().get(), 1.0);
            assert_eq!(<[f64; 3]>::from(circle.start().get()), [1.0, 0.0, 0.0]);
            assert_eq!(<[f64; 3]>::from(circle.end().get()), [-1.0, 0.0, 0.0]);
            assert!(!circle.center_stored());
        } else {
            assert!(circle.is_none());
        }
    }
}

#[test]
fn singleton_reference_ellipse_admits_only_its_source_visit() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
    use cadmpeg_ir::units::{FiniteVector, UnitVector3};
    let conic = super::super::ReferenceConic {
        entity_id: 7,
        type_id: super::super::ConicType::Ellipse,
        flip: 1,
        start: FinitePoint3::new([-1.0, 0.0, 0.0].into()).expect("start"),
        end: FinitePoint3::new([1.0, 0.0, 0.0].into()).expect("end"),
        parameter_start: None,
        parameter_end: None,
        coefficient_1: FiniteReal::ONE,
        coefficient_2: FiniteReal::ONE,
        local_system: FiniteVector::new([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        ]),
        body: Vec::new(),
        offset: 12,
    };
    let expected = super::super::ReferenceEllipse::try_new(
        7,
        FinitePoint3::new([0.0; 3].into()).expect("center"),
        UnitVector3::Z_AXIS,
        UnitVector3::X_AXIS.reversed(),
        [PositiveLength::new(1.0).expect("radius"); 2],
        12,
    )
    .expect("circle is an ellipse");
    let actual =
        crate::test_support::assert_work_boundaries(&["creo reference conic traversal"], |ctx| {
            super::super::ellipse_carriers(ctx, std::slice::from_ref(&conic))
        });
    assert_eq!(actual.as_slice(), std::slice::from_ref(&expected));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::ellipse_carriers(&ctx, std::slice::from_ref(&conic))
            .expect("one conic visit"),
        [expected]
    );
    let refusal = ctx
        .charge_work_limit(1, "after exact owner work")
        .expect_err("exact cap exhausted");
    assert_eq!(
        (refusal.dimension, refusal.used, refusal.additional),
        (ResourceDimension::WorkUnits, 1, 1)
    );
}

#[test]
fn empty_arc_trials_are_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let cache = ScalarCache::default();
    assert!(arc_z_fields(&ctx, &[], &cache, 7)
        .expect("no source offset")
        .is_none());
}

#[test]
fn empty_reference_outputs_are_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let results = [
        super::super::named_conics(&ctx, &[]).map(|v| v.is_empty()),
        super::super::positional_conics(&ctx, &[]).map(|v| v.is_empty()),
        super::super::lines(&ctx, &[]).map(|v| v.is_empty()),
        super::super::line3d_lines(&ctx, &[]).map(|v| v.is_empty()),
        super::super::arc_z_circles(&ctx, &[]).map(|v| v.is_empty()),
        super::super::ellipse_carriers(&ctx, &[]).map(|v| v.is_empty()),
    ];
    for result in results {
        assert!(result.expect("empty output"));
    }
}

#[test]
fn reference_searches_preserve_bounded_marker_ends() {
    for marker in [
        b"ent_list(conic)\0".as_slice(),
        b"ent_list(line)\0".as_slice(),
        b"ent_list(line3d)\0".as_slice(),
        b"ent_list(arc_z)\0".as_slice(),
    ] {
        let mut repeated = marker.to_vec();
        repeated.extend_from_slice(b"\xe0\x00");
        repeated.extend_from_slice(marker);
        for payload in (0..=marker.len())
            .map(|end| &marker[..end])
            .chain(std::iter::once(repeated.as_slice()))
        {
            let empty = crate::decode::with_test_decode_ctx(|ctx| {
                Ok::<_, cadmpeg_core::CodecError>([
                    super::super::named_conics(ctx, payload)?.is_empty(),
                    super::super::positional_conics(ctx, payload)?.is_empty(),
                    super::super::lines(ctx, payload)?.is_empty(),
                    super::super::line3d_lines(ctx, payload)?.is_empty(),
                    super::super::arc_z_circles(ctx, payload)?.is_empty(),
                ])
            })
            .expect("marker ends remain within the payload");
            assert_eq!(empty, [true; 5]);
        }
    }
}
