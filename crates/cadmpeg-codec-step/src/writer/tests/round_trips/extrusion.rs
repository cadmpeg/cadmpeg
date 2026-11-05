// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn an_extrusion_whose_magnitude_overflows_uses_held_carrier() {
    use cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction;
    use cadmpeg_ir::geometry::{ProceduralSurface, ProceduralSurfaceDefinition};
    use cadmpeg_ir::report::Severity;

    for held_cache in [false, true] {
        let mut ir = CadIr::empty();
        let directrix: CurveId = "test:model:curve#directrix".try_into().unwrap();
        ir.model.curves.push(Curve {
            native_ref: None,
            id: directrix.clone(),
            parameter_range: None,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
        });
        let definition = ExtrusionSurfaceConstruction::legacy(
            directrix,
            None,
            cadmpeg_ir::features::FiniteVector3::new(Vector3::new(1.3e308, 1.3e308, 0.0)).unwrap(),
            None,
            None,
        );
        let construction = ProceduralSurface::new(
            "test:model:procedural-surface#extrusion"
                .try_into()
                .unwrap(),
            ProceduralSurfaceDefinition::Extrusion(definition),
            None,
        );
        ir.model.surfaces.push(Surface {
            native_ref: None,
            id: "test:model:surface#extrusion".try_into().unwrap(),
            source_object: None,
            geometry: SurfaceGeometry::Procedural {
                construction: construction.id.clone(),
                cache: held_cache.then(|| {
                    SolvedSurfaceGeometry::Plane(
                        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .unwrap(),
                    )
                }),
            },
        });
        ir.model.procedural_surfaces.push(construction);
        let mut bytes = Vec::new();
        let report = write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .expect("entity degradation still writes valid STEP");
        let (code, severity) = if held_cache {
            (StepLossCode::ProceduralReducedToCarrier, Severity::Info)
        } else {
            (
                StepLossCode::ProceduralDefinitionNotWritten,
                Severity::Error,
            )
        };
        assert!(report.losses.iter().any(|loss| {
            loss.code == code.kind()
                && loss.severity == severity
                && loss
                    .message
                    .contains("test:model:procedural-surface#extrusion")
        }));
        let text = String::from_utf8(bytes).expect("STEP text");
        assert!(!text.contains("inf"));
        assert_eq!(text.contains("PLANE("), held_cache);
        assert!(!text.contains("SURFACE_OF_LINEAR_EXTRUSION("));
    }
}
