// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::test_archive::rewrite_entry;
use cadmpeg_ir::appearance::AppearanceTarget;

const GUI_MODEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
));

#[test]
fn face_materials_follow_shape_indices_without_complete_persistent_names() {
    let loss_facts = |decoded: &cadmpeg_ir::codec::DecodeResult| {
        decoded
            .report()
            .losses
            .iter()
            .map(|loss| (loss.code.clone(), loss.severity, loss.message.clone()))
            .collect::<Vec<_>>()
    };
    let baseline = FcstdCodec
        .decode(&mut Cursor::new(GUI_MODEL), &DecodeOptions::default())
        .unwrap();
    for names in [
        "",
        "<ElementMap/>",
        r#"<ElementMap count="2"><Element key="named-first" value="Face1"/><Element key="named-third" value="Face3"/></ElementMap>"#,
    ] {
        let source = rewrite_entry(GUI_MODEL, "Document.xml", |bytes| {
            std::str::from_utf8(bytes)
                .unwrap()
                .replace("<ElementMap/>", names)
                .into_bytes()
        });
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(loss_facts(&decoded), loss_facts(&baseline));
        let model = &decoded.ir().model;
        assert_eq!(model.faces.len(), 8);
        let face_bindings = model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .collect::<Vec<_>>();
        assert_eq!(face_bindings.len(), 8);
        for face in &model.faces {
            assert_eq!(
                face_bindings
                    .iter()
                    .filter(|binding| { binding.target == AppearanceTarget::Face(face.id.clone()) })
                    .count(),
                1
            );
        }
        assert_valid_document(decoded.ir());
    }
}

#[test]
fn accepted_primitive_color_arrays_replace_defaults_and_keep_primitive_sizes() {
    for (entry, size_property, expected_size, target_count) in [
        ("LineColorArray", "line_width", 2.5, 15),
        ("PointColorArray", "point_size", 4.0, 10),
    ] {
        for count in [0_u32, 1, 2] {
            let source = rewrite_entry(GUI_MODEL, entry, |_| {
                let mut bytes = count.to_le_bytes().to_vec();
                for _ in 0..count {
                    bytes.extend_from_slice(&0xff00_ffff_u32.to_le_bytes());
                }
                bytes
            });
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            let model = &decoded.ir().model;
            let targets = if entry == "LineColorArray" {
                model
                    .edges
                    .iter()
                    .map(|edge| AppearanceTarget::Edge(edge.id.clone()))
                    .collect::<Vec<_>>()
            } else {
                model
                    .vertices
                    .iter()
                    .map(|vertex| AppearanceTarget::Vertex(vertex.id.clone()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(targets.len(), target_count);
            for target in targets {
                let bindings = model
                    .appearance_bindings
                    .iter()
                    .filter(|binding| binding.target == target)
                    .collect::<Vec<_>>();
                assert_eq!(bindings.len(), 1, "{entry}, count {count}, {target:?}");
                let appearance = model
                    .appearances
                    .iter()
                    .find(|appearance| appearance.id == bindings[0].appearance)
                    .unwrap();
                assert_eq!(
                    appearance.properties.get(size_property).unwrap().get(),
                    expected_size
                );
                let override_color =
                    Some(cadmpeg_ir::topology::Color::from_rgba8(255, 0, 255, 255));
                if count == 1 {
                    assert_eq!(appearance.base_color, override_color);
                } else {
                    assert_ne!(appearance.base_color, override_color);
                }
            }
            assert_eq!(
                decoded
                    .report()
                    .losses
                    .iter()
                    .filter(
                        |loss| loss.code.local_code() == "appearance.topology-color-count-mismatch"
                    )
                    .count(),
                usize::from(count == 2)
            );
        }
    }
}
