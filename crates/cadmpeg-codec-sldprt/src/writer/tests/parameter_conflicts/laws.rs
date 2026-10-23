// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::features::{edge_treatments::RadiusSpec, RibDraft};

#[test]
fn rib_without_draft_keeps_its_current_thickness_and_moves_only_conflicting_parameters_to_the_global_owner(
) {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Sketch Name="RibProfile" Type="Sketch" id="41"/><Rib Name="Web" Type="Rib" id="42" Profile="41" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension><Dimension Name="Draft">5deg</Dimension></Rib><Feature Name="Peer" Type="EquationDriven" id="43"><Dimension Name="Independent">7</Dimension></Feature></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let owner = decoded.ir().model.features[1].id.clone();
    let moved = decoded
        .ir()
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&owner))
        .map(|parameter| {
            (
                parameter.id.to_string(),
                parameter.name.clone(),
                parameter.expression.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(moved.len(), 2);
    decoded.ir_mut().model.features[1]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Rib { construction, .. }) =
                definition
            else {
                panic!("rib")
            };
            construction.draft = RibDraft::None;
            construction.thickness = Some(cadmpeg_ir::Dimension::Literal(
                cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
            ));
        });
    let input = decoded.ir().model.clone();
    let input_native = decoded.ir().native.clone();
    for fidelity in [Some(decoded.source_fidelity()), None] {
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(decoded.ir(), fidelity),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert_regenerated(plan.report());
        let mut bytes = Vec::new();
        let report = plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        for (parameter, name, expression) in &moved {
            assert_moved_to_global(&report, written.ir(), parameter, name, expression);
        }
        let native = sldprt_native(written.ir());
        let rib = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|feature| feature.name == "Web")
            .unwrap();
        assert!(!rib.parameters.contains_key("Draft"));
        assert_eq!(rib.parameters["Thickness"], "3mm");
        assert!(written.ir().model.features.iter().any(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Rib { construction, .. }) if matches!(construction.draft, RibDraft::None) && written.ir().dimension_value(construction.thickness.as_ref().unwrap()).unwrap().get() == 3.0)));
        assert!(written
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "Independent"
                && parameter.expression == cadmpeg_ir::features::Expression::text("7")));
        assert_eq!(written.ir().model.bodies.len(), input.bodies.len());
        assert_eq!(written.ir().model.faces.len(), input.faces.len());
    }
    assert_eq!(decoded.ir().model, input);
    assert_eq!(decoded.ir().native, input_native);
}

#[test]
fn constant_fillet_keeps_its_current_radius_and_moves_the_unused_variable_law_parameters_to_the_global_owner(
) {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Fillet Name="Blend" Type="Fillet" id="61" Edges="edge:1"><Dimension Name="Position0">0</Dimension><Dimension Name="Radius0">2mm</Dimension><Dimension Name="Position1">0.5</Dimension><Dimension Name="Radius1">4mm</Dimension><Dimension Name="Position2">1</Dimension><Dimension Name="Radius2">3mm</Dimension></Fillet><Feature Name="Peer" Type="EquationDriven" id="62"><Dimension Name="Independent">7</Dimension></Feature></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let owner = decoded.ir().model.features[0].id.clone();
    let moved = decoded
        .ir()
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&owner))
        .map(|parameter| {
            (
                parameter.id.to_string(),
                parameter.name.clone(),
                parameter.expression.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(moved.len(), 6);
    decoded.ir_mut().model.features[0]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition
            else {
                panic!("fillet")
            };
            groups[0].radius = RadiusSpec::Constant {
                radius: cadmpeg_ir::Dimension::Literal(
                    cadmpeg_ir::scalar::PositiveLength::new(6.0).unwrap(),
                ),
            };
        });
    let input = decoded.ir().model.clone();
    let input_native = decoded.ir().native.clone();
    for fidelity in [Some(decoded.source_fidelity()), None] {
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(decoded.ir(), fidelity),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert_regenerated(plan.report());
        let mut bytes = Vec::new();
        let report = plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        for (parameter, name, expression) in &moved {
            assert_moved_to_global(&report, written.ir(), parameter, name, expression);
        }
        let native = sldprt_native(written.ir());
        let fillet = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|feature| feature.name == "Blend")
            .unwrap();
        assert_eq!(fillet.parameters["Radius"], "6mm");
        assert_eq!(fillet.properties["Edges"], "edge:1");
        assert!(!fillet
            .parameters
            .keys()
            .any(|name| name.as_str().starts_with("Position")
                || name.as_str().starts_with("Radius0")
                || name.as_str().starts_with("Radius1")
                || name.as_str().starts_with("Radius2")));
        assert!(written.ir().model.features.iter().any(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if matches!(&groups[0].radius, RadiusSpec::Constant { radius } if written.ir().dimension_value(radius).unwrap().get() == 6.0))));
        assert!(written
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "Independent"
                && parameter.expression == cadmpeg_ir::features::Expression::text("7")));
        assert_eq!(written.ir().model.bodies.len(), input.bodies.len());
        assert_eq!(written.ir().model.faces.len(), input.faces.len());
    }
    assert_eq!(decoded.ir().model, input);
    assert_eq!(decoded.ir().native, input_native);
}

#[test]
fn variable_fillet_keeps_the_edited_sample_and_moves_only_its_two_conflicting_parameters_to_the_global_owner(
) {
    use cadmpeg_ir::features::edge_treatments::{VariableRadii, VariableRadius};
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Fillet Name="Blend" Type="Fillet" id="61" Edges="edge:1"><Dimension Name="Position0">0</Dimension><Dimension Name="Radius0">2mm</Dimension><Dimension Name="Position1">0.5</Dimension><Dimension Name="Radius1">4mm</Dimension><Dimension Name="Position2">1</Dimension><Dimension Name="Radius2">3mm</Dimension></Fillet><Feature Name="Peer" Type="EquationDriven" id="62"><Dimension Name="Independent">7</Dimension></Feature></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let owner = decoded.ir().model.features[0].id.clone();
    let moved = decoded
        .ir()
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.owner.as_ref() == Some(&owner)
                && matches!(parameter.name.as_str(), "Position1" | "Radius1")
        })
        .map(|parameter| {
            (
                parameter.id.to_string(),
                parameter.name.clone(),
                parameter.expression.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(moved.len(), 2);
    decoded.ir_mut().model.features[0]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition
            else {
                panic!("fillet")
            };
            let RadiusSpec::Variable { points } = &mut groups[0].radius else {
                panic!("variable law")
            };
            let mut samples = points
                .as_slice()
                .iter()
                .map(VariableRadius::to_raw)
                .collect::<Vec<_>>();
            samples[1].parameter = 0.4;
            samples[1].radius =
                cadmpeg_ir::Dimension::Literal(cadmpeg_ir::scalar::Length::new(5.0).unwrap());
            *points = VariableRadii::new(samples).unwrap();
        });
    let input = decoded.ir().model.clone();
    let input_native = decoded.ir().native.clone();
    for fidelity in [Some(decoded.source_fidelity()), None] {
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(decoded.ir(), fidelity),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert_regenerated(plan.report());
        assert_eq!(
            plan.report()
                .losses
                .iter()
                .filter(
                    |loss| loss.code == SldprtLossCode::WriterParameterMovedToGlobal.kind()
                        && loss.message.starts_with("parameter ")
                )
                .count(),
            2
        );
        let mut bytes = Vec::new();
        let report = plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        for (parameter, name, expression) in &moved {
            assert_moved_to_global(&report, written.ir(), parameter, name, expression);
        }
        let native = sldprt_native(written.ir());
        let fillet = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|feature| feature.name == "Blend")
            .unwrap();
        assert_eq!(fillet.parameters["Position1"], "0.4");
        assert_eq!(fillet.parameters["Radius1"], "5mm");
        assert!(written.ir().model.features.iter().any(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if matches!(&groups[0].radius, RadiusSpec::Variable { points } if points.as_slice()[1].parameter.get() == 0.4 && written.ir().dimension_value(&points.as_slice()[1].radius).unwrap().get() == 5.0))));
        assert!(written
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "Independent"
                && parameter.expression == cadmpeg_ir::features::Expression::text("7")));
        assert_eq!(written.ir().model.bodies.len(), input.bodies.len());
        assert_eq!(written.ir().model.faces.len(), input.faces.len());
    }
    assert_eq!(decoded.ir().model, input);
    assert_eq!(decoded.ir().native, input_native);
}
