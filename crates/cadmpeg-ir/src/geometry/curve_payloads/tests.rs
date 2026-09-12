// SPDX-License-Identifier: Apache-2.0
use super::{ProjectionCurvePayload, SpringCurvePayload, ThreeSurfaceIntersectionCurvePayload};
use crate::geometry::{
    DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide, LinePcurve,
    PcurveGeometry, ProceduralCurveDefinition, ProjectionRole, ProjectionTail, SpringLayout,
    SpringPcurve, SpringSupport, SupportPcurve,
};
use crate::ids::CurveId;
use crate::math::Point2;

fn context(range: [f64; 2]) -> IntcurveSupportContext {
    IntcurveSupportContext::try_new(
        std::array::from_fn(|_| IntcurveSupportSide {
            surface: None,
            pcurve: None,
        }),
        range,
        std::array::from_fn(|_| Vec::new()),
    )
    .unwrap()
}

#[test]
fn third_ranged_pcurve_requires_a_nonzero_context_interval() {
    let third = |ranged: bool| IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
            ),
            ranged.then(|| DirectedParameterRange::new([2.0, -1.0]).unwrap()),
        )),
    };
    assert!(
        ThreeSurfaceIntersectionCurvePayload::try_new(context([1.0, 1.0]), 0, third(false)).is_ok()
    );
    assert!(
        ThreeSurfaceIntersectionCurvePayload::try_new(context([1.0, 1.0]), 0, third(true)).is_err()
    );
    let valid = ProceduralCurveDefinition::ThreeSurfaceIntersection(
        ThreeSurfaceIntersectionCurvePayload::try_new(context([0.0, 1.0]), 0, third(true)).unwrap(),
    );
    let mut wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        valid
    );
    wire["context"]["parameter_range"] = serde_json::json!([1.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(wire).is_err());
}

#[test]
fn projection_payload_admits_finite_ordered_source_intervals() {
    let projection = |range| {
        ProjectionCurvePayload::try_new(
            context([0.0, 0.0]),
            false,
            CurveId::mint("synthetic:test:curve#source").unwrap(),
            ProjectionTail::Ranged {
                flag: false,
                parameter_range: range,
                role: ProjectionRole::Surf1,
            },
        )
    };
    let valid = ProceduralCurveDefinition::Projection(projection([1.0, 1.0]).unwrap());
    let mut wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        valid
    );
    for range in [[2.0, 1.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(projection(range).is_err());
    }
    wire["tail"]["parameter_range"] = serde_json::json!([2.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(wire).is_err());
}

#[test]
fn spring_payload_checks_inline_ranges_and_the_shared_context() {
    let spring = |range, shared| {
        SpringCurvePayload::try_new(
            SpringLayout::ContextFirst {
                supports: std::array::from_fn(|_| SpringSupport::Ranges([range, [0.0, 1.0]])),
                first_pcurve: SpringPcurve::Range(range),
                second_pcurve: None,
                parameter_range: shared,
                discontinuities: std::array::from_fn(|_| Vec::new()),
                discontinuity_flag: false,
                cache: None,
            },
            0,
        )
    };
    let valid = ProceduralCurveDefinition::Spring(spring([1.0, 1.0], [0.0, 0.0]).unwrap());
    let mut wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        valid
    );
    for range in [[2.0, 1.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(spring(range, [0.0, 1.0]).is_err());
        assert!(spring([0.0, 1.0], range).is_err());
    }
    wire["layout"]["first_pcurve"]["value"] = serde_json::json!([2.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(wire).is_err());
}
