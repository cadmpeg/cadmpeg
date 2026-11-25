// SPDX-License-Identifier: Apache-2.0
//! A design saved as CADIR JSON, read back with its decode sidecar, edited,
//! and written as F3D: the output is the one the document that holds its
//! history arenas writes with the same edit.

use std::io::Cursor;
use std::num::NonZeroI8;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::edit::Literal;
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::units::{Unit, UnitFactor, UnitSymbol};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::parameters::ParameterValue as HistoryValue;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::export::{ExportReport, WritePath};
use cadmpeg_ir::scalar::{FiniteReal, Length};
use cadmpeg_ir::CadIr;

use crate::F3dCodec;

const SOURCE: &[u8] = include_bytes!("../../../../corpus/f3d/region_A.f3d");

fn decode(bytes: &[u8]) -> DecodeResult {
    F3dCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect("decode")
}

fn saved(ir: &CadIr) -> CadIr {
    let read = CadIr::from_json(&ir.to_canonical_json().expect("CADIR JSON")).expect("read back");
    assert!(!read.model.omitted_arenas_held());
    read
}

/// The read-back document of `decoded` holding the history arenas of its
/// decode.
fn held(decoded: &DecodeResult) -> CadIr {
    let mut held = saved(decoded.ir());
    held.model.adopt_omitted_arenas(&decoded.ir().model);
    assert!(held.model.omitted_arenas_held());
    held
}

fn write(ir: &CadIr, decoded: &DecodeResult) -> (Vec<u8>, ExportReport) {
    let plan = F3dCodec
        .plan(
            EncodeInput::new(ir, Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("plan");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write");
    (bytes, report)
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

fn parameter(ir: &CadIr, name: &str) -> ParameterId {
    ir.design
        .parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .is_some_and(|candidate| *candidate == name)
        })
        .expect("design parameter")
        .id
        .clone()
}

#[test]
fn a_saved_design_writes_an_edited_dimension_as_its_decode_does() {
    let decoded = decode(SOURCE);
    let id = parameter(decoded.ir(), "d1");
    let mut read = saved(decoded.ir());
    read.edit()
        .expect("editor")
        .set_dimension(&id, millimetres(3.0))
        .expect("edit");
    let mut held = held(&decoded);
    held.edit()
        .expect("editor")
        .set_dimension(&id, millimetres(3.0))
        .expect("edit");
    let history = held
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .expect("history parameter");
    history.expression = "3 mm".into();
    history.value = Some(HistoryValue::Length(Length::new(3.0).expect("length")));

    let (from_saved, _) = write(&read, &decoded);
    assert_eq!(from_saved, write(&held, &decoded).0);
    let written = decode(&from_saved);
    let d1 = written
        .ir()
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.name.as_ref().is_some_and(|name| *name == "d1"))
        .expect("d1 written");
    assert_eq!(
        d1.value,
        Some(ParameterValue::Number(
            FiniteReal::new(3.0).expect("finite")
        ))
    );
}

#[test]
fn a_saved_design_patches_an_edited_point() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_manifest_version(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        "3-3-0-0",
    );
    let decoded = decode(&source);
    let moved = |ir: &mut CadIr| {
        let at = ir.geometry.points[0].position().get();
        ir.geometry.points[0].set_position(
            FinitePoint3::new(Point3::new(at.x + 1.0, at.y, at.z)).expect("a finite point"),
        );
    };
    let mut read = saved(decoded.ir());
    moved(&mut read);
    let mut held = held(&decoded);
    moved(&mut held);

    let (from_saved, report) = write(&read, &decoded);
    assert!(
        matches!(report.write_path(), WritePath::Patched { .. }),
        "{:?}",
        report.losses
    );
    assert_eq!(from_saved, write(&held, &decoded).0);
}

#[test]
fn a_design_edit_the_history_cannot_state_is_named() {
    let decoded = decode(SOURCE);
    let id = parameter(decoded.ir(), "d1");
    let mut read = saved(decoded.ir());
    read.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .expect("design parameter")
        .comment = Some("edited".into());

    let (_, report) = write(&read, &decoded);
    let comment = format!("design.parameters[{id}].comment");
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains(&comment)),
        "{:?}",
        report.losses
    );
}
