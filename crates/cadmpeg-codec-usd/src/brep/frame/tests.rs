// SPDX-License-Identifier: Apache-2.0

use super::{derived, projected_up, surface_class, OriginRule};
use crate::brep::{Brep, SurfaceKind};
use crate::layers::Layers;
use crate::schema::Issues;
use crate::stage::compose;
use crate::test_support::with_context;
use crate::text::grammar::parse_layer;

/// Agreement of a frame component with its closed form.
const FRAME_TOLERANCE: f64 = 1e-12;

fn cylinder() -> Brep {
    let text = "#usda 1.0\ndef Cylinder \"Cylinder\"\n{\n    double height = 2\n    double radius = 1\n    uniform token axis = \"Z\"\n}\n";
    with_context(text.as_bytes(), |ctx| {
        let layers = Layers::single(parse_layer(ctx, text.as_bytes()).unwrap());
        let stage = compose(ctx, &layers).unwrap();
        crate::schema::intrinsic::records(
            stage.find("/Cylinder").expect("prim"),
            &mut Issues::default(),
        )
        .unwrap()
    })
}

/// The face of `brep` whose surface is of class `kind`.
fn face_of(brep: &Brep, kind: SurfaceKind) -> usize {
    (0..brep.faces.len())
        .find(|face| surface_class(brep, *face) == Some(kind))
        .expect("a face of the class")
}

fn close(found: [f64; 3], expected: [f64; 3]) -> bool {
    found
        .iter()
        .zip(expected)
        .all(|(a, b)| (a - b).abs() <= FRAME_TOLERANCE)
}

#[test]
fn a_planar_face_frame_sits_at_the_area_centroid_of_its_region() {
    let cube = crate::brep::primitive::cube(2.0);
    let frame = (0..cube.faces.len())
        .filter_map(|face| {
            derived(
                &cube,
                face,
                OriginRule::FaceFrame,
                [0.0, 0.0, 1.0],
                [0.0; 3],
            )
        })
        .find(|frame| close(frame[2], [0.0, 0.0, 1.0]))
        .expect("the top face");
    assert!(close(frame[3], [0.0, 0.0, 1.0]), "{frame:?}");
    // `up` parallel to the normal falls back to the cyclic partner.
    assert!(close(frame[1], [0.0, 1.0, 0.0]), "{frame:?}");
    assert!(close(frame[0], [1.0, 0.0, 0.0]), "{frame:?}");
}

#[test]
fn an_axis_frame_sits_at_the_chosen_end_of_the_face() {
    let cylinder = cylinder();
    let side = face_of(&cylinder, SurfaceKind::Cylinder);
    let low = derived(
        &cylinder,
        side,
        OriginRule::Axis { high: false },
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
    )
    .unwrap();
    assert!(close(low[3], [0.0, 0.0, -1.0]), "{low:?}");
    let high = derived(
        &cylinder,
        side,
        OriginRule::Axis { high: true },
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
    )
    .unwrap();
    // The direction senses z, so the high end is the end at z = -1.
    assert!(close(high[2], [0.0, 0.0, -1.0]), "{high:?}");
    assert!(close(high[3], [0.0, 0.0, -1.0]), "{high:?}");
}

#[test]
fn a_rule_does_not_admit_another_class() {
    let cylinder = cylinder();
    assert!(derived(
        &cylinder,
        face_of(&cylinder, SurfaceKind::Cylinder),
        OriginRule::FaceFrame,
        [0.0, 0.0, 1.0],
        [0.0; 3]
    )
    .is_none());
}

#[test]
fn the_cyclic_partner_takes_the_sign_of_the_largest_component() {
    let y = projected_up([0.0, 0.0, -1.0], [0.0, 0.0, -1.0]).unwrap();
    assert!(close(y, [0.0, -1.0, 0.0]), "{y:?}");
}
