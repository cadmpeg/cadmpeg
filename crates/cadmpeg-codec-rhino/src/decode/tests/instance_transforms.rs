// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

const EPS_PLACED_POINT: f64 = 1.0e-12;

/// `ON_Plane::Transform` maps both plane axes, so a plane surface keeps the
/// point of every `(u, v)` under an orientation-reversing transform.
#[test]
fn plane_surface_keeps_its_parameterization_under_a_reflection() {
    let plane = PlaneSurface::try_new(
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("plane surface");
    let rows = [
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
    ];
    let reflection = Transform::affine(rows).expect("affine reflection");
    let mut surface = Surface {
        id: cadmpeg_ir::ids::SurfaceId::try_from("rhino:test:surface#plane".to_owned())
            .expect("surface id"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)),
        source_object: None,
    };
    super::super::transform_surface(&mut surface, reflection)
        .unwrap_or_else(|_| panic!("plane transform"));
    let SurfaceGeometry::Solved(placed) = &surface.geometry else {
        panic!("solved plane");
    };
    for (u, v) in [(0.0, 0.0), (2.0, 0.0), (0.0, 3.0), (-1.5, 4.0)] {
        let source = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &SolvedSurfaceGeometry::Plane(plane),
            u,
            v,
        )
        .expect("source point")
        .get();
        let expected = Point3::new(
            rows[0][0] * source.x + rows[0][1] * source.y + rows[0][2] * source.z + rows[0][3],
            rows[1][0] * source.x + rows[1][1] * source.y + rows[1][2] * source.z + rows[1][3],
            rows[2][0] * source.x + rows[2][1] * source.y + rows[2][2] * source.z + rows[2][3],
        );
        let actual = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            placed,
            u,
            v,
        )
        .expect("placed point")
        .get();
        assert!(
            actual.distance(expected) <= EPS_PLACED_POINT,
            "u={u} v={v} actual={actual:?} expected={expected:?}"
        );
    }
}

fn curve_id(name: &str) -> cadmpeg_ir::ids::CurveId {
    cadmpeg_ir::ids::CurveId::try_from(format!("rhino:test:curve#{name}")).expect("curve id")
}

/// A sum surface `b + c0(u) + c1(v)` under `x -> Lx + t` keeps its curves'
/// images `L c + t`, so its base point becomes `L b - t`.
#[test]
fn sum_surface_construction_places_its_base_point_under_an_affine_map() {
    use cadmpeg_ir::geometry::surface_payloads::SumSurfaceConstruction;
    use cadmpeg_ir::geometry::{CacheContract, ProceduralSurfaceDefinition};
    let mut definition = ProceduralSurfaceDefinition::Sum(
        SumSurfaceConstruction::try_new(
            curve_id("first"),
            curve_id("second"),
            Vector3::new(1.0, 2.0, 3.0),
            CacheContract::from_form(None),
        )
        .expect("sum construction"),
    );
    let transform = Transform::affine([
        [0.0, -1.0, 0.0, 10.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
    ])
    .expect("affine map");
    assert!(
        super::super::place_surface_construction(&mut definition, transform)
            .unwrap_or_else(|_| panic!("sum placement"))
    );
    let ProceduralSurfaceDefinition::Sum(sum) = definition else {
        panic!("sum construction");
    };
    assert_eq!(sum.basepoint().get(), Vector3::new(-12.0, 1.0, 6.0));
}

/// `ON_RevSurface::Transform` reverses the axis under a reflection, and a
/// non-similar map has no revolution image.
#[test]
fn revolution_construction_follows_a_similarity_and_reverses_under_reflection() {
    use cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction;
    use cadmpeg_ir::geometry::{CacheContract, ProceduralSurfaceDefinition};
    let revolution = || {
        ProceduralSurfaceDefinition::Revolution(
            RevolutionSurfaceConstruction::try_new(
                curve_id("profile"),
                (
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .expect("origin"),
                    cadmpeg_ir::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("axis"),
                ),
                [0.0, std::f64::consts::PI],
                None,
                Some([0.0, 1.0]),
                false,
                CacheContract::from_form(None),
            )
            .expect("revolution construction"),
        )
    };
    let reflection = Transform::affine([
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
    ])
    .expect("reflection");
    let mut definition = revolution();
    assert!(
        super::super::place_surface_construction(&mut definition, reflection)
            .unwrap_or_else(|_| panic!("revolution placement"))
    );
    let ProceduralSurfaceDefinition::Revolution(placed) = &definition else {
        panic!("revolution construction");
    };
    assert_eq!(placed.axis_origin().get(), Point3::new(5.0, 0.0, 0.0));
    assert_eq!(
        *placed.axis_direction().as_raw(),
        Vector3::new(0.0, -1.0, 0.0)
    );

    let stretch = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("stretch");
    let mut definition = revolution();
    assert!(
        !super::super::place_surface_construction(&mut definition, stretch)
            .unwrap_or_else(|_| panic!("revolution placement"))
    );
}
