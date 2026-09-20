// SPDX-License-Identifier: Apache-2.0

#[test]
fn numerical_ranges_meridian_circles_retain_two_small_intersections() {
    for radius in [1e-7, 1.0, 1e100] {
        let roots = super::meridian_circle_intersections([0., 0.], radius, [radius, 0.], radius);
        assert_eq!(roots.len(), 2);
        for point in roots {
            assert!((point[0] / radius - 0.5).abs() < 32.0 * f64::EPSILON);
            assert!((point[1].abs() / radius - 3.0_f64.sqrt() * 0.5).abs() < 32.0 * f64::EPSILON);
        }
        assert!(
            super::meridian_circle_intersections([0., 0.], radius, [3. * radius, 0.], radius)
                .is_empty()
        );
    }
}
