// SPDX-License-Identifier: Apache-2.0
//! The unit cube fixture: six planar faces, twelve line edges.

use crate::report::RowStatus;
use crate::select::{Selection, SurfaceKind};
use crate::test_support::builders::{assert_measured, measure, value};
use crate::value::BoxKind;

#[test]
fn the_unit_cube_body_has_area_600_a_tight_box_and_a_closed_shell() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let report = measure(&cube, &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_measured(body.area().as_ref().expect("area"), 600.0, 600.0);
    assert_eq!(*body.faces(), 6);
    assert_eq!(*body.edges(), 12);
    assert_eq!(*body.vertices(), 8);
    assert_eq!(body.surface_kinds().get(&SurfaceKind::Plane), Some(&6));
    let [shell] = body.shells().as_slice() else {
        panic!("one shell");
    };
    assert!(*shell.closed());
    assert_eq!(*shell.open_edge_count(), 0);
    let bbox = body.bbox().measured().expect("box").value();
    assert_eq!(bbox.kind, BoxKind::Tight);
    assert_eq!(
        [bbox.min.get().x, bbox.min.get().y, bbox.min.get().z],
        [0.0; 3]
    );
    assert_eq!(
        [bbox.max.get().x, bbox.max.get().y, bbox.max.get().z],
        [10.0; 3]
    );
    assert_eq!(*body.status(), RowStatus::Measured);
    let document = report.document();
    assert_measured(document.area().as_ref().expect("total"), 600.0, 600.0);
    let degenerate_axes = std::collections::BTreeMap::from([(
        crate::reason::Category::Undefined,
        std::collections::BTreeMap::from([("mass.principal-axes-degenerate".to_owned(), 3)]),
    )]);
    assert_eq!(*document.unavailable(), degenerate_axes);
    assert!(!document.work().exhausted());
    assert!(document.work().used() > 0);
}

#[test]
fn every_cube_face_is_a_consistent_square() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let selection = Selection {
        all_faces: true,
        ..Selection::default()
    };
    let report = measure(&cube, &selection);
    assert_eq!(report.faces().len(), 6);
    for face in report.faces() {
        assert_measured(face.measures().area(), 100.0, 200.0);
        assert_eq!(value(face.measures().boundary_length()), 40.0);
        assert_eq!(*face.orientation().consistent(), Some(true));
        assert_eq!(*face.status(), RowStatus::Measured);
    }
}

#[test]
fn every_cube_edge_has_length_10() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let selection = Selection {
        all_edges: true,
        ..Selection::default()
    };
    let report = measure(&cube, &selection);
    assert_eq!(report.edges().len(), 12);
    for edge in report.edges() {
        assert_eq!(value(edge.length()), 10.0);
        assert_eq!(edge.faces().len(), 2);
        assert!(!*edge.closed());
    }
    assert!(report.bodies().is_empty());
}

#[test]
fn an_unknown_body_selector_is_refused() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let missing = cadmpeg_ir::ids::BodyId::mint("test:model:body#missing").unwrap();
    let ctx = crate::test_support::builders::service_context();
    let input = crate::session::MeasureInput {
        ir: &cube,
        fidelity: None,
        geometry_transferred: true,
    };
    let measurer = crate::session::Measurer::new(input, crate::session::MeasureOptions::default());
    let unknown = measurer.measure(
        &ctx,
        &Selection {
            bodies: vec![missing],
            ..Selection::default()
        },
    );
    assert!(matches!(
        unknown,
        Err(crate::session::MeasureError::UnknownEntity {
            arena: "model.bodies",
            ..
        })
    ));
}
