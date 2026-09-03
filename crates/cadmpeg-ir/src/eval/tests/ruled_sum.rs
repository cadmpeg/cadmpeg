// SPDX-License-Identifier: Apache-2.0

use super::*;

fn direct_surface_fixture(
    definition: ProceduralSurfaceDefinition,
    surface_name: &str,
) -> (CadIr, SurfaceId) {
    let construction_id = ProceduralSurfaceId(format!("{surface_name}-construction"));
    let surface_id = SurfaceId(surface_name.into());
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: CurveId("first".into()),
            geometry: CurveGeometry::Line {
                origin: Point3::new(1.0, 2.0, 3.0),
                direction: Vector3::new(2.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Curve {
            id: CurveId("second".into()),
            geometry: CurveGeometry::Line {
                origin: Point3::new(5.0, 10.0, 13.0),
                direction: Vector3::new(0.0, 3.0, 0.0),
            },
            source_object: None,
        },
    ];
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction_id.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction_id,
        surface: surface_id.clone(),
        definition: definition,
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    (ir, surface_id)
}

#[test]
fn cacheless_ruled_surface_interpolates_profiles_and_partials() {
    let (ir, surface_id) = direct_surface_fixture(
        ProceduralSurfaceDefinition::Ruled {
            first: CurveId("first".into()),
            second: CurveId("second".into()),
        },
        "ruled",
    );
    let index = crate::index::ModelIndex::new(&ir);
    let point =
        model_surface_point_by_id(&index, &surface_id, 0.25, 0.5).expect("cacheless ruled point");
    assert_eq!(point, Point3::new(3.25, 6.375, 8.0));
    assert_eq!(
        model_surface_point(&ir, &ir.model.surfaces[0].geometry, 0.25, 0.5),
        Some(point)
    );
    let partials = model_surface_second_partials_by_id(&index, &surface_id, 0.25, 0.5)
        .expect("cacheless ruled second partials");
    assert_eq!(partials.point, point);
    assert_eq!(partials.du, Vector3::new(1.0, 1.5, 0.0));
    assert_eq!(partials.dv, Vector3::new(3.5, 8.75, 10.0));
    assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(partials.duv, Vector3::new(-2.0, 3.0, 0.0));
    assert_eq!(partials.dvv, Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn cacheless_sum_surface_adds_independent_curve_parameters() {
    let (ir, surface_id) = direct_surface_fixture(
        ProceduralSurfaceDefinition::Sum {
            first: CurveId("first".into()),
            second: CurveId("second".into()),
            basepoint: Vector3::new(0.5, 1.0, 2.0),
            revision_form: None,
        },
        "sum",
    );
    let index = crate::index::ModelIndex::new(&ir);
    let point =
        model_surface_point_by_id(&index, &surface_id, 0.25, 0.5).expect("cacheless sum point");
    assert_eq!(point, Point3::new(6.0, 12.5, 14.0));
    let partials = model_surface_partials_by_id(&index, &surface_id, 0.25, 0.5)
        .expect("cacheless sum partials");
    assert_eq!(partials.point, point);
    assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 3.0, 0.0));
}
