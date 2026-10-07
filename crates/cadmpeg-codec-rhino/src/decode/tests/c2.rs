// SPDX-License-Identifier: Apache-2.0

use super::super::c2_curve_to_nurbs_join;
use super::{decoded_nurbs, finite_parameter, line_nurbs, with_collection_limit, with_expand_bytes, Diagnostics, NurbsCurve, Point3};

#[test]
fn c2_polycurve_merges_clamped_rational_segments_in_parent_domain() {
    let compound = crate::curves::DecodedCurve::Compound {
        children: vec![
            (
                finite_parameter(10.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, true)),
            ),
            (
                finite_parameter(20.0),
                decoded_nurbs(line_nurbs(-2.0, 2.0, false)),
            ),
        ],
        end_parameter: finite_parameter(40.0),
        warnings: Diagnostics::new(),
    };
    let merged = with_expand_bytes(&[], |expand| {
        c2_curve_to_nurbs_join(expand.ctx(), compound, 0)
    })
    .expect("merge")
    .curve;
    assert_eq!(
        merged.knots().as_slice(),
        vec![10.0, 10.0, 20.0, 40.0, 40.0]
    );
    assert_eq!(merged.control_points().len(), 3);
    assert_eq!(merged.pole_rows().weights(), Some(vec![2.0, 1.0, 1.0]));
    assert!(!merged.periodic());
}

#[test]
fn c2_joined_segments_refuse_collection_limit() {
    let compound = crate::curves::DecodedCurve::Compound {
        children: vec![(
            finite_parameter(0.0),
            decoded_nurbs(line_nurbs(0.0, 1.0, true)),
        )],
        end_parameter: finite_parameter(1.0),
        warnings: Diagnostics::new(),
    };
    let error = with_collection_limit(0, |ctx| c2_curve_to_nurbs_join(ctx, compound, 0))
        .err()
        .expect("one C2 segment exceeds zero collection items");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.operation == "Rhino C2 joined segments"
    ));
}

#[test]
fn recursive_c2_polycurve_preserves_nested_parent_parameterization() {
    let nested = crate::curves::DecodedCurve::Compound {
        children: vec![
            (
                finite_parameter(0.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
            ),
            (
                finite_parameter(1.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
            ),
        ],
        end_parameter: finite_parameter(2.0),
        warnings: Diagnostics::new(),
    };
    let outer = crate::curves::DecodedCurve::Compound {
        children: vec![(finite_parameter(5.0), nested)],
        end_parameter: finite_parameter(9.0),
        warnings: Diagnostics::new(),
    };
    let merged = with_expand_bytes(&[], |expand| c2_curve_to_nurbs_join(expand.ctx(), outer, 0))
        .expect("nested merge")
        .curve;
    assert_eq!(merged.knots().as_slice(), vec![5.0, 5.0, 7.0, 9.0, 9.0]);
}

#[test]
fn unequal_degree_c2_polycurve_elevates_lower_degree() {
    let quadratic = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid quadratic");
    let compound = crate::curves::DecodedCurve::Compound {
        children: vec![
            (
                finite_parameter(0.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
            ),
            (finite_parameter(1.0), decoded_nurbs(quadratic)),
        ],
        end_parameter: finite_parameter(2.0),
        warnings: Diagnostics::new(),
    };
    let merged = with_expand_bytes(&[], |expand| {
        c2_curve_to_nurbs_join(expand.ctx(), compound, 0)
    })
    .expect("degree elevation")
    .curve;
    assert_eq!(merged.degree(), 2);
    assert_eq!(merged.control_points().len(), 5);
    assert_eq!(
        merged.knots().as_slice(),
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0]
    );
}

