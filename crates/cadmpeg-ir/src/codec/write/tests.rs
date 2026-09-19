// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{
    target::{Catalog, DialectFree, ResolvedWrite, TargetRequest},
    CadirEncoder, Consumption, EncodeInput, Encoder, EncoderBackend, ExportBody, PatchConsumption,
    WritePath,
};
use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::codec::FormatId;
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::export::FidelityResolution;
use crate::source_fidelity::SourceFidelity;
use crate::validate::validate_neutral;
use crate::CadIr;
use cadmpeg_core::target::TargetRefusalKind;
use cadmpeg_core::CodecError;

#[test]
fn cadir_encoder_streams_the_canonical_json_shape() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let mut encoded = Vec::new();
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("empty-catalog inheritance resolves to CADIR identity");
    assert_eq!(plan.report().target(), None);
    plan.write_to(&mut encoded).unwrap();
    let mut canonical = ir.to_canonical_json().unwrap();
    canonical.push('\n');
    assert_eq!(encoded, canonical.as_bytes());
}

#[test]
fn cadir_encoder_census_matches_validation_counts() {
    let ir = directed_subd_sum().unwrap();
    let validation_counts = validate_neutral(&ir, Vec::new()).entity_counts;
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan CADIR export");

    assert_eq!(plan.report().census.counts, validation_counts);
}

struct NeutralEncoder;

impl EncoderBackend for NeutralEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        // The body carries no identity: whatever the backend does, the
        // wrapper stamps FORMAT.
        Ok(ExportBody::synthesized(Vec::new(), input.ir))
    }
}

#[test]
fn the_wrapper_stamps_cadir_on_a_dialect_free_plan() {
    let ir = CadIr::empty();
    let plan = NeutralEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert_eq!(plan.report().format(), "cadir");
    assert_eq!(plan.report().target(), None);
    assert_eq!(plan.report().fidelity(), FidelityResolution::NotProvided {});
}

#[test]
fn a_dialect_free_encoder_refuses_an_explicit_target() {
    let ir = CadIr::empty();
    let error = NeutralEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("any"))
        .unwrap_err();
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("a dialect-free encoder has no explicit targets")
    };
    assert!(matches!(
        refusal.kind(),
        TargetRefusalKind::UnknownExplicit { .. }
    ));
    assert!(refusal.available().is_empty());
}

struct CatalogEncoder;

impl EncoderBackend for CatalogEncoder {
    const FORMAT: FormatId = FormatId::new("test");
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(CATALOG_WRITE_TARGETS, Some(1));

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(Vec::new(), input.ir);
        body.notes
            .push(format!("resolved {}", target.target_id().as_str()));
        body.write_path = WritePath::Synthesized {
            consumption: Consumption::Degraded {
                reason: "test backend never replays".to_owned(),
            },
        };
        Ok(body)
    }
}

#[test]
fn the_wrapper_stamps_the_resolved_target_on_a_catalog_plan() {
    let ir = CadIr::empty();
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("new"))
        .unwrap();
    assert_eq!(plan.report().format(), "test");
    assert_eq!(
        plan.report().target(),
        Some(&cadmpeg_core::dialect_id!("test:new"))
    );
    assert_eq!(plan.report().notes, vec!["resolved test:new".to_owned()]);
}

#[test]
fn fidelity_resolution_is_not_provided_whenever_the_input_carries_none() {
    let ir = CadIr::empty();
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("new"))
        .unwrap();
    assert_eq!(plan.report().fidelity(), FidelityResolution::NotProvided {});
}

#[test]
fn fidelity_resolution_follows_the_backend_consumption_when_provided() {
    let ir = CadIr::empty();
    let fidelity = SourceFidelity::default();
    let plan = CatalogEncoder
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Explicit("new"),
        )
        .unwrap();
    assert_eq!(
        plan.report().fidelity(),
        FidelityResolution::Degraded {
            reason: "test backend never replays".to_owned()
        }
    );
}

#[test]
fn write_path_structurally_authors_fidelity_resolution() {
    assert_eq!(
        WritePath::VerbatimReplay.into_report(true),
        crate::report::export::WritePath::VerbatimReplay {
            fidelity: crate::report::export::ReplayFidelity::Replayed {}
        }
    );
    assert_eq!(
        WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        }
        .into_report(true),
        crate::report::export::WritePath::Patched {
            fidelity: FidelityResolution::Replayed {}
        }
    );
    assert_eq!(
        WritePath::Patched {
            consumption: PatchConsumption::Independent(Consumption::NotConsumed),
        }
        .into_report(true),
        crate::report::export::WritePath::Patched {
            fidelity: FidelityResolution::NotConsumed {}
        }
    );
    assert_eq!(
        WritePath::Synthesized {
            consumption: Consumption::Degraded {
                reason: "missing source".into(),
            },
        }
        .into_report(false),
        crate::report::export::WritePath::Synthesized {
            fidelity: crate::report::export::SynthesisFidelity::NotProvided {}
        }
    );
}

/// The CADIR encoder writes through the finite adapter: a document
/// holding a non-finite float is refused, not written with `null` in its
/// place.
#[test]
fn the_cadir_encoder_refuses_a_non_finite_document() {
    let mut ir = crate::document::CadIr::empty();
    crate::test_support::push_texture_offset(&mut ir, f64::NAN);
    let Err(error) = CadirEncoder.plan_resolved(EncodeInput::new(&ir, None), ()) else {
        panic!("a non-finite float has no canonical JSON");
    };
    let error = error.to_string();
    assert!(error.contains("non-finite"), "{error}");

    ir.model.appearances[0].textures[0].mapping.u_offset = 1.0;
    let body = CadirEncoder
        .plan_resolved(EncodeInput::new(&ir, None), ())
        .expect("a finite document writes");
    let text = String::from_utf8(body.bytes).expect("CADIR is UTF-8");
    assert!(!text.contains("null"), "{text}");
}
