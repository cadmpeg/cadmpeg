// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::target::{
    DefaultSource, TargetCatalog, TargetDescriptor, TargetRefusalKind, TargetToken,
};
use cadmpeg_core::CodecError;

use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::FidelityResolution;
use crate::source_fidelity::SourceFidelity;
use crate::validate::validate_neutral;
use crate::CadIr;

use super::resolve::resolve_write_request;
use super::*;

#[test]
fn cadir_encoder_streams_the_canonical_json_shape() {
    let ir = unit_cube();
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
    const FORMAT: &'static str = "cadir";
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
    assert_eq!(plan.report().fidelity, FidelityResolution::NotProvided);
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
    const FORMAT: &'static str = "test";
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
    assert_eq!(plan.report().target(), Some(&DialectId::pinned("test:new")));
    assert_eq!(plan.report().notes, vec!["resolved test:new".to_owned()]);
}

#[test]
fn fidelity_resolution_is_not_provided_whenever_the_input_carries_none() {
    let ir = CadIr::empty();
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("new"))
        .unwrap();
    assert_eq!(plan.report().fidelity, FidelityResolution::NotProvided);
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
        plan.report().fidelity,
        FidelityResolution::Degraded {
            reason: "test backend never replays".to_owned()
        }
    );
}

#[test]
fn write_path_structurally_authors_fidelity_resolution() {
    assert_eq!(
        WritePath::VerbatimReplay.into_report(true),
        (
            crate::report::WritePath::VerbatimReplay,
            FidelityResolution::Replayed
        )
    );
    assert_eq!(
        WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        }
        .into_report(true),
        (
            crate::report::WritePath::Patched,
            FidelityResolution::Replayed
        )
    );
    assert_eq!(
        WritePath::Patched {
            consumption: PatchConsumption::Independent(Consumption::NotConsumed),
        }
        .into_report(true),
        (
            crate::report::WritePath::Patched,
            FidelityResolution::NotConsumed
        )
    );
    assert_eq!(
        WritePath::Synthesized {
            consumption: Consumption::Degraded {
                reason: "missing source".into(),
            },
        }
        .into_report(false),
        (
            crate::report::WritePath::Synthesized,
            FidelityResolution::NotProvided
        )
    );
}

#[test]
fn an_empty_native_catalog_has_no_format_identity_request() {
    let ir = CadIr::empty();
    let error = resolve_write_request(&ir, TargetRequest::Inherit, "cadir", TargetCatalog::EMPTY)
        .unwrap_err();

    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("an empty native catalog must refuse without inventing an identity request")
    };
    let TargetRefusalKind::NoDefault { source, .. } = refusal.kind() else {
        panic!("a source-free inherit request must report a missing default")
    };
    assert_eq!(source, &DefaultSource::NoSource);
    assert!(refusal.available().is_empty());
}

const CATALOG_WRITE_TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: DialectId::pinned("test:old"),
        aliases: &["old"],
    },
    TargetDescriptor {
        id: DialectId::pinned("test:new"),
        aliases: &["new"],
    },
];
const CATALOG_WRITE_CATALOG: TargetCatalog = TargetCatalog::new(CATALOG_WRITE_TARGETS, Some(1));

fn catalog_write_ir(source: Option<(&str, Option<&'static str>)>) -> CadIr {
    let mut ir = CadIr::empty();
    ir.source = source.map(|(format, dialect)| match dialect {
        Some(id) => crate::document::SourceMeta::classified(
            DialectLayers::of(DialectMatch::admitted(DialectId::pinned(id))),
            BTreeMap::new(),
        ),
        None => serde_json::from_value(serde_json::json!({
            "format": format,
            "attributes": {},
        }))
        .unwrap(),
    });
    ir
}

#[test]
fn write_request_resolves_an_explicit_on_catalog_target() {
    let ir = catalog_write_ir(None);
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("old"),
        "test",
        CATALOG_WRITE_CATALOG,
    )
    .unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:old");
    assert!(!resolved.preserves_source());
    assert!(!resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_refuses_an_unknown_explicit_target_with_the_catalog() {
    let ir = catalog_write_ir(None);
    let error = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:missing"),
        "test",
        CATALOG_WRITE_CATALOG,
    )
    .unwrap_err();
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("expected an unsupported target");
    };
    let TargetRefusalKind::UnknownExplicit { requested, .. } = refusal.kind() else {
        panic!("the explicit token is outside the catalog")
    };
    assert_eq!(requested, &TargetToken::new("test:missing"));
    assert_eq!(
        refusal
            .available()
            .iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        ["test:old", "test:new"]
    );
}

#[test]
fn write_request_inherit_with_a_cross_format_source_uses_the_default() {
    let ir = catalog_write_ir(Some(("other", Some("other:only"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:new");
    assert!(!resolved.preserves_source());
    assert!(!resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_refuses_a_same_format_unrecorded_source() {
    let ir = catalog_write_ir(Some(("test", None)));
    let error = resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_CATALOG)
        .unwrap_err();

    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("an unrecorded same-format source must produce a target refusal")
    };
    assert!(matches!(
        refusal.kind(),
        TargetRefusalKind::UnrecordedSource
    ));
    assert_eq!(refusal.format(), "test");
}

#[test]
fn write_request_explicit_over_an_unrecorded_source_has_no_recorded_relation() {
    let ir = catalog_write_ir(Some(("test", None)));
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:old"),
        "test",
        CATALOG_WRITE_CATALOG,
    )
    .unwrap();

    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:old");
    assert!(!resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_preserves_a_same_format_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:old");
    assert!(resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_preserves_a_same_format_off_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(resolved.entry(), None);
    assert_eq!(resolved.target_id().as_str(), "test:future");
    assert!(resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn catalog_write_explicit_difference_returns_the_displaced_dialect() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:new"),
        "test",
        CATALOG_WRITE_CATALOG,
    )
    .unwrap();
    let entry = resolved.entry().expect("expected a catalog target");
    let displaced = resolved
        .displaced_source()
        .expect("the explicit target displaces the recorded source");
    assert_eq!(entry.id.as_str(), "test:new");
    assert_eq!(displaced, &DialectId::pinned("test:old"));
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(
        resolved.displacement_message().as_deref(),
        Some(
        "source dialect test:old was displaced by target dialect test:new; the source dialect identity is not preserved"
        )
    );
}
