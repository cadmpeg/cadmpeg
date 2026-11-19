// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::math::portable::Portable;

use super::circular_cone_normal_curvature;

#[test]
fn a_circular_cone_generatrix_is_straight() {
    assert_eq!(circular_cone_normal_curvature(1, 1.0, 2.0, 0.5), Some(0.0));
}

#[test]
fn a_circular_cone_parallel_uses_the_section_radius() {
    let radius = 2.0_f64;
    let half_angle = 0.5_f64;
    let v = 1.5_f64;
    let rho = radius + v * half_angle.tan();
    let expected = half_angle.cos().abs() / rho.abs();
    assert!(floats_agree(
        circular_cone_normal_curvature(0, v, radius, half_angle).expect("finite"),
        expected
    ));
}

#[test]
fn a_circular_cone_apex_has_no_finite_parallel_curvature() {
    let radius = 2.0_f64;
    let half_angle = 0.5_f64;
    let apex = -radius / half_angle.portable_tan();
    assert_eq!(
        circular_cone_normal_curvature(0, apex, radius, half_angle),
        None
    );
}

#[test]
fn a_cone_apex_has_no_normal_even_when_its_parameter_rounds() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::geometry::analytic::ConeSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let surface = &mut ir.geometry.surfaces[0];
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            15.0,
            1.0,
            0.5_f64.atan(),
        )
        .unwrap(),
    ));
    let id = surface.id.clone();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    assert!(carrier.normal([0.0, -30.0]).unwrap().is_none());
    assert!(carrier.normal([0.0, -29.0]).unwrap().is_some());
}

/// A model whose only surface is a procedural sweep of a straight NURBS
/// directrix, with an unknown solved cache.
fn swept(revolution: bool) -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::SurfaceId) {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::surface_payloads::{
        AxisRevolutionSurfaceConstruction, LinearSweepSurfaceConstruction,
    };
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
        SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let curve = CurveId::mint("test:model:curve#directrix").unwrap();
    let surface = SurfaceId::mint("test:model:surface#swept").unwrap();
    let construction = ProceduralSurfaceId::mint("test:model:surface-construction#swept").unwrap();
    let (poles, definition) = if revolution {
        (
            vec![Point3::new(5.0, 0.0, 0.0), Point3::new(5.0, 0.0, 2.0)],
            ProceduralSurfaceDefinition::AxisRevolution(
                AxisRevolutionSurfaceConstruction::try_new(
                    curve.clone(),
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
        )
    } else {
        (
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)],
            ProceduralSurfaceDefinition::LinearSweep(
                LinearSweepSurfaceConstruction::try_new(curve.clone(), Vector3::new(0.0, 0.0, 2.0))
                    .unwrap(),
            ),
        )
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.geometry.curves.push(Curve {
        id: curve,
        source_object: None,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0], poles, None, false).unwrap(),
        )),
        native_ref: None,
    });
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: Some(SolvedSurfaceGeometry::Unknown { record: None }),
        },
        native_ref: None,
    });
    ir.geometry
        .procedural_surfaces
        .push(ProceduralSurface::new(construction, definition, None));
    (ir, surface)
}

#[test]
fn a_linear_sweep_inverts_a_point_with_no_seed() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::math::Point3;

    let (ir, id) = swept(false);
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(100_000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    let uv = carrier
        .invert(Point3::new(2.0, 0.0, 3.0), None)
        .unwrap()
        .expect("the point lies on the sweep");
    assert!(floats_agree(uv[0], 0.5), "{uv:?}");
    assert!(floats_agree(uv[1], 1.5), "{uv:?}");
}

#[test]
fn a_revolution_inverts_a_point_with_no_seed() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::math::Point3;
    use std::f64::consts::FRAC_PI_2;

    let (ir, id) = swept(true);
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(100_000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    let uv = carrier
        .invert(Point3::new(0.0, 5.0, 1.0), None)
        .unwrap()
        .expect("the point lies on the revolution");
    assert!(floats_agree(uv[0], FRAC_PI_2), "{uv:?}");
    assert!(floats_agree(uv[1], 0.5), "{uv:?}");
}

#[test]
fn a_rectangular_trim_bounds_its_carrier_domain() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::geometry::surface_payloads::SubsetSurfaceConstruction;
    use cadmpeg_ir::geometry::{
        ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface,
        SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::index::ModelIndex;

    let (mut ir, support) = swept(false);
    let trimmed = SurfaceId::mint("test:model:surface#trimmed").unwrap();
    let construction =
        ProceduralSurfaceId::mint("test:model:surface-construction#trimmed").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: trimmed.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: Some(SolvedSurfaceGeometry::Unknown { record: None }),
        },
        native_ref: None,
    });
    ir.geometry.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::Subset(
            SubsetSurfaceConstruction::try_new(
                support,
                [[0.25, 0.75], [2.0, 0.5]],
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        None,
    ));
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1000);
    let carrier = super::Carrier::new(&index, &work, &trimmed).unwrap();
    assert_eq!(carrier.domain, Some([[0.0, 0.5], [0.0, 1.5]]));
}

#[test]
fn a_periodic_nurbs_inversion_continues_across_the_seam() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::math::Point3;

    // A closed cubic profile in `u` over the domain `[0, 4]`, swept along
    // `z` in `v`: the last three pole rows repeat the first three.
    let profile = [
        (1.0, 0.0),
        (0.0, 1.0),
        (-1.0, 0.0),
        (0.0, -1.0),
        (1.0, 0.0),
        (0.0, 1.0),
        (-1.0, 0.0),
    ];
    let rows = profile
        .iter()
        .map(|&(x, y)| vec![Point3::new(x, y, 0.0), Point3::new(x, y, 1.0)])
        .collect();
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let surface = &mut ir.geometry.surfaces[0];
    surface.geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::new(
            NurbsSurfaceAxis::new(
                3,
                vec![-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
                true,
            ),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsPoleGrid::from_lanes(rows, None).unwrap(),
            false,
        )
        .unwrap(),
    ));
    let id = surface.id.clone();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(10_000_000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    assert_eq!(carrier.periods[0], Some(4.0));
    let point = carrier.point([0.3, 0.5]).unwrap().expect("a surface point");
    let uv = carrier
        .invert(point, Some([3.9, 0.5]))
        .unwrap()
        .expect("the point inverts");
    assert!(floats_agree(uv[0], 4.3), "{uv:?}");
    assert!(floats_agree(uv[1], 0.5), "{uv:?}");
    // Past the seam the carrier repeats, where the NURBS would extrapolate.
    let wrapped = carrier.point([4.3, 0.5]).unwrap().expect("a surface point");
    assert!(wrapped.distance(point) < 1.0e-12, "{wrapped:?} {point:?}");
    // With no seed, a point just short of the seam inverts to itself.
    for u in [0.02, 3.98] {
        let point = carrier.point([u, 0.25]).unwrap().expect("a surface point");
        let uv = carrier
            .invert(point, None)
            .unwrap()
            .expect("the point inverts");
        let found = carrier.point(uv).unwrap().expect("a surface point");
        assert!(found.distance(point) < 1.0e-9, "{u}: {uv:?}");
    }
}

/// A smooth strip folded back and forth 16 times along `u`: its folds lie
/// closer together than the steps of a coarse start grid.
fn accordion() -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::SurfaceId) {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    let poles = 33_u8;
    let rows = (0..poles)
        .map(|pole| {
            let x = f64::from(pole) / f64::from(poles - 1);
            let y = if pole % 2 == 0 { 1.0 } else { -1.0 };
            vec![Point3::new(x, y, 0.0), Point3::new(x, y, 1.0)]
        })
        .collect();
    let spans = poles - 2;
    let mut u_knots = vec![0.0, 0.0];
    u_knots.extend((0..=spans).map(|step| f64::from(step) / f64::from(spans)));
    u_knots.extend([1.0, 1.0]);
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let surface = &mut ir.geometry.surfaces[0];
    surface.geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::new(
            NurbsSurfaceAxis::new(2, u_knots, false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsPoleGrid::from_lanes(rows, None).unwrap(),
            false,
        )
        .unwrap(),
    ));
    let id = surface.id.clone();
    (ir, id)
}

#[test]
fn an_unseeded_inversion_finds_its_fold_of_an_accordion() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::index::ModelIndex;

    let (ir, id) = accordion();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(10_000_000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    for u in [0.3137, 0.5068] {
        let point = carrier.point([u, 0.4]).unwrap().expect("a surface point");
        let uv = carrier
            .invert(point, None)
            .unwrap()
            .expect("the point inverts");
        let found = carrier.point(uv).unwrap().expect("a surface point");
        assert!(found.distance(point) < 1.0e-9, "{u}: {uv:?}");
    }
}

/// A seed on the first fold stops a projection on a fold between it and
/// the point; the inversion starts again with no seed and reaches it.
#[test]
fn a_seeded_inversion_that_stops_short_starts_again() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::index::ModelIndex;

    let (ir, id) = accordion();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(10_000_000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    let point = carrier
        .point([0.5068, 0.4])
        .unwrap()
        .expect("a surface point");
    let uv = carrier
        .invert(point, Some([0.0, 0.4]))
        .unwrap()
        .expect("the point inverts");
    let found = carrier.point(uv).unwrap().expect("a surface point");
    assert!(found.distance(point) < 1.0e-9, "{uv:?}");
}

/// A surface point or partial evaluated once is read back for the same
/// parameters without charging evaluation work again.
#[test]
fn a_repeated_evaluation_spends_no_work() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::index::ModelIndex;

    let (ir, id) = accordion();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(10_000_000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    let uv = [0.3137, 0.4];
    let point = carrier.point(uv).unwrap();
    let partials = carrier.partials(uv).unwrap();
    let spent = work.remaining();
    assert_eq!(carrier.point(uv).unwrap(), point);
    assert_eq!(carrier.partials(uv).unwrap(), partials);
    assert_eq!(work.remaining(), spent);
}
