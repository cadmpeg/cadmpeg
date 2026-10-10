// SPDX-License-Identifier: Apache-2.0

use super::super::InterpolationGrid;

#[test]
fn fixed_mixed_validation_keeps_only_variable_grid_work() {
    for (u_count, v_count) in [(2_u32, 2_u32), (2, 3), (3, 2)] {
        let u = usize::try_from(u_count).expect("fixture axis fits usize");
        let v = usize::try_from(v_count).expect("fixture axis fits usize");
        for nonfinite in [false, true] {
            let actual = super::work_output(|ctx| {
                let mut mixed = [[1.0; 3]; 4];
                if nonfinite {
                    mixed[3][2] = f64::INFINITY;
                }
                InterpolationGrid::try_new(
                    ctx,
                    vec![[0.0; 3]; u * v],
                    (0..u_count).map(f64::from).collect(),
                    (0..v_count).map(f64::from).collect(),
                    vec![[0.0; 3]; 2 * v],
                    vec![[0.0; 3]; 2 * u],
                    mixed,
                )
            });
            if nonfinite {
                assert!(actual.is_none());
            } else {
                let grid = actual.expect("finite complete grid");
                assert_eq!(grid.points().len(), u * v);
                assert_eq!(grid.mixed_derivatives(), &[[1.0; 3]; 4]);
            }
        }
    }
}
