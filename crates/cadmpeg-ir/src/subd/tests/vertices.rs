// SPDX-License-Identifier: Apache-2.0

use crate::math::Point3;
use crate::subd::{SubdVertex, SubdVertexTag};

#[test]
fn vertex_admission_and_point_edits_require_finite_coordinates() {
    let mut vertex =
        SubdVertex::new(Point3::new(-1.0, 2.0, 3.0), SubdVertexTag::Smooth, None).unwrap();
    let original = vertex.clone();
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for point in [
            Point3::new(invalid, 0.0, 0.0),
            Point3::new(0.0, invalid, 0.0),
            Point3::new(0.0, 0.0, invalid),
        ] {
            assert!(SubdVertex::new(point, SubdVertexTag::Smooth, None).is_err());
            assert!(vertex.set_point(point).is_err());
            assert_eq!(vertex, original);
            let mut wire = serde_json::to_value(&vertex).unwrap();
            wire["point"] = serde_json::json!(point);
            assert!(serde_json::from_value::<SubdVertex>(wire).is_err());
        }
    }
    let point = Point3::new(f64::MAX, -f64::MAX, 0.0);
    vertex.set_point(point).unwrap();
    assert_eq!(vertex.point(), point);
    let wire = serde_json::to_value(&vertex).unwrap();
    assert_eq!(wire, serde_json::json!({"point": point, "tag": "smooth"}));
    assert_eq!(serde_json::from_value::<SubdVertex>(wire).unwrap(), vertex);
}

#[test]
fn cage_edits_roll_back_when_a_vertex_rejects_its_position() {
    let mut cage = super::triangle_cage();
    let original = cage.clone();
    assert!(cage
        .edit_vertices(|vertices| {
            vertices[0].set_point(Point3::new(2.0, 3.0, 4.0))?;
            vertices[1].set_point(Point3::new(f64::INFINITY, 0.0, 0.0))
        })
        .is_err());
    assert_eq!(cage, original);
    cage.edit_vertices(|vertices| vertices[0].set_point(Point3::new(2.0, 3.0, 4.0)))
        .unwrap();
    assert_eq!(cage.vertices()[0].point(), Point3::new(2.0, 3.0, 4.0));
}
