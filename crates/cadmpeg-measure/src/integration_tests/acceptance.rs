// SPDX-License-Identifier: Apache-2.0
//! The #188 acceptance cases on built documents, with expectations from
//! geometry.

use std::f64::consts::PI;

use crate::reason::CaveatCode;
use crate::report::{Classification, DeclaredRole, ListedOrder, LoopRole, RegionRule, Winding};
use crate::select::Selection;
use crate::test_support::builders::{assert_measured, measure, plate_with_hole, value};
use crate::tolerance::QUADRATURE_RELATIVE_TARGET;
use crate::value::BoxKind;

fn faces() -> Selection {
    Selection {
        all_faces: true,
        ..Selection::default()
    }
}

#[test]
fn a_plate_with_a_hole_measures_area_lengths_box_and_roles() {
    for classified in [true, false] {
        let ir = plate_with_hole(classified);
        let report = measure(&ir, &faces());
        let [face] = report.faces().as_slice() else {
            panic!("one face row");
        };
        let measures = face.measures();
        let expected = 1600.0 - 100.0 * PI;
        let scale = 40.0_f64.hypot(40.0).powi(2);
        assert_measured(measures.area(), expected, scale);
        let area = measures.area().measured().expect("area");
        assert!(
            area.numeric_error().get() <= QUADRATURE_RELATIVE_TARGET * expected.abs().max(scale)
        );
        assert_measured(measures.boundary_length(), 160.0 + 20.0 * PI, 160.0);
        assert_eq!(value(measures.seam_length()), 0.0);
        assert_eq!(*measures.region(), Some(RegionRule::Nesting));

        let [outer, inner] = measures.loops().as_slice() else {
            panic!("two loops");
        };
        assert_eq!(*outer.role(), Some(LoopRole::Outer));
        assert_eq!(*outer.winding(), Some(Winding::Counterclockwise));
        assert!((outer.length().expect("outer length") - 160.0).abs() < 1.0e-12);
        assert_eq!(*inner.role(), Some(LoopRole::Inner));
        assert_eq!(*inner.winding(), Some(Winding::Clockwise));
        assert!((inner.length().expect("inner length") - 20.0 * PI).abs() < 1.0e-12);
        let expected_role = if classified {
            DeclaredRole::Outer
        } else {
            DeclaredRole::Unspecified
        };
        assert_eq!(*outer.declared_role(), expected_role);

        let bbox = measures.bbox().measured().expect("box");
        let value = bbox.value();
        assert_eq!(value.kind, BoxKind::Tight);
        let (min, max) = (value.min.get(), value.max.get());
        for (got, want) in [
            (min.x, 0.0),
            (min.y, 0.0),
            (min.z, 0.0),
            (max.x, 40.0),
            (max.y, 40.0),
            (max.z, 0.0),
        ] {
            assert!((got - want).abs() < 1.0e-12, "{got} {want}");
        }

        let orientation = face.orientation();
        assert_eq!(*orientation.consistent(), Some(true));
        let classification = if classified {
            Classification::Consistent
        } else {
            Classification::Unspecified
        };
        assert_eq!(*orientation.classification(), classification);
        assert!(face.caveats().is_empty());
    }
}

#[test]
fn a_ring_listed_out_of_order_keeps_its_area_and_reports_the_order() {
    let mut builder = crate::test_support::builders::Builder::new();
    let corners = [
        builder.vertex([0.0, 0.0, 0.0]),
        builder.vertex([40.0, 0.0, 0.0]),
        builder.vertex([40.0, 40.0, 0.0]),
        builder.vertex([0.0, 40.0, 0.0]),
    ];
    let sides: Vec<_> = (0..4)
        .map(|index| {
            (
                builder.line(&corners[index], &corners[(index + 1) % 4]),
                cadmpeg_ir::topology::Sense::Forward,
            )
        })
        .collect();
    let surface = builder.plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let face = builder.face(
        &surface,
        cadmpeg_ir::topology::Sense::Forward,
        &[sides],
        true,
    );
    builder.reverse_listing(&face, 0);
    let ir = builder.finish(cadmpeg_ir::topology::BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let face = &report.faces()[0];
    assert_measured(face.measures().area(), 1600.0, 3200.0);
    let entry = &face.measures().loops()[0];
    assert_eq!(*entry.listed_order(), Some(ListedOrder::Reversed));
    assert_eq!(*entry.winding(), Some(Winding::Counterclockwise));
    let [caveat] = face.caveats().as_slice() else {
        panic!("one caveat");
    };
    assert_eq!(caveat.code(), CaveatCode::BoundaryRingOrder);
    assert_eq!(caveat.entity(), Some(entry.loop_id().as_str()));
    assert_eq!(
        report.document().orientation().ring_order(),
        &[face.face().clone()]
    );
}
