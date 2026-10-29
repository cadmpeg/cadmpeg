// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::Approximator;
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry, PlacedPcurve};
use crate::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
    ProceduralCurveDefinition, SupportPcurve, SurfaceCurveFamily, MAX_GEOMETRY_NESTING,
};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::math::Point2;
use crate::transform::Transform2;

#[test]
fn fitting_retains_an_evaluation_resource_refusal() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let mut pcurve = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
    );
    for _ in 0..MAX_GEOMETRY_NESTING {
        pcurve = PcurveGeometry::Transformed(
            PlacedPcurve::try_new(Box::new(pcurve), Transform2::identity()).unwrap(),
        );
    }
    let curve = CurveId::mint("test:approximation:curve#deep").unwrap();
    let construction = ProceduralCurveId::mint("test:approximation:construction#deep").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(ir.model.faces[0].surface.clone()),
                pcurve: Some(SupportPcurve::new(pcurve, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });
    let mut approximator = Approximator::new(&ir);
    assert!(approximator.curve(curve.as_str()).is_none());
    let refusal = approximator
        .resource_limit()
        .expect("recursion refusal is retained");
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
}
