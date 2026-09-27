// SPDX-License-Identifier: Apache-2.0
//! Parameter-lane equivalence for offset supports.

use crate::decode::support_uv::{
    complete_parameterization_equivalent_support_uv, parameterization_equivalent_surfaces,
};
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, Curve, CurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_test_support::edit;

#[test]
fn equivalent_offset_supports_share_a_complete_parameter_lane() {
    use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let supports = [
        SurfaceId::mint("test:model:entity#support-a").expect("identity grammar"),
        SurfaceId::mint("test:model:entity#support-b").expect("identity grammar"),
    ];
    for support in &supports {
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
    }
    let offsets = [
        SurfaceId::mint("test:model:entity#offset-a").expect("identity grammar"),
        SurfaceId::mint("test:model:entity#offset-b").expect("identity grammar"),
    ];
    for (ordinal, (surface, support)) in offsets.iter().zip(&supports).enumerate() {
        let construction =
            ProceduralSurfaceId::mint(format!("test:model:entity#offset-construction-{ordinal}"))
                .expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            construction,
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    support.clone(),
                    30.0,
                    Some(0),
                    Some(0),
                    false,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy {
                        flags: cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                        cache: None,
                    },
                )
                .unwrap(),
            ),
            None,
        ));
    }
    let carrier = CurveId::mint("test:model:entity#curve").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: carrier.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        carrier,
        ProceduralCurve::new(
            ProceduralCurveId::mint("test:model:entity#intersection").expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                    [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(offsets[0].clone()),
                            pcurve: None,
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(offsets[1].clone()),
                            pcurve: Some(
                                PcurveGeometry::Line(
                                    cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                                        Point2::new(1.0, 2.0),
                                        Point2::new(3.0, 4.0),
                                    )
                                    .unwrap(),
                                )
                                .into(),
                            ),
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

    assert!(parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    complete_parameterization_equivalent_support_uv(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves[0].definition()
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides()[0].pcurve, context.sides()[1].pcurve);

    ir.model.procedural_surfaces[1].edit_definition(|definition| {
        if let ProceduralSurfaceDefinition::Offset(definition_payload) = definition {
            {
                let replacement = true;
                edit::replace(definition_payload, |previous| {
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                        previous.support().clone(),
                        previous.distance().get(),
                        *previous.u_sense(),
                        *previous.v_sense(),
                        replacement,
                        previous.extension().to_raw(),
                    )
                })
                .unwrap();
            };
        }
    });
    assert!(!parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    ir.model.procedural_surfaces[1].edit_definition(|definition| {
        if let ProceduralSurfaceDefinition::Offset(definition_payload) = definition {
            {
                let replacement = false;
                edit::replace(definition_payload, |previous| {
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                        previous.support().clone(),
                        previous.distance().get(),
                        *previous.u_sense(),
                        *previous.v_sense(),
                        replacement,
                        previous.extension().to_raw(),
                    )
                })
                .unwrap();
            };
            {
                let replacement = 31.0;
                edit::replace(definition_payload, |previous| {
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                        previous.support().clone(),
                        replacement,
                        *previous.u_sense(),
                        *previous.v_sense(),
                        previous.linear_support_extension(),
                        previous.extension().to_raw(),
                    )
                })
            }
            .unwrap();
        }
    });
    assert!(!parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
}
