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

use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::tessellation::display_list_payload;
use crate::SldprtCodec;

/// A part whose history holds one extrusion with a 12 mm depth.
fn part() -> Vec<u8> {
    part_with_history(&triangle_body())
}

fn part_with_history(body: &[u8]) -> Vec<u8> {
    let mut bytes = sldprt_with_body(body);
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
        .plan(EncodeInput::new(ir, Some(fidelity)))
        .expect("plan");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write");
    (bytes, report.losses, report.notes)
}

/// The bytes `ir` writes with the decode's sidecar, or why it refuses.
fn written(ir: &CadIr, decoded: &DecodeResult) -> Result<Vec<u8>, String> {
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, Some(decoded.source_fidelity())))
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

    let image_len = decoded
        .source_fidelity()
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .unwrap()
        .len();
    let mut from_saved = Vec::new();
    SldprtCodec
        .plan(EncodeInput {
            limits: cadmpeg_core::decode::ResourceLimits {
                max_input_bytes: cadmpeg_core::decode::u64_from_index(image_len),
                ..cadmpeg_core::decode::ResourceLimits::desktop()
            },
            ..EncodeInput::new(&read, Some(decoded.source_fidelity()))
        })
        .expect("the edited part writes within one image length")
        .write_to(&mut from_saved)
        .unwrap();
    assert_eq!(Ok(&from_saved), written(&held, &decoded).as_ref());
    let feature = &sldprt_native(decode(from_saved).ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Depth"], "20mm");
}

fn source_entities(source: &[u8]) -> u64 {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let root = ctx.open_input(source, "fixture input").unwrap();
    crate::decode::decode(&ctx, root).unwrap();
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
        ctx.charge_entities(u64::MAX, "measure fixture entities")
    else {
        panic!("the entity ceiling refuses the measurement charge")
    };
    limit.used
}

#[test]
fn the_native_patch_and_history_comparison_share_the_partition() {
    use cadmpeg_core::decode::{u64_from_index, ResourceLimits};
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::report::export::WritePath;

    let mut body = triangle_body();
    body.extend(crate::test_support::parasolid::edge_use(40, 999));
    let source = part_with_history(&body);
    let decoded = decode(source.clone());
    let mut ir = decoded.ir().clone();
    let point = &mut ir.geometry.points[1];
    let id = point.id.clone();
    let at = point.position().get();
    let moved = FinitePoint3::new(Point3::new(at.x + 500.0, at.y, at.z)).unwrap();
    point.set_position(moved);
    let limits = ResourceLimits {
        max_input_bytes: u64_from_index(source.len()),
        max_entities: source_entities(&source),
        ..ResourceLimits::desktop()
    };
    let plan = SldprtCodec
        .plan(EncodeInput {
            limits,
            ..EncodeInput::new(&ir, Some(decoded.source_fidelity()))
        })
        .expect("one partition decode fits");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Patched { .. }
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let regenerated = decode(bytes);
    assert!(regenerated.ir().geometry.points.iter().any(|point| {
        point.id == id && (point.position().get().x - moved.get().x).abs() < 1.0e-9
    }));
}

#[test]
fn the_restore_charges_its_source_before_semantic_planning() {
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension,
        ResourceLimits,
    };
    use cadmpeg_ir::codec::write::{target::TargetDomain, EncoderBackend};

    let image = part();
    let limit = u64_from_index(image.len());
    let decoded = decode(image);
    let mut ir = saved(decoded.ir());
    set_depth(&mut ir, &depth(decoded.ir()));

    let input = EncodeInput::new(&ir, Some(decoded.source_fidelity()));
    let target = SldprtCodec::TARGET
        .resolve(&ir, input.target, <SldprtCodec as EncoderBackend>::FORMAT)
        .unwrap();
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_input_bytes: limit,
            ..ResourceLimits::desktop()
        },
    };
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let image = decoded
        .source_fidelity()
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .unwrap();
    let source = crate::writer::SourceInput::new(image);
    let restored = crate::writer::saved::restore(
        &ctx,
        input,
        Some(&source),
        &target,
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .unwrap();
    assert!(restored.is_some());
    let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) =
        ctx.charge_input(1, "an independent input")
    else {
        panic!("restoration must use the encode's available input bytes")
    };
    assert_eq!(
        (refusal.dimension, refusal.used),
        (ResourceDimension::InputBytes, limit)
    );
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
    let unwritten = messages(&losses, "export/edit.dropped");
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
    let unwritten = messages(&losses, "export/edit.dropped");
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
                EncodeInput::new(ir, Some(decoded.source_fidelity()))
                    .with_target(TargetRequest::Explicit("sldprt:unknown")),
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
