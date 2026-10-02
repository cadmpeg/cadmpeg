// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::{PcurveNurbs, PolarPcurveNurbs},
};
use crate::math::{Point2, Point3};

pub(crate) fn curve() -> NurbsCurve {
    NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        1,
        vec![2.0, 2.0, 5.0, 5.0],
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        Some(vec![-1.0, 2.0]),
        true,
    ).expect("fixture constructor admission")
    .unwrap()
}

pub(crate) fn surface() -> NurbsSurface {
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        crate::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], true),
        crate::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 5.0, 5.0], false),
        crate::geometry::nurbs::NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            Some(vec![-1.0, 1.0, 2.0, -2.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        true,
    ).expect("fixture constructor admission")
    .unwrap()
}

pub(crate) fn pcurve() -> PcurveNurbs {
    PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        1,
        vec![2.0, 2.0, 5.0, 5.0],
        vec![Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)],
        Some(vec![1.0, 2.0]),
        true,
    ).expect("fixture pcurve construction admission")
    .unwrap()
}

pub(crate) fn polar() -> PolarPcurveNurbs {
    PolarPcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        1,
        vec![2.0, 2.0, 5.0, 5.0],
        vec![
            crate::geometry::pcurve::PolarNurbsPole {
                radial: Point2::new(1.0, 2.0),
                axial: 5.0,
            },
            crate::geometry::pcurve::PolarNurbsPole {
                radial: Point2::new(3.0, 4.0),
                axial: 6.0,
            },
        ],
        Some(vec![1.0, 2.0]),
        true,
    ).expect("fixture pcurve construction admission")
    .unwrap()
}
