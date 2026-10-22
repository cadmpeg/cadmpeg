// SPDX-License-Identifier: Apache-2.0
//! Conflicting inactive parameter records lose their own encoding, not the operation.
use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{
    target::TargetRequest, ArenaDisposition, EncodeInput, Encoder, NativeDisposition,
};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation, ScaleFactors};
use std::io::Cursor;

fn assert_omission(report: &cadmpeg_ir::report::export::ExportReport, parameter: &str) {
    assert!(report.losses.iter().any(|loss| loss.code
        == SldprtLossCode::WriterEntityOmitted.kind()
        && loss.message.contains(parameter)));
    assert_eq!(
        report
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::DesignParameter),
        ArenaDisposition::Reported
    );
    assert_eq!(
        report.native_dispositions.namespaces["sldprt"].arenas["feature_histories"].disposition,
        NativeDisposition::Regenerated
    );
}

#[test]
fn per_axis_scale_omits_only_its_unused_uniform_parameter_and_keeps_the_independent_peer() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Scale Name="Scale" Type="Scale" id="16" Bodies="body:1" CenterType="Origin"><Dimension Name="Factor">2</Dimension></Scale><Feature Name="Peer" Type="EquationDriven" id="17"><Dimension Name="Independent">7</Dimension></Feature></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let omitted = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Factor")
        .unwrap()
        .id
        .to_string();
    decoded.ir_mut().model.features[0]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Scale {
                bodies, factors, ..
            }) = definition
            else {
                panic!("scale");
            };
            *bodies = BodySelection::Native("body:2,body:3".into());
            *factors = ScaleFactors::PerAxis {
                factors: [1.5, 2.0, 2.5].map(|value| {
                    cadmpeg_ir::Dimension::Literal(
                        cadmpeg_ir::scalar::NonZeroReal::new(value).unwrap(),
                    )
                }),
            };
        });
    let parameters = decoded.ir().model.parameters.clone();
    let input_native = decoded.ir().native.clone();
    for fidelity in [Some(decoded.source_fidelity()), None] {
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(decoded.ir(), fidelity),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert_omission(plan.report(), &omitted);
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(written.ir());
        let scale = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|feature| feature.name == "Scale")
            .unwrap();
        assert!(!scale.parameters.contains_key("Factor"));
        assert_eq!(scale.parameters["ScaleX"], "1.5");
        assert_eq!(scale.parameters["ScaleY"], "2");
        assert_eq!(scale.parameters["ScaleZ"], "2.5");
        assert!(written.ir().model.features.iter().any(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Scale { factors: ScaleFactors::PerAxis { factors }, .. }) if factors.iter().map(|factor| written.ir().dimension_value(factor).unwrap().get()).collect::<Vec<_>>() == [1.5, 2.0, 2.5])));
        assert!(written
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "Independent"
                && parameter.expression == cadmpeg_ir::features::Expression::text("7")));
        assert!(!written
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "Factor"));
        assert_eq!(
            written.ir().model.bodies.len(),
            decoded.ir().model.bodies.len()
        );
        assert_eq!(
            written.ir().model.faces.len(),
            decoded.ir().model.faces.len()
        );
    }
    assert_eq!(decoded.ir().model.parameters, parameters);
    assert_eq!(decoded.ir().native, input_native);
}

#[test]
fn knit_without_gap_omits_only_the_unused_tolerance_and_keeps_its_operation_and_peer() {
    let mut source = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    let face = base.ir().model.faces[0].id.to_string();
    let xml = format!(
        r#"<Keywords><KnitSurface Name="Knit" Type="Knit" id="34" Faces="{face}" MergeEntities="false" CreateSolid="false" CheckGeometry="true"><Dimension Name="GapTolerance">0.01mm</Dimension></KnitSurface><Feature Name="Peer" Type="EquationDriven" id="35"><Dimension Name="Independent">7</Dimension></Feature></Keywords>"#
    );
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let omitted = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "GapTolerance")
        .unwrap()
        .id
        .to_string();
    decoded.ir_mut().model.features[0]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                gap_tolerance,
                merge_entities,
                create_solid,
                ..
            }) = definition
            else {
                panic!("knit");
            };
            *gap_tolerance = None;
            *merge_entities = Some(true);
            *create_solid = Some(true);
        });
    let parameters = decoded.ir().model.parameters.clone();
    let input_native = decoded.ir().native.clone();
    for fidelity in [Some(decoded.source_fidelity()), None] {
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(decoded.ir(), fidelity),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert_omission(plan.report(), &omitted);
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(written.ir());
        let knit = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|feature| feature.name == "Knit")
            .unwrap();
        assert!(!knit.parameters.contains_key("GapTolerance"));
        assert_eq!(knit.properties["CheckGeometry"], "true");
        assert!(written.ir().model.features.iter().any(|feature| matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                gap_tolerance: None,
                merge_entities: Some(true),
                create_solid: Some(true),
                ..
            })
        )));
        assert!(written
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "Independent"
                && parameter.expression == cadmpeg_ir::features::Expression::text("7")));
        assert!(!written
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "GapTolerance"));
        assert_eq!(
            written.ir().model.bodies.len(),
            decoded.ir().model.bodies.len()
        );
        assert_eq!(
            written.ir().model.faces.len(),
            decoded.ir().model.faces.len()
        );
    }
    assert_eq!(decoded.ir().model.parameters, parameters);
    assert_eq!(decoded.ir().native, input_native);
}

mod laws;
mod shortened_law;
