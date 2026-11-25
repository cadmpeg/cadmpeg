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
use cadmpeg_ir::{CadIr, RetainedSourceRecord, SourceFidelity};

use crate::loss::F3dLossCode;
use crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh;
use crate::test_support::zip_test::{f3d_with_smbh_and_manifest_version, with_manifest_version};
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
        .plan(EncodeInput::new(ir, Some(decoded.source_fidelity())))
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

    let (from_saved, report) = write(&read, &decoded);
    let (from_held, held_report) = write(&held, &decoded);
    assert_eq!(from_saved, from_held);
    assert_eq!(report.losses, held_report.losses);
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
        report.notes.iter().any(|note| note.contains(&comment)),
        "{:?}",
        report.notes
    );
    assert_eq!(
        charged(&report, F3dLossCode::WriterPatchDeclined).len(),
        1,
        "{:#?}",
        report.losses
    );
    assert!(charged(&report, F3dLossCode::WriterHistoryUnrestored).is_empty());
}

/// The only catalog row. `SOURCE` is of this dialect, so a write to it
/// replays its retained archive.
const CATALOG_ROW: &str = "f3d:manifest-3-2-0-0";

/// `SOURCE` stated in a dialect outside the catalog, so a write to the
/// catalog row synthesizes the file.
fn off_catalog_source() -> Vec<u8> {
    with_manifest_version(SOURCE, "3-3-0-0")
}

fn write_to(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    request: TargetRequest<'_>,
) -> (Vec<u8>, ExportReport) {
    let plan = F3dCodec
        .plan(EncodeInput::new(ir, Some(fidelity)).with_target(request))
        .expect("plan");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write");
    (bytes, report)
}

/// The fidelity of `decoded` with a retained archive that does not decode.
fn undecodable(decoded: &DecodeResult) -> SourceFidelity {
    let mut fidelity = decoded.source_fidelity().clone();
    fidelity
        .remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .expect("a retained archive");
    fidelity
        .insert_retained_record(
            crate::ids::file_source_image_id(),
            RetainedSourceRecord::whole("f3d", b"not an archive".to_vec()),
        )
        .expect("one retained archive");
    fidelity
}

/// The losses of `report` with `code`.
fn charged(report: &ExportReport, code: F3dLossCode) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The value of `d1` in the file `bytes`.
fn d1(bytes: &[u8]) -> Option<ParameterValue> {
    decode(bytes)
        .ir()
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.name.as_ref().is_some_and(|name| *name == "d1"))
        .expect("d1 written")
        .value
        .clone()
}

/// `decoded`, saved and read back, with `d1` set to 3 mm.
fn with_d1_edited(decoded: &DecodeResult) -> CadIr {
    let id = parameter(decoded.ir(), "d1");
    let mut read = saved(decoded.ir());
    read.edit()
        .expect("editor")
        .set_dimension(&id, millimetres(3.0))
        .expect("edit");
    read
}

fn three_millimetres() -> ParameterValue {
    ParameterValue::Number(FiniteReal::new(3.0).expect("finite"))
}

/// A saved design whose retained archive does not decode is written from its
/// `design` records. The patch of that archive charges the failure where the
/// target patches it, and the restore charges it on a catalog row.
#[test]
fn a_retained_archive_that_does_not_decode_is_charged_once() {
    let decoded = decode(SOURCE);
    let (bytes, report) = write_to(
        &with_d1_edited(&decoded),
        &undecodable(&decoded),
        TargetRequest::Inherit,
    );
    assert_eq!(
        charged(&report, F3dLossCode::WriterPatchDeclined).len(),
        1,
        "{:#?}",
        report.losses
    );
    assert!(charged(&report, F3dLossCode::WriterHistoryUnrestored).is_empty());
    assert_eq!(d1(&bytes), Some(three_millimetres()));

    let decoded = decode(&off_catalog_source());
    let (bytes, report) = write_to(
        &with_d1_edited(&decoded),
        &undecodable(&decoded),
        TargetRequest::Explicit(CATALOG_ROW),
    );
    assert!(matches!(report.write_path(), WritePath::Synthesized { .. }));
    assert_eq!(
        charged(&report, F3dLossCode::WriterHistoryUnrestored).len(),
        1,
        "{:#?}",
        report.losses
    );
    assert!(charged(&report, F3dLossCode::WriterPatchDeclined).is_empty());
    assert_eq!(d1(&bytes), Some(three_millimetres()));
}

/// A design edit that the history arenas cannot state is charged on a
/// catalog row, where no patch is tried.
#[test]
fn a_design_edit_the_history_cannot_state_is_charged_on_a_catalog_row() {
    let decoded = decode(&off_catalog_source());
    let id = parameter(decoded.ir(), "d1");
    let mut read = saved(decoded.ir());
    read.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .expect("design parameter")
        .comment = Some("edited".into());

    let (_, report) = write_to(
        &read,
        decoded.source_fidelity(),
        TargetRequest::Explicit(CATALOG_ROW),
    );
    let comment = format!("design.parameters[{id}].comment");
    let unrestored = charged(&report, F3dLossCode::WriterHistoryUnrestored);
    assert_eq!(unrestored.len(), 1, "{:#?}", report.losses);
    assert!(unrestored[0].contains(&comment), "{unrestored:?}");
    assert!(charged(&report, F3dLossCode::WriterPatchDeclined).is_empty());
}

/// A saved design written to a catalog row other than its source's dialect
/// is generated from the restored history arenas, as the document that holds
/// them is.
#[test]
fn a_saved_design_synthesizes_as_its_decode_does() {
    let decoded = decode(&f3d_with_smbh_and_manifest_version(
        &synthetic_geometry_with_history_smbh(),
        "3-3-0-0",
    ));
    let synthesized = |ir: &CadIr| {
        let plan = F3dCodec
            .plan(
                EncodeInput::new(ir, Some(decoded.source_fidelity()))
                    .with_target(TargetRequest::Explicit("f3d:manifest-3-2-0-0")),
            )
            .expect("plan");
        assert!(matches!(
            plan.report().write_path(),
            WritePath::Synthesized { .. }
        ));
        let mut bytes = Vec::new();
        let report = plan.write_to(&mut bytes).expect("write");
        (bytes, report.losses, report.notes)
    };
    assert_eq!(
        synthesized(&saved(decoded.ir())),
        synthesized(&held(&decoded))
    );
}
