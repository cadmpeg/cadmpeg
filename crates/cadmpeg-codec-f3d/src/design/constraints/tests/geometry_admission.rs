// SPDX-License-Identifier: Apache-2.0
use crate::design::constraints::{
    rotated_sketch_geometry_matches, translated_sketch_geometry_matches,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

#[test]
fn nurbs_pattern_comparisons_admit_knots_and_poles() {
    for (weights, operation) in [
        (None, "f3d pattern knot scan"),
        (None, "f3d pattern polynomial pole scan"),
        (Some(vec![1.0, 2.0]), "f3d pattern rational pole scan"),
    ] {
        let curve = PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 0.0), Point2::new(2.0, 0.0)],
            weights,
            false,
        )
        .unwrap()
        .unwrap();
        let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap();
        for rotated in [false, true] {
            let compare = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
                if rotated {
                    rotated_sketch_geometry_matches(
                        ctx,
                        &geometry,
                        &geometry,
                        Point2::new(0.0, 0.0),
                        0.0,
                    )
                } else {
                    translated_sketch_geometry_matches(
                        ctx,
                        &geometry,
                        &geometry,
                        Point2::new(0.0, 0.0),
                    )
                }
            };
            assert!(crate::test_support::with_decode_context(compare).unwrap());
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                operation,
                0,
                compare,
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
        }
    }
}
