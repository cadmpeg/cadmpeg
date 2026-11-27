// SPDX-License-Identifier: Apache-2.0
//! Support-UV completion lane budgets.

use crate::decode::support_uv::SerializedSupportUv;
use cadmpeg_ir::geometry::intersection_curves::{IntcurveSupportContext, IntcurveSupportSide};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralCurve, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;

#[test]
fn a_support_uv_lane_that_spends_its_work_leaves_a_later_lane_its_own() {
    const FIT_TOLERANCE: f64 = 1.0e-9;
    const LANE_WORK: usize = 64;

    crate::test_support::with_decode_context(|geometry_ctx| {
        let surface_id = SurfaceId::mint("test:model:entity#synthetic:lane-work-surface")
            .expect("identity grammar");
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        ir.geometry.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                NurbsSurface::from_lanes(
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                        vec![
                            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0)],
                            vec![Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)],
                        ],
                        None,
                    ),
                    false,
                )
                .expect("valid lane-work surface"),
            )),
            source_object: None,
            native_ref: None,
        });
        let mut procedural_ids = Vec::new();
        for name in ["off-surface", "on-surface"] {
            let curve_id = CurveId::mint(format!(
                "test:model:entity#synthetic:lane-work-{name}-curve"
            ))
            .expect("identity grammar");
            let procedural_id =
                ProceduralCurveId::mint(format!("test:model:entity#synthetic:lane-work-{name}"))
                    .expect("identity grammar");
            ir.geometry.curves.push(Curve {
                parameter_range: None,
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
                native_ref: None,
            });
            let _attached = ir.geometry.add_procedural_curve(
                &curve_id,
                ProceduralCurve::new(
                    procedural_id.clone(),
                    ProceduralCurveDefinition::Intersection {
                        context: IntcurveSupportContext::try_new(
                            [
                                IntcurveSupportSide {
                                    surface: Some(surface_id.clone()),
                                    pcurve: None,
                                },
                                IntcurveSupportSide {
                                    surface: None,
                                    pcurve: None,
                                },
                            ],
                            [0.0, 1.0],
                            [Vec::new(), Vec::new(), Vec::new()],
                        )
                        .unwrap(),
                        discontinuity_flag: false,
                        cache: None,
                    },
                ),
            );
            procedural_ids.push(procedural_id);
        }

        let parameters = [Point2::new(0.2, 0.3), Point2::new(0.7, 0.8)];
        let index = cadmpeg_ir::index::ModelIndex::build(&ir);
        let on_surface = parameters.map(|parameter| {
            cadmpeg_ir::eval::model_surface_point_by_id(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &index,
                &surface_id,
                parameter.u,
                parameter.v,
            )
            .expect("NURBS chart point")
            .get()
        });
        drop(index);
        let off_surface = on_surface.map(|point| Point3::new(point.x, point.y, point.z + 5.0));
        let lane = |procedural_id: &ProceduralCurveId, points: [Point3; 2], serialized| {
            (
                procedural_id.clone(),
                crate::intersection::chart_samples::ChartSamples::from_test_values(
                    points.to_vec(),
                    vec![0.0, 1.0],
                )
                .unwrap(),
                FIT_TOLERANCE,
                serialized,
            )
        };
        let pending = vec![
            lane(
                &procedural_ids[0],
                off_surface,
                SerializedSupportUv::default(),
            ),
            lane(
                &procedural_ids[1],
                on_surface,
                SerializedSupportUv::from_values([
                    Some(
                        parameters
                            .map(|parameter| [parameter.u, parameter.v])
                            .to_vec(),
                    ),
                    None,
                ]),
            ),
        ];
        let lane_work =
            crate::decode::geometry_work::LaneGeometryWork::from_context(geometry_ctx, LANE_WORK);
        let complete = |pending: &[_]| {
            let mut ir = ir.clone();
            let (exhausted, work) = cadmpeg_core::decode::test_support::work_charged(|| {
                crate::decode::support_uv::complete_support_uv_with_budget(
                    &mut ir, pending, &lane_work, &lane_work,
                )
            });
            (ir, exhausted.unwrap(), work)
        };
        let (_, off_exhausted, off_work) = complete(&pending[..1]);
        let (_, _, on_work) = complete(&pending[1..]);
        let (ir, exhausted, work) = complete(&pending);

        assert!(off_exhausted);
        assert!(off_work + on_work > u64::try_from(LANE_WORK).unwrap());
        assert!(exhausted);
        // The completed lane fills no side that the failed lane reads, so the
        // second wave does not attempt the failed lane again.
        assert_eq!(work, off_work + on_work);
        let completed = |index: usize| {
            let ProceduralCurveDefinition::Intersection { context, .. } =
                ir.geometry.procedural_curves[index].definition()
            else {
                panic!("intersection");
            };
            context.sides()[0].pcurve.is_some()
        };
        assert!(!completed(0));
        assert!(completed(1));
        assert!(geometry_ctx.resource_refusal().is_none());
    });
}
