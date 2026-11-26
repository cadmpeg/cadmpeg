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
use crate::test_support::readback;
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

fn decode_read_back(bytes: &[u8]) -> DecodeResult {
    crate::decode::decode_for_read_back(bytes).unwrap()
}

fn messages(report: &ExportReport, code: SldprtLossCode) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The message of every loss but the stale-solid notes, which name an edit
/// as written and name no difference of the read-back.
fn all_messages(report: &ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code != SldprtLossCode::WriterSolidStaleAfterEdit.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The messages of the `export:edit.dropped` losses.
fn unwritten(report: &ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| crate::test_support::is_edit_dropped(&loss.code))
        .map(|loss| loss.message.as_str())
        .collect()
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
    let written = decode_read_back(&output);
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

    let unwritten = unwritten(&report);
    assert_eq!(unwritten.len(), 1, "{:?}", report.losses);
    assert!(unwritten[0].contains(id.as_str()), "{}", unwritten[0]);
    assert_eq!(stale(&report), Vec::<&str>::new());
    let written = decode_read_back(&output);
    let source = source(&decoded);
    assert_eq!(
        readback::unnamed(
            readback::beyond(
                readback::diff(source.ir(), written.ir()),
                &readback::baseline(&source)
            ),
            &all_messages(&report)
        ),
        Vec::new()
    );
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
    let written = decode_read_back(&output);
    let parameter = named_parameter(written.ir(), &owner, &name);
    assert!((length(parameter) - value).abs() <= 1.0e-9, "{parameter:?}");
}

/// The hole encoder cannot restate every field the decoder states for these
/// holes; an unedited definition keeps its stored record.
#[test]
fn a_hole_the_encoder_cannot_restate_is_renamed() {
    a_rename_keeps_every_other_record(ftc_06(), is_hole);
}

/// The first extrusion's lane object name is a repeated-class record, away
/// from its class declaration; it is the one lane name that carries it.
#[test]
fn an_extrusion_named_by_a_repeated_class_record_is_renamed() {
    a_rename_keeps_every_other_record(ftc_06(), is_extrusion);
}

/// The fillets' compact edge selections resolve further than the lane form
/// the writer rebuilds; an unedited selection is not an edit, and the
/// fillets read back as stored.
#[test]
fn a_rename_beside_decoder_resolved_fillet_edges_is_written() {
    a_rename_keeps_every_other_record(ftc_08(), is_hole);
}

/// An edited compound-file part with a global parameter, which no feature
/// record holds, is written into a native envelope without that parameter,
/// and the history drop names it.
#[test]
fn an_edited_compound_file_part_drops_a_global_parameter() {
    let source = cadmpeg_test_support::compound::compound_fixture();
    let mut decoded = EditableDecodeResult::from(decode(source.clone()));
    decoded.ir_mut().model.parameters.push(DesignParameter {
        product_definition: None,
        id: cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#edited-parameter").unwrap(),
        owner: None,
        ordinal: 0,
        name: "Edited".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });

    let (report, output) = write(&decoded);

    assert!(
        !crate::test_support::container::is_compound_file(&output),
        "{:?}",
        report.losses
    );
    let dropped = messages(&report, SldprtLossCode::WriterFeaturesOmitted);
    assert_eq!(dropped.len(), 1, "{:?}", report.losses);
    assert!(
        dropped[0].contains("parameters [synthetic:test:id#edited-parameter]"),
        "{}",
        dropped[0]
    );
    decode_read_back(&output);
}

/// An edit that the patch path cannot write, of a part whose synthesis
/// without its `SolidWorks` history lands on another dialect than the
/// source's, leaves the part as stored. The decoded source isolates the
/// edited record: one `export:edit.dropped` loss names it and why each path
/// declines it (§3.4, Paths item 4).
#[test]
fn an_edit_no_path_writes_leaves_the_part_as_stored() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let source = decoded
        .source_fidelity()
        .retained_record(crate::SOURCE_IMAGE_ID)
        .and_then(|record| record.data())
        .unwrap()
        .to_vec();
    let constraint = add_horizontal_relation(&mut decoded);

    let (report, output) = write(&decoded);

    assert_eq!(output, source);
    let losses = unwritten(&report);
    assert_eq!(losses.len(), 1, "{:?}", report.losses);
    assert!(
        losses[0].contains(&format!("as stored ({})", crate::SOURCE_IMAGE_ID))
            && losses[0].contains("sketch relation editing")
            && losses[0].contains(&constraint),
        "{}",
        losses[0]
    );
}

/// The sketch relation the history writer cannot write, added to `ftc_06`.
fn add_horizontal_relation(decoded: &mut EditableDecodeResult) -> String {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
        SketchConstraintId,
    };

    let sketch = decoded.ir().model.sketches[0].id.clone();
    let entity = decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.sketch == sketch)
        .unwrap()
        .id()
        .clone();
    let constraint = SketchConstraintId::mint("synthetic:test:constraint#horizontal").unwrap();
    decoded
        .ir_mut()
        .model
        .sketch_constraints
        .push(SketchConstraint {
            id: constraint.clone(),
            sketch,
            definition: SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::Horizontal { entity },
            )
            .unwrap(),
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    constraint.as_str().to_owned()
}

/// A write the history writer declines is synthesized without the `sldprt`
/// namespace. Each arena of the namespace is declared dropped, so one
/// `export:native.records-omitted` loss per arena names its records, and no
/// loss names an `unknowns` stub (§3.4, Rule W3).
#[test]
fn a_declined_history_write_charges_each_native_arena_by_id() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    add_horizontal_relation(&mut decoded);
    let mut output = Vec::new();
    let report = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Explicit("sldprt:unknown"),
        )
        .unwrap()
        .write_to(&mut output)
        .unwrap();

    let omitted = report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "native.records-omitted")
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let namespace = decoded.ir().native.namespace("sldprt").unwrap();
    let mut charged_arenas = 0;
    for (arena, records) in namespace.arenas() {
        let ids = records
            .iter()
            .map(cadmpeg_ir::NativeRecord::id)
            .collect::<Vec<_>>();
        let naming = omitted
            .iter()
            .filter(|message| ids.iter().any(|id| names(message, id)))
            .collect::<Vec<_>>();
        if arena.as_str() == "unknowns" || ids.is_empty() {
            assert!(naming.is_empty(), "{arena}: {naming:?}");
            continue;
        }
        charged_arenas += 1;
        assert_eq!(naming.len(), 1, "{arena}: {omitted:?}");
        let message = naming[0];
        assert!(
            message.contains(&format!("of `sldprt` arena `{arena}`"))
                && message.contains("because the patch of the document")
                && ids.iter().all(|id| names(message, id)),
            "{message}"
        );
    }
    assert!(charged_arenas > 0, "{omitted:?}");
}

/// Whether `message` names `id` as a whole token (§3.2).
fn names(message: &str, id: &str) -> bool {
    message
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | ';'))
        .any(|token| token == id)
}

/// A compound-file part with no decode baseline is written into a native
/// envelope, which holds none of the part's compound-file streams: the write
/// names the stream.
#[test]
fn a_compound_file_part_read_as_edited_is_written_into_a_native_envelope() {
    let source = cadmpeg_test_support::compound::compound_fixture();
    let mut decoded = EditableDecodeResult::from(decode(source));
    decoded.source_fidelity_mut().content_digest = None;

    let (report, output) = write(&decoded);

    assert!(
        !crate::test_support::container::is_compound_file(&output),
        "{:?}",
        report.losses
    );
    assert!(unwritten(&report).is_empty(), "{:?}", report.losses);
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.local_code() == "native.records-omitted"
                && loss.message.contains("sldprt:file:compound-stream#")
        }),
        "{:?}",
        report.losses
    );
}

/// The source document decoded from the retained source image.
fn source(decoded: &EditableDecodeResult) -> DecodeResult {
    if let Some(digest) = decoded.source_fidelity().content_digest.as_ref() {
        for cached in [ftc_06(), ftc_08()] {
            if cached.source_fidelity().content_digest.as_ref() == Some(digest) {
                return cached.clone();
            }
        }
    }
    decode(
        decoded
            .source_fidelity()
            .retained_record(crate::SOURCE_IMAGE_ID)
            .and_then(|record| record.data())
            .unwrap()
            .to_vec(),
    )
}

fn stale(report: &ExportReport) -> Vec<&str> {
    messages(report, SldprtLossCode::WriterSolidStaleAfterEdit)
}

/// One parameter edit changes no other feature or parameter of the part.
fn a_parameter_edit_keeps_every_other_record(source_part: &DecodeResult) {
    let mut decoded = EditableDecodeResult::from(source_part.clone());
    let (owner, name, value) = edit_extrusion_length(&mut decoded);

    let (report, output) = write(&decoded);

    assert_eq!(unwritten(&report), Vec::<&str>::new());
    let written = decode_read_back(&output);
    assert!((length(named_parameter(written.ir(), &owner, &name)) - value).abs() <= 1.0e-9);
    let source = source(&decoded);
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(owner.as_str()))
        .unwrap()
        .id
        .clone();
    assert_eq!(
        readback::unaccounted(
            decoded.ir(),
            &[feature.as_str()],
            &["definition", "source_properties", "source_content"],
            readback::unnamed(
                readback::beyond(
                    readback::diff(decoded.ir(), written.ir()),
                    &readback::baseline(&source)
                ),
                &all_messages(&report)
            ),
        ),
        Vec::<String>::new()
    );
}

/// The active configuration of `ir`.
fn active(ir: &CadIr) -> &cadmpeg_ir::features::parameters::DesignConfiguration {
    ir.model
        .configurations
        .iter()
        .find(|configuration| configuration.active)
        .unwrap()
}

/// Whether the active configuration's row of `ir` carries the stale mark.
fn active_stale(ir: &CadIr) -> bool {
    crate::decode::configuration_source_needs_update(ir, active(ir))
}

/// The messages of the `export:evaluation.stale` losses.
fn evaluation_stale(report: &ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "evaluation.stale")
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The writer regenerates no state: a written parameter edit marks the
/// active configuration's row `swConfigurationNeedsUpdate`, and an
/// `export:evaluation.stale` loss names the edit (§3.4, States).
#[test]
fn a_written_parameter_edit_marks_the_active_configuration_stale() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    assert!(!active_stale(decoded.ir()));
    let (owner, name, _) = edit_extrusion_length(&mut decoded);
    let parameter = named_parameter(decoded.ir(), &owner, &name).id.clone();

    let (report, output) = write(&decoded);

    assert!(active_stale(decode_read_back(&output).ir()));
    assert!(
        evaluation_stale(&report).iter().any(|message| {
            message.contains(parameter.as_str()) && message.contains("swConfigurationNeedsUpdate")
        }),
        "{:#?}",
        report.losses
    );
}

/// A suppression edit is no construction edit, and it shapes the stored
/// solid: it marks the active configuration's row, and
/// `export:evaluation.stale` names the feature (§3.4, States).
#[test]
fn a_suppression_edit_marks_the_active_configuration_stale() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let feature = {
        let mut ir = decoded.ir_mut();
        let feature = ir
            .model
            .features
            .iter_mut()
            .find(|feature| is_extrusion(feature) && feature.suppressed != Some(true))
            .unwrap();
        feature.suppressed = Some(true);
        feature.id.clone()
    };

    let (report, output) = write(&decoded);

    assert!(
        active_stale(decode_read_back(&output).ir()),
        "{:#?}",
        report.losses
    );
    assert!(
        evaluation_stale(&report)
            .iter()
            .any(|message| message.contains(feature.as_str())),
        "{:#?}",
        report.losses
    );
}

/// A parameter's display is a record difference that shapes no state: the
/// write marks no configuration and charges no `export:evaluation.stale`.
#[test]
fn a_parameter_display_edit_marks_no_configuration_stale() {
    use cadmpeg_ir::features::parameters::DimensionDisplay;

    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    {
        let mut ir = decoded.ir_mut();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| {
                parameter.display.is_none()
                    && matches!(parameter.value, Some(ParameterValue::Length(_)))
            })
            .unwrap();
        parameter.display = Some(DimensionDisplay::Radius);
    }

    let (report, output) = write(&decoded);

    assert!(
        !active_stale(decode_read_back(&output).ir()),
        "{:#?}",
        report.losses
    );
    assert_eq!(evaluation_stale(&report), Vec::<&str>::new());
}

#[test]
fn a_parameter_edit_of_ftc_06_keeps_every_other_record() {
    a_parameter_edit_keeps_every_other_record(ftc_06());
}

#[test]
fn a_parameter_edit_of_ftc_08_keeps_every_other_record() {
    a_parameter_edit_keeps_every_other_record(ftc_08());
}

/// Renames the first feature that `selected` accepts: the rename changes the
/// feature's name and the references that name it, and no other feature or
/// parameter.
fn a_rename_keeps_every_other_record(source_part: &DecodeResult, selected: fn(&Feature) -> bool) {
    let mut decoded = EditableDecodeResult::from(source_part.clone());
    let (old, new) = {
        let mut ir = decoded.ir_mut();
        let feature = ir
            .model
            .features
            .iter_mut()
            .find(|feature| selected(feature))
            .unwrap();
        let old = feature.name.clone().unwrap();
        let new = format!("{old}_r");
        feature.name = Some(new.clone());
        (old, new)
    };

    let (report, output) = write(&decoded);

    assert_eq!(unwritten(&report), Vec::<&str>::new());
    assert_eq!(stale(&report), Vec::<&str>::new());
    let written = decode_read_back(&output);
    let source = source(&decoded);
    let id = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(new.as_str()))
        .unwrap()
        .id
        .as_str()
        .to_owned();
    let expected = readback::renamed(source.ir(), &[(id, old, new)]);
    assert_eq!(
        readback::unnamed(
            readback::beyond(
                readback::diff(&expected, written.ir()),
                &readback::baseline(&source)
            ),
            &all_messages(&report)
        ),
        Vec::new()
    );
}

fn is_fillet(feature: &Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
    )
}

/// Edits the wire form of `feature`'s definition.
fn edit_definition(feature: &mut Feature, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut wire = serde_json::to_value(feature.evaluation.definition()).unwrap();
    edit(&mut wire);
    feature
        .evaluation
        .set_definition(serde_json::from_value(wire).unwrap());
}

/// Adds one to the first nonzero real number under `value`, depth first.
fn bump_first_real(value: &mut serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Number(number) if number.is_f64() => {
            let Some(real) = number.as_f64().filter(|real| *real != 0.0) else {
                return false;
            };
            *value = serde_json::json!(real + 1.0);
            true
        }
        Value::Array(items) => items.iter_mut().any(bump_first_real),
        Value::Object(fields) => fields.values_mut().any(bump_first_real),
        _ => false,
    }
}

/// FTC-08 with the first fillet's compact edge selection edited, which the
/// writer cannot write, and with `edit` applied to the fillet.
fn ftc_08_with_fillet_selection_edited(edit: impl FnOnce(&mut Feature)) -> EditableDecodeResult {
    use cadmpeg_ir::features::selections::EdgeSelection;

    let mut decoded = EditableDecodeResult::from(ftc_08().clone());
    {
        let mut ir = decoded.ir_mut();
        let feature = ir
            .model
            .features
            .iter_mut()
            .find(|feature| is_fillet(feature))
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Fillet { mut groups }) =
            feature.evaluation.definition().clone()
        else {
            unreachable!("a fillet");
        };
        groups[0].edges = EdgeSelection::Native("sldprt:test:edited-edges".into());
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups,
            }));
        edit(feature);
    }
    decoded
}

/// An edit of a fillet's compact edge selection keeps the stored lane; the
/// write names it and calls nothing written.
#[test]
fn a_fillet_edge_selection_edit_is_named_and_not_called_written() {
    let decoded = ftc_08_with_fillet_selection_edited(|_| {});

    let unwritten =
        crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("compact edge selection"),
        "{}",
        unwritten[0]
    );
}

/// A fillet whose selection edit keeps its stored record does not carry a
/// radius edit of the same definition either; its one loss names both.
#[test]
fn a_kept_fillet_names_its_radius_edit_beside_the_selection() {
    let decoded = ftc_08_with_fillet_selection_edited(|fillet| {
        edit_definition(fillet, |wire| {
            assert!(bump_first_real(
                wire.pointer_mut("/value/value/groups/0/radius").unwrap()
            ));
        });
    });

    let unwritten =
        crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("compact edge selection")
            && unwritten[0].contains("definition groups[0].radius")
            && unwritten[0].contains("definition groups[0].edges"),
        "{}",
        unwritten[0]
    );
}

/// A kept fillet whose radius edit its parameter carries reads back with the
/// edited radius, and its loss names the selection only.
#[test]
fn a_kept_fillet_does_not_name_the_radius_its_parameter_carries() {
    const RADIUS: &str = "/value/value/groups/0/radius/radius";
    // The part states the radius dimension in inches: `R0.25` is 6.35 mm.
    const EDITED: f64 = 6.35;
    let mut decoded = ftc_08_with_fillet_selection_edited(|fillet| {
        edit_definition(fillet, |wire| {
            *wire.pointer_mut(RADIUS).unwrap() = serde_json::json!(EDITED);
        });
    });
    let name = {
        let mut ir = decoded.ir_mut();
        let fillet = ir
            .model
            .features
            .iter()
            .find(|feature| is_fillet(feature))
            .unwrap();
        let (id, name) = (fillet.id.clone(), fillet.name.clone().unwrap());
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.owner.as_ref() == Some(&id) && parameter.name == "D1")
            .unwrap();
        parameter.expression = "R0.25".into();
        parameter.value = Some(ParameterValue::Length(Length::new(EDITED).unwrap()));
        name
    };

    let (report, output) = write(&decoded);

    let unwritten = unwritten(&report);
    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("definition groups[0].edges") && !unwritten[0].contains("radius"),
        "{}",
        unwritten[0]
    );
    let written = decode_read_back(&output);
    let fillet = written
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name.as_str()))
        .unwrap();
    let wire = serde_json::to_value(fillet.evaluation.definition()).unwrap();
    assert!((wire.pointer(RADIUS).unwrap().as_f64().unwrap() - EDITED).abs() <= 1.0e-9);
}

/// A fillet whose selection edit keeps its stored record keeps its stored
/// source properties: a property edit beside it is named, and no stale-solid
/// loss calls the fillet written.
#[test]
fn a_kept_fillet_names_its_source_property_edit_beside_the_selection() {
    let decoded = ftc_08_with_fillet_selection_edited(|fillet| {
        fillet.source_properties.insert(
            cadmpeg_core::text::NonBlankString::new("Probe".to_owned()).unwrap(),
            "x".into(),
        );
    });

    let unwritten =
        crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("compact edge selection")
            && unwritten[0].contains("source property Probe"),
        "{}",
        unwritten[0]
    );
}

/// A fillet radius edited in the definition alone is written: the write
/// names no unwritten edit, the stored solid is named stale, and the radius
/// reads back as edited.
#[test]
fn a_fillet_radius_definition_edit_is_written() {
    const RADIUS: &str = "/value/value/groups/0/radius";
    let mut decoded = EditableDecodeResult::from(ftc_08().clone());
    let (name, radius) = {
        let mut ir = decoded.ir_mut();
        let fillet = ir
            .model
            .features
            .iter_mut()
            .find(|feature| is_fillet(feature))
            .unwrap();
        edit_definition(fillet, |wire| {
            assert!(bump_first_real(wire.pointer_mut(RADIUS).unwrap()));
        });
        let wire = serde_json::to_value(fillet.evaluation.definition()).unwrap();
        (
            fillet.name.clone().unwrap(),
            wire.pointer(RADIUS).unwrap().clone(),
        )
    };

    let (report, output) = write(&decoded);

    assert_eq!(unwritten(&report), Vec::<&str>::new());
    assert_eq!(stale(&report).len(), 1, "{:?}", report.losses);
    let written = decode_read_back(&output);
    let fillet = written
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name.as_str()))
        .unwrap();
    let wire = serde_json::to_value(fillet.evaluation.definition()).unwrap();
    assert_eq!(wire.pointer(RADIUS), Some(&radius));
}

/// A hole's placement is read from its sketch, which a definition edit does
/// not change: the write names the edit, and the hole reads back as stored.
#[test]
fn a_hole_placement_edit_the_records_do_not_state_is_named() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    {
        let mut ir = decoded.ir_mut();
        let hole = ir
            .model
            .features
            .iter_mut()
            .find(|feature| is_hole(feature))
            .unwrap();
        edit_definition(hole, |wire| {
            assert!(bump_first_real(
                wire.pointer_mut("/value/value/placements/0/origin")
                    .unwrap()
            ));
        });
    }

    let unwritten =
        crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("definition placements[0].origin"),
        "{}",
        unwritten[0]
    );
}

/// A reference axis's origin is read from its feature-input lane, which a
/// definition edit does not change: the write names the edit, the axis reads
/// back as stored, and its record keeps its stored properties.
#[test]
fn a_datum_axis_origin_edit_the_records_do_not_state_is_named() {
    let mut decoded = EditableDecodeResult::from(ftc_08().clone());
    {
        let mut ir = decoded.ir_mut();
        let axis = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Axis1"))
            .unwrap();
        edit_definition(axis, |wire| {
            assert!(bump_first_real(
                wire.pointer_mut("/value/value/axis/origin").unwrap()
            ));
        });
    }

    let unwritten =
        crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());
    let (_, written) =
        crate::test_support::write_unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("definition axis.origin"),
        "{}",
        unwritten[0]
    );
    let record = |ir: &CadIr| {
        crate::native::SldprtNative::load(ir.native.namespace("sldprt").unwrap())
            .unwrap()
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|feature| feature.name == "Axis1")
            .unwrap()
            .properties
            .clone()
    };
    assert_eq!(record(written.ir()), record(ftc_08().ir()));
}

/// A hole record the encoder cannot restate from any definition is kept for a
/// source-property edit, and its loss says that the record cannot be restated.
#[test]
fn a_hole_record_the_encoder_cannot_restate_is_named_as_such() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    {
        let mut ir = decoded.ir_mut();
        let hole = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("#97 (0.0059) Diameter Hole1"))
            .unwrap();
        hole.source_properties.insert(
            cadmpeg_core::text::NonBlankString::new("Probe".to_owned()).unwrap(),
            "x".into(),
        );
    }

    let unwritten =
        crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("cannot restate the stored record")
            && unwritten[0].contains("source property Probe")
            && !unwritten[0].contains("hole construction"),
        "{}",
        unwritten[0]
    );
}

/// An integer value bound to a feature-input scalar is written to the scalar
/// exactly.
#[test]
fn an_integer_value_bound_to_a_scalar_is_written_exactly() {
    use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
    use crate::test_support::history::resolved_features_payload;
    use crate::test_support::native::sldprt_native;
    use crate::test_support::parasolid::triangle_body;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let mut decoded = EditableDecodeResult::from(decode(native_envelope_fixture(&source)));
    let native_ref = {
        let mut ir = decoded.ir_mut();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1" && parameter.native_ref.is_some())
            .unwrap();
        parameter.expression = "3".into();
        parameter.value = Some(ParameterValue::Integer(3));
        parameter.native_ref.clone().unwrap()
    };

    let mut output = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut output,
    )
    .unwrap();

    let written = decode_read_back(&output);
    let scalar = sldprt_native(written.ir())
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.scalars)
        .find(|scalar| scalar.id == native_ref)
        .map(|scalar| scalar.value.get())
        .unwrap();
    assert_eq!(scalar.to_bits(), 3.0_f64.to_bits());
}

/// The record states an unbound parameter's value through its expression: a
/// value edit that the expression does not state is named, and the part reads
/// back as stored.
#[test]
fn a_value_the_expression_does_not_state_is_named_and_not_called_written() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let id = {
        let mut ir = decoded.ir_mut();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| {
                parameter.native_ref.is_none()
                    && parameter.pmi.is_none()
                    && matches!(parameter.value, Some(ParameterValue::Length(_)))
                    && plain_length(&parameter.expression).is_some()
                    && !is_reference(parameter)
            })
            .unwrap();
        let stored = length(parameter);
        parameter.value = Some(ParameterValue::Length(Length::new(stored + 1.0).unwrap()));
        parameter.id.clone()
    };

    let (report, output) = write(&decoded);

    let unwritten = unwritten(&report);
    assert_eq!(unwritten.len(), 1, "{:?}", report.losses);
    assert!(unwritten[0].contains(id.as_str()), "{}", unwritten[0]);
    assert_eq!(stale(&report), Vec::<&str>::new());
    let source = source(&decoded);
    assert_eq!(
        readback::unnamed(
            readback::beyond(
                readback::diff(source.ir(), decode_read_back(&output).ir()),
                &readback::baseline(&source)
            ),
            &all_messages(&report)
        ),
        Vec::new()
    );
}

/// A principal plane's role is its place in the default plane triplet, which
/// its record does not state: a role edit keeps the stored plane, and the
/// write names it.
#[test]
fn a_principal_plane_role_edit_keeps_the_stored_plane_and_is_named() {
    use cadmpeg_ir::features::PrincipalPlane;

    use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
    use crate::test_support::history::resolved_feature_classes_with_ids;
    use crate::test_support::parasolid::triangle_body;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Vorne" Type="Ebene" id="2"/><Feature Name="Oben" Type="Ebene" id="3"/><Feature Name="Rechts" Type="Ebene" id="4"/><Feature Name="Plane2" Type="Plane" id="39"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moRefPlane_c", "Vorne", 2),
            ("moRefPlane_c", "Oben", 3),
            ("moRefPlane_c", "Rechts", 4),
        ]),
    ));
    let mut decoded = EditableDecodeResult::from(decode(native_envelope_fixture(&source)));
    decoded.ir_mut().model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DatumPrincipalPlane {
                plane: PrincipalPlane::Right,
            },
        ));

    let unwritten =
        crate::test_support::unwritten_feature_edits(decoded.ir(), decoded.source_fidelity());

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("principal-plane role"),
        "{}",
        unwritten[0]
    );
}

/// A configuration's `Config-N` object lane and its
/// `Config-N-ResolvedFeatures` lane are each written to the stream they were
/// read from.
#[test]
fn an_edit_writes_each_configuration_lane_to_its_own_stream() {
    use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
    use crate::test_support::history::resolved_features_payload;
    use crate::test_support::parasolid::triangle_body;

    let mut objects = Vec::new();
    for class in ["sgSketch", "sgEntHandle"] {
        objects.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        objects.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
        objects.extend_from_slice(class.as_bytes());
    }
    let resolved = resolved_features_payload(&[0]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(0x20, "Contents/Config-0", &objects));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let mut decoded = EditableDecodeResult::from(decode(native_envelope_fixture(&source)));
    assert_eq!(
        crate::test_support::native::sldprt_native(decoded.ir())
            .feature_input_lanes
            .len(),
        2
    );
    {
        let mut ir = decoded.ir_mut();
        let parameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1")
            .unwrap();
        parameter.expression = "50mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(50.0).unwrap()));
    }

    let mut output = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut output,
    )
    .unwrap();

    let scan = crate::test_support::container::scan(&output);
    let payload = |name: &str| {
        scan.sections()
            .find(|section| section.name() == Some(name))
            .map(|section| section.payload().to_vec())
            .unwrap()
    };
    assert_eq!(payload("Contents/Config-0"), objects);
    assert_eq!(
        payload("Contents/Config-0-ResolvedFeatures").len(),
        resolved.len()
    );
}

/// A lane the caller did not edit is written as stored, even where the
/// writer, binding the lane alone, would derive other scalar owners than the
/// decoder did. A lane whose records differ from the stored lane is checked
/// against its bytes.
#[test]
fn an_unedited_lane_is_written_as_stored() {
    use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
    use crate::test_support::history::resolved_features_payload;
    use crate::test_support::native::sldprt_native;
    use crate::test_support::parasolid::triangle_body;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = decode(native_envelope_fixture(&source));
    let native = sldprt_native(decoded.ir());
    let decoded_lane = native.feature_input_lanes[0].clone();
    // The decoder's owner of the scalar differs from the one the writer
    // derives from the lane alone.
    let mut lane = decoded_lane.clone();
    lane.scalars[0].feature_ref = Some("sldprt:test:other-owner".into());

    let written =
        crate::writer::lane_payload(&lane, Some(&lane), &native.feature_histories, &[]).unwrap();
    assert_eq!(written, lane.native_payload.to_vec());

    let error =
        crate::writer::lane_payload(&lane, Some(&decoded_lane), &native.feature_histories, &[])
            .unwrap_err();
    assert!(
        error.to_string().contains("edited named scalars"),
        "{error}"
    );
}

/// The JSON pointer of the first place in `value` whose pointer ends with
/// `suffix`.
fn place(value: &serde_json::Value, pointer: &str, suffix: &str) -> Option<String> {
    if pointer.ends_with(suffix) {
        return Some(pointer.to_owned());
    }
    match value {
        serde_json::Value::Object(fields) => fields
            .iter()
            .find_map(|(key, value)| place(value, &format!("{pointer}/{key}"), suffix)),
        serde_json::Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, value)| place(value, &format!("{pointer}/{index}"), suffix)),
        _ => None,
    }
}

/// Changes the first place of feature `name`'s definition whose pointer ends
/// with `suffix`; the feature's id and the place's pointer.
fn edit_definition_at(
    decoded: &mut EditableDecodeResult,
    name: &str,
    suffix: &str,
    change: impl FnOnce(&mut serde_json::Value),
) -> (cadmpeg_ir::features::FeatureId, String) {
    let mut ir = decoded.ir_mut();
    let feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap();
    let mut wire = serde_json::to_value(feature.evaluation.definition()).unwrap();
    let pointer = place(&wire, "", suffix).unwrap();
    change(wire.pointer_mut(&pointer).unwrap());
    feature
        .evaluation
        .set_definition(serde_json::from_value(wire).unwrap());
    (feature.id.clone(), pointer)
}

fn plus_one(value: &mut serde_json::Value) {
    *value = serde_json::json!(value.as_f64().unwrap() + 1.0);
}

/// `Sketch4` is placed on `Plane3`, and a `Plane3` distance edit written as
/// edited would move it. The written part reads back as the source, and a
/// loss names the edit.
#[test]
fn a_plane_distance_edit_reads_back_as_the_source_or_as_edited() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let (plane, _) = edit_definition_at(&mut decoded, "Plane3", "/distance", plus_one);

    let (report, output) = write(&decoded);

    let unwritten = unwritten(&report);
    assert!(
        unwritten
            .iter()
            .any(|message| message.contains(plane.as_str())),
        "{unwritten:?}"
    );
    let source = source(&decoded);
    assert_eq!(
        readback::unnamed(
            readback::beyond(
                readback::diff(source.ir(), decode_read_back(&output).ir()),
                &readback::baseline(&source)
            ),
            &all_messages(&report)
        ),
        Vec::new()
    );
}

/// Two edits in one save, one of which is written with its stored record:
/// its loss names it alone, and the other edit is written as edited.
#[test]
fn a_held_back_edit_names_only_its_own_feature() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let (plane, _) = edit_definition_at(&mut decoded, "Plane3", "/distance", plus_one);
    let (extrusion, pointer) =
        edit_definition_at(&mut decoded, "Boss-Extrude1", "/length", plus_one);
    let edited = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id == extrusion)
        .map(|feature| serde_json::to_value(feature.evaluation.definition()).unwrap())
        .unwrap();

    let (report, output) = write(&decoded);

    let unwritten = unwritten(&report);
    assert!(
        unwritten
            .iter()
            .any(|message| message.contains(plane.as_str())),
        "{unwritten:?}"
    );
    assert!(
        unwritten
            .iter()
            .all(|message| !message.contains(extrusion.as_str())),
        "{unwritten:?}"
    );
    let written = decode_read_back(&output);
    let read = written
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss-Extrude1"))
        .map(|feature| serde_json::to_value(feature.evaluation.definition()).unwrap())
        .unwrap();
    assert_eq!(read.pointer(&pointer), edited.pointer(&pointer));
}

/// A through-all cut edited to a blind depth is written in the blind record
/// shape, which states the depth under a parameter the record did not hold:
/// the edit is written, and a loss names the added parameter.
#[test]
fn a_variant_edit_names_the_parameter_its_record_gains() {
    let mut decoded = EditableDecodeResult::from(ftc_08().clone());
    let (cut, _) = edit_definition_at(&mut decoded, "Cut-Extrude2", "/termination", |value| {
        *value = serde_json::json!({"kind": "blind", "length": 5.0});
    });

    let (report, output) = write(&decoded);

    let reshaped = messages(&report, SldprtLossCode::WriterFeatureRecordReshaped);
    assert!(
        reshaped
            .iter()
            .any(|message| message.contains(cut.as_str())
                && message.contains("adds parameter Depth")),
        "{:?}",
        report.losses
    );
    assert!(
        unwritten(&report)
            .iter()
            .all(|message| !message.contains(cut.as_str())),
        "{:?}",
        report.losses
    );
    let written = decode_read_back(&output);
    assert!(written
        .ir()
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "Depth"));
}

/// The blind depth of the extent under `value`.
fn first_depth(value: &mut serde_json::Value) -> Option<&mut serde_json::Value> {
    use serde_json::Value;
    if value.get("extent").is_some() {
        return value.pointer_mut("/extent/first/termination/length");
    }
    match value {
        Value::Object(fields) => fields.values_mut().find_map(first_depth),
        Value::Array(items) => items.iter_mut().find_map(first_depth),
        _ => None,
    }
}

fn named_feature<'a>(ir: &'a CadIr, name: &str) -> &'a Feature {
    ir.model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap()
}

/// Two features whose tree places are swapped: each reads back where the
/// document places it, or a read-back loss names it and no stale-solid
/// note calls its place written.
#[test]
fn a_swapped_pair_is_written_in_place_or_named() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let names = ["Boss-Extrude4", "Cut-Extrude1"];
    {
        let mut ir = decoded.ir_mut();
        let ordinals = names.map(|name| named_feature(&ir, name).ordinal);
        for (name, ordinal) in names.iter().zip(ordinals.iter().rev()) {
            ir.model
                .features
                .iter_mut()
                .find(|feature| feature.name.as_deref() == Some(*name))
                .unwrap()
                .ordinal = *ordinal;
        }
    }

    let (report, output) = write(&decoded);

    let written = decode_read_back(&output);
    let messages = all_messages(&report);
    for name in names {
        let asked = named_feature(decoded.ir(), name);
        if named_feature(written.ir(), name).ordinal == asked.ordinal {
            continue;
        }
        assert!(
            messages.iter().any(
                |message| message.contains(asked.id.as_str()) && message.contains("reads back")
            ),
            "{name}: {messages:?}"
        );
        assert!(
            stale(&report)
                .iter()
                .all(|message| !(message.contains(asked.id.as_str())
                    && message.contains("edited place"))),
            "{name}: {:?}",
            stale(&report)
        );
    }
}

/// A depth edit in the same save as a removed hole reads back as edited:
/// the edited extrusion is found through the pairing, whatever the
/// removal does to the tree places.
#[test]
fn a_depth_edit_beside_a_removed_hole_is_carried() {
    let mut decoded = EditableDecodeResult::from(ftc_06().clone());
    let depth = {
        let mut ir = decoded.ir_mut();
        let hole = named_feature(&ir, "#97 (0.0059) Diameter Hole1").id.clone();
        ir.model.features.retain(|feature| feature.id != hole);
        let extrusion = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Boss-Extrude1"))
            .unwrap();
        let mut depth = 0.0;
        edit_definition(extrusion, |wire| {
            let leaf = first_depth(wire).unwrap();
            depth = leaf.as_f64().unwrap() + 1.0;
            *leaf = serde_json::json!(depth);
        });
        depth
    };

    let (report, output) = write(&decoded);

    let written = decode_read_back(&output);
    let mut wire = serde_json::to_value(
        named_feature(written.ir(), "Boss-Extrude1")
            .evaluation
            .definition(),
    )
    .unwrap();
    let read = first_depth(&mut wire).unwrap().as_f64().unwrap();
    assert!(
        (read - depth).abs() <= 1.0e-9,
        "{read} {:?}",
        unwritten(&report)
    );
    let id = named_feature(decoded.ir(), "Boss-Extrude1").id.as_str();
    assert!(
        unwritten(&report)
            .iter()
            .all(|message| !message.contains(&format!("feature {id} "))),
        "{:?}",
        unwritten(&report)
    );
}
