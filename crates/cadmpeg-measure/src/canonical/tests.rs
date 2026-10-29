// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::geometry::analytic::{ConeSurface, CylinderSurface, SphereSurface, TorusSurface};
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

use super::placement::Placement;
use super::{Carrier, Sources};
use crate::reason::UnavailableReason;
use crate::report::{CarrierParameters, NormalDirection, PlacementClass, TorusForm};
use crate::select::SurfaceKind;
use crate::value::{Method, Quantity, Scalar, SourceExactness};

fn surface(geometry: SolvedSurfaceGeometry) -> Surface {
    Surface {
        native_ref: None,
        id: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#s").unwrap(),
        geometry: SurfaceGeometry::Solved(geometry),
        source_object: None,
    }
}

fn cylinder() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            10.0,
        )
        .unwrap(),
    )
}

fn sources<'a>(carrier: &'a Carrier<'_>, body: Option<&'a str>) -> Sources<'a> {
    Sources {
        fidelity: None,
        surface: carrier.id(),
        depth: carrier.depth,
        body,
    }
}

fn scalar(quantity: &Quantity<Scalar>) -> (f64, Method) {
    let measured = quantity.measured().expect("measured");
    (measured.value().value.get(), measured.method())
}

fn translated(x: f64) -> Transform {
    Transform::affine([
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap()
}

#[test]
fn a_stored_cylinder_reports_source_parameters_with_provenance() {
    let record = surface(cylinder());
    let carrier = Carrier::resolve(&record, Placement::identity()).unwrap();
    assert_eq!(carrier.kind, SurfaceKind::Cylinder);
    assert_eq!(carrier.chain, ["cylinder"]);
    let row = carrier.measures(sources(&carrier, None));
    let CarrierParameters::Cylinder {
        radius,
        diameter,
        axis_point,
        ..
    } = &row.parameters
    else {
        panic!("cylinder parameters");
    };
    assert_eq!(scalar(radius), (10.0, Method::Source));
    assert_eq!(scalar(diameter), (20.0, Method::ClosedForm));
    let provenance = radius.measured().unwrap().provenance();
    assert_eq!(provenance.len(), 1);
    assert_eq!(provenance[0].field(), "geometry.radius");
    assert_eq!(provenance[0].exactness(), SourceExactness::NotRecorded);
    assert_eq!(axis_point.measured().unwrap().method(), Method::Source);
}

#[test]
fn a_translated_placement_computes_points_and_keeps_the_radius_stored() {
    let placed = SolvedSurfaceGeometry::Transformed(
        PlacedSurface::try_new(Box::new(cylinder()), translated(5.0)).unwrap(),
    );
    let record = surface(placed);
    let carrier = Carrier::resolve(&record, Placement::identity()).unwrap();
    assert_eq!(carrier.kind, SurfaceKind::Cylinder);
    assert_eq!(carrier.chain, ["transformed", "cylinder"]);
    let row = carrier.measures(sources(&carrier, None));
    assert_eq!(row.placement.class, PlacementClass::Rigid);
    let CarrierParameters::Cylinder {
        radius, axis_point, ..
    } = &row.parameters
    else {
        panic!("cylinder parameters");
    };
    assert_eq!(scalar(radius), (10.0, Method::Source));
    let fields: Vec<&str> = radius
        .measured()
        .unwrap()
        .provenance()
        .iter()
        .map(crate::value::SourceField::field)
        .collect();
    assert_eq!(fields, ["geometry.basis.radius"]);
    let point = axis_point.measured().unwrap();
    assert_eq!(point.method(), Method::ClosedForm);
    assert_eq!(point.value().value.get().x, 5.0);
    let fields: Vec<&str> = point
        .provenance()
        .iter()
        .map(crate::value::SourceField::field)
        .collect();
    assert_eq!(fields, ["geometry.basis.origin", "geometry.transform"]);
}

#[test]
fn a_body_transform_applies_once_and_names_the_body() {
    let record = surface(cylinder());
    let body = Placement::of(translated(-3.0)).unwrap();
    let carrier = Carrier::resolve(&record, body).unwrap();
    let row = carrier.measures(sources(&carrier, Some("test:model:body#b")));
    let CarrierParameters::Cylinder { axis_point, .. } = &row.parameters else {
        panic!("cylinder parameters");
    };
    let point = axis_point.measured().unwrap();
    assert_eq!(point.value().value.get().x, -3.0);
    let provenance = point.provenance();
    assert_eq!(provenance.last().unwrap().entity(), "test:model:body#b");
    assert_eq!(provenance.last().unwrap().field(), "transform");
}

#[test]
fn a_uniform_scale_scales_radii_and_an_affine_map_refuses_them() {
    let scale = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
    ])
    .unwrap();
    let record = surface(cylinder());
    let carrier = Carrier::resolve(&record, Placement::of(scale).unwrap()).unwrap();
    let row = carrier.measures(sources(&carrier, None));
    let CarrierParameters::Cylinder { radius, .. } = &row.parameters else {
        panic!("cylinder parameters");
    };
    assert_eq!(scalar(radius), (20.0, Method::ClosedForm));

    let shear = Transform::affine([
        [1.0, 0.5, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let carrier = Carrier::resolve(&record, Placement::of(shear).unwrap()).unwrap();
    assert_eq!(
        carrier.chart().unwrap_err(),
        crate::failure::MeasureFailure::unavailable(
            UnavailableReason::PlacementNonSimilarity,
            Some("test:model:surface#s"),
            "the composed placement is not a similarity",
        )
    );
}

#[test]
fn cone_charts_with_opposite_angle_and_axis_report_one_canonical_angle() {
    let half_angle = 0.3_f64;
    for (angle, axis) in [(half_angle, 1.0), (-half_angle, -1.0)] {
        let record = surface(SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, axis),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
                1.0,
                angle,
            )
            .unwrap(),
        ));
        let carrier = Carrier::resolve(&record, Placement::identity()).unwrap();
        let row = carrier.measures(sources(&carrier, None));
        let CarrierParameters::Cone {
            half_angle: reported,
            opening_direction,
            apex,
            ratio,
            ..
        } = &row.parameters
        else {
            panic!("cone parameters");
        };
        assert!((scalar(reported).0 - half_angle).abs() < 1.0e-15);
        assert!(ratio.is_none());
        let opening = opening_direction.measured().unwrap().value().value.get();
        assert!((opening.z - 1.0).abs() < 1.0e-15);
        let apex = apex
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!((apex.z + 2.0 / half_angle.tan()).abs() < 1.0e-12);
    }
}

#[test]
fn a_negative_sphere_radius_reports_its_magnitude_and_an_inward_normal() {
    let record = surface(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -4.0,
        )
        .unwrap(),
    ));
    let carrier = Carrier::resolve(&record, Placement::identity()).unwrap();
    let row = carrier.measures(sources(&carrier, None));
    let CarrierParameters::Sphere {
        radius,
        normal_direction,
        ..
    } = &row.parameters
    else {
        panic!("sphere parameters");
    };
    assert_eq!(scalar(radius), (4.0, Method::ClosedForm));
    assert_eq!(*normal_direction, NormalDirection::Inward);
}

#[test]
fn torus_forms_follow_the_tube_radius() {
    for (minor, form) in [
        (2.0, TorusForm::Ring),
        (-2.0, TorusForm::Ring),
        (10.0, TorusForm::Horn),
        (12.0, TorusForm::Spindle),
    ] {
        let record = surface(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                10.0,
                minor,
            )
            .unwrap(),
        ));
        let carrier = Carrier::resolve(&record, Placement::identity()).unwrap();
        let row = carrier.measures(sources(&carrier, None));
        let CarrierParameters::Torus {
            minor_radius,
            form: reported,
            ..
        } = &row.parameters
        else {
            panic!("torus parameters");
        };
        assert_eq!(scalar(minor_radius).0, f64::abs(minor));
        assert_eq!(*reported, form);
    }
}
