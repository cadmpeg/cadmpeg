// SPDX-License-Identifier: Apache-2.0

use crate::eval::model_curve_differential_by_id;
use crate::eval::model_curve_parameter_near_point_with_tolerance;
use crate::eval::model_curve_point_by_id;
use crate::geometry::Curve;
use crate::geometry::CurveGeometry;
use crate::geometry::ProceduralCurve;
use crate::geometry::ProceduralCurveDefinition;
use crate::geometry::SolvedCurveGeometry;
use crate::ids::CurveId;
use crate::math::Point3;
use crate::math::Vector3;
use crate::scalar::NonNegativeLength;
use crate::CadIr;

use crate::ids::ProceduralCurveId;

#[test]
fn cached_subset_retains_local_parameters_for_points_derivatives_and_inversion() {
    let source = CurveId::mint("test:model:curve#source").unwrap();
    let subset = CurveId::mint("test:model:curve#subset").unwrap();
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        crate::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    for sense in [true, false] {
        let mut ir = CadIr::empty();
        for id in [&source, &subset] {
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: line.clone(),
                source_object: None,
            });
        }
        ir.model
            .add_procedural_curve(
                None,
                &subset,
                ProceduralCurve::new(
                    ProceduralCurveId::mint("test:model:procedural-curve#subset").unwrap(),
                    ProceduralCurveDefinition::Subset(
                        crate::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                            source.clone(),
                            [2.0, 5.0],
                            sense,
                            None,
                        )
                        .unwrap(),
                    ),
                ),
            )
            .unwrap()
            .unwrap();
        let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
        let expected = Point3::new(if sense { 3.0 } else { 4.0 }, 0.0, 0.0);
        assert_eq!(
            model_curve_point_by_id(
                crate::eval::admission::EvaluationAdmission::Standard,
                &index,
                &subset,
                1.0
            )
            .map(crate::features::FinitePoint3::get),
            Ok(expected)
        );
        let differential = model_curve_differential_by_id(
            crate::eval::admission::EvaluationAdmission::Standard,
            &index,
            &subset,
            1.0,
        )
        .unwrap();
        assert_eq!(differential.point, expected);
        assert_eq!(
            differential.tangent.unwrap().get(),
            Vector3::new(if sense { 1.0 } else { -1.0 }, 0.0, 0.0)
        );
        assert_eq!(
            differential.acceleration.unwrap().get(),
            Vector3::new(0.0, 0.0, 0.0)
        );
        assert_eq!(
            model_curve_parameter_near_point_with_tolerance(
                &cadmpeg_test_support::service_decode_context(),
                &index,
                &subset,
                expected,
                1.0,
                NonNegativeLength::ZERO,
            )
            .expect("resource allocation did not fail")
            .map(crate::scalar::FiniteReal::get),
            Some(1.0),
        );
        assert_eq!(
            model_curve_point_by_id(
                crate::eval::admission::EvaluationAdmission::Standard,
                &index,
                &subset,
                4.0
            ),
            Err(crate::eval::EvaluationFailure::NoValue)
        );
    }
}

#[test]
fn subset_curve_over_wide_interval_maps_finite_local_parameter() {
    let source = CurveId::mint("test:model:curve#wide-source").unwrap();
    let subset = CurveId::mint("test:model:curve#wide-subset").unwrap();
    let mut ir = CadIr::empty();
    for id in [&source, &subset] {
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                crate::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
    }
    ir.model
        .add_procedural_curve(
            None,
            &subset,
            ProceduralCurve::new(
                ProceduralCurveId::mint("test:model:procedural-curve#wide-subset").unwrap(),
                ProceduralCurveDefinition::Subset(
                    crate::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                        source,
                        [-f64::MAX, f64::MAX],
                        true,
                        None,
                    )
                    .unwrap(),
                ),
            ),
        )
        .unwrap()
        .unwrap();
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    assert_eq!(
        model_curve_point_by_id(
            crate::eval::admission::EvaluationAdmission::Standard,
            &index,
            &subset,
            f64::MAX
        )
        .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.0, 0.0, 0.0))
    );
}

#[test]
fn model_differential_resource_refusal_stops_later_derivatives() {
    use crate::eval::EvaluationFailure;
    use crate::features::{FinitePoint3, FiniteVector3};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    for tangent_refuses in [false, true] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = ctx
            .charge_work_limit(1, "test model derivative")
            .unwrap_err();
        let acceleration_called = std::cell::Cell::new(false);
        let result = crate::eval::differential_at(
            Ok(FinitePoint3::ZERO),
            || {
                if tangent_refuses {
                    Err(EvaluationFailure::ResourceLimit(first))
                } else {
                    Ok(FiniteVector3::ZERO)
                }
            },
            || {
                acceleration_called.set(true);
                Err(EvaluationFailure::ResourceLimit(first))
            },
        );
        let Err(failure) = result else {
            panic!("a derivative resource refusal must leave the model evaluation");
        };
        assert_eq!(failure, EvaluationFailure::ResourceLimit(first));
        assert_eq!(acceleration_called.get(), !tangent_refuses);
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first)
        );
    }
}
