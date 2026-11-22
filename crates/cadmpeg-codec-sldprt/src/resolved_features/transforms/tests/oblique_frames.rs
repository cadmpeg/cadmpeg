//! Placement-derived marker transforms on oblique and axis-aligned sketch planes.

use crate::resolved_features::transforms::sketch_frame_marker_transform;
use cadmpeg_core::convert::f64_from_i64;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

const QUANTUM: f64 = 1.0e-8;
const TOLERANCE: f64 = 1.0e-9;

fn sketch(origin: Point3, normal: Vector3, u_axis: Vector3) -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(origin, normal, u_axis).unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

fn cells(u: f64, v: f64) -> (i64, i64) {
    let cell = |value: f64| {
        cadmpeg_core::convert::truncate_f64_to_i64((value / QUANTUM).round())
            .expect("coordinate fits i64")
    };
    (cell(u), cell(v))
}

#[test]
fn oblique_sketch_frame_keeps_marker_lengths_and_arc_radii() {
    let origin = Point3::new(5.0, -3.0, 7.0);
    let normal = Vector3::new(1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
    let u_axis = Vector3::new(2.0 / 3.0, 1.0 / 3.0, -2.0 / 3.0);
    let v_axis = normal.cross(u_axis);
    let transform =
        sketch_frame_marker_transform(&sketch(origin, normal, u_axis), QUANTUM).expect("frame");
    let place = |marker: (f64, f64)| {
        let (u, v) = transform.apply(cells(marker.0, marker.1)).expect("placed");
        origin
            .translated(u_axis, f64_from_i64(u).unwrap() * QUANTUM)
            .translated(v_axis, f64_from_i64(v).unwrap() * QUANTUM)
    };

    for marker in [(0.0, 0.0), (3.0, 0.0), (0.0, 4.0), (-12.5, 31.25)] {
        assert_eq!(
            transform.apply(cells(marker.0, marker.1)),
            Some(cells(marker.0, marker.1))
        );
    }
    let [a, b, c] = [(0.0, 0.0), (3.0, 0.0), (0.0, 4.0)].map(place);
    assert!((a.distance(b) - 3.0).abs() <= TOLERANCE);
    assert!((a.distance(c) - 4.0).abs() <= TOLERANCE);
    assert!((b.distance(c) - 5.0).abs() <= TOLERANCE);

    let center = place((10.0, 10.0));
    for endpoint in [(15.0, 10.0), (10.0, 15.0), (10.0 - 3.0, 10.0 + 4.0)] {
        assert!((center.distance(place(endpoint)) - 5.0).abs() <= TOLERANCE);
    }
}

#[test]
fn axis_aligned_sketch_frame_reads_markers_as_model_components() {
    let origin = Point3::new(4.0, 6.0, -9.0);
    let normal = Vector3::new(0.0, 1.0, 0.0);
    let u_axis = Vector3::new(1.0, 0.0, 0.0);
    let v_axis = normal.cross(u_axis);
    let transform =
        sketch_frame_marker_transform(&sketch(origin, normal, u_axis), QUANTUM).expect("frame");

    let (u, v) = transform.apply(cells(10.0, 20.0)).expect("placed");
    assert_eq!((u, v), cells(6.0, -29.0));
    let placed = origin
        .translated(u_axis, f64_from_i64(u).unwrap() * QUANTUM)
        .translated(v_axis, f64_from_i64(v).unwrap() * QUANTUM);
    assert!(placed.distance(Point3::new(10.0, 6.0, 20.0)) <= TOLERANCE);
}
