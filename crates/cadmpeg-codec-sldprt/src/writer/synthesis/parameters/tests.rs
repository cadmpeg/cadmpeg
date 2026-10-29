// SPDX-License-Identifier: Apache-2.0
use super::move_unreferenced;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use std::io::Cursor;

#[test]
fn a_parameter_used_by_another_feature_pmi_or_itself_is_not_silently_removed() {
    for consumer in ["feature", "pmi", "self"] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><KnitSurface Name="First" Type="Knit" id="34" Faces="native-face" MergeEntities="false" CreateSolid="false"><Dimension Name="GapTolerance">0.01mm</Dimension></KnitSurface><KnitSurface Name="Other" Type="Knit" id="35" Faces="native-face" MergeEntities="false" CreateSolid="false"><Dimension Name="GapTolerance">0.02mm</Dimension></KnitSurface></Keywords>"#));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let (mut ir, _, _) = decoded.into_parts();
        let owner = ir.model.features[0].id.clone();
        let parameter = ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.owner.as_ref() == Some(&owner))
            .unwrap()
            .id
            .clone();
        ir.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                gap_tolerance, ..
            }) = definition
            else {
                panic!("first knit");
            };
            *gap_tolerance = None;
        });
        if consumer == "pmi" {
            ir.model.pmi.push(cadmpeg_ir::pmi::PmiAnnotation {
                id: "test:model:pmi#consumer".try_into().unwrap(),
                native_ref: None,
                name: None,
                visible: None,
                targets: Vec::new(),
                definition: cadmpeg_ir::pmi::PmiDefinition::Dimension(
                    cadmpeg_ir::pmi::PmiDimension::new(
                        cadmpeg_ir::pmi::DimensionKind::Size,
                        Some(cadmpeg_ir::Dimension::Parameter(parameter)),
                        None,
                    )
                    .unwrap(),
                ),
            });
        } else if consumer == "self" {
            ir.model
                .parameters
                .iter_mut()
                .find(|held| held.id == parameter)
                .unwrap()
                .expression = cadmpeg_ir::features::Expression {
                tokens: vec![cadmpeg_ir::features::ExpressionToken::Parameter(
                    parameter.clone(),
                )],
            };
        } else {
            let other_owner = &ir.model.features[1].id;
            let other_parameter = ir
                .model
                .parameters
                .iter()
                .find(|held| held.owner.as_ref() == Some(other_owner))
                .unwrap()
                .id
                .clone();
            ir.model.features[1].evaluation.edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                    gap_tolerance,
                    ..
                }) = definition
                else {
                    panic!("other knit");
                };
                *gap_tolerance = Some(cadmpeg_ir::Dimension::Parameter(parameter));
            });
            ir.model.pmi.push(cadmpeg_ir::pmi::PmiAnnotation {
                id: "test:model:pmi#other-parameter-consumer"
                    .try_into()
                    .unwrap(),
                native_ref: None,
                name: None,
                visible: None,
                targets: Vec::new(),
                definition: cadmpeg_ir::pmi::PmiDefinition::Dimension(
                    cadmpeg_ir::pmi::PmiDimension::new(
                        cadmpeg_ir::pmi::DimensionKind::Size,
                        Some(cadmpeg_ir::Dimension::Parameter(other_parameter)),
                        None,
                    )
                    .unwrap(),
                ),
            });
        }
        let before = ir.model.clone();
        let mut native = sldprt_native(&ir);
        let native_before = native.clone();
        let namespace_before = ir.native.clone();
        let mut losses = Vec::new();
        move_unreferenced(
            &cadmpeg_test_support::service_decode_context(),
            &mut ir,
            &mut native,
            &mut losses,
        )
        .unwrap();
        assert_eq!(ir.model, before);
        assert_eq!(native, native_before);
        assert_eq!(ir.native, namespace_before);
        assert!(losses.is_empty());
    }
}

#[test]
fn the_owning_feature_keeps_a_parameter_used_by_its_actual_definition_after_source_order_is_removed(
) {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Scale Name="Scale" Type="Scale" id="16" Bodies="body:1" CenterType="Origin"><Dimension Name="Factor">2</Dimension></Scale></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    let parameter = ir.model.parameters[0].id.clone();
    assert!(ir.model.features[0]
        .source_content
        .iter()
        .any(|content| matches!(content,
        cadmpeg_ir::features::FeatureSourceContent::Parameter(id) if id == &parameter)));
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Scale { factors, .. }) = definition
        else {
            panic!("scale")
        };
        *factors = cadmpeg_ir::features::ScaleFactors::PerAxis {
            factors: [
                cadmpeg_ir::Dimension::Parameter(parameter.clone()),
                cadmpeg_ir::Dimension::Literal(cadmpeg_ir::scalar::NonZeroReal::new(2.0).unwrap()),
                cadmpeg_ir::Dimension::Literal(cadmpeg_ir::scalar::NonZeroReal::new(3.0).unwrap()),
            ],
        };
    });
    let model = ir.model.clone();
    let mut native = sldprt_native(&ir);
    let original_native = native.clone();
    let mut losses = Vec::new();
    move_unreferenced(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
        &mut native,
        &mut losses,
    )
    .unwrap();
    assert_eq!(ir.model, model);
    assert_eq!(native, original_native);
    assert!(losses.is_empty());
}
