// SPDX-License-Identifier: Apache-2.0
use crate::geometry::surface_payloads::ExtrusionSurfaceConstruction;
use crate::geometry::{
    nurbs::NurbsCurve, CacheContract, Curve, CurveGeometry, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::CadIr;

fn line_in_nurbs_carrier() -> (CadIr, CurveId) {
    let curve_id = CurveId::mint("test:model:curve#construction-parameter").unwrap();
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1e200, 1e200],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let mut ir = CadIr::empty();
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        source_object: None,
        native_ref: None,
    });
    (ir, curve_id)
}

#[test]
fn construction_mapping_refuses_nonfinite_widths_and_derivatives() {
    let (ir, id) = line_in_nurbs_carrier();
    let index = crate::index::ModelIndex::build(&ir);
    assert_eq!(
        super::super::construction_curve_parameter(
            crate::eval::admission::EvaluationAdmission::Standard,
            &index,
            &id,
            5e-301,
            crate::scalar::FiniteReal::array([0.0, 1e308]),
            crate::scalar::FiniteReal::array([0.0, 1e-300]),
            false,
        ),
        Err(crate::eval::EvaluationFailure::NonFinite(()))
    );
    assert_eq!(
        super::super::construction_curve_parameter(
            crate::eval::admission::EvaluationAdmission::Standard,
            &index,
            &id,
            0.0,
            crate::scalar::FiniteReal::array([-1e308, 1e308]),
            None,
            false,
        ),
        Err(crate::eval::EvaluationFailure::NonFinite(()))
    );
}

#[test]
fn extrusion_partials_preserve_zero_acceleration_at_large_parameter_scale() {
    let (ir, id) = line_in_nurbs_carrier();
    let index = crate::index::ModelIndex::build(&ir);
    let direction = Vector3::new(0.0, 0.0, 1.0);
    let extrusion = |direction| {
        ExtrusionSurfaceConstruction::try_new(
            id.clone(),
            Some([0.0, 1e200]),
            direction,
            None,
            CacheContract::from_form(None),
        )
    };
    let partials = super::super::model_native_extrusion_jet(
        crate::eval::admission::EvaluationAdmission::Standard,
        &index,
        &extrusion(direction).unwrap(),
        crate::scalar::FiniteReal::array([0.0, 1.0]),
        0.5,
        0.0,
    )
    .and_then(super::super::SurfaceJet::second_partials)
    .unwrap()
    .into_raw();
    assert!((partials.point.x - 0.5).abs() <= 8.0 * f64::EPSILON);
    assert!((partials.du.x - 1.0).abs() <= 8.0 * f64::EPSILON);
    assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(partials.dv, direction);
    assert!(extrusion(Vector3::new(f64::NAN, 0.0, 1.0)).is_err());
}

#[test]
fn extrusion_breaks_lie_at_the_directrix_knots_inside_the_domain_past_an_unknown_cache() {
    let directrix = CurveId::mint("test:model:curve#square").unwrap();
    let square = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0],
        vec![
            Point3::new(-10.0, -10.0, 0.0),
            Point3::new(10.0, -10.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(-10.0, 10.0, 0.0),
            Point3::new(-10.0, -10.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    let surface = SurfaceId::mint("test:model:surface#square-extrusion").unwrap();
    let construction = ProceduralSurfaceId::mint("test:model:procedural#square-extrusion").unwrap();
    let mut ir = CadIr::empty();
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(square)),
        source_object: None,
        native_ref: None,
    });
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: Some(SolvedSurfaceGeometry::Unknown { record: None }),
        },
        source_object: None,
        native_ref: None,
    });
    ir.geometry.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                directrix,
                None,
                Vector3::new(0.0, 0.0, -1.0),
                None,
                CacheContract::from_form(None),
            )
            .unwrap(),
        ),
        None,
    ));
    let index = crate::index::ModelIndex::build(&ir);
    let breaks = |u: [f64; 2]| {
        super::super::procedural_surface_breaks(&index, &surface, [u, [-5.0, 0.0]]).unwrap()
    };
    assert_eq!(breaks([0.0, 3.0]), [vec![1.0, 2.0], Vec::new()]);
    assert_eq!(breaks([0.5, 1.5]), [vec![1.0], Vec::new()]);
    assert_eq!(breaks([1.0, 2.0]), [Vec::<f64>::new(), Vec::new()]);
}
