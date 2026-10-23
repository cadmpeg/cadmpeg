// SPDX-License-Identifier: Apache-2.0
//! Surface rows: carrier parameters only, in model space, with expectations
//! from the stored carriers and body placements.

use cadmpeg_ir::ids::{BodyId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Body, BodyKind, Region, Sense, Shell};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::reason::{Category, UnavailableReason};
use crate::report::{CarrierParameters, RowStatus, SurfaceMeasures};
use crate::select::{Selection, SurfaceKind};
use crate::test_support::{assert_measured, measure, Builder, Frame};
use crate::value::Quantity;

const RADIUS: f64 = 10.0;
const HEIGHT: f64 = 30.0;

/// The seam-cut side of the cylinder `r = 10`, `z ∈ [0, 30]`.
fn cylinder_side() -> (CadIr, SurfaceId) {
    let mut builder = Builder::new();
    let low = builder.vertex([RADIUS, 0.0, 0.0]);
    let high = builder.vertex([RADIUS, 0.0, HEIGHT]);
    let bottom = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], RADIUS, &low);
    let top = builder.circle(
        [0.0, 0.0, HEIGHT],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        RADIUS,
        &high,
    );
    let seam = builder.line(&low, &high);
    let surface = builder.cylinder(Frame::z_up([0.0; 3]), RADIUS);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![
            (bottom, Sense::Forward),
            (seam.clone(), Sense::Forward),
            (top, Sense::Reversed),
            (seam, Sense::Reversed),
        ]],
        true,
    );
    (builder.finish(BodyKind::Sheet), surface)
}

fn surface_row(ir: &CadIr, surface: &SurfaceId) -> SurfaceMeasures {
    let report = measure(
        ir,
        &Selection {
            surfaces: vec![surface.clone(), surface.clone()],
            ..Selection::default()
        },
    );
    assert!(report.bodies().is_empty());
    assert!(report.faces().is_empty());
    assert_eq!(*report.document().surfaces(), 1);
    let [row] = report.surfaces().as_slice() else {
        panic!("one surface row");
    };
    row.clone()
}

fn point(quantity: &Quantity<crate::value::Point>) -> [f64; 3] {
    let point = quantity
        .measured()
        .expect("a measured point")
        .value()
        .value
        .get();
    [point.x, point.y, point.z]
}

#[test]
fn a_surface_row_reports_the_carrier_and_the_faces_that_use_it() {
    let (ir, surface) = cylinder_side();
    let row = surface_row(&ir, &surface);
    assert_eq!(row.carrier().surface(), surface.as_str());
    assert_eq!(*row.carrier().kind(), SurfaceKind::Cylinder);
    let CarrierParameters::Cylinder {
        radius,
        diameter,
        axis_point,
        axis_direction,
        ..
    } = row.carrier().parameters()
    else {
        panic!("cylinder parameters");
    };
    assert_measured(radius, RADIUS, RADIUS);
    assert_measured(diameter, 2.0 * RADIUS, RADIUS);
    assert_eq!(point(axis_point), [0.0; 3]);
    assert_eq!(point(axis_direction), [0.0, 0.0, 1.0]);
    assert_eq!(row.faces(), &[ir.model.faces[0].id.as_str().to_owned()]);
    assert_eq!(row.bodies(), &[ir.model.bodies[0].id.as_str().to_owned()]);
    assert!(row.unresolved().is_none());
    assert_eq!(*row.status(), RowStatus::Measured);
}

#[test]
fn a_surface_row_is_placed_by_the_body_that_uses_it() {
    let (mut ir, surface) = cylinder_side();
    ir.model.bodies[0].transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, -2.0],
            [0.0, 0.0, 1.0, 7.0],
        ])
        .expect("translation"),
    );
    let row = surface_row(&ir, &surface);
    let CarrierParameters::Cylinder { axis_point, .. } = row.carrier().parameters() else {
        panic!("cylinder parameters");
    };
    assert_eq!(point(axis_point), [5.0, -2.0, 7.0]);
}

/// The cylinder side with a second body whose face uses the same surface
/// under another placement.
fn shared_by_two_placements() -> (CadIr, SurfaceId) {
    let (mut ir, surface) = cylinder_side();
    let mut face = ir.model.faces[0].clone();
    let shell = ShellId::mint("test:model:shell#second").expect("identity");
    let region = RegionId::mint("test:model:region#second").expect("identity");
    let body = BodyId::mint("test:model:body#second").expect("identity");
    face.id = cadmpeg_ir::ids::FaceId::mint("test:model:face#second").expect("identity");
    face.shell = shell.clone();
    ir.model.shells.push(
        Shell::new(
            shell.clone(),
            region.clone(),
            vec![face.id.clone()],
            Vec::new(),
            Vec::new(),
        )
        .expect("shell"),
    );
    ir.model.faces.push(face);
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell],
    });
    ir.model.bodies.push(Body {
        id: body,
        kind: BodyKind::Sheet,
        regions: vec![region],
        transform: Transform::affine([
            [1.0, 0.0, 0.0, 40.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]),
        name: None,
        color: None,
        visible: None,
    });
    ir.finalize();
    (ir, surface)
}

#[test]
fn a_surface_shared_by_bodies_with_different_placements_has_no_placed_parameters() {
    let (ir, surface) = shared_by_two_placements();
    let row = surface_row(&ir, &surface);
    assert_eq!(*row.carrier().kind(), SurfaceKind::Cylinder);
    assert!(matches!(
        row.carrier().parameters(),
        CarrierParameters::None {}
    ));
    assert_eq!(row.bodies().len(), 2);
    assert_eq!(row.faces().len(), 2);
    let unresolved = row.unresolved().as_ref().expect("unresolved");
    assert_eq!(
        unresolved.reason(),
        UnavailableReason::PlacementCarrierShared
    );
    assert_eq!(unresolved.category(), Category::Placement);
    assert_eq!(row.unavailable_codes(), ["placement.carrier-shared"]);
    assert_eq!(*row.status(), RowStatus::Unavailable);
}

/// The cylinder side with a plane surface no face uses.
fn with_unused_plane() -> (CadIr, SurfaceId) {
    let (mut ir, _) = cylinder_side();
    let mut builder_surface = ir.model.surfaces[0].clone();
    builder_surface.id = SurfaceId::mint("test:model:surface#unused").expect("identity");
    builder_surface.geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        ),
    );
    let id = builder_surface.id.clone();
    ir.model.surfaces.push(builder_surface);
    ir.finalize();
    (ir, id)
}

#[test]
fn a_surface_no_face_uses_is_reported_as_stored() {
    let (mut ir, surface) = with_unused_plane();
    ir.model.bodies[0].transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("translation"),
    );
    let row = surface_row(&ir, &surface);
    let CarrierParameters::Plane { origin, .. } = row.carrier().parameters() else {
        panic!("plane parameters");
    };
    assert_eq!(point(origin), [1.0, 2.0, 3.0]);
    assert!(row.faces().is_empty());
    assert!(row.bodies().is_empty());
}

#[test]
fn a_surface_outside_the_body_scope_is_refused() {
    let (ir, surface) = with_unused_plane();
    let ctx = crate::test_support::service_context();
    let input = crate::session::MeasureInput {
        ir: &ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let refused = crate::session::Measurer::new(input, crate::session::MeasureOptions::default())
        .measure(
            &ctx,
            &Selection {
                bodies: vec![ir.model.bodies[0].id.clone()],
                surfaces: vec![surface],
                ..Selection::default()
            },
        );
    assert!(matches!(
        refused,
        Err(crate::session::MeasureError::OutsideScope { .. })
    ));
}
