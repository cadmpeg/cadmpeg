// SPDX-License-Identifier: Apache-2.0
use super::super::{
      interpolation_knots,
      
     saved_spline_off_plane_input, 
     solve_vector_system,  OffPlaneInput,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn work_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}


#[test]
fn dense_solver_admits_only_present_rows_columns_and_elimination() {
    let shape = "creo interpolation matrix shape";
    let column = "creo interpolation column scan";
    let pivot = "creo interpolation pivot work";
    let normalization = "creo interpolation normalization work";
    let row = "creo interpolation elimination row scan";
    let elimination = "creo interpolation elimination work";
    for (matrix, values, expected, operations) in [
        (Vec::new(), Vec::new(), Some(Vec::new()), &[][..]),
        (
            vec![vec![1.0, 0.0], vec![1.0, 1.0]],
            vec![[1.0; 3], [2.0; 3]],
            Some(vec![[1.0; 3]; 2]),
            &[shape, column, pivot, normalization, row, elimination][..],
        ),
        (
            vec![vec![2.0, 0.0], vec![1.0, 1.0]],
            vec![[2.0; 3], [2.0; 3]],
            Some(vec![[1.0; 3]; 2]),
            &[shape, column, pivot, normalization, row, elimination][..],
        ),
        (
            vec![vec![1.0, 0.0], vec![0.0, 1.0]],
            vec![[1.0; 3], [2.0; 3]],
            Some(vec![[1.0; 3], [2.0; 3]]),
            &[shape, column, pivot, normalization, row][..],
        ),
        (
            vec![vec![0.0, 0.0], vec![0.0, 1.0]],
            vec![[1.0; 3], [2.0; 3]],
            None,
            &[shape, column, pivot][..],
        ),
    ] {
        let result = crate::test_support::assert_work_boundaries(operations, |ctx| {
            solve_vector_system(ctx, matrix.clone(), values.clone())
        });
        assert_eq!(result, expected);
    }
    let mut matrix = vec![vec![0.0]];
    matrix.extend(std::iter::repeat_n(vec![0.0; 129], 128));
    let values = vec![[0.0; 3]; 129];
    let arena = DecodeArena::new();
    let policy = work_policy(1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        solve_vector_system(&ctx, matrix, values).expect("first malformed row"),
        None
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn spline_order_and_off_plane_queries_stop_before_unrelated_tail() {
    let mut duplicate_parameters = vec![0.0, 0.0];
    duplicate_parameters.extend(std::iter::repeat_n(1.0, 128));
    for (parameters, visits) in [
        (Vec::new(), 0),
        (vec![0.0], 0),
        (duplicate_parameters, 1),
        (vec![0.0, 1.0, f64::INFINITY], 2),
    ] {
        crate::test_support::assert_work_boundaries(
            if visits == 0 {
                &[]
            } else {
                &["creo interpolation parameter order"]
            },
            |ctx| interpolation_knots(ctx, &parameters),
        );
        let arena = DecodeArena::new();
        let policy = work_policy(visits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(interpolation_knots(&ctx, &parameters)
            .expect("parameter visits admitted")
            .is_none());
        assert_eq!(ctx.resource_refusal(), None);
    }
    for (points, visits, index) in [
        (Vec::new(), 0, None),
        (vec![[0.0; 3]; 3], 3, None),
        (vec![[0.0; 3], [0.0, 0.0, 2.0], [0.0; 3]], 2, Some(1)),
        (
            {
                let mut points = vec![[0.0, 0.0, 2.0]];
                points.extend(std::iter::repeat_n([0.0; 3], 128));
                points
            },
            1,
            Some(0),
        ),
    ] {
        let mut spline = super::planar_or_offset_spline(0.0);
        spline.interpolation_points = points;
        crate::test_support::assert_work_boundaries(
            if visits == 0 {
                &[]
            } else {
                &["creo saved spline off-plane input scan"]
            },
            |ctx| saved_spline_off_plane_input(ctx, &spline),
        );
        let arena = DecodeArena::new();
        let policy = work_policy(visits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = saved_spline_off_plane_input(&ctx, &spline)
            .expect("point visits admitted")
            .map(|(input, z)| {
                let OffPlaneInput::Point(index) = input else {
                    panic!("point identity");
                };
                (index, z)
            });
        assert_eq!(result, index.map(|index| (index, 2.0)));
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn fixed_interpolation_knot_endpoints_do_not_consume_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two parameters have exactly one ordering comparison and no interior knots.
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        interpolation_knots(&ctx, &[0.0, 1.0]).expect("only parameter order"),
        Some(vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
    );
    assert_eq!(ctx.resource_refusal(), None);
}
