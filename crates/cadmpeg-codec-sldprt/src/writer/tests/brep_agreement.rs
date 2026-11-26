// SPDX-License-Identifier: Apache-2.0
//! Edits that shape the solid are written over the stored B-rep and named.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::extents::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_test_support::EditableDecodeResult;

use crate::container;
use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::native::{sldprt_native, update_sldprt_native};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

fn decode(source: &[u8]) -> EditableDecodeResult {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                source,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    EditableDecodeResult::from(decoded)
}

/// The active Parasolid partition: the stored solid.
fn partition(bytes: &[u8]) -> Vec<u8> {
    container::select_active_parasolid_site(&crate::test_support::container::scan(bytes))
        .unwrap()
        .section
        .payload()
        .to_vec()
}

/// Write through the public encoder: the export report and the bytes.
fn write(decoded: &EditableDecodeResult) -> (ExportReport, Vec<u8>) {
    let mut output = Vec::new();
    let report = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        TargetRequest::Unnamed,
    )
    .unwrap()
    .write_to(&mut output)
    .unwrap();
    (report, output)
}

/// Decode without the native envelope fixture, so that the write plans
/// through the public encoder.
fn decode_without_envelope(source: &[u8]) -> EditableDecodeResult {
    EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(source.to_vec()), &DecodeOptions::default())
            .unwrap(),
    )
}

/// Write with the retained source image reduced to its digest.
fn write_without_source_image(decoded: &mut EditableDecodeResult) -> (ExportReport, Vec<u8>) {
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    let mut output = Vec::new();
    let report = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Unnamed,
        )
        .unwrap()
        .write_to(&mut output)
        .unwrap();
    (report, output)
}

/// The stale-solid note of a write without the retained source image.
fn imageless_note(records: &str) -> String {
    format!(
        "The {records} differ from the decoded source. The stored solid is written as decoded, \
         and without the retained source image the writer cannot tell whether the edit shapes it."
    )
}

fn has_synthesized_snapshot(decoded: &EditableDecodeResult) -> bool {
    decoded
        .ir()
        .source
        .as_ref()
        .unwrap()
        .attributes
        .contains_key("sldprt_configuration_snapshot_synthesized")
}

fn stale_messages(report: &ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::WriterSolidStaleAfterEdit.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

fn unwritten_messages(report: &ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::WriterConfigurationSnapshotEditUnwritten.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

fn named<'a>(decoded: &'a EditableDecodeResult, name: &str) -> &'a Feature {
    decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap()
}

fn blind_depth(feature: &Feature) -> f64 {
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length },
                        ..
                    },
            },
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("one-sided blind extrusion: {feature:?}");
    };
    length.get()
}

fn set_blind_depth(decoded: &mut EditableDecodeResult, index: usize, depth: f64) {
    decoded.ir_mut().model.features[index]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = definition
            else {
                panic!("typed extrusion feature");
            };
            *extent = ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(depth).unwrap(),
                    },
                    draft: None,
                },
            };
        });
}

#[test]
fn an_extrusion_depth_edit_is_written_over_the_stored_solid_and_named() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let boss = named(&decoded, "Boss").id.clone();
    let index = decoded
        .ir()
        .model
        .features
        .iter()
        .position(|feature| feature.id == boss)
        .unwrap();
    assert!((blind_depth(&decoded.ir().model.features[index]) - 12.5).abs() <= 1.0e-9);
    set_blind_depth(&mut decoded, index, 50.0);

    let (report, output) = write(&decoded);

    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(
        stale[0].contains(boss.as_str()) && stale[0].contains("\"Boss\""),
        "{}",
        stale[0]
    );
    assert!(stale[0].contains("before the edit"), "{}", stale[0]);
    let written = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(output.clone()), &DecodeOptions::default())
            .unwrap(),
    );
    assert!((blind_depth(named(&written, "Boss")) - 50.0).abs() <= 1.0e-9);
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn a_suppression_edit_is_written_over_the_stored_solid_and_named() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let boss = named(&decoded, "Boss").id.clone();
    assert_ne!(named(&decoded, "Boss").suppressed, Some(true));
    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == boss)
        .unwrap()
        .suppressed = Some(true);
    decoded
        .ir_mut()
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.active)
        .unwrap()
        .feature_states
        .get_mut(&boss)
        .unwrap()
        .evaluation = cadmpeg_ir::features::parameters::ConfigurationEvaluation::Suppressed {};

    let (report, output) = write(&decoded);

    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(
        stale[0].contains(boss.as_str()) && stale[0].contains("written suppressed"),
        "{}",
        stale[0]
    );
    let written = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(output.clone()), &DecodeOptions::default())
            .unwrap(),
    );
    assert_eq!(named(&written, "Boss").suppressed, Some(true));
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn removing_a_feature_that_shapes_the_solid_is_written_and_named() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords Name="Bracket"><Configuration Name="Default" SourceIndex="0" Material="Steel" DisplayState="Shaded"/><Extrusion Name="Boss" Type="BossExtrude" id="7" Scope="Body1"><Dimension Name="Depth">12.5mm</Dimension></Extrusion><Extrusion Name="Cut" Type="CutExtrude" id="9" Scope="Body1"><Dimension Name="Depth">2mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = decode(&source);
    let cut = named(&decoded, "Cut").id.clone();
    {
        let mut edit = decoded.ir_mut();
        edit.model.features.retain(|feature| feature.id != cut);
        edit.model
            .parameters
            .retain(|parameter| parameter.owner.as_ref() != Some(&cut));
    }

    let (report, output) = write(&decoded);

    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(
        stale[0].contains(cut.as_str()) && stale[0].contains("removed"),
        "{}",
        stale[0]
    );
    let written = SldprtCodec
        .decode(&mut Cursor::new(output.clone()), &DecodeOptions::default())
        .unwrap();
    let names = written
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| feature.name.as_deref())
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"Boss") && !names.contains(&"Cut"),
        "{names:?}"
    );
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn feature_output_scope_edit_is_written_and_named() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let feature_id = decoded.ir().model.features[0].id.clone();
    let body = decoded.ir().geometry.bodies[0].id.clone();
    let replacement = if decoded.ir().model.features[0]
        .evaluation
        .outputs()
        .is_empty()
    {
        vec![body]
    } else {
        Vec::new()
    };
    decoded.ir_mut().model.features[0].evaluation.set_outputs(
        cadmpeg_ir::features::selections::DistinctMembers::try_from(replacement).unwrap(),
    );
    let (report, output) = write(&decoded);
    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(
        stale[0].contains(feature_id.as_str()) && stale[0].contains("output bodies"),
        "{}",
        stale[0]
    );
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn feature_output_edit_with_nongeometric_history_edit_is_written_and_named() {
    let mut source = sldprt_with_body_and_history(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords-Equations",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="99"><Dimension Name="Width">4mm</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = decode(&source);
    let feature_id = named(&decoded, "Boss").id.clone();
    let body = decoded.ir().geometry.bodies[0].id.clone();
    let mut edit = decoded.ir_mut();
    let modeling = edit
        .model
        .features
        .iter()
        .position(|feature| feature.id == feature_id)
        .unwrap();
    let nongeometric = edit
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("Equations"))
        .unwrap();
    let replacement = if edit.model.features[modeling]
        .evaluation
        .outputs()
        .is_empty()
    {
        vec![body]
    } else {
        Vec::new()
    };
    edit.model.features[modeling].evaluation.set_outputs(
        cadmpeg_ir::features::selections::DistinctMembers::try_from(replacement).unwrap(),
    );
    edit.model.features[nongeometric].source_text = Some("changed".into());
    drop(edit);
    let (report, _) = write(&decoded);
    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(stale[0].contains(feature_id.as_str()), "{}", stale[0]);
}

#[test]
fn extrusion_parameter_value_edit_is_written_and_named() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let boss = named(&decoded, "Boss").id.clone();
    let mut edit = decoded.ir_mut();
    let parameter = edit
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    let parameter_id = parameter.id.clone();
    parameter.expression = "50mm".into();
    parameter.value = Some(cadmpeg_ir::features::parameters::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
    ));
    drop(edit);
    let (report, output) = write(&decoded);
    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(
        stale[0].contains(parameter_id.as_str())
            && stale[0].contains("50mm")
            && stale[0].contains(boss.as_str()),
        "{}",
        stale[0]
    );
    let written = SldprtCodec
        .decode(&mut Cursor::new(output.clone()), &DecodeOptions::default())
        .unwrap();
    let depth = written
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    assert_eq!(depth.expression, "50mm");
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn removing_referenced_extrusion_parameter_is_written_and_named() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let parameter_id = decoded.ir().model.parameters[0].id.clone();
    decoded.ir_mut().model.parameters.clear();
    let (report, _) = write(&decoded);
    assert!(
        stale_messages(&report)
            .iter()
            .any(|message| message.contains(parameter_id.as_str())),
        "{:#?}",
        report.losses
    );
}

#[test]
fn native_extrusion_depth_edit_is_written_and_named() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let native_id = sldprt_native(decoded.ir()).feature_histories[0].features[0]
        .id
        .clone();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), "50mm".into());
    });
    let (report, output) = write(&decoded);
    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(stale[0].contains(&native_id), "{}", stale[0]);
    let written = SldprtCodec
        .decode(&mut Cursor::new(output.clone()), &DecodeOptions::default())
        .unwrap();
    let depth = written
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    assert_eq!(depth.expression, "50mm");
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn native_extrusion_edit_without_source_image_is_written_and_named() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode_without_envelope(&source);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), "50mm".into());
    });
    let (report, _) = write_without_source_image(&mut decoded);
    assert_eq!(
        stale_messages(&report),
        [imageless_note("native history records").as_str()]
    );
    assert!(decoded
        .source_fidelity()
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .is_none());
}

#[test]
fn an_unedited_write_without_the_source_image_reports_no_stale_solid() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode_without_envelope(&source);
    assert!(has_synthesized_snapshot(&decoded));
    let (report, _) = write_without_source_image(&mut decoded);
    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    assert!(
        unwritten_messages(&report).is_empty(),
        "{:?}",
        report.losses
    );
}

#[test]
fn an_unedited_write_without_a_sidecar_reports_no_stale_solid() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let decoded = decode_without_envelope(&source);
    assert!(has_synthesized_snapshot(&decoded));
    let report = SldprtCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Unnamed)
        .unwrap()
        .write_to(&mut Vec::new())
        .unwrap();
    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    assert!(
        unwritten_messages(&report).is_empty(),
        "{:?}",
        report.losses
    );
}

#[test]
fn a_feature_rename_without_the_source_image_is_named_as_an_edit_that_may_not_shape_the_solid() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode_without_envelope(&source);
    decoded.ir_mut().model.features[0].name = Some("RenamedBoss".into());
    let (report, _) = write_without_source_image(&mut decoded);
    assert_eq!(
        stale_messages(&report),
        [imageless_note("features").as_str()]
    );
    assert!(
        unwritten_messages(&report).is_empty(),
        "{:?}",
        report.losses
    );
}

#[test]
fn a_parameter_rename_without_the_source_image_is_named_as_an_edit_that_may_not_shape_the_solid() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode_without_envelope(&source);
    decoded.ir_mut().model.parameters[0].name = "RenamedDepth".into();
    let (report, _) = write_without_source_image(&mut decoded);
    assert_eq!(
        stale_messages(&report),
        [imageless_note("parameters").as_str()]
    );
    assert!(
        unwritten_messages(&report).is_empty(),
        "{:?}",
        report.losses
    );
}

#[test]
fn parameter_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    decoded.ir_mut().model.parameters[0].name = "RenamedDepth".into();
    let (report, output) = write(&decoded);
    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn feature_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    decoded.ir_mut().model.features[0].name = Some("RenamedBoss".into());
    let (report, output) = write(&decoded);
    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn native_feature_name_edit_keeps_retained_brep() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0].name = "RenamedBoss".into();
    });
    let (report, output) = write(&decoded);
    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn a_model_suppression_with_an_unedited_snapshot_is_written_and_named_once() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let boss = named(&decoded, "Boss").id.clone();
    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == boss)
        .unwrap()
        .suppressed = Some(true);

    let (report, output) = write(&decoded);

    let stale = stale_messages(&report);
    assert_eq!(stale.len(), 1, "{:?}", report.losses);
    assert!(
        stale[0].contains(boss.as_str()) && stale[0].contains("written suppressed"),
        "{}",
        stale[0]
    );
    assert!(
        unwritten_messages(&report).is_empty(),
        "{:?}",
        report.losses
    );
    // The written part restates the active snapshot from the model, so its
    // active configuration states the suppression the model states.
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.invalid"),
        "{:?}",
        report.losses
    );
    let written = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .unwrap(),
    );
    assert_eq!(named(&written, "Boss").suppressed, Some(true));
}

#[test]
fn a_suppression_edited_only_in_the_synthesized_snapshot_is_named_unwritten() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let boss = named(&decoded, "Boss").id.clone();
    decoded
        .ir_mut()
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.active)
        .unwrap()
        .feature_states
        .get_mut(&boss)
        .unwrap()
        .evaluation = cadmpeg_ir::features::parameters::ConfigurationEvaluation::Suppressed {};

    let (report, output) = write(&decoded);

    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    let unwritten = unwritten_messages(&report);
    assert_eq!(unwritten.len(), 1, "{:?}", report.losses);
    assert!(
        unwritten[0].contains(boss.as_str()) && unwritten[0].contains("does not write"),
        "{}",
        unwritten[0]
    );
    let written = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(output.clone()), &DecodeOptions::default())
            .unwrap(),
    );
    assert_ne!(named(&written, "Boss").suppressed, Some(true));
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn a_parameter_value_edited_only_in_the_synthesized_snapshot_is_named_unwritten() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode(&source);
    let depth = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap()
        .id
        .clone();
    *decoded
        .ir_mut()
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.active)
        .unwrap()
        .parameter_values
        .get_mut(&depth)
        .unwrap() = cadmpeg_ir::features::parameters::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
    );

    let (report, output) = write(&decoded);

    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    let unwritten = unwritten_messages(&report);
    assert_eq!(unwritten.len(), 1, "{:?}", report.losses);
    assert!(
        unwritten[0].contains(depth.as_str()) && unwritten[0].contains("does not write"),
        "{}",
        unwritten[0]
    );
    let written = SldprtCodec
        .decode(&mut Cursor::new(output.clone()), &DecodeOptions::default())
        .unwrap();
    let written_depth = written
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    assert_eq!(written_depth.expression, "12.5mm");
    assert_eq!(partition(&output), partition(&source));
}

#[test]
fn a_snapshot_only_parameter_edit_without_the_source_image_is_named_unwritten() {
    let source = sldprt_with_body_and_history(&triangle_body());
    let mut decoded = decode_without_envelope(&source);
    assert!(has_synthesized_snapshot(&decoded));
    let depth = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap()
        .id
        .clone();
    *decoded
        .ir_mut()
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.active)
        .unwrap()
        .parameter_values
        .get_mut(&depth)
        .unwrap() = cadmpeg_ir::features::parameters::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(50.0).unwrap(),
    );

    let (report, output) = write_without_source_image(&mut decoded);

    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    let unwritten = unwritten_messages(&report);
    assert_eq!(unwritten.len(), 1, "{:?}", report.losses);
    assert!(
        unwritten[0].contains(depth.as_str()) && unwritten[0].contains("does not write"),
        "{}",
        unwritten[0]
    );
    let written = SldprtCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    let written_depth = written
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    assert_eq!(written_depth.expression, "12.5mm");
    assert_eq!(
        written
            .ir()
            .model
            .configurations
            .iter()
            .find(|configuration| configuration.active)
            .unwrap()
            .parameter_values
            .get(&written_depth.id),
        Some(&cadmpeg_ir::features::parameters::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(12.5).unwrap()
        ))
    );
}

/// A dependency that the feature's operands do not state reads back as
/// stored: the write keeps it as stored and names it.
#[test]
fn a_dependency_its_operands_do_not_state_is_written_and_named() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords Name="Bracket"><Configuration Name="Default" SourceIndex="0" Material="Steel" DisplayState="Shaded"/><Extrusion Name="Boss" Type="BossExtrude" id="7" Scope="Body1"><Dimension Name="Depth">12.5mm</Dimension></Extrusion><Extrusion Name="Cut" Type="CutExtrude" id="9" Scope="Body1"><Dimension Name="Depth">2mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = decode(&source);
    let boss = named(&decoded, "Boss").id.clone();
    let cut = named(&decoded, "Cut").id.clone();
    {
        let mut edit = decoded.ir_mut();
        for feature in &mut edit.model.features {
            if feature.id == boss {
                feature.suppressed = Some(true);
            }
            if feature.id == cut {
                let mut dependencies = feature.dependencies.to_vec();
                dependencies.push(boss.clone());
                feature.dependencies =
                    cadmpeg_ir::features::selections::DistinctMembers::try_from(dependencies)
                        .unwrap();
            }
        }
    }

    let ir = decoded.ir().clone();
    let report = crate::test_support::plan_fixture_with_native_envelope(
        cadmpeg_ir::codec::write::EncodeInput::new(&ir, Some(decoded.source_fidelity())),
        cadmpeg_ir::codec::write::target::TargetRequest::Unnamed,
    )
    .unwrap()
    .write_to(&mut Vec::new())
    .unwrap();
    let unwritten = report
        .losses
        .iter()
        .filter(|loss| crate::test_support::is_edit_dropped(&loss.code))
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        unwritten
            .iter()
            .any(|message| message.contains(cut.as_str()) && message.contains("dependencies")),
        "{:?}",
        report.losses
    );
}

#[test]
fn a_rename_beside_a_bound_profile_sketch_reports_no_stale_solid() {
    let mut source =
        crate::test_support::history::sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords Name="Bracket"><Configuration Name="Default" SourceIndex="0" Material="Steel" DisplayState="Shaded"/><Sketch Name="Sketch1" Type="ProfileFeature"/><Extrusion Name="Boss" Type="BossExtrude" id="7" Scope="Body1"><Dimension Name="Depth">12.5mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = decode(&source);
    assert!(matches!(
        named(&decoded, "Sketch1").evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(_)),
        })
    ));
    let boss = named(&decoded, "Boss").id.clone();
    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == boss)
        .unwrap()
        .name = Some("RenamedBoss".into());

    let (report, output) = write(&decoded);

    assert!(stale_messages(&report).is_empty(), "{:?}", report.losses);
    assert!(
        unwritten_messages(&report).is_empty(),
        "{:?}",
        report.losses
    );
    assert_eq!(partition(&output), partition(&source));
}
