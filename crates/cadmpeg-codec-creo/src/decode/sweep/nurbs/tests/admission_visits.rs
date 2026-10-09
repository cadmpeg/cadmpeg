// SPDX-License-Identifier: Apache-2.0
use super::super::{
    extrusion_brep_side_surface, interpolation_controls, interpolation_knots,
    interpolation_spline_surface, placed_tabulated_cylinder_directrix, saved_spline_curve,
    saved_spline_nurbs, saved_spline_off_plane_input, saved_spline_sketch_geometry,
    sketch_nurbs_curve, solve_vector_system, ExtrusionSpan, OffPlaneInput,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn work_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

#[test]
fn fixed_spline_routes_are_free_and_preserve_original_refusal() {
    let geometry = cadmpeg_ir::sketches::SketchGeometry::native(
        cadmpeg_core::text::NonBlankString::try_from("unsupported-spline-route")
            .expect("native kind"),
    );
    let transform = crate::placement::FeatureSectionTransform::new(
        7, Some(7), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0,
    ).expect("section frame");
    let span = ExtrusionSpan::new(0.0, 1.0).expect("extrusion span");
    let grid = super::interpolation_grid();
    let replay = crate::surface::TabulatedCylinderCurveReplay {
        body: Vec::new(), surface_id: 7, curve_id: 9, curve_type: 0x13,
        flip: 1, tangent_condition: 0, degree: 3, parameter_body: Vec::new(),
        control_point_ids: [1, 2, 3, 4], successor_reference: 5,
        control_point_bodies: std::array::from_fn(|_| Vec::new()),
        control_points: [None; 4], terminal_reference: 6, offset: 0, surface_row_offset: 0,
    };
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 7, body: Vec::new(), scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(), scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::TabulatedCylinder),
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose, offset: 0, body_offset: 0,
    };
    for variant in 0..5 {
        let mut spline = super::planar_or_offset_spline(0.0);
        spline.interpolation_points.clear();
        spline.declared_point_count = (variant != 0).then_some(0);
        spline.parameters = (variant >= 2).then(|| crate::feature::definitions::DecodedField {
            value: if variant == 3 { vec![0.0] } else { Vec::new() }, body: Vec::new(),
        });
        spline.endpoint_tangents = (variant >= 3).then(|| crate::feature::definitions::DecodedField {
            value: [[0.0; 3]; 2], body: Vec::new(),
        });
        let arena = DecodeArena::new();
        let policy = work_policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let queries = || {
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let results = [
                solve_vector_system(&ctx, Vec::new(), vec![[0.0; 3]]).map(|v| v.is_none()),
                interpolation_knots(&ctx, &[]).map(|v| v.is_none()),
                interpolation_knots(&ctx, &[1.0]).map(|v| v.is_none()),
                interpolation_controls(&ctx, &[], &[], &[], [[0.0; 3]; 2]).map(|v| v.is_none()),
                saved_spline_curve(&ctx, &spline).map(|v| v.is_none()),
                saved_spline_off_plane_input(&ctx, &spline).map(|v| v.is_none()),
                saved_spline_nurbs(&ctx, &spline, &mut refusal).map(|v| v.is_none()),
                saved_spline_sketch_geometry(&ctx, &spline, &mut refusal).map(|v| v.is_none()),
                sketch_nurbs_curve(&ctx, &geometry).map(|v| v.is_none()),
                extrusion_brep_side_surface(
                    &ctx, &transform, &geometry, false, [[0.0; 2]; 2], span,
                    &mut crate::lane_refusal::LaneRefusalContext::new(&"unsupported", &mut refusal),
                ).map(|v| v.is_none()),
                placed_tabulated_cylinder_directrix(&ctx, &replay, &parameters, None, &mut refusal)
                    .map(|v| v.is_none()),
            ];
            assert!(refusal.take_records_checked().expect("empty refusal sink").is_empty());
            results
        };
        for result in queries() { assert!(result.expect("fixed route")); }
        let original = ctx.charge_work_limit(1, "seed spline route refusal").expect_err("zero cap");
        assert_eq!((original.used, original.additional), (0, 1));
        for result in queries() {
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(matches!(interpolation_spline_surface(&ctx, &grid, &"grid", &mut crate::lane_refusal::LaneRefusals::new()), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn dense_solver_admits_only_present_rows_columns_and_elimination() {
    let shape = "creo interpolation matrix shape";
    let column = "creo interpolation column scan";
    let pivot = "creo interpolation pivot work";
    let normalization = "creo interpolation normalization work";
    let row = "creo interpolation elimination row scan";
    let elimination = "creo interpolation elimination work";
    let triangular = [(shape, 1), (shape, 1), (column, 1), (pivot, 2),
        (normalization, 2), (row, 1), (elimination, 2), (column, 1),
        (pivot, 1), (normalization, 1), (row, 1), (elimination, 1)];
    let unique_pivot = [(shape, 1), (shape, 1), (column, 1), (pivot, 2),
        (normalization, 2), (row, 1), (elimination, 2), (column, 1),
        (pivot, 1), (normalization, 1), (row, 1)];
    let diagonal = [(shape, 1), (shape, 1), (column, 1), (pivot, 2),
        (normalization, 2), (row, 1), (column, 1), (pivot, 1),
        (normalization, 1), (row, 1)];
    let singular = [(shape, 1), (shape, 1), (column, 1), (pivot, 2)];
    for (matrix, values, expected, charges) in [
        (Vec::new(), Vec::new(), Some(Vec::new()), &[][..]),
        (vec![vec![1.0, 0.0], vec![1.0, 1.0]], vec![[1.0; 3], [2.0; 3]], Some(vec![[1.0; 3]; 2]), triangular.as_slice()),
        (vec![vec![2.0, 0.0], vec![1.0, 1.0]], vec![[2.0; 3], [2.0; 3]], Some(vec![[1.0; 3]; 2]), unique_pivot.as_slice()),
        (vec![vec![1.0, 0.0], vec![0.0, 1.0]], vec![[1.0; 3], [2.0; 3]], Some(vec![[1.0; 3], [2.0; 3]]), diagonal.as_slice()),
        (vec![vec![0.0, 0.0], vec![0.0, 1.0]], vec![[1.0; 3], [2.0; 3]], None, singular.as_slice()),
    ] {
        let total = charges.iter().map(|(_, n)| n).sum::<u64>();
        for cap in 0..=total {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = solve_vector_system(&ctx, matrix.clone(), values.clone());
            if cap == total {
                assert_eq!(result.expect("source work admitted"), expected);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let mut used = 0;
                let (operation, additional) = charges.iter().copied().find(|(_, n)| {
                    if used + n > cap { true } else { used += n; false }
                }).expect("source-derived next admission");
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("work refusal"); };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!((original.used, original.additional, original.operation), (used, additional, operation));
                assert!(matches!(solve_vector_system(&ctx, matrix.clone(), values.clone()), Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!(ctx.resource_refusal(), Some(original));
            }
        }
    }
    let mut matrix = vec![vec![0.0]];
    matrix.extend(std::iter::repeat_n(vec![0.0; 129], 128));
    let values = vec![[0.0; 3]; 129];
    let arena = DecodeArena::new();
    let policy = work_policy(1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(solve_vector_system(&ctx, matrix, values).expect("first malformed row"), None);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn spline_order_and_off_plane_queries_stop_before_unrelated_tail() {
    let mut duplicate_parameters = vec![0.0, 0.0];
    duplicate_parameters.extend(std::iter::repeat_n(1.0, 128));
    for (parameters, visits) in [
        (Vec::new(), 0), (vec![0.0], 0), (duplicate_parameters, 1),
        (vec![0.0, 1.0, f64::INFINITY], 2),
    ] {
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = interpolation_knots(&ctx, &parameters);
            if cap == visits {
                assert!(result.expect("parameter visits admitted").is_none());
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("parameter visit refusal"); };
                assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, cap, 1));
                assert!(matches!(interpolation_knots(&ctx, &parameters), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
    for (points, visits, index) in [
        (Vec::new(), 0, None), (vec![[0.0; 3]; 3], 3, None),
        (vec![[0.0; 3], [0.0, 0.0, 2.0], [0.0; 3]], 2, Some(1)),
        ({ let mut points = vec![[0.0, 0.0, 2.0]]; points.extend(std::iter::repeat_n([0.0; 3], 128)); points }, 1, Some(0)),
    ] {
        let mut spline = super::planar_or_offset_spline(0.0);
        spline.interpolation_points = points;
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = saved_spline_off_plane_input(&ctx, &spline);
            if cap == visits {
                let result = result.expect("point visits admitted").map(|(input, z)| {
                    let OffPlaneInput::Point(index) = input else { panic!("point identity"); };
                    (index, z)
                });
                assert_eq!(result, index.map(|index| (index, 2.0)));
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("point visit refusal"); };
                assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, cap, 1));
                assert!(matches!(saved_spline_off_plane_input(&ctx, &spline), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}
