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
use cadmpeg_ir::scalar::{FiniteReal, Length};
use cadmpeg_ir::CadIr;

use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
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
    read.edit()
        .expect("editor")
        .set_dimension(&id, millimetres(20.0))
        .expect("edit");
    let mut held = held(&decoded);
    held.edit()
        .expect("editor")
        .set_dimension(&id, millimetres(20.0))
        .expect("edit");
    let history = held
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .expect("history parameter");
    history.expression = "20mm".into();
    history.value = Some(HistoryValue::Length(Length::new(20.0).expect("length")));

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

#[test]
fn a_design_edit_the_history_cannot_state_refuses_and_names_it() {
    let decoded = decode(part());
    let id = depth(decoded.ir());
    let mut read = saved(decoded.ir());
    read.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .expect("design parameter")
        .comment = Some("edited".into());

    let error = written(&read, &decoded).expect_err("the comment has no history field");
    assert!(
        error.contains(&format!("design.parameters[{id}].comment")),
        "{error}"
    );
}
