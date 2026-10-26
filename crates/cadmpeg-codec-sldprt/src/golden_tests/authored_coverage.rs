// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn complete_authored_inputs_cover_nested_sketch_geometry_and_constraints() {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
    let fixtures = harness().fixture_inputs();
    for (name, sketches, entities, constraints) in [
        ("authored_nested_profile", 1, 3, 3),
        ("authored_nested_compressed", 1, 3, 3),
        ("authored_nested_arc", 1, 3, 3),
        ("authored_nested_circle", 1, 1, 0),
        ("authored_nested_ellipse", 1, 1, 0),
        ("authored_nested_nurbs", 2, 6, 6),
    ] {
        let (_, bytes) = fixtures
            .iter()
            .find(|(fixture, _)| fixture == name)
            .expect("commit the authored nested sketch input");
        let result = SldprtCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .expect("complete authored nested sketch input decodes");
        let ir = result.ir();
        assert_eq!(
            arena_sizes(ir),
            [1, 1, 1, 1, 1, 3, 3, 3],
            "{name}: exact outer body"
        );
        assert_eq!(
            ir.model.configurations.iter().filter(|c| c.active).count(),
            1,
            "{name}"
        );
        assert_eq!(ir.model.sketches.len(), sketches, "{name}");
        assert_eq!(ir.model.sketch_entities.len(), entities, "{name}");
        assert_eq!(ir.model.sketch_constraints.len(), constraints, "{name}");
        let mut kinds = [0usize; 5];
        let mut rational = 0;
        for entity in &ir.model.sketch_entities {
            let index = match entity.geometry.definition() {
                SketchGeometryDefinition::Line { .. } => 0,
                SketchGeometryDefinition::Arc { .. } => 1,
                SketchGeometryDefinition::Circle { .. } => 2,
                SketchGeometryDefinition::Ellipse { .. } => 3,
                SketchGeometryDefinition::Nurbs { curve } => {
                    rational += usize::from(curve.pole_rows().weights().is_some());
                    4
                }
                other => panic!("{name}: unexpected authored sketch geometry {other:?}"),
            };
            kinds[index] += 1;
        }
        let expected = match name {
            "authored_nested_profile" | "authored_nested_compressed" => [3, 0, 0, 0, 0],
            "authored_nested_arc" => [2, 1, 0, 0, 0],
            "authored_nested_circle" => [0, 0, 1, 0, 0],
            "authored_nested_ellipse" => [0, 0, 0, 1, 0],
            "authored_nested_nurbs" => [4, 0, 0, 0, 2],
            _ => unreachable!(),
        };
        assert_eq!(kinds, expected, "{name}: exact authored primitive roster");
        assert_eq!(
            rational,
            usize::from(name == "authored_nested_nurbs"),
            "{name}"
        );
    }
}

#[test]
fn complete_authored_inputs_cover_presentation_metadata_and_pmi() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    use cadmpeg_ir::attributes::AttributeValue;
    const EPS_AUTHORED_COLOR_COMPONENT: f32 = f32::EPSILON;
    let fixtures = harness().fixture_inputs();
    for name in [
        "authored_partition_deltas_color",
        "authored_material",
        "authored_display_list",
        "authored_document_envelope",
        "authored_pmi_dimension",
    ] {
        let (_, bytes) = fixtures
            .iter()
            .find(|(fixture, _)| fixture == name)
            .expect("commit the authored presentation input");
        let result = SldprtCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .expect("complete authored presentation input decodes");
        let ir = result.ir();
        assert_eq!(
            arena_sizes(ir),
            [1, 1, 1, 1, 1, 3, 3, 3],
            "{name}: exact body"
        );
        assert_eq!(
            ir.model.configurations.iter().filter(|c| c.active).count(),
            1,
            "{name}"
        );
        match name {
            "authored_partition_deltas_color" => {
                assert_eq!(ir.model.appearances.len(), 1);
                assert_eq!(ir.model.appearance_bindings.len(), 1);
                assert!(
                    matches!(&ir.model.appearance_bindings[0].target, AppearanceTarget::Face(face) if face == &ir.model.faces[0].id)
                );
                let color = ir.model.appearances[0]
                    .base_color
                    .expect("authored entity53 color");
                assert_eq!([color.r(), color.g(), color.b()], [0.25, 0.5, 0.75]);
            }
            "authored_material" => {
                assert_eq!(ir.model.appearances.len(), 1);
                assert!(ir.model.appearance_bindings.is_empty());
                assert_eq!(
                    ir.model.appearances[0].name.as_deref(),
                    Some("Golden Steel")
                );
                let color = ir.model.appearances[0]
                    .base_color
                    .expect("authored visual property");
                for (actual, byte) in [color.r(), color.g(), color.b()]
                    .into_iter()
                    .zip([32.0, 64.0, 128.0])
                {
                    assert!((actual - byte / 255.0).abs() < EPS_AUTHORED_COLOR_COMPONENT);
                }
            }
            "authored_display_list" => {
                assert_eq!(ir.model.tessellations.len(), 1);
                assert_eq!(
                    ir.model.tessellations[0].body.as_ref(),
                    Some(&ir.model.bodies[0].id)
                );
                assert_eq!(
                    ir.model.tessellations[0].faces,
                    [ir.model.faces[0].id.clone()]
                );
            }
            "authored_document_envelope" => {
                let part = ir
                    .model
                    .attributes
                    .iter()
                    .find(|a| a.name == "part_record")
                    .expect("authored part metadata");
                assert!(matches!(
                    part.values.as_slice(),
                    [
                        Some(AttributeValue::Integer(42)),
                        Some(AttributeValue::Integer(2026))
                    ]
                ));
                let bbox = ir
                    .model
                    .attributes
                    .iter()
                    .find(|a| a.name == "bounding_envelope")
                    .expect("authored bounding envelope");
                let [Some(AttributeValue::Vector(values))] = bbox.values.as_slice() else {
                    panic!("bounding vector");
                };
                assert_eq!(
                    values.iter().map(|v| v.get()).collect::<Vec<_>>(),
                    [10.0, 20.0, -30.0, 40.0]
                );
            }
            "authored_pmi_dimension" => {
                let native = crate::test_support::native::sldprt_native(ir);
                assert_eq!(native.pmi_dimensions.len(), 1);
                assert_eq!(native.pmi_dimensions[0].cad_text, "D1@Sketch1");
                assert_eq!(native.pmi_dimensions[0].value.get(), 0.025);
                assert_eq!(native.pmi_dimensions[0].display_text(), Some("25.000 mm"));
                assert_eq!(ir.model.parameters.len(), 1);
            }
            _ => unreachable!(),
        }
    }
}
