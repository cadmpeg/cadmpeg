// SPDX-License-Identifier: Apache-2.0
//! Saved mass properties compare at their serialized precision.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::declared_measures::{
    DeclaredMeasure, DeclaredMeasureProvenance, DeclaredQuantity, DeclaredTarget,
};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::SldprtCodec;

fn specimen(name: &str) -> DecodeResult {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/sldprt")
        .join(name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
}

fn findings(ir: &CadIr) -> Vec<Finding> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let findings = cadmpeg_measure::declared_findings(&ctx, ir, MeasureOptions::default())
        .expect("measure declarations");
    ctx.finish_session().expect("measurement session");
    assert!(findings
        .iter()
        .all(|finding| finding.check == Check::DeclaredMeasure));
    findings
}

fn assert_volume_unavailable_for_orientation(ir: &CadIr, body: &cadmpeg_ir::ids::BodyId) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).expect("context");
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(
            &ctx,
            &Selection {
                bodies: vec![body.clone()],
                ..Selection::default()
            },
        )
        .expect("measure active body");
    let volume = report
        .bodies()
        .iter()
        .find(|row| row.body() == body.as_str())
        .expect("active body measure")
        .volume()
        .as_ref()
        .expect("volume quantity");
    assert_eq!(
        volume
            .unavailable()
            .expect("orientation-ambiguous volume")
            .reason()
            .code(),
        "volume.orientation-unverified"
    );
    ctx.finish_session().expect("measurement session");
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} differs from {expected} by more than {tolerance}"
    );
}

fn assert_active_body_target(ir: &CadIr, measure: &DeclaredMeasure) {
    let mut active = ir
        .model
        .configurations
        .iter()
        .filter(|config| config.active);
    let configuration = active.next().expect("active configuration");
    assert!(active.next().is_none(), "one active configuration");
    assert!(
        configuration.source_index.is_some(),
        "configuration source index"
    );
    let bodies = configuration
        .bodies
        .as_ref()
        .expect("configuration body set");
    let [body] = bodies.as_slice() else {
        panic!("one active configuration body")
    };
    assert_eq!(ir.model.bodies.len(), 1, "one document body");
    assert_eq!(
        measure.target,
        DeclaredTarget::Bodies {
            bodies: vec![body.clone()]
        }
    );
}

fn assert_volume(measure: &DeclaredMeasure, value: f64, source_error: f64) {
    let DeclaredQuantity::Volume { value: actual } = &measure.quantity else {
        panic!("volume declaration")
    };
    assert_close(actual.get(), value, 1.0e-8);
    assert_close(
        measure.source_error.expect("source precision").get(),
        source_error,
        1.0e-8,
    );
}

fn assert_area(measure: &DeclaredMeasure, value: f64, source_error: f64) {
    let DeclaredQuantity::SolidArea { value: actual } = &measure.quantity else {
        panic!("solid-area declaration")
    };
    assert_close(actual.get(), value, 1.0e-8);
    assert_close(
        measure.source_error.expect("source precision").get(),
        source_error,
        1.0e-8,
    );
}

#[test]
fn ftc_08_saved_mass_properties_agree_with_active_body_at_six_decimal_precision() {
    let result = specimen("nist_ftc_08_asme1_rc_sw1802.SLDPRT");
    let ir = result.ir();
    assert_eq!(ir.model.declared_measures.len(), 2);
    let volume = ir
        .model
        .declared_measures
        .iter()
        .find(|measure| measure.id.as_str().ends_with("-volume"))
        .expect("saved volume");
    let area = ir
        .model
        .declared_measures
        .iter()
        .find(|measure| measure.id.as_str().ends_with("-area"))
        .expect("saved area");
    assert_active_body_target(ir, volume);
    assert_active_body_target(ir, area);
    assert_eq!(volume.provenance, DeclaredMeasureProvenance::SavedCache);
    assert_eq!(area.provenance, DeclaredMeasureProvenance::SavedCache);
    assert_volume(volume, 504_000.0, 500.0);
    assert_area(area, 191_706.0, 0.5);
    assert!(findings(ir).is_empty());
}

#[test]
fn ftc_06_saved_area_mismatch_warns_while_orientation_keeps_volume_uncompared() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    assert_eq!(ir.model.declared_measures.len(), 2);
    let volume = ir
        .model
        .declared_measures
        .iter()
        .find(|measure| measure.id.as_str().ends_with("-volume"))
        .expect("saved volume");
    let area = ir
        .model
        .declared_measures
        .iter()
        .find(|measure| measure.id.as_str().ends_with("-area"))
        .expect("saved area");
    assert_active_body_target(ir, volume);
    assert_active_body_target(ir, area);
    assert_eq!(volume.provenance, DeclaredMeasureProvenance::SavedCache);
    assert_eq!(area.provenance, DeclaredMeasureProvenance::SavedCache);
    assert_volume(volume, 3_325_000.0, 500.0);
    assert_area(area, 282_664.0, 0.5);
    let DeclaredTarget::Bodies { bodies } = &volume.target else {
        panic!("active body target")
    };
    assert_volume_unavailable_for_orientation(ir, &bodies[0]);

    let findings = findings(ir);
    let area_finding = findings
        .iter()
        .find(|finding| finding.entity.as_deref() == Some(area.id.as_str()))
        .expect("stale area finding");
    assert_eq!(area_finding.severity, Severity::Warning);
    let volume_finding = findings
        .iter()
        .find(|finding| finding.entity.as_deref() == Some(volume.id.as_str()))
        .expect("unresolved volume finding");
    assert_eq!(volume_finding.severity, Severity::Warning);
}
