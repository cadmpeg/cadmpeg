// SPDX-License-Identifier: Apache-2.0
//! Native sketch-relation grouping and unit decode tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
use cadmpeg_ir::LossTaxonomy;

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_projects_owned_native_sketch_relation() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    let cadmpeg_ir::features::FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch),
        ..
    } = &feature.definition
    else {
        panic!("bound sketch feature");
    };
    let native = sldprt_native(decoded.ir());
    assert!(native.feature_input_lanes[0]
        .sketch_entities
        .iter()
        .all(|entity| entity.feature_ref.as_deref() == feature.native_ref.as_deref()));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected relation parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.is_some())
        .expect("projected native relation");
    assert_eq!(&constraint.sketch, sketch);
    assert!(constraint
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:relation-instance#")));
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            entities,
            parameter: Some(relation_parameter),
            operands,
            ..
        } if native_kind == "sgPntPntDist"
            && entities.is_empty()
            && relation_parameter == &parameter.id
            && operands.len() == 2
            && operands[0].native_kind == "d6"
            && operands[0].object_index == 0
            && operands[0].native_ref.is_some()
            && operands[1].native_kind == "d6"
            && operands[1].object_index == 2
            && operands[1].native_ref.is_none()
    ));
    let findings = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).findings;
    assert!(findings.is_empty(), "{findings:#?}");
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_groups_compact_relation_scalar_pair() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one compact relation instance");
    };
    assert_eq!(relation.scalar_refs.len(), 2);
    let driving = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Driving)
        .expect("driving scalar");
    let display = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Display)
        .expect("display scalar");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        Some(driving.id.as_str())
    );
    assert_eq!(
        relation.display_scalar_ref.as_deref(),
        Some(display.id.as_str())
    );
    assert_eq!(relation.operands.len(), 2);
    assert_eq!(relation.operands[0].entity_index, 0);
    assert_eq!(relation.operands[1].entity_index, 2);

    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected compact relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            parameter: Some(parameter),
            ..
        } if native_kind == "sgPntPntDist"
            && decoded.ir().model.parameters.iter().any(|candidate| {
                &candidate.id == parameter
                    && candidate.native_ref.as_deref() == Some(driving.id.as_str())
            })
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_starts_another_relation_after_two_repeated_operand_scalars() {
    let mut source = sldprt_with_tagged_compact_relation_names(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        &["Sketch1", "D1", "D2", "D3"],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_input_lanes[0].relation_instances.len(), 2);
    assert_eq!(
        native.feature_input_lanes[0]
            .relation_instances
            .iter()
            .map(|relation| relation.scalar_refs.len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn decode_groups_native_tagged_point_line_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntLineDist",
        [[0x7b, 0x83], [0x86, 0x83]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving point-line parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.references.len(), 4);
    assert!(lane
        .references
        .iter()
        .enumerate()
        .all(|(ordinal, reference)| {
            reference.kind
                == crate::records::FeatureInputOperandKind::Native(if ordinal % 2 == 0 {
                    0x837b
                } else {
                    0x8386
                })
        }));
    let [relation] = lane.relation_instances.as_slice() else {
        panic!("one point-line relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::PointLineDistance
    );
    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected point-line relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            operands,
            ..
        } if native_kind == "sgPntLineDist"
            && operands[0].native_kind == "7b83"
            && operands[1].native_kind == "8683"
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_uses_relation_units_for_bare_integer_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntPntVertDist",
        [[0xcb, 0x8d]; 2],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving vertical-distance parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(25.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_boolean_shaped_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation_scalar(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        0.001,
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">1</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving distance parameter");
    assert_eq!(parameter.expression, "1");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(1.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_bare_integer_angles() {
    use cadmpeg_ir::features::{Angle, ParameterValue};

    let mut source =
        sldprt_with_tagged_compact_relation(&triangle_body(), "sgAnglDim", [[0xda, 0x8d]; 2]);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving angle parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Angle(Angle(0.025))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_groups_unary_circle_diameter_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgCircleDim",
        [[0xfe, 0x83], [0, 0]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">&lt;MOD-DIAM&gt;25mm</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one circle-diameter relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::CircleDiameter
    );
    assert_eq!(relation.operands.len(), 1);
    assert_eq!(
        relation.operands[0].kind,
        crate::records::FeatureInputOperandKind::Native(0x83fe)
    );
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("diameter parameter");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        parameter.native_ref.as_deref()
    );
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            constraint.native_ref.as_deref() == Some(relation.id.as_str())
                && matches!(
                    &constraint.definition,
                    SketchConstraintDefinition::Native {
                        native_kind,
                        parameter: Some(bound_parameter),
                        operands,
                        ..
                    } if native_kind == "sgCircleDim"
                        && bound_parameter == &parameter.id
                        && operands.len() == 1
                        && operands[0].native_kind == "fe83"
                )
        }));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_groups_each_circle_dimension_operand_tag() {
    for tag in [
        [0xcc, 0x80],
        [0xfe, 0x83],
        [0xb6, 0x8a],
        [0x9d, 0x92],
        [0x69, 0xbd],
        [0x46, 0x81],
    ] {
        let mut source =
            sldprt_with_tagged_compact_relation(&triangle_body(), "sgCircleDim", [tag, [0, 0]]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(decoded.ir());
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one circle-diameter relation for tag {tag:02x?}");
        };
        assert_eq!(
            relation.family,
            crate::records::FeatureInputRelationFamily::CircleDiameter
        );
        let [operand] = relation.operands.as_slice() else {
            panic!("one circle-diameter operand for tag {tag:02x?}");
        };
        assert_eq!(
            operand.kind,
            crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))
        );
        assert_eq!(operand.entity_index, 0);
    }
}

#[test]
fn decode_uses_declaration_to_disambiguate_native_relation_tags() {
    let cases = [
        (
            "sgPntPntDist",
            [0x7b, 0x83],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgPntPntDist",
            [0xd5, 0x80],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgPntPntDist",
            [0x38, 0x81],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgPntPntDist",
            [0xac, 0x80],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x86, 0x83],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgLLDist",
            [0x0f, 0x81],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntDist",
            [0x7c, 0xbc],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x87, 0xbc],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntHorDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointHorizontalDistance,
        ),
        (
            "sgPntPntHorDist",
            [0xd5, 0x80],
            crate::records::FeatureInputRelationFamily::PointPointHorizontalDistance,
        ),
        (
            "sgPntPntVertDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointVerticalDistance,
        ),
        (
            "sgPntPntVertDist",
            [0xd5, 0x80],
            crate::records::FeatureInputRelationFamily::PointPointVerticalDistance,
        ),
        (
            "sgAnglDim",
            [0xda, 0x8d],
            crate::records::FeatureInputRelationFamily::Angle,
        ),
        (
            "sgAnglDim",
            [0xd5, 0x80],
            crate::records::FeatureInputRelationFamily::Angle,
        ),
    ];
    for (class, tag, family) in cases {
        let mut source = sldprt_with_tagged_compact_relation(&triangle_body(), class, [tag; 2]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let parameter = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D2")
            .expect("driving relation parameter");
        if family == crate::records::FeatureInputRelationFamily::Angle {
            assert_eq!(parameter.expression, "0.025rad");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Angle(
                    cadmpeg_ir::features::Angle(0.025)
                ))
            );
        } else {
            assert_eq!(parameter.expression, "25mm");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Length(
                    cadmpeg_ir::features::Length(25.0)
                ))
            );
        }
        let native = sldprt_native(decoded.ir());
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one native-tagged relation instance for {class}");
        };
        assert_eq!(relation.family, family);
        assert!(relation.operands.iter().all(|operand| operand.kind
            == crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))));
        assert!(decoded
            .ir()
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| {
                constraint.native_ref.as_deref() == Some(relation.id.as_str())
                    && matches!(
                        &constraint.definition,
                        cadmpeg_ir::sketches::SketchConstraintDefinition::Native {
                            native_kind,
                            ..
                        } if native_kind == class
                    )
            }));
    }
}
