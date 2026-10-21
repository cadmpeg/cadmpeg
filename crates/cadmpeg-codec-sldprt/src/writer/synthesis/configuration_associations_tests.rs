// SPDX-License-Identifier: Apache-2.0
//! Authored native configuration associations are explicit representation changes.

use std::io::Cursor;

use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{
    target::TargetRequest, ArenaCoverage, ArenaDisposition, EncodeInput, Encoder,
};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

#[test]
fn partition_configuration_reports_only_its_new_native_association() {
    let source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [configuration] = decoded.ir().model.configurations.as_slice() else {
        panic!("one actual partition configuration");
    };
    assert!(configuration.native_ref.is_none());
    let snapshot = configuration.clone();
    let plan = SldprtCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    assert!(
        matches!(plan.report().coverage, ArenaCoverage::Declared(coverage)
        if coverage.configurations == ArenaDisposition::Reported)
    );
    let notes = plan
        .report()
        .losses
        .iter()
        .filter(|note| note.code.local_code() == "writer.configuration-association-generated")
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one association change");
    };
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Info);
    assert_eq!(
        note.code.taxonomy(),
        cadmpeg_ir::report::loss::LossTaxonomy::CarrierSummary
    );
    assert!(note.message.contains(snapshot.id.as_str()));
    let mut projected = decoded.ir().clone();
    let mut losses = Vec::new();
    let native = super::design::prepare(&mut projected, &mut losses, &Default::default())
        .unwrap()
        .unwrap();
    let association = projected.model.configurations[0]
        .native_ref
        .as_ref()
        .unwrap();
    assert!(note.message.contains(association));
    assert_eq!(
        native
            .feature_histories
            .iter()
            .flat_map(|history| &history.configurations)
            .filter(|record| &record.id == association)
            .count(),
        1
    );
    assert_eq!(decoded.ir().model.configurations[0], snapshot);
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).unwrap();
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let [actual] = written.ir().model.configurations.as_slice() else {
        panic!("one authored configuration");
    };
    assert!(actual.native_ref.is_some());
    assert_eq!(actual.ordinal, snapshot.ordinal);
    assert_eq!(actual.active, snapshot.active);
    assert_eq!(actual.source_index, snapshot.source_index);
    assert_eq!(actual.name, snapshot.name);
    assert_eq!(actual.material, snapshot.material);
    assert_eq!(actual.properties, snapshot.properties);
    assert_eq!(
        actual.bodies.as_ref().unwrap().len(),
        snapshot.bodies.as_ref().unwrap().len()
    );
    assert_eq!(actual.parameter_values, snapshot.parameter_values);
    assert_eq!(actual.parameter_overrides, snapshot.parameter_overrides);
    assert_eq!(actual.feature_states, snapshot.feature_states);
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        decoded.ir(),
        written.ir(),
    );

    // A real existing counterpart is preserved, with no new-association note
    // and full configuration conservation through the same global checker.
    let bound = SldprtCodec
        .plan(EncodeInput::new(written.ir(), None), TargetRequest::Inherit)
        .unwrap();
    assert!(
        matches!(bound.report().coverage, ArenaCoverage::Declared(coverage)
        if coverage.configurations == ArenaDisposition::Written)
    );
    assert!(!bound
        .report()
        .losses
        .iter()
        .any(|note| note.code.local_code() == "writer.configuration-association-generated"));
    let mut bytes = Vec::new();
    let bound_report = bound.write_to(&mut bytes).unwrap();
    let rewritten = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &bound_report,
        written.ir(),
        rewritten.ir(),
    );
}
