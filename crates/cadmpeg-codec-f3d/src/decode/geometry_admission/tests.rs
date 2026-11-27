// SPDX-License-Identifier: Apache-2.0

use super::admit;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::geometry::legacy_cache::LegacyCache;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, ProceduralCurve, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::CadIr;

fn exact_curve() -> CadIr {
    let mut ir = CadIr::empty();
    let curve = CurveId::mint("test:brep:curve#0").unwrap();
    let poles = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0].map(|t| Point3::new(100.0 * t, 0.0, 0.0));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                poles.to_vec(),
                None,
                false,
            )
            .unwrap(),
        )),
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });
    ir.geometry
        .add_procedural_curves(vec![(
            curve,
            ProceduralCurve::new(
                ProceduralCurveId::mint("test:brep:construction#0").unwrap(),
                ProceduralCurveDefinition::Exact {
                    cache: Some(LegacyCache::try_new(0.0).unwrap()),
                },
            ),
        )])
        .unwrap()
        .unwrap();
    ir
}

#[test]
fn unresolved_spring_support_keeps_the_solved_cache_and_support_surface() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = exact_curve();
        ir.geometry.surfaces.push(
            json::from_value(cadmpeg_core::json!({
                "id": "test:brep:surface#1",
                "geometry": {"kind": "solved", "value": {"kind": "plane", "origin": {"x":0.0,"y":0.0,"z":0.0},
                    "normal": {"x":0.0,"y":0.0,"z":1.0}, "u_axis":{"x":1.0,"y":0.0,"z":0.0}}}
            }))
            .unwrap(),
        );
        let definition = json::from_value(cadmpeg_core::json!({
            "kind":"spring", "direction":0,
            "layout":{"kind":"cache_first",
                "context":{"sides":[{"surface":"test:brep:surface#1",
                    "pcurve":{"geometry":{"kind":"nurbs", "degree":1,
                        "knots":[0.0,0.0,1.0,1.0], "periodic":false,
                        "poles":{"kind":"polynomial","points":[{"u":4.0,"v":4.0},{"u":6.0,"v":4.0}]}}}},{}],
                    "parameter_range":[0.0,1.0], "discontinuities":[[],[],[]]},
                "form":{"revision":23000,"cache":{"kind":"solved_cache","fit_tolerance":0.0},
                    "support_bounds":[[null,null,null,null],[null,null,null,null]],
                    "solved_range":[null,null],"extension":0}}
        })).unwrap();
        ir.geometry.procedural_curves[0].replace_definition(definition);
        let curves = ir.geometry.curves.clone();
        let surfaces = ir.geometry.surfaces.clone();
        let mut losses = Vec::new();
        admit(ctx, &mut ir, &mut losses).unwrap();
        assert_eq!(ir.geometry.curves, curves);
        assert_eq!(ir.geometry.surfaces, surfaces);
        let ProceduralCurveDefinition::Spring(spring) =
            ir.geometry.procedural_curves[0].definition()
        else {
            panic!("spring construction");
        };
        assert!(spring.support_context().sides()[0].pcurve.is_none());
        assert!(spring.support_context().sides()[0].surface.is_some());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "geometry.spring-support-pcurve-unresolved"
        );
    });
}

#[test]
fn conic_extrusion_unbound_support_has_one_loss_per_native_pcurve() {
    use cadmpeg_ir::geometry::intersection_curves::{IntcurveSupportContext, IntcurveSupportSide};
    use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use cadmpeg_ir::geometry::procedural_curves::{ProceduralCurveDefinition, SupportPcurve};
    use cadmpeg_ir::geometry::ProceduralCurve;
    use cadmpeg_ir::ids::{ProceduralCurveId, SurfaceId};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::CadIr;

    let pcurve = SupportPcurve::from(PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
        .unwrap(),
    });
    for surface_present in [false, true] {
        for pcurve_present in [false, true] {
            let context = IntcurveSupportContext::try_new(
                [
                    IntcurveSupportSide {
                        surface: surface_present
                            .then(|| SurfaceId::mint("test:model:surface#0").unwrap()),
                        pcurve: pcurve_present.then(|| pcurve.clone()),
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    },
                ],
                [0.0, 1.0],
                std::array::from_fn(|_| Vec::new()),
            )
            .unwrap();
            let mut ir = CadIr::empty();
            ir.geometry.procedural_curves.push(ProceduralCurve::new(
                ProceduralCurveId::mint("test:model:construction#0").unwrap(),
                ProceduralCurveDefinition::Intersection {
                    context,
                    discontinuity_flag: false,
                    cache: None,
                },
            ));
            let mut losses = Vec::new();
            let ctx = cadmpeg_core::decode::test_support::service_decode_context();
            admit(&ctx, &mut ir, &mut losses).unwrap();
            assert_eq!(
                losses.len(),
                usize::from(pcurve_present && !surface_present)
            );
            if let Some(loss) = losses.first() {
                assert_eq!(
                    loss.code,
                    crate::loss::F3dLossCode::ProceduralSupportChartUnresolved.kind()
                );
                assert!(loss.message.contains("support side 0"));
            }
        }
    }
}
