//! Placement-derived marker transforms on oblique sketch planes.

use crate::resolved_features::transforms::{sketch_frame_marker_transform, MarkerTransform};
use cadmpeg_core::convert::f64_from_i64;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

const QUANTUM: f64 = 1.0e-8;
const TOLERANCE: f64 = 1.0e-7;
/// A marker sketch, placed on the stored frame of its support plane.
const MARKER_SKETCH: &str = "sldprt:model:sketch#markers:1:30";
/// A sketch projected from a B-rep face, on a frame the decoder computes.
const FACE_SKETCH: &str = "sldprt:model:sketch#38:0:0";

struct Frame {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

impl Frame {
    fn transform(&self, id: &str) -> MarkerTransform {
        let sketch = Sketch {
            id: SketchId::mint(id).unwrap(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(self.origin, self.normal, self.u_axis)
                .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: None,
        };
        sketch_frame_marker_transform(&sketch, QUANTUM).expect("frame")
    }

    /// The model point at which `transform` places the marker pair `(a, b)`.
    fn place(&self, transform: MarkerTransform, (a, b): (f64, f64)) -> Point3 {
        let (u, v) = transform.apply(cells(a, b)).expect("placed");
        self.origin
            .translated(self.u_axis, f64_from_i64(u).unwrap() * QUANTUM)
            .translated(
                self.normal.cross(self.u_axis),
                f64_from_i64(v).unwrap() * QUANTUM,
            )
    }

    /// Whether every marker pair lands on the model point whose `x` and `y`
    /// are the pair: the model-component reading of a plane whose normal is
    /// mostly `z`.
    fn reads_model_components(&self, transform: MarkerTransform) -> bool {
        [(10.0, 20.0), (-4.0, 3.5)].into_iter().all(|(a, b)| {
            let placed = self.place(transform, (a, b));
            (placed.x - a).abs() <= TOLERANCE && (placed.y - b).abs() <= TOLERANCE
        })
    }
}

fn cells(u: f64, v: f64) -> (i64, i64) {
    let cell = |value: f64| {
        cadmpeg_core::convert::truncate_f64_to_i64((value / QUANTUM).round())
            .expect("coordinate fits i64")
    };
    (cell(u), cell(v))
}

/// A frame through `(5, -3, 7)` whose normal is tilted `tilt` radians from
/// `z` toward `x`, and whose `u` axis is turned 30 degrees in the plane, so
/// that no axis of the frame is a model axis.
fn tilted(tilt: f64) -> Frame {
    let normal = Vector3::new(tilt.sin(), 0.0, tilt.cos());
    let tangent = Vector3::new(tilt.cos(), 0.0, -tilt.sin());
    let (sin, cos) = 30.0_f64.to_radians().sin_cos();
    let u_axis = Vector3::new(cos * tangent.x, sin + cos * tangent.y, cos * tangent.z);
    Frame {
        origin: Point3::new(5.0, -3.0, 7.0),
        normal,
        u_axis,
    }
}

#[test]
fn oblique_support_plane_frame_reads_markers_as_sketch_coordinates() {
    let frame = Frame {
        origin: Point3::new(5.0, -3.0, 7.0),
        normal: Vector3::new(2.0 / 7.0, 3.0 / 7.0, 6.0 / 7.0),
        u_axis: Vector3::new(3.0 / 7.0, -6.0 / 7.0, 2.0 / 7.0),
    };
    let transform = frame.transform(MARKER_SKETCH);

    assert_eq!(transform, MarkerTransform::identity());
    for (a, b) in [(0.0, 0.0), (3.0, 0.0), (0.0, 4.0), (-12.5, 31.25)] {
        assert_eq!(transform.apply(cells(a, b)), Some(cells(a, b)));
    }
}

/// The identity reading is witnessed on support-plane frames only, so a
/// frame the decoder computed keeps the model-component reading (open item
/// DI-80).
#[test]
fn oblique_computed_frame_keeps_the_model_component_reading() {
    let frame = Frame {
        origin: Point3::new(5.0, -3.0, 7.0),
        normal: Vector3::new(2.0 / 7.0, 3.0 / 7.0, 6.0 / 7.0),
        u_axis: Vector3::new(3.0 / 7.0, -6.0 / 7.0, 2.0 / 7.0),
    };
    let transform = frame.transform(FACE_SKETCH);

    assert_ne!(transform, MarkerTransform::identity());
    assert!(frame.reads_model_components(transform));
}

/// The two readings differ by the whole origin translation, so the oblique
/// threshold is tested on both sides: a normal `1e-5` from a model axis is
/// axis-parallel, and one `1e-3` from it is oblique.
#[test]
fn oblique_threshold_separates_a_tilt_of_1e_5_from_one_of_1e_3() {
    let near = tilted(1.0e-5);
    let near_transform = near.transform(MARKER_SKETCH);
    assert_ne!(near_transform, MarkerTransform::identity());
    assert!(near.reads_model_components(near_transform));

    let far = tilted(1.0e-3);
    assert_eq!(far.transform(MARKER_SKETCH), MarkerTransform::identity());
    assert!(far.reads_model_components(far.transform(FACE_SKETCH)));
}
