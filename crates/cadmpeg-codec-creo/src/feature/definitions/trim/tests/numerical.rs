// SPDX-License-Identifier: Apache-2.0

const SMALL_LINE_LENGTH: f64 = 1e-7;
const SMALL_CIRCLE_RADIUS: f64 = 1e-6;

#[test]
fn trim_endpoint_radius_preserves_missing_agreement_and_refusal() {
    let segment = crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Arc([1, 2]),
        directions: [None; 3],
        center_id: Some(3),
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 7,
        body: Vec::new(),
        offset: 0,
    };
    for (points, expected) in [
        (Vec::new(), Ok(None)),
        (vec![(1, [Some(3.0), Some(4.0)])], Ok(Some(5.0))),
        (
            vec![(1, [Some(3.0), Some(4.0)]), (2, [Some(0.0), Some(5.0)])],
            Ok(Some(5.0)),
        ),
        (
            vec![(1, [Some(3.0), Some(4.0)]), (2, [Some(0.0), Some(6.0)])],
            Err(()),
        ),
        (vec![(1, [Some(0.0), Some(0.0)])], Err(())),
        (vec![(1, [Some(f64::NAN), Some(0.0)])], Err(())),
    ] {
        let points = points.into_iter().collect();
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| super::super::trim_endpoint_radius(
                ctx, &segment, [0.0; 2], &points
            ))
            .expect("endpoint budget")
            .map(|radius| radius.map(cadmpeg_ir::scalar::PositiveReal::get)),
            expected
        );
    }
}

#[test]
fn numerical_ranges_trim_line_intersection_is_scale_independent() {
    for length in [SMALL_LINE_LENGTH, 1.0, 1e150] {
        assert_eq!(
            super::super::trim_line_line_intersection(
                [-length, 0.],
                [length, 0.],
                [0., -length],
                [0., length]
            ),
            Some([0., 0.])
        );
        assert_eq!(
            super::super::trim_line_line_intersection(
                [-length, 0.],
                [length, 0.],
                [-length, length],
                [length, length]
            ),
            None
        );
    }
}

#[test]
fn numerical_followup_circle_intersection_requires_a_unique_tangent() {
    for r in [1.0, SMALL_CIRCLE_RADIUS, 1e-150, 1e150] {
        assert_eq!(
            super::super::trim_circle_circle_intersection([0., 0.], r, [r, 0.], r),
            None
        );
        assert_eq!(
            super::super::trim_circle_circle_intersection([0., 0.], r, [2. * r, 0.], r),
            Some([r, 0.])
        );
        assert_eq!(
            super::super::trim_circle_circle_intersection([0., 0.], r, [3. * r, 0.], r),
            None
        );
    }
}

#[test]
fn numerical_audit_trim_line_circle_rejects_disjoint_small_carriers() {
    for radius in [1.0e-150, 1.0e-4, 1.0, 1.0e150] {
        assert_eq!(
            super::super::trim_line_circle_intersection(
                [-radius, 2.0 * radius],
                [radius, 2.0 * radius],
                [0.0; 2],
                radius
            ),
            None
        );
        assert_eq!(
            super::super::trim_line_circle_intersection(
                [-radius, radius],
                [radius, radius],
                [0.0; 2],
                radius
            ),
            Some([0.0, radius])
        );
    }
}
