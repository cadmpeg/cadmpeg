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
    assert_eq!(stale(&report), Vec::<&str>::new());
    let written = decode(output);
    assert_eq!(
        crate::test_support::readback::differences(source(&decoded).ir(), written.ir(), &[]),
        Vec::<String>::new()
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
    let written = decode(output);
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

/// The writer has no compound-file serializer. An edited compound-file part
/// is written as its stored part, and the write states that no edit is
/// written.
#[test]
fn an_edited_compound_file_part_is_written_as_stored_and_named() {
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

    assert_eq!(output, source);
    let losses = messages(&report, SldprtLossCode::WriterCompoundFileEditUnwritten);
    assert_eq!(losses.len(), 1, "{:?}", report.losses);
    assert!(losses[0].contains("compound-file writer"), "{}", losses[0]);
}

/// The source document decoded from the retained source image.
fn source(decoded: &EditableDecodeResult) -> DecodeResult {
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
    let written = decode(output);
    assert!((length(named_parameter(written.ir(), &owner, &name)) - value).abs() <= 1.0e-9);
    let edited_feature = format!("F:{owner}");
    let edited_parameter = format!("P:F:{owner}.{name}");
    assert_eq!(
        crate::test_support::readback::differences(
            source(&decoded).ir(),
            written.ir(),
            &[&edited_feature, &edited_parameter],
        ),
        Vec::<String>::new()
    );
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
    let written = decode(output);
    // A reference `name@feature` names the feature by name.
    let mut expected = decoded.ir().clone();
    let (from, to) = (format!("@{old}"), format!("@{new}"));
    for parameter in &mut expected.model.parameters {
        parameter.expression = parameter.expression.replace(&from, &to);
        for value in parameter.properties.values_mut() {
            *value = value.replace(&from, &to);
        }
    }
    assert_eq!(
        crate::test_support::readback::differences(&expected, written.ir(), &[]),
        Vec::<String>::new()
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
    let written = decode(output);
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
/// definition edit does not change: the write names the edit, and the axis
/// reads back as stored.
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

    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(
        unwritten[0].contains("definition axis.origin"),
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

    let written = decode(output);
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

    let unwritten = messages(&report, SldprtLossCode::WriterParameterEditUnwritten);
    assert_eq!(unwritten.len(), 1, "{:?}", report.losses);
    assert!(unwritten[0].contains(id.as_str()), "{}", unwritten[0]);
    assert_eq!(stale(&report), Vec::<&str>::new());
    assert_eq!(
        crate::test_support::readback::differences(source(&decoded).ir(), decode(output).ir(), &[],),
        Vec::<String>::new()
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
