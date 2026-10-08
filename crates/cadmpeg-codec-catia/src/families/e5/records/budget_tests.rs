// SPDX-License-Identifier: Apache-2.0
//! NURBS pole rejection work.

#[test]
fn e5_invalid_first_pole_does_not_charge_unvisited_rows_or_columns() {
    for (rows, columns) in [(1, 1), (10_000, 1), (1, 10_000)] {
        let mut visits = 0;
        let result = crate::test_support::with_work_limit(2, |ctx| {
            super::nurbs_pole_rows(ctx, rows, columns, |_| {
                visits += 1;
                None::<[f64; 3]>
            })
        })
        .expect("one row visit and one pole visit");
        assert!(result.is_none());
        assert_eq!(visits, 1);
    }
}
