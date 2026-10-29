// SPDX-License-Identifier: Apache-2.0

use crate::eval::admission::EvaluationAdmission;
use crate::eval::model_surface_point;
use crate::eval::model_surface_point_by_id;
use crate::eval::model_surface_point_in_index;
use crate::eval::EvaluationFailure;
use crate::geometry::Curve;
use crate::geometry::CurveGeometry;
use crate::geometry::ProceduralSurface;
use crate::geometry::ProceduralSurfaceDefinition;
use crate::geometry::SolvedCurveGeometry;
use crate::geometry::Surface;
use crate::geometry::SurfaceGeometry;
use crate::ids::CurveId;
use crate::ids::ProceduralSurfaceId;
use crate::ids::SurfaceId;
use crate::math::Point3;
use crate::math::Vector3;
use crate::CadIr;

#[test]
fn procedural_surface_points_read_the_callers_index() {
    // Indexing every entity again per point would be 2^29 insertions.
    const POINTS: usize = 1 << 17;
    const EVALUATIONS: usize = 1 << 12;
    let directrix_id = CurveId::mint("test:model:entity#indexed-profile").unwrap();
    let surface_id = SurfaceId::mint("test:model:entity#indexed-revolution").unwrap();
    // A construction that the by-id evaluator hands to the model surface
    // point, which has no value for it.
    let unknown_id = SurfaceId::mint("test:model:entity#indexed-unknown").unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: directrix_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(2.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let mut cacheless =
        |surface: &SurfaceId, construction: &str, definition: ProceduralSurfaceDefinition| {
            let construction = ProceduralSurfaceId::mint(construction).unwrap();
            ir.model.surfaces.push(Surface {
                native_ref: None,
                id: surface.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: construction.clone(),
                    cache: None,
                },
                source_object: None,
            });
            ir.model
                .add_procedural_surface(
                    surface,
                    procedural_surface! {
                        id: construction,
                        definition: definition,
                        cache_fit_tolerance: None,
                        record_bounds: None,
                    },
                )
                .unwrap();
        };
    cacheless(
        &surface_id,
        "test:model:entity#indexed-construction",
        ProceduralSurfaceDefinition::AxisRevolution(
            crate::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::try_new(
                directrix_id,
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
    );
    cacheless(
        &unknown_id,
        "test:model:entity#indexed-unknown-construction",
        ProceduralSurfaceDefinition::Unknown {
            record: None,
            cache: None,
        },
    );
    for index in 0..POINTS {
        ir.model.points.push(crate::topology::Point::new(
            crate::ids::PointId::mint(format!("test:model:point#{index}")).unwrap(),
            crate::features::FinitePoint3::ZERO,
            None,
        ));
    }
    let revolution = &ir.model.surfaces[0].geometry;
    let expected = model_surface_point(
        EvaluationAdmission::Standard,
        &ir,
        revolution,
        std::f64::consts::FRAC_PI_2,
        1.5,
    )
    .unwrap();
    let index = crate::index::ModelIndex::build(&ir);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..EVALUATIONS {
        assert_eq!(
            model_surface_point_by_id(
                EvaluationAdmission::Decode(&ctx),
                &index,
                &unknown_id,
                0.5,
                0.5,
            ),
            Err(EvaluationFailure::NoValue)
        );
        assert_eq!(
            model_surface_point_in_index(
                EvaluationAdmission::Decode(&ctx),
                &index,
                revolution,
                std::f64::consts::FRAC_PI_2,
                1.5,
            ),
            Ok(expected)
        );
    }
    ctx.finish_session().unwrap();
}
