// SPDX-License-Identifier: Apache-2.0
//! Synthetic evidence must not bless a Model, unresolved or absent stream.

use super::*;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::b1::provenance::NativeOwnership;
use cadmpeg_ir::source_fidelity::RecordLayout;
use cadmpeg_ir::{CodecFormat, SourceMeta};
use serde_json::json;

const STREAM_CLASSES: StreamClasses = StreamClasses::new(CodecFormat::Step, &["design"]);
const DESIGN: &str = "test:design:configuration#0";
const MODEL: &str = "test:model:product#0";

struct NoOwners;
impl NativeOwnerResolver for NoOwners {
    fn visit_owners(
        &self,
        _: &DecodeContext<'_>,
        _: &CadIr,
        _: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }
}

fn document() -> (CadIr, SourceFidelity) {
    let mut ir = CadIr::decoded(SourceMeta::unclassified(CodecFormat::Step, BTreeMap::new()));
    ir.model
        .configurations
        .push(serde_json::from_value(json!({"id":DESIGN})).unwrap());
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, DESIGN, "design");
    (ir, fidelity)
}
fn layout(fidelity: &mut SourceFidelity, id: &str, stream: &str) {
    fidelity.insert_record_layout(id.into(), serde_json::from_value::<RecordLayout>(json!({"stream":stream,"offset":0,"byte_len":4,"reads":{},"pointers":{},"payloads":{}})).unwrap()).unwrap();
}

#[test]
fn every_supplied_golden_must_have_exclusively_design_records_in_the_stream() {
    let ctx = crate::service_decode_context();
    let first = document();
    let mut second = document();
    check_design_only_streams(
        &ctx,
        STREAM_CLASSES,
        &[
            (&first.0, &first.1, &NoOwners),
            (&second.0, &second.1, &NoOwners),
        ],
    )
    .unwrap();
    second
        .0
        .model
        .product_definitions
        .push(serde_json::from_value(json!({"id":MODEL,"kind":"part"})).unwrap());
    layout(&mut second.1, MODEL, "design");
    assert!(check_design_only_streams(
        &ctx,
        STREAM_CLASSES,
        &[
            (&first.0, &first.1, &NoOwners),
            (&second.0, &second.1, &NoOwners)
        ]
    )
    .is_err());
}

#[test]
fn a_missing_layout_cannot_hide_a_model_record_from_the_golden_proof() {
    let ctx = crate::service_decode_context();
    let (mut ir, fidelity) = document();
    ir.model
        .product_definitions
        .push(serde_json::from_value(json!({"id":MODEL,"kind":"part"})).unwrap());
    assert!(
        check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &NoOwners)]).is_err()
    );
}

#[test]
fn an_unresolved_native_record_cannot_prove_a_design_only_stream() {
    struct NativeOwner;
    impl NativeOwnerResolver for NativeOwner {
        fn visit_owners(
            &self,
            _: &DecodeContext<'_>,
            _: &CadIr,
            visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            visitor(NativeOwnership::new(
                cadmpeg_ir::ids::Identity::new("test:source:record#0").unwrap(),
                None,
            ))
        }
    }
    let ctx = crate::service_decode_context();
    let (mut ir, mut fidelity) = document();
    let native = cadmpeg_ir::native::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new("test:source:record#0").unwrap(),
        serde_json::Map::new(),
    )
    .unwrap();
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .insert("objects".into(), vec![native]);
    layout(&mut fidelity, "test:source:record#0", "design");
    assert!(
        check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &NativeOwner)]).is_err()
    );
}

#[test]
fn no_goldens_no_records_and_a_missing_stream_are_not_design_proofs() {
    let ctx = crate::service_decode_context();
    assert!(check_design_only_streams(&ctx, STREAM_CLASSES, &[]).is_err());
    let empty = CadIr::decoded(SourceMeta::unclassified(CodecFormat::Step, BTreeMap::new()));
    assert!(check_design_only_streams(
        &ctx,
        STREAM_CLASSES,
        &[(&empty, &SourceFidelity::default(), &NoOwners)]
    )
    .is_err());
    let (ir, _) = document();
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, DESIGN, "other");
    assert!(
        check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &NoOwners)]).is_err()
    );
}

#[test]
fn the_golden_proof_rejects_another_format_and_invalid_table_names() {
    let ctx = crate::service_decode_context();
    let (mut ir, fidelity) = document();
    ir.source = Some(SourceMeta::unclassified(
        CodecFormat::Rhino,
        BTreeMap::new(),
    ));
    assert!(
        check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &NoOwners)]).is_err()
    );
    for names in [&[""][..], &["design", "design"][..]] {
        assert!(
            check_design_only_streams(&ctx, StreamClasses::new(CodecFormat::Step, names), &[])
                .is_err()
        );
    }
}

const BACKING_NATIVE: &str = "test:source:configuration#0";
struct BackedOwner;
impl NativeOwnerResolver for BackedOwner {
    fn visit_owners(
        &self,
        _: &DecodeContext<'_>,
        ir: &CadIr,
        visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for namespace in ir.native.0.values() {
            for records in namespace.arenas().values() {
                for record in records {
                    visitor(NativeOwnership::new(
                        cadmpeg_ir::ids::Identity::new(record.id()).unwrap(),
                        None,
                    ))?;
                }
            }
        }
        Ok(())
    }
}
fn backed_document() -> (CadIr, SourceFidelity) {
    let (mut ir, _) = document();
    ir.model.configurations[0].native_ref = Some(BACKING_NATIVE.into());
    ir.native.namespace_mut("test").arenas_mut().insert(
        "objects".into(),
        vec![cadmpeg_ir::native::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(BACKING_NATIVE).unwrap(),
            serde_json::Map::new(),
        )
        .unwrap()],
    );
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, BACKING_NATIVE, "design");
    (ir, fidelity)
}

#[test]
fn native_backed_design_records_use_the_actual_native_layout_without_a_neutral_duplicate() {
    let ctx = crate::service_decode_context();
    let (ir, fidelity) = backed_document();
    assert!(!fidelity.record_layout().contains_key(DESIGN));
    assert!(fidelity.record_layout().contains_key(BACKING_NATIVE));
    check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &BackedOwner)]).unwrap();
}

#[test]
fn a_native_reference_requires_an_actual_native_record_even_beside_valid_design_evidence() {
    let ctx = crate::service_decode_context();
    let (mut ir, mut fidelity) = document();
    const UNBACKED: &str = "test:design:configuration#unbacked";
    ir.model
        .configurations
        .push(serde_json::from_value(json!({"id":UNBACKED,"native_ref":BACKING_NATIVE})).unwrap());
    layout(&mut fidelity, UNBACKED, "design");
    assert!(fidelity.record_layout().contains_key(DESIGN));
    assert!(
        check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &NoOwners)]).is_err()
    );
}

#[test]
fn a_backed_neutral_layout_cannot_substitute_for_a_missing_native_layout() {
    let ctx = crate::service_decode_context();
    let (ir, _) = backed_document();
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, DESIGN, "design");
    assert!(!fidelity.record_layout().contains_key(BACKING_NATIVE));
    assert!(
        check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &BackedOwner)]).is_err()
    );
}

#[test]
fn unknown_link_proof_requires_each_actual_target_even_when_own_class_is_design() {
    let ctx = crate::service_decode_context();
    for (target, valid) in [(DESIGN, true), ("test:source:missing#0", false)] {
        let (mut ir, _) = document();
        let id = cadmpeg_ir::ids::UnknownId::mint("test:source:unknown#proof").unwrap();
        ir.model.configurations[0].native_ref = Some(id.as_str().into());
        ir.native.namespace_mut("test").arenas_mut().insert(
            "unknowns".into(),
            vec![cadmpeg_ir::native::NativeRecord::from(
                &cadmpeg_ir::NativeUnknownRecord {
                    id: id.clone(),
                    links: vec![cadmpeg_ir::ids::Identity::new(target).unwrap()],
                },
            )],
        );
        let mut fidelity = SourceFidelity::default();
        layout(&mut fidelity, id.as_str(), "design");
        assert_eq!(
            check_design_only_streams(&ctx, STREAM_CLASSES, &[(&ir, &fidelity, &NoOwners)]).is_ok(),
            valid
        );
    }
}
