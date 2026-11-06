// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_measure::test_support::round_trip::measured;

use crate::FcstdCodec;

const VOLUME_TOLERANCE: f64 = 1.0e-5;
const AREA_TOLERANCE: f64 = 1.0e-5;
const BOUNDS_TOLERANCE: f64 = 1.0e-6;
const TRANSPARENCY_TOLERANCE: f64 = 1.0e-6;
const AUTHORED_TRANSPARENCY: f32 = 0.12;

#[test]
fn authored_gui_appearance_keeps_fused_measures_and_topology_styles() {
    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;

    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.faces.len(), 8);
    assert_eq!(model.edges.len(), 15);
    assert_eq!(model.vertices.len(), 10);

    let report = measured("gui_appearance", decoded.ir()).unwrap();
    let [body] = report.bodies().as_slice() else {
        panic!("one authored fused body");
    };
    let volume = body
        .volume()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    assert!(
        (volume - (1512.0 + 108.0 * std::f64::consts::PI)).abs() < VOLUME_TOLERANCE,
        "volume {volume}"
    );
    let area = body
        .area()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    assert!(
        (area - (852.0 + 72.0 * std::f64::consts::PI)).abs() < AREA_TOLERANCE,
        "area {area}"
    );
    let bounds = body.bbox().measured().unwrap().value();
    for (actual, expected) in bounds
        .min
        .coordinates()
        .into_iter()
        .chain(bounds.max.coordinates())
        .zip([0.0, 0.0, 0.0, 18.0, 12.0, 19.0])
    {
        assert!(
            (actual.get() - expected).abs() < BOUNDS_TOLERANCE,
            "bound coordinate {} expected {expected}",
            actual.get()
        );
    }

    let appearance = |id: &cadmpeg_ir::ids::AppearanceId| {
        model
            .appearances
            .iter()
            .find(|appearance| appearance.id == *id)
            .expect("every topology binding resolves to an appearance")
    };
    let face_color = |rgb: [u8; 3]| {
        Color::new(
            f32::from(rgb[0]) / 255.0,
            f32::from(rgb[1]) / 255.0,
            f32::from(rgb[2]) / 255.0,
            1.0 - AUTHORED_TRANSPARENCY,
        )
        .unwrap()
    };
    let expected_face_colors = [
        face_color([64, 89, 153]),
        face_color([77, 99, 161]),
        face_color([89, 110, 168]),
        face_color([102, 89, 176]),
        face_color([64, 99, 184]),
        face_color([77, 110, 153]),
        face_color([89, 89, 161]),
        face_color([102, 99, 168]),
    ];
    let expected_face_targets = model
        .faces
        .iter()
        .map(|face| AppearanceTarget::Face(face.id.clone()))
        .collect::<BTreeSet<_>>();
    let face_bindings = model
        .appearance_bindings
        .iter()
        .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .collect::<Vec<_>>();
    assert_eq!(face_bindings.len(), expected_face_targets.len());
    assert_eq!(
        face_bindings
            .iter()
            .map(|binding| binding.target.clone())
            .collect::<BTreeSet<_>>(),
        expected_face_targets
    );
    let mut remaining_colors = expected_face_colors.to_vec();
    for binding in face_bindings {
        let appearance = appearance(&binding.appearance);
        let color = appearance.base_color.expect("face material color");
        let index = remaining_colors
            .iter()
            .position(|expected| *expected == color)
            .expect("authored face color");
        remaining_colors.remove(index);
        let transparency = appearance
            .properties
            .get("transparency")
            .expect("face material transparency")
            .get();
        assert!(
            (transparency - f64::from(AUTHORED_TRANSPARENCY)).abs() < TRANSPARENCY_TOLERANCE,
            "face transparency {transparency}"
        );
    }
    assert!(remaining_colors.is_empty());

    let expected_edge_targets = model
        .edges
        .iter()
        .map(|edge| AppearanceTarget::Edge(edge.id.clone()))
        .collect::<BTreeSet<_>>();
    let edge_bindings = model
        .appearance_bindings
        .iter()
        .filter(|binding| matches!(binding.target, AppearanceTarget::Edge(_)))
        .collect::<Vec<_>>();
    assert_eq!(edge_bindings.len(), expected_edge_targets.len());
    assert_eq!(
        edge_bindings
            .iter()
            .map(|binding| binding.target.clone())
            .collect::<BTreeSet<_>>(),
        expected_edge_targets
    );
    for binding in edge_bindings {
        let appearance = appearance(&binding.appearance);
        assert_eq!(
            appearance.base_color,
            Some(Color::from_rgba8(26, 26, 26, 255))
        );
        assert_eq!(
            appearance
                .properties
                .get("line_width")
                .map(|width| width.get()),
            Some(2.5)
        );
    }

    let expected_vertex_targets = model
        .vertices
        .iter()
        .map(|vertex| AppearanceTarget::Vertex(vertex.id.clone()))
        .collect::<BTreeSet<_>>();
    let vertex_bindings = model
        .appearance_bindings
        .iter()
        .filter(|binding| matches!(binding.target, AppearanceTarget::Vertex(_)))
        .collect::<Vec<_>>();
    assert_eq!(vertex_bindings.len(), expected_vertex_targets.len());
    assert_eq!(
        vertex_bindings
            .iter()
            .map(|binding| binding.target.clone())
            .collect::<BTreeSet<_>>(),
        expected_vertex_targets
    );
    for binding in vertex_bindings {
        let appearance = appearance(&binding.appearance);
        assert_eq!(
            appearance.base_color,
            Some(Color::from_rgba8(242, 89, 38, 255))
        );
        assert_eq!(
            appearance
                .properties
                .get("point_size")
                .map(|size| size.get()),
            Some(4.0)
        );
    }
}
