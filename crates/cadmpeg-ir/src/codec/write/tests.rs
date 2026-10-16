// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::wire;

use super::{
    target::{Catalog, DialectFree, ResolvedWrite, TargetRequest},
    ArenaCoverage, ArenaDisposition, ArenaDispositions, CadirEncoder, Consumption, EncodeInput,
    Encoder, EncoderBackend, ExportBody, PatchConsumption, WritePath,
};
use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::codec::FormatId;
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::check::{Check, Finding, ValidationReport};
use crate::report::export::FidelityResolution;
use crate::report::Severity;
use crate::schema::{EntityKind, EntitySchema};
use crate::source_fidelity::SourceFidelity;
use crate::validate::validate_neutral;
use crate::CadIr;
use cadmpeg_core::CodecError;

#[test]
fn cadir_encoder_streams_the_canonical_json_shape() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let mut encoded = Vec::new();
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("empty-catalog inheritance resolves to CADIR identity");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref(),
        None
    );
    plan.write_to(&mut encoded).unwrap();
    let mut canonical = ir.to_canonical_json().unwrap();
    canonical.push('\n');
    assert_eq!(encoded, canonical.as_bytes());
}

#[test]
fn cadir_encoder_census_matches_validation_counts() {
    let ir = directed_subd_sum().unwrap();
    let validation_counts = validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .entity_counts;
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
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
        ))
    }
}

#[test]
fn the_wrapper_stamps_cadir_on_a_dialect_free_plan() {
    let ir = CadIr::empty();
    let plan = NeutralEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert_eq!(plan.report().format(), "cadir");
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref(),
        None
    );
    assert_eq!(
        wire::field::<crate::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        FidelityResolution::NotProvided {}
    );
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
    assert_eq!(
        serde_json::to_value(&refusal).expect("serialize refusal")["refusal"]["kind"],
        "unknown_explicit"
    );
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
        let mut body = ExportBody::synthesized(Vec::new(), input.ir, ArenaCoverage::Complete);
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
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref(),
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
    assert_eq!(
        wire::field::<crate::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        FidelityResolution::NotProvided {}
    );
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
        wire::field::<crate::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
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

macro_rules! uniform_dispositions {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        fn uniform_dispositions(disposition: ArenaDisposition) -> ArenaDispositions {
            ArenaDispositions { $($field: disposition,)* }
        }
    };
}

crate::document::arena_registry!(uniform_dispositions);

struct CoverageEncoder(ArenaCoverage);

impl EncoderBackend for CoverageEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(Vec::new(), input.ir, self.0))
    }
}

fn coverage_losses(ir: &CadIr, coverage: ArenaCoverage) -> Vec<crate::report::loss::LossNote> {
    CoverageEncoder(coverage)
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap()
        .report()
        .losses
        .clone()
}

#[test]
fn the_wrapper_charges_one_loss_per_omitted_nonempty_arena() {
    let ir = unit_cube().expect("valid unit cube fixture");
    assert_eq!(ir.model.faces.len(), 6);
    assert!(ir.model.subds.is_empty());
    let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
    dispositions.faces = ArenaDisposition::Omitted;
    dispositions.subds = ArenaDisposition::Omitted;

    let losses = coverage_losses(&ir, ArenaCoverage::Declared(dispositions));

    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(losses[0].code.namespace(), "export");
    assert_eq!(losses[0].code.local_code(), "arena.omitted");
    assert_eq!(
        losses[0].code.taxonomy(),
        crate::report::loss::LossTaxonomy::TopologyNotTransferred
    );
    assert!(
        losses[0].message.contains("`faces`"),
        "{}",
        losses[0].message
    );
    assert!(
        losses[0].message.contains("6 record(s)"),
        "{}",
        losses[0].message
    );
}

#[test]
fn written_reported_dropped_and_complete_coverage_charge_no_loss() {
    let ir = unit_cube().expect("valid unit cube fixture");
    for coverage in [
        ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Written)),
        ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Reported)),
        ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Dropped)),
        ArenaCoverage::Complete,
    ] {
        assert_eq!(coverage_losses(&ir, coverage), Vec::new(), "{coverage:?}");
    }
}

#[test]
fn arena_dispositions_carry_each_declared_field() {
    let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
    dispositions.faces = ArenaDisposition::Dropped;
    dispositions.features = ArenaDisposition::Omitted;
    assert!(!dispositions.carries(EntityKind::Face));
    assert!(!dispositions.carries(EntityKind::Feature));
    assert!(dispositions.carries(EntityKind::Edge));
    let reported = uniform_dispositions(ArenaDisposition::Reported);
    for kind in EntityKind::ALL {
        assert!(reported.carries(kind), "{kind:?}");
        assert!(ArenaCoverage::Complete.carries(kind), "{kind:?}");
    }
}

struct F3dCoverageEncoder(ArenaCoverage);

impl EncoderBackend for F3dCoverageEncoder {
    const FORMAT: FormatId = FormatId::new("f3d");
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(Vec::new(), input.ir, self.0))
    }
}

fn finding(entity: Option<&str>, message: &str, severity: Severity) -> Finding {
    Finding {
        related: Vec::new(),
        check: Check::ReferentialIntegrity,
        severity,
        entity: entity.map(str::to_owned),
        message: message.into(),
    }
}

fn validation_report(findings: Vec<Finding>) -> ValidationReport {
    ValidationReport {
        entity_counts: std::collections::BTreeMap::new(),
        findings,
        losses: Vec::new(),
    }
}

#[test]
fn carried_errors_exclude_warnings_and_include_unlocated_errors() {
    let ir = CadIr::empty();
    let plan = CoverageEncoder(ArenaCoverage::Declared(uniform_dispositions(
        ArenaDisposition::Dropped,
    )))
    .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
    .unwrap();
    let mut report = validation_report(vec![finding(None, "warning", Severity::Warning)]);
    assert!(plan.carried_errors(&ir, &report).is_empty());
    report
        .findings
        .push(finding(None, "error", Severity::Error));
    report.findings.push(finding(
        Some("test:face#missing"),
        "missing",
        Severity::Blocking,
    ));
    assert_eq!(
        plan.carried_errors(&ir, &report),
        vec![&report.findings[1], &report.findings[2]]
    );
}

#[test]
fn carried_errors_follow_model_coverage_and_related_locations() {
    let ir = unit_cube().expect("valid cube");
    let face = ir.model.faces[0].identity();
    let edge = ir.model.edges[0].identity();
    let report = validation_report(vec![
        finding(Some(face), "invalid face", Severity::Error),
        finding(
            Some(face),
            &format!("invalid reference `{edge}`."),
            Severity::Error,
        ),
    ]);
    let mut report = report;
    report.findings[1].related.push(edge.to_owned());
    for (disposition, carries_face) in [
        (ArenaDisposition::Written, true),
        (ArenaDisposition::Reported, true),
        (ArenaDisposition::Dropped, false),
        (ArenaDisposition::Omitted, false),
    ] {
        let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
        dispositions.faces = disposition;
        let plan = CoverageEncoder(ArenaCoverage::Declared(dispositions))
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let expected = if carries_face {
            vec![&report.findings[0], &report.findings[1]]
        } else {
            vec![&report.findings[1]]
        };
        assert_eq!(plan.carried_errors(&ir, &report), expected);
    }
}

#[test]
fn carried_errors_follow_native_format_and_complete_coverage() {
    let mut ir = CadIr::empty();
    let id = "f3d:test:asm-history#0";
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "asm_histories".into(),
        vec![crate::NativeRecord::new(
            crate::ids::Identity::new(id).expect("valid native identity"),
            serde_json::Map::default(),
        )
        .expect("valid native record")],
    );
    ir.finalize();
    let report = validation_report(vec![finding(
        Some(id),
        "invalid native record",
        Severity::Error,
    )]);
    let declared = ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Written));
    let cadir = CoverageEncoder(declared)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(cadir.carried_errors(&ir, &report).is_empty());
    let f3d = F3dCoverageEncoder(declared)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let complete = CoverageEncoder(ArenaCoverage::Complete)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for plan in [f3d, complete] {
        assert_eq!(plan.carried_errors(&ir, &report), vec![&report.findings[0]]);
    }
}
