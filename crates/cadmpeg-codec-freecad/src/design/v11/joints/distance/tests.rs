// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::design::construction::FrameAxis;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::shared::{JointKind, JointMotion};

use super::{
    classify, restatement, CurveType, Element, Form, Restatement, Shape, Side, SurfaceType,
    Unresolved,
};

fn face(surface: SurfaceType, radius: f64) -> Side {
    Side {
        kind: "Face".to_owned(),
        element: Ok(Some(Element::Face { surface, radius })),
    }
}

fn edge(curve: CurveType, radius: f64) -> Side {
    Side {
        kind: "Edge".to_owned(),
        element: Ok(Some(Element::Edge { curve, radius })),
    }
}

fn vertex() -> Side {
    Side {
        kind: "Vertex".to_owned(),
        element: Ok(None),
    }
}

fn unresolved(kind: &str) -> Side {
    Side {
        kind: kind.to_owned(),
        element: Err(Unresolved),
    }
}

fn classified(first: &Side, second: &Side) -> (bool, Form) {
    classify([first, second]).unwrap()
}

#[test]
fn two_vertices_read_no_geometry() {
    assert_eq!(
        classify([&unresolved("Vertex"), &unresolved("Vertex")]),
        Ok((false, Form::PointPoint))
    );
}

#[test]
fn an_edge_pair_puts_a_line_then_a_circle_first() {
    let line = edge(CurveType::Line, 0.0);
    let circle = edge(CurveType::Circle, 3.0);
    let other = edge(CurveType::Other, 0.0);
    assert_eq!(
        classified(&line, &line),
        (false, Form::RevCyl { extra: 0.0 })
    );
    assert_eq!(
        classified(&circle, &line),
        (true, Form::RevCyl { extra: 3.0 })
    );
    assert_eq!(
        classified(&circle, &edge(CurveType::Circle, 2.0)),
        (false, Form::RevCyl { extra: 5.0 })
    );
    assert_eq!(classified(&other, &line), (true, Form::Planar));
    assert_eq!(classified(&other, &circle), (true, Form::Planar));
    assert_eq!(classified(&other, &other), (false, Form::Planar));
}

#[test]
fn a_face_pair_puts_the_first_type_in_order_first() {
    let plane = face(SurfaceType::Plane, 0.0);
    let cylinder = face(SurfaceType::Cylinder, 1.0);
    let sphere = face(SurfaceType::Sphere, 2.0);
    let torus = face(SurfaceType::Torus, 0.0);
    let cone = face(SurfaceType::Cone, 0.0);
    let other = face(SurfaceType::Other, 0.0);
    assert_eq!(classified(&plane, &plane), (false, Form::Planar));
    assert_eq!(
        classified(&cylinder, &plane),
        (true, Form::LineInPlane { extra: 1.0 })
    );
    assert_eq!(
        classified(&plane, &sphere),
        (false, Form::PointInPlane { extra: 2.0 })
    );
    assert_eq!(classified(&plane, &torus), (false, Form::Planar));
    assert_eq!(classified(&cone, &plane), (true, Form::Planar));
    assert_eq!(
        classified(&cylinder, &cylinder),
        (false, Form::RevCyl { extra: 2.0 })
    );
    assert_eq!(
        classified(&sphere, &cylinder),
        (true, Form::CylSph { extra: 3.0 })
    );
    assert_eq!(
        classified(&torus, &cylinder),
        (true, Form::RevCyl { extra: 1.0 })
    );
    assert_eq!(classified(&cylinder, &cone), (false, Form::Planar));
    assert_eq!(classified(&torus, &torus), (false, Form::Planar));
    assert_eq!(
        classified(&sphere, &torus),
        (true, Form::CylSph { extra: 2.0 })
    );
    assert_eq!(
        classified(&sphere, &sphere),
        (false, Form::SphSph { extra: 4.0 })
    );
    assert_eq!(classified(&other, &other), (false, Form::Planar));
    assert_eq!(classified(&other, &plane), (true, Form::Planar));
}

#[test]
fn a_face_of_no_shape_feature_has_no_type() {
    let none = Side {
        kind: "Face".to_owned(),
        element: Ok(None),
    };
    let plane = face(SurfaceType::Plane, 0.0);
    assert_eq!(classified(&none, &plane), (true, Form::Planar));
}

#[test]
fn a_mixed_pair_puts_the_face_then_the_edge_first() {
    let plane = face(SurfaceType::Plane, 0.0);
    let cylinder = face(SurfaceType::Cylinder, 4.0);
    let sphere = face(SurfaceType::Sphere, 2.0);
    let line = edge(CurveType::Line, 0.0);
    let circle = edge(CurveType::Circle, 3.0);
    assert_eq!(
        classified(&vertex(), &cylinder),
        (true, Form::CylSph { extra: 4.0 })
    );
    assert_eq!(
        classified(&plane, &vertex()),
        (false, Form::PointInPlane { extra: 0.0 })
    );
    assert_eq!(
        classified(&sphere, &vertex()),
        (false, Form::SphSph { extra: 2.0 })
    );
    assert_eq!(
        classified(&line, &plane),
        (true, Form::LineInPlane { extra: 0.0 })
    );
    assert_eq!(classified(&line, &cylinder), (true, Form::Planar));
    assert_eq!(classified(&circle, &plane), (true, Form::Planar));
    assert_eq!(
        classified(&vertex(), &circle),
        (true, Form::PointInPlane { extra: 0.0 })
    );
    assert_eq!(
        classified(&line, &vertex()),
        (false, Form::CylSph { extra: 0.0 })
    );
}

#[test]
fn an_element_of_another_kind_is_the_default_case() {
    let plane = face(SurfaceType::Plane, 0.0);
    let datum = Side {
        kind: String::new(),
        element: Ok(None),
    };
    assert_eq!(classified(&plane, &datum), (false, Form::Planar));
    assert_eq!(classified(&datum, &vertex()), (false, Form::Planar));
}

#[test]
fn a_type_the_case_reads_must_resolve() {
    let plane = face(SurfaceType::Plane, 0.0);
    assert_eq!(classify([&unresolved("Face"), &plane]), Err(Unresolved));
    assert_eq!(classify([&plane, &unresolved("Face")]), Err(Unresolved));
    assert_eq!(classify([&vertex(), &unresolved("Edge")]), Err(Unresolved));
    // A curve that is no line makes the pair the default case whatever the
    // face is.
    assert_eq!(
        classify([&edge(CurveType::Circle, 1.0), &unresolved("Face")]),
        Ok((true, Form::Planar))
    );
}

fn length(kind: &JointKind) -> Option<f64> {
    let (JointKind::Distance {
        distance: Some(Dimension::Literal(length)),
        ..
    }
    | JointKind::Coincident {
        offset: Some(Dimension::Literal(length)),
    }) = kind
    else {
        return None;
    };
    Some(length.get())
}

fn restated(form: Form, distance: f64) -> Restatement {
    restatement(form, distance).unwrap()
}

#[test]
fn a_planar_case_offsets_along_the_first_z_axis() {
    let planar = restated(Form::Planar, -2.0);
    let JointKind::Motion {
        motion: JointMotion::Planar { axis: FrameAxis::Z },
        offset,
        limits,
        ..
    } = &planar.kind
    else {
        panic!("{:?}", planar.kind);
    };
    assert!(matches!(offset.z, Some(Dimension::Literal(z)) if z.get() == -2.0));
    assert_eq!(
        (offset.x.clone(), offset.y.clone(), offset.angle.clone()),
        (None, None, None)
    );
    assert!(limits.is_empty());
    assert_eq!(planar.shapes, [Shape::Frame, Shape::Frame]);
}

#[test]
fn an_in_plane_case_is_a_signed_coincidence() {
    let point = restated(Form::PointInPlane { extra: 3.0 }, -1.0);
    assert!(matches!(point.kind, JointKind::Coincident { .. }));
    assert_eq!(length(&point.kind), Some(2.0));
    assert_eq!(point.shapes, [Shape::Plane, Shape::Point]);
    let line = restated(Form::LineInPlane { extra: 0.0 }, -5.0);
    assert_eq!(length(&line.kind), Some(-5.0));
    assert_eq!(line.shapes, [Shape::Plane, Shape::Axis]);
    assert!(!point.parallel && !line.parallel);
}

#[test]
fn an_axis_pair_keeps_its_axes_parallel() {
    let apart = restated(Form::RevCyl { extra: 5.0 }, 1.0);
    assert!(matches!(apart.kind, JointKind::Distance { .. }));
    assert_eq!(length(&apart.kind), Some(6.0));
    assert_eq!(apart.shapes, [Shape::Axis, Shape::Axis]);
    assert!(apart.parallel);
    let coaxial = restated(Form::RevCyl { extra: 5.0 }, -5.0);
    assert_eq!(coaxial.kind, JointKind::Concentric);
    assert!(!coaxial.parallel);
    assert_eq!(restatement(Form::RevCyl { extra: 0.0 }, -1.0), None);
}

#[test]
fn a_separation_from_an_axis_or_a_point_adds_the_radii() {
    let axis = restated(Form::CylSph { extra: 2.0 }, -1.0);
    assert_eq!(length(&axis.kind), Some(1.0));
    assert_eq!(axis.shapes, [Shape::Axis, Shape::Point]);
    assert_eq!(restatement(Form::CylSph { extra: 2.0 }, -3.0), None);
    let points = restated(Form::SphSph { extra: 4.0 }, -1.5);
    assert_eq!(length(&points.kind), Some(2.5));
    assert_eq!(points.shapes, [Shape::Point, Shape::Point]);
    assert_eq!(restatement(Form::SphSph { extra: 0.0 }, -0.5), None);
}

#[test]
fn a_point_pair_below_the_tolerance_is_a_coincidence() {
    for distance in [-1.0, 0.0, 5e-8] {
        let coincident = restated(Form::PointPoint, distance);
        assert_eq!(coincident.kind, JointKind::Coincident { offset: None });
        assert_eq!(coincident.shapes, [Shape::Point, Shape::Point]);
    }
    let apart = restated(Form::PointPoint, 4.0);
    assert!(matches!(apart.kind, JointKind::Distance { .. }));
    assert_eq!(length(&apart.kind), Some(4.0));
}
