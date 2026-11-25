// SPDX-License-Identifier: Apache-2.0
//! A part saved as CADIR JSON, read back with its decode sidecar, edited in
//! its `design` records, and written as SLDPRT: the output is the one the
//! part that holds its history arenas writes with the same edit.

use std::io::Cursor;
use std::num::NonZeroI8;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::edit::Literal;
use cadmpeg_ir::design::units::{Unit, UnitFactor, UnitSymbol};
use cadmpeg_ir::features::parameters::ParameterValue as HistoryValue;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, Length};
use cadmpeg_ir::{CadIr, RetainedSourceRecord, SourceFidelity};

use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::tessellation::display_list_payload;
use crate::SldprtCodec;

/// A part whose history holds one extrusion with a 12 mm depth.
fn part() -> Vec<u8> {
    let mut bytes = sldprt_with_body(&triangle_body());
    bytes.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    bytes.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="13100"><swModel swName="part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    native_envelope_fixture(&bytes)
}

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode")
}

fn saved(ir: &CadIr) -> CadIr {
    let read = CadIr::from_json(&ir.to_canonical_json().expect("CADIR JSON")).expect("read back");
    assert!(!read.model.omitted_arenas_held());
    read
}

/// The read-back part of `decoded` holding the history arenas of its decode.
fn held(decoded: &DecodeResult) -> CadIr {
    let mut held = saved(decoded.ir());
    held.model.adopt_omitted_arenas(&decoded.ir().model);
    assert!(held.model.omitted_arenas_held());
    held
}

/// The bytes, losses and notes `ir` writes with `fidelity`.
fn report(ir: &CadIr, fidelity: &SourceFidelity) -> (Vec<u8>, Vec<LossNote>, Vec<String>) {
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)
        .expect("plan");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write");
    (bytes, report.losses, report.notes)
}

/// The bytes `ir` writes with the decode's sidecar, or why it refuses.
fn written(ir: &CadIr, decoded: &DecodeResult) -> Result<Vec<u8>, String> {
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(ir, Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// The messages of the losses of `code` in `losses`.
fn messages(losses: &[LossNote], code: &str) -> Vec<String> {
    losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.clone())
        .collect()
}

fn set_depth(ir: &mut CadIr, id: &ParameterId) {
    ir.edit()
        .expect("editor")
        .set_dimension(id, millimetres(20.0))
        .expect("edit");
}

/// `held` with the 20 mm depth stated in its history parameter.
fn held_depth(held: &mut CadIr, id: &ParameterId) {
    set_depth(held, id);
    let history = held
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == *id)
        .expect("history parameter");
    history.expression = "20mm".into();
    history.value = Some(HistoryValue::Length(Length::new(20.0).expect("length")));
}

fn depth(ir: &CadIr) -> ParameterId {
    ir.design
        .parameters
        .iter()
        .find(|parameter| parameter.name.as_ref().is_some_and(|name| *name == "Depth"))
        .expect("design parameter")
        .id
        .clone()
}

fn millimetres(value: f64) -> Literal {
    Literal::Number {
        value: FiniteReal::new(value).expect("finite"),
        unit: Unit(vec![UnitFactor {
            symbol: UnitSymbol::Mm,
            power: NonZeroI8::new(1).expect("power"),
        }]),
    }
}

#[test]
fn a_saved_part_writes_an_edited_dimension_as_its_decode_does() {
    let decoded = decode(part());
    let id = depth(decoded.ir());
    let mut read = saved(decoded.ir());
    set_depth(&mut read, &id);
    let mut held = held(&decoded);
    held_depth(&mut held, &id);

    let from_saved = written(&read, &decoded).expect("the edited part writes");
    assert_eq!(Ok(&from_saved), written(&held, &decoded).as_ref());
    let feature = &sldprt_native(decode(from_saved).ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Depth"], "20mm");
}

#[test]
fn a_saved_part_writes_an_edited_suppression_as_its_decode_does() {
    let decoded = decode(part());
    let id: FeatureId = decoded
        .ir()
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .expect("design feature")
        .id
        .clone();
    let suppress = |ir: &mut CadIr| {
        ir.design
            .features
            .iter_mut()
            .find(|feature| feature.id == id)
            .expect("design feature")
            .suppressed = Some(Dimension::Literal(true));
    };
    let mut read = saved(decoded.ir());
    suppress(&mut read);
    let mut held = held(&decoded);
    suppress(&mut held);
    held.model
        .features
        .iter_mut()
        .find(|feature| feature.id == id)
        .expect("history feature")
        .suppressed = Some(true);

    assert_eq!(written(&read, &decoded), written(&held, &decoded));
}

/// A parameter comment has no field in the feature history: the part is
/// written with the dimension edit beside it, and the loss names the comment.
#[test]
fn a_design_edit_the_history_cannot_state_is_left_out_and_named() {
    let decoded = decode(part());
    let id = depth(decoded.ir());
    let mut read = saved(decoded.ir());
    set_depth(&mut read, &id);
    read.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .expect("design parameter")
        .comment = Some("edited".into());
    let mut held = held(&decoded);
    held_depth(&mut held, &id);

    let (bytes, losses, _) = report(&read, decoded.source_fidelity());
    assert_eq!(Ok(&bytes), written(&held, &decoded).as_ref());
    let unwritten = messages(
        &losses,
        &SldprtLossCode::WriterParameterEditUnwritten
            .kind()
            .to_string(),
    );
    let path = format!("design.parameters[{id}].comment");
    assert!(
        unwritten.iter().any(|message| message.contains(&path)),
        "{losses:?}"
    );
}

/// A compound-file part whose only edit the history cannot state is the
/// part its decode states, so it replays, and the loss names the edit.
#[test]
fn an_edited_saved_compound_file_part_keeps_its_stored_part() {
    let source = cadmpeg_test_support::compound::compound_fixture();
    let decoded = decode(source.clone());
    let mut parameter = saved(decode(part()).ir()).design.parameters.remove(0);
    parameter.comment = Some("edited".into());
    let id = parameter.id.clone();
    let mut read = saved(decoded.ir());
    read.design.parameters.push(parameter);

    let (bytes, losses, _) = report(&read, decoded.source_fidelity());
    assert_eq!(bytes, source);
    let unwritten = messages(
        &losses,
        &SldprtLossCode::WriterParameterEditUnwritten
            .kind()
            .to_string(),
    );
    let path = format!("design.parameters[{id}]");
    assert!(
        unwritten.iter().any(|message| message.contains(&path)),
        "{losses:?}"
    );
}

/// A part whose display-face header states two triangles over a table of
/// one, which the decode refuses.
fn undecodable() -> Vec<u8> {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let header = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .expect("face tessellation class")
        + marker.len();
    payload[header..header + 4].copy_from_slice(&2_u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));
    source
}

/// A retained source that does not decode restores no history: the edited
/// part is written as the stored part, and `export:edit.dropped` names it.
#[test]
fn a_saved_part_whose_source_does_not_decode_is_written_as_stored() {
    let decoded = decode(part());
    let stored = undecodable();
    let mut fidelity = decoded.source_fidelity().clone();
    fidelity
        .remove_retained_record(crate::SOURCE_IMAGE_ID)
        .expect("the decode retains the source image");
    fidelity
        .insert_retained_record(
            crate::source_image_id(),
            RetainedSourceRecord::whole("sldprt", stored.clone()),
        )
        .expect("one source image");
    let mut read = saved(decoded.ir());
    set_depth(&mut read, &depth(decoded.ir()));

    let (bytes, losses, notes) = report(&read, &fidelity);
    assert_eq!(bytes, stored);
    assert!(
        messages(&losses, "export/edit.dropped")
            .iter()
            .any(|message| message.contains(crate::SOURCE_IMAGE_ID)),
        "{losses:?}"
    );
    assert!(
        notes
            .iter()
            .any(|note| note.contains("not restored from its retained source part")),
        "{notes:?}"
    );
}

#[test]
fn a_saved_part_synthesizes_as_its_decode_does() {
    let decoded = decode(part());
    let synthesized = |ir: &CadIr| {
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(ir, Some(decoded.source_fidelity())),
                TargetRequest::Explicit("sldprt:unknown"),
            )
            .expect("plan");
        let mut bytes = Vec::new();
        let report = plan.write_to(&mut bytes).expect("write");
        (bytes, report.losses, report.notes)
    };
    assert_eq!(
        synthesized(&saved(decoded.ir())),
        synthesized(&held(&decoded))
    );
}
