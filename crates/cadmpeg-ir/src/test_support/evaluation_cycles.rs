// SPDX-License-Identifier: Apache-2.0
//! A cyclic model shared by validator and sealed decode tests.

use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry, TolerantIntersectionConstruction, TolerantIntersectionParameterization,
};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::CadIr;

const EPS_INTERSECTION_FIXTURE: f64 = 1.0e-9;

pub(crate) fn cyclic_model() -> (CadIr, CurveId, SurfaceId) {
    let support = SurfaceId::mint("test:model:surface#first").expect("valid identity");
    let other = SurfaceId::mint("test:model:surface#second").expect("valid identity");
    let curve = CurveId::mint("test:model:curve#intersection").expect("valid identity");
    let pcurve = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
            .expect("line pcurve fixture"),
    );
    let mut ir = CadIr::empty();
    for (id, normal) in [
        (support.clone(), Vector3::new(0.0, 0.0, 1.0)),
        (other.clone(), Vector3::new(0.0, -1.0, 0.0)),
    ] {
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    normal,
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("plane fixture"),
            )),
            source_object: None,
        });
    }
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("line curve fixture"),
        )),
        source_object: None,
    });
    ir.model
        .add_procedural_curve(
            curve.clone(),
            ProceduralCurve::new(
                ProceduralCurveId::mint("test:model:procedural#intersection")
                    .expect("valid identity"),
                ProceduralCurveDefinition::TolerantIntersection {
                    construction: TolerantIntersectionConstruction::try_new(
                        [support.clone(), other],
                        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                        EPS_INTERSECTION_FIXTURE,
                    )
                    .expect("tolerant intersection fixture"),
                    parameterization: Some(
                        TolerantIntersectionParameterization::try_new(
                            [pcurve.clone(), pcurve],
                            [-1.0, 1.0],
                        )
                        .expect("parameterization fixture"),
                    ),
                    cache: None,
                },
            ),
        )
        .expect("procedural curve fixture");
    ir.model
        .add_procedural_surface(
            support.clone(),
            ProceduralSurface::new(
                ProceduralSurfaceId::mint("test:model:procedural#cyclic-support")
                    .expect("valid identity"),
                ProceduralSurfaceDefinition::AxisRevolution(
                    crate::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::try_new(
                        curve.clone(),
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .expect("axis revolution fixture"),
                ),
                None,
            ),
        )
        .expect("procedural surface fixture");
    (ir, curve, support)
}
