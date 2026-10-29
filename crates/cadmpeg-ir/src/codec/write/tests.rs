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
use cadmpeg_core::decode::DecodeDepth;
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
    const REUSES_SOURCE: bool = false;
    const FORMAT: FormatId = FormatId::new("cadir");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
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
    const REUSES_SOURCE: bool = false;
    const FORMAT: FormatId = FormatId::new("test");
    const REQUEST: DecodeDepth = DecodeDepth::Geometry;
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

fn recorded_at(depth: DecodeDepth) -> CadIr {
    let mut ir = CadIr::empty();
    ir.set_scope(match depth {
        DecodeDepth::Container => crate::ContentScope::container(),
        DecodeDepth::Geometry => crate::ContentScope::geometry(),
        DecodeDepth::Full => crate::ContentScope::full(),
    })
    .unwrap();
    ir
}

#[test]
fn synthesized_writers_accept_scopes_and_report_absent_consumed_classes() {
    let ir = recorded_at(DecodeDepth::Geometry);
    let plan = F3dCoverageEncoder(ArenaCoverage::Complete)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .notes
        .iter()
        .any(|note| note.contains("scope excludes design")));
    assert!(plan.report().losses.is_empty());
    let ir = recorded_at(DecodeDepth::Container);
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("new"))
        .unwrap();
    assert!(plan
        .report()
        .notes
        .iter()
        .any(|note| note.contains("scope excludes model")));
}

#[test]
fn the_cadir_encoder_writes_the_scope_a_document_holds() {
    assert_eq!(CadirEncoder.request(), DecodeDepth::Full);
    for depth in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Full,
    ] {
        let ir = recorded_at(depth);
        let plan = CadirEncoder
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut encoded = Vec::new();
        plan.write_to(&mut encoded).unwrap();
        let written: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(written["scope"], serde_json::to_value(ir.scope).unwrap());
    }
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
    const REUSES_SOURCE: bool = false;
    const FORMAT: FormatId = FormatId::new("cadir");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
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
    const REUSES_SOURCE: bool = false;
    const FORMAT: FormatId = FormatId::new("f3d");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
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

struct PathEncoder(WritePath);

impl EncoderBackend for PathEncoder {
    const REUSES_SOURCE: bool = true;
    const FORMAT: FormatId = FormatId::new("test");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(Vec::new(), input.ir, ArenaCoverage::Complete);
        if input.fidelity.is_some() {
            body.write_path = self.0.clone();
        }
        Ok(body)
    }
}

#[test]
fn the_sealed_wrapper_synthesizes_instead_of_exposing_untyped_source_bytes() {
    let ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
    fidelity
        .insert_retained_record(
            crate::ids::UnknownId::mint("test:source:record#image").unwrap(),
            crate::RetainedSourceRecord::whole("", b"unread design".to_vec())
                .with_untyped(crate::ArenaClasses::from([crate::ArenaClass::Design])),
        )
        .unwrap();
    for path in [
        WritePath::VerbatimReplay,
        WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        },
    ] {
        let plan = PathEncoder(path)
            .plan(
                EncodeInput::new(&ir, Some(&fidelity)),
                TargetRequest::Inherit,
            )
            .expect("unrepresented source content selects synthesis, never refusal");
        assert!(matches!(
            plan.report().write_path(),
            crate::report::export::WritePath::Synthesized { .. }
        ));
        assert!(matches!(
            plan.report().synthesis_cause(),
            Some(super::SynthesisCause::SourceUnrepresented { classes })
                if classes == &crate::ArenaClasses::from([crate::ArenaClass::Design])
        ));
    }
    let cadir = CadirEncoder
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(cadir.report().synthesis_cause().is_none());
}

#[test]
fn declared_native_omission_is_informational_and_written_records_are_not_charged() {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("test").arenas_mut().insert(
        "records".into(),
        vec![crate::native::NativeRecord::new(
            crate::ids::Identity::new("test:native:record#0").unwrap(),
            [("value".into(), serde_json::json!(1))]
                .into_iter()
                .collect(),
        )
        .unwrap()],
    );
    let plan = PathEncoder(WritePath::Synthesized {
        consumption: Consumption::NotConsumed,
    })
    .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
    .unwrap();
    let losses = &plan.report().losses;
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "native.records-omitted");
    assert_eq!(losses[0].severity, Severity::Info);
    let written = super::NativeDispositions::written(&ir);
    assert!(written.omission_losses(&ir, "test").is_empty());
    assert_eq!(
        super::NativeDispositions::default()
            .omission_losses(&ir, "test")
            .len(),
        1
    );
}

#[test]
fn patch_decline_is_reported_once_as_structured_synthesis_metadata() {
    struct Declined;
    impl EncoderBackend for Declined {
        const REUSES_SOURCE: bool = false;
        const FORMAT: FormatId = FormatId::new("test");
        const REQUEST: DecodeDepth = DecodeDepth::Full;
        type Target = DialectFree;
        const TARGET: DialectFree = DialectFree;
        fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
            let mut body = ExportBody::synthesized(Vec::new(), input.ir, ArenaCoverage::Complete);
            body.synthesis_cause = Some(super::SynthesisCause::PatchDeclined {
                reason: "edited design".into(),
            });
            Ok(body)
        }
    }
    let ir = CadIr::empty();
    let plan = Declined
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(
        matches!(plan.report().synthesis_cause(), Some(super::SynthesisCause::PatchDeclined { reason }) if reason == "edited design")
    );
    assert!(plan.report().notes.is_empty());
}

#[test]
fn foreign_native_content_uses_the_wrappers_typed_arena_boundary() {
    let mut ir = CadIr::empty();
    let format = FormatId::new("test");
    assert!(!EncodeInput::new(&ir, None).has_foreign_native_content(format));
    let record = || {
        crate::native::NativeRecord::new(
            crate::ids::Identity::new("foreign:native:record#0").unwrap(),
            [("value".into(), serde_json::json!(1))]
                .into_iter()
                .collect(),
        )
        .unwrap()
    };
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .insert("records".into(), vec![record()]);
    ir.native
        .namespace_mut("foreign")
        .arenas_mut()
        .insert("records".into(), Vec::new());
    ir.native
        .namespace_mut("foreign")
        .arenas_mut()
        .insert("unknowns".into(), vec![record()]);
    assert!(!EncodeInput::new(&ir, None).has_foreign_native_content(format));
    let mut dispositions = super::NativeDispositions::for_namespace(
        "test",
        ["records"],
        super::NativeDisposition::Written,
    );
    dispositions.complete(&ir);
    assert!(dispositions.dropped_counts(&ir).is_empty());
    assert!(!dispositions.namespaces["foreign"]
        .arenas
        .contains_key("unknowns"));
    ir.native
        .namespace_mut("foreign")
        .arenas_mut()
        .insert("records".into(), vec![record()]);
    assert!(EncodeInput::new(&ir, None).has_foreign_native_content(format));
    let losses = dispositions.omission_losses(&ir, "test");
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("foreign"));
    assert_eq!(losses[0].code.local_code(), "native.records-omitted");
    assert_eq!(
        losses[0].message,
        "the test write omits 1 native record(s) from `foreign.records`: foreign:native:record#0"
    );
}

#[test]
fn malformed_scope_is_rejected_at_the_writer_boundary() {
    let mut ir = unit_cube().unwrap();
    // A caller can bypass set_scope through the public field, but cannot
    // export the resulting contradiction through a backend that ignores it.
    ir.scope = crate::ContentScope::container();
    let error = NeutralEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap_err();
    let CodecError::Malformed(message) = error else {
        panic!("expected malformed document state: {error}");
    };
    assert!(message.contains("scope excludes model"), "{message}");
    assert!(message.contains("bodies"), "{message}");
}

mod framework;

mod budgets;
