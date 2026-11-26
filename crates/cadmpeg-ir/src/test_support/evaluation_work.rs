// SPDX-License-Identifier: Apache-2.0
//! A chain of model carriers whose evaluation work doubles at each level.

/// A line, then `levels` tolerant intersections. Each intersection reads
/// two axis revolutions of the curve below, so a point of the top curve
/// reads the bottom line `2^levels` times.
pub(crate) fn doubling_model(levels: usize) -> (crate::CadIr, crate::ids::CurveId) {
    use crate::geometry::intersection_curves::{
        TolerantIntersectionConstruction, TolerantIntersectionParameterization,
    };
    use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use crate::geometry::procedural_curves::ProceduralCurveDefinition;
    use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
    use crate::geometry::surface_payloads::AxisRevolutionSurfaceConstruction;
    use crate::geometry::{
        Curve, CurveGeometry, ProceduralCurve, ProceduralSurface, SolvedCurveGeometry,
        SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use crate::math::{Point2, Point3, Vector3};

    let line = |id: &CurveId| Curve {
        parameter_range: None,
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    };
    let pcurve = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
    );
    let mut ir = crate::CadIr::empty();
    let mut below = CurveId::mint("test:model:curve#0").unwrap();
    ir.geometry.curves.push(line(&below));
    for level in 1..=levels {
        let supports = [0, 1]
            .map(|side| SurfaceId::mint(format!("test:model:surface#{level}-{side}")).unwrap());
        for (side, support) in supports.iter().enumerate() {
            ir.geometry.surfaces.push(Surface {
                id: support.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    crate::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
                native_ref: None,
            });
            ir.geometry
                .add_procedural_surface(
                    support,
                    ProceduralSurface::new(
                        ProceduralSurfaceId::mint(format!(
                            "test:model:procedural#surface-{level}-{side}"
                        ))
                        .unwrap(),
                        ProceduralSurfaceDefinition::AxisRevolution(
                            AxisRevolutionSurfaceConstruction::try_new(
                                below.clone(),
                                Point3::new(0.0, 0.0, 0.0),
                                Vector3::new(0.0, 0.0, 1.0),
                            )
                            .unwrap(),
                        ),
                        None,
                    ),
                )
                .unwrap();
        }
        let curve = CurveId::mint(format!("test:model:curve#{level}")).unwrap();
        ir.geometry.curves.push(line(&curve));
        ir.geometry
            .add_procedural_curve(
                &curve,
                ProceduralCurve::new(
                    ProceduralCurveId::mint(format!("test:model:procedural#curve-{level}"))
                        .unwrap(),
                    ProceduralCurveDefinition::TolerantIntersection {
                        construction: TolerantIntersectionConstruction::try_new(
                            supports,
                            [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                            1.0e-9,
                        )
                        .unwrap(),
                        parameterization: Some(
                            TolerantIntersectionParameterization::try_new(
                                [pcurve.clone(), pcurve.clone()],
                                [-1.0, 1.0],
                            )
                            .unwrap(),
                        ),
                        cache: None,
                    },
                ),
            )
            .unwrap();
        below = curve;
    }
    (ir, below)
}
