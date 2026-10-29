// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::features::edge_treatments::RadiusSpec;

#[test]
fn shortening_a_variable_fillet_moves_unused_removed_samples_to_the_global_owner_without_resurrecting_them(
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
                && matches!(
                    parameter.name.as_str(),
                    "Position1" | "Radius1" | "Position2" | "Radius2"
                )
        })
        .map(|parameter| {
            (
                parameter.id.to_string(),
                parameter.name.clone(),
                parameter.expression.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(moved.len(), 4);
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
            assert_eq!(samples.len(), 3);
            samples.truncate(2);
            samples[1].parameter = 1.0;
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
            4
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
        assert_eq!(fillet.parameters["Position0"], "0");
        assert_eq!(fillet.parameters["Radius0"], "2mm");
        assert_eq!(fillet.parameters["Position1"], "1");
        assert!(!fillet.parameters.contains_key("Position2"));
        assert!(!fillet.parameters.contains_key("Radius2"));
        assert_eq!(fillet.parameters["Radius1"], "5mm");
        assert!(written.ir().model.features.iter().any(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if matches!(&groups[0].radius, RadiusSpec::Variable { points } if points.as_slice().len() == 2 && points.as_slice()[1].parameter.get() == 1.0 && written.ir().dimension_value(&points.as_slice()[1].radius).unwrap().get() == 5.0))));
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
