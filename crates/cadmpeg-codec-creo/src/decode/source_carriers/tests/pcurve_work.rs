// SPDX-License-Identifier: Apache-2.0

use super::admission_pcurve;
use crate::decode::source_carriers::SourceUnitCarriers;
use cadmpeg_ir::document::CadIr;

#[test]
fn pcurve_normalization_scales_owned_nurbs_coordinates() {
    let mut pcurve = admission_pcurve();
    pcurve.geometry = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![cadmpeg_ir::math::Point2::new(1.0, 2.0); 2],
            None,
            false,
        )
        .expect("curve"),
    };
    let mut ir = CadIr::empty();
    let mut expected = pcurve.clone();
    expected
        .geometry
        .try_scale_coordinates([2.0, 3.0])
        .expect("reference");
    crate::decode::with_service_context(|ctx| {
        SourceUnitCarriers::push_pcurve(ctx, &mut ir, pcurve, Some([2.0, 3.0]))
    })
    .expect("service");
    assert_eq!(ir.model.pcurves, vec![expected]);
}
