// SPDX-License-Identifier: Apache-2.0

use super::admission_pcurve;
use crate::decode::source_carriers::SourceUnitCarriers;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

#[test]
fn pcurve_normalization_propagates_owned_scaling_work_refusal() {
    let mut pcurve = admission_pcurve();
    pcurve.geometry = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![cadmpeg_ir::math::Point2::new(1.0, 2.0); 2],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .expect("curve"),
    };
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "IR pcurve pole coordinate scaling work",
        |ctx| {
            let mut ir = CadIr::empty();
            let result =
                SourceUnitCarriers::push_pcurve(ctx, &mut ir, pcurve.clone(), Some([2.0, 3.0]));
            if result.is_err() {
                assert!(ir.model.pcurves.is_empty());
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "IR pcurve pole coordinate scaling work"));
    let mut ir = CadIr::empty();
    let mut expected = pcurve.clone();
    expected
        .geometry
        .try_scale_coordinates([2.0, 3.0])
        .expect("reference");
    crate::decode::with_test_decode_ctx(|ctx| {
        SourceUnitCarriers::push_pcurve(ctx, &mut ir, pcurve, Some([2.0, 3.0]))
    })
    .expect("service");
    assert_eq!(ir.model.pcurves, vec![expected]);
}
