// SPDX-License-Identifier: Apache-2.0
//! Edits of decoded parts that the writer writes instead of refusing: what it
//! cannot carry is kept as stored and named.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::features::parameters::{DesignParameter, ParameterValue};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::EditableDecodeResult;

use crate::loss::SldprtLossCode;
use crate::test_support::nist::{ftc_06, ftc_08};
use crate::SldprtCodec;

/// Writes through the public encoder: the export report and the bytes.
fn write(decoded: &EditableDecodeResult) -> (ExportReport, Vec<u8>) {
    let mut output = Vec::new();
    let report = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap()
        .write_to(&mut output)
        .unwrap();
    (report, output)
}

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn messages(report: &ExportReport, code: SldprtLossCode) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

fn unwritten(report: &ExportReport) -> Vec<&str> {
    let mut messages = messages(report, SldprtLossCode::WriterParameterEditUnwritten);
    messages.extend(self::messages(
        report,
        SldprtLossCode::WriterFeatureEditUnwritten,
    ));
    messages
}

fn owner<'a>(ir: &'a CadIr, parameter: &DesignParameter) -> &'a Feature {
    ir.model
        .features
        .iter()
        .find(|feature| Some(&feature.id) == parameter.owner.as_ref())
        .unwrap()
}

/// The parameter `name` of the feature named `owner`; ids move across a write.
fn named_parameter<'a>(ir: &'a CadIr, owner: &str, name: &str) -> &'a DesignParameter {
    ir.model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.name == name && self::owner(ir, parameter).name.as_deref() == Some(owner)
        })
        .unwrap()
}

fn is_extrusion(feature: &Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
    )
}

fn is_hole(feature: &Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Hole { .. })
    )
}

/// A plain length expression: a number with an optional `mm`.
fn plain_length(expression: &str) -> Option<(f64, &'static str)> {
    let trimmed = expression.trim();
    let (number, unit) = trimmed
        .strip_suffix("mm")
        .map_or((trimmed, ""), |number| (number.trim(), "mm"));
    Some((number.parse().ok()?, unit))
}

/// Moves the first plain length parameter of an extrusion by one millimetre,
/// in its expression and its value. Returns the owner and parameter names
/// and the new value.
fn edit_extrusion_length(decoded: &mut EditableDecodeResult) -> (String, String, f64) {
    let mut ir = decoded.ir_mut();
    let extrusions = ir
        .model
        .features
        .iter()
        .filter(|feature| is_extrusion(feature))
        .map(|feature| (feature.id.clone(), feature.name.clone().unwrap()))
        .collect::<Vec<_>>();
    let parameter = ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| {
            matches!(parameter.value, Some(ParameterValue::Length(_)))
                && plain_length(&parameter.expression).is_some()
                && extrusions
                    .iter()
                    .any(|(id, _)| Some(id) == parameter.owner.as_ref())
        })
        .unwrap();
    let Some(ParameterValue::Length(length)) = parameter.value else {
        unreachable!("a length parameter");
    };
    let (_, unit) = plain_length(&parameter.expression).unwrap();
    let value = length.get() + 1.0;
    parameter.expression = format!("{value}{unit}");
    parameter.value = Some(ParameterValue::Length(Length::new(value).unwrap()));
    let owner = extrusions
        .iter()
        .find(|(id, _)| Some(id) == parameter.owner.as_ref())
        .unwrap()
        .1
        .clone();
    (owner, parameter.name.clone(), value)
}

fn length(parameter: &DesignParameter) -> f64 {
    let Some(ParameterValue::Length(length)) = parameter.value else {
        panic!("{} is not a length", parameter.id);
    };
    length.get()
}

fn is_reference(parameter: &DesignParameter) -> bool {
    crate::resolved_features::relation_geometry::is_reference_relation_parameter(parameter)
}

#[test]
fn an_edit_beside_display_only_relation_parameters_is_written() {
    let mut decoded = EditableDecodeResult::from(ftc_08().clone());
    assert!(decoded.ir().model.parameters.iter().any(is_reference));
    let (owner, name, value) = edit_extrusion_length(&mut decoded);

    let (report, output) = write(&decoded);

    assert_eq!(unwritten(&report), Vec::<&str>::new());
    let written = decode(output);
    let parameter = named_parameter(written.ir(), &owner, &name);
    assert!((length(parameter) - value).abs() <= 1.0e-9, "{parameter:?}");
    assert!(written.ir().model.parameters.iter().any(is_reference));
}

#[test]
fn an_edited_display_only_relation_parameter_is_named_and_kept_as_solved() {
    let mut decoded = EditableDecodeResult::from(ftc_08().clone());
    let (id, owner, name, stored) = {
        let mut ir = decoded.ir_mut();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| is_reference(parameter))
            .unwrap();
        let stored = length(parameter);
        parameter.value = Some(ParameterValue::Length(Length::new(stored + 1.0).unwrap()));
        (
            parameter.id.clone(),
            parameter.owner.clone().unwrap(),
            parameter.name.clone(),
            stored,
        )
    };
    let owner = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id == owner)
        .unwrap()
        .name
        .clone()
        .unwrap();

    let (report, output) = write(&decoded);

    let unwritten = messages(&report, SldprtLossCode::WriterParameterEditUnwritten);
    assert_eq!(unwritten.len(), 1, "{:?}", report.losses);
    assert!(unwritten[0].contains(id.as_str()), "{}", unwritten[0]);
    let written = decode(output);
    let parameter = named_parameter(written.ir(), &owner, &name);
    assert!(
        (length(parameter) - stored).abs() <= 1.0e-9,
        "{parameter:?}"
    );
}

#[test]
fn a_bare_number_dimension_bound_to_a_scalar_is_written() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let (owner, name, value) = edit_extrusion_length(&mut decoded);
    let parameter = named_parameter(decoded.ir(), &owner, &name);
    assert!(parameter.native_ref.is_some());
    assert!(
        !parameter.expression.ends_with("mm"),
        "{}",
        parameter.expression
    );

    let (report, output) = write(&decoded);

    assert_eq!(unwritten(&report), Vec::<&str>::new());
    let written = decode(output);
    let parameter = named_parameter(written.ir(), &owner, &name);
    assert!((length(parameter) - value).abs() <= 1.0e-9, "{parameter:?}");
}

/// Renames the first feature that `selected` accepts and writes; the
/// feature is written with the new name, and nothing is reported unwritten.
fn rename_first(source: &DecodeResult, selected: fn(&Feature) -> bool) {
    let mut decoded = EditableDecodeResult::from(source.clone());
    let name = {
        let mut ir = decoded.ir_mut();
        let feature = ir
            .model
            .features
            .iter_mut()
            .find(|feature| selected(feature))
            .unwrap();
        let name = format!("{} renamed", feature.name.as_deref().unwrap());
        feature.name = Some(name.clone());
        name
    };

    let (report, output) = write(&decoded);

    assert_eq!(unwritten(&report), Vec::<&str>::new());
    let written = decode(output);
    assert!(written
        .ir()
        .model
        .features
        .iter()
        .any(|feature| selected(feature) && feature.name.as_deref() == Some(name.as_str())));
}

/// The hole encoder cannot restate every field the decoder states for these
/// holes; an unedited definition keeps its stored record.
#[test]
fn a_hole_the_encoder_cannot_restate_is_renamed() {
    rename_first(ftc_06(), is_hole);
}

/// The first extrusion's lane object name is a repeated-class record, away
/// from its class declaration; it is the one lane name that carries it.
#[test]
fn an_extrusion_named_by_a_repeated_class_record_is_renamed() {
    rename_first(ftc_06(), is_extrusion);
}

/// The fillets' compact edge selections resolve further than the lane form
/// the writer rebuilds; an unedited selection is not an edit.
#[test]
fn a_rename_beside_decoder_resolved_fillet_edges_is_written() {
    rename_first(ftc_08(), is_hole);
}

/// The envelope is regenerated from the source's block container, and a
/// compound-file source has none: no output can be produced.
#[test]
fn a_semantic_write_of_a_compound_file_part_is_refused_by_name() {
    let decoded = decode(cadmpeg_test_support::compound::compound_fixture());
    let (ir, _, fidelity) = decoded.into_parts();
    let records = crate::source_records(&ir, &fidelity).unwrap();
    let mut output = Vec::new();
    let result = crate::writer::write_semantic_with_records(
        &ir,
        &fidelity.annotations,
        &records,
        &mut output,
    );
    assert!(
        matches!(&result, Err(cadmpeg_core::CodecError::NotImplemented(message)) if message.contains("compound-file writer")),
        "{:?}",
        result.err()
    );
    assert!(output.is_empty());
}
