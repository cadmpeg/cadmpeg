// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::wire;

use super::{
    target::{Catalog, DialectFree, ResolvedWrite, TargetRequest},
    ArenaCoverage, ArenaDisposition, ArenaDispositions, CadirEncoder, Consumption, EncodeError,
    EncodeInput, Encoder, EncoderBackend, ExportBody, PatchConsumption, WritePath,
};
use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::codec::FormatId;
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::check::{Check, Finding, ValidationReport};
use crate::report::export::FidelityResolution;
use crate::report::strictness::{code_name, Strictness};
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
    let EncodeError::UnsupportedTarget(refusal) = error else {
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
    ir.set_scope(depth.scope())
        .expect("an empty document admits every scope");
    ir
}

fn depth_notes(plan: &super::ExportPlan) -> Vec<&String> {
    plan.report()
        .notes
        .iter()
        .filter(|note| note.starts_with("the document records "))
        .collect()
}

#[test]
fn plan_never_refuses_on_scope_and_notes_a_shortfall() {
    for recorded in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Design,
    ] {
        let ir = recorded_at(recorded);
        let plan = F3dCoverageEncoder(ArenaCoverage::Complete)
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        assert_eq!(
            depth_notes(&plan),
            [&format!(
                "the document records {recorded} depth; the f3d writer plans from full, so it \
                 wrote what the document holds"
            )]
        );
    }
    let full = recorded_at(DecodeDepth::Full);
    let plan = F3dCoverageEncoder(ArenaCoverage::Complete)
        .plan(EncodeInput::new(&full, None), TargetRequest::Inherit)
        .unwrap();
    assert!(depth_notes(&plan).is_empty());
}

#[test]
fn a_geometry_request_encoder_plans_a_container_document_with_a_note() {
    let geometry = recorded_at(DecodeDepth::Geometry);
    let plan = CatalogEncoder
        .plan(
            EncodeInput::new(&geometry, None),
            TargetRequest::Explicit("new"),
        )
        .unwrap();
    assert!(depth_notes(&plan).is_empty());
    let container = recorded_at(DecodeDepth::Container);
    let plan = CatalogEncoder
        .plan(
            EncodeInput::new(&container, None),
            TargetRequest::Explicit("new"),
        )
        .unwrap();
    assert_eq!(depth_notes(&plan).len(), 1);
}

#[test]
fn the_cadir_encoder_writes_the_depth_a_document_records() {
    assert_eq!(CadirEncoder.request(), DecodeDepth::Full);
    for depth in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Design,
        DecodeDepth::Full,
    ] {
        let ir = recorded_at(depth);
        let plan = CadirEncoder
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut encoded = Vec::new();
        plan.write_to(&mut encoded).unwrap();
        let written: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            written["scope"],
            serde_json::to_value(depth.scope()).unwrap()
        );
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
    let fidelity = SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(b"test source"));
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
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        fn uniform_dispositions(disposition: ArenaDisposition) -> ArenaDispositions {
            ArenaDispositions {
                $($field: disposition,)*
                design: super::DesignArenaDispositions::uniform(disposition),
                process: super::ProcessArenaDispositions::uniform(disposition),
            }
        }
    };
}

crate::document::arena_registry!(uniform_dispositions);

struct CoverageEncoder(ArenaCoverage);

impl EncoderBackend for CoverageEncoder {
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

fn design_sheet(key: &str) -> crate::design::spreadsheet::Spreadsheet {
    crate::design::spreadsheet::Spreadsheet::new(
        crate::spreadsheets::SpreadsheetId::mint(format!("test:design:spreadsheet#{key}")).unwrap(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
    )
    .unwrap()
}

fn v10_sheet() -> crate::spreadsheets::Spreadsheet {
    crate::spreadsheets::Spreadsheet::new(
        crate::spreadsheets::SpreadsheetId::mint("test:design:spreadsheet#v10").unwrap(),
        crate::features::FeatureId::mint("test:design:feature#v10").unwrap(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
    )
    .unwrap()
}

/// A CADIR write charges a record of an arena the wire omits only where no
/// `design` record it writes restates the record under its id; a write that
/// omits the design record too charges the omitted record.
#[test]
fn a_cadir_write_charges_only_omitted_records_no_written_design_record_restates() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    let cadir = super::coverage::cadir_dispositions();
    assert_eq!(cadir.omitted(&ir), ["spreadsheets"]);
    ir.design.spreadsheets.push(design_sheet("v10"));
    assert!(cadir.omitted(&ir).is_empty());
    assert_eq!(
        uniform_dispositions(ArenaDisposition::Omitted).omitted(&ir),
        ["spreadsheets"]
    );
}

/// `omitted` names the arenas the `arena.omitted` losses name: an omitted
/// design arena that a v10 record restates is not among them.
#[test]
fn omitted_names_the_arenas_the_omission_losses_name() {
    let mut ir = CadIr::empty();
    ir.design.spreadsheets.push(design_sheet("v11"));
    let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
    assert!(dispositions.omitted(&ir).is_empty());
    dispositions.design.spreadsheets = ArenaDisposition::Omitted;
    assert_eq!(dispositions.omitted(&ir), ["design.spreadsheets"]);

    ir.model.spreadsheets.push(v10_sheet());
    dispositions.spreadsheets = ArenaDisposition::Omitted;
    assert_eq!(dispositions.omitted(&ir), ["spreadsheets"]);
}

/// While a document holds a v10 design record, an omitted design-home arena
/// restates what the v10 path writes and is charged nothing.
#[test]
fn an_omitted_design_arena_is_charged_only_without_v10_design_records() {
    let mut ir = CadIr::empty();
    ir.design.spreadsheets.push(design_sheet("v11"));
    let omitted = ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Omitted));
    let losses = coverage_losses(&ir, omitted);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(
        losses[0].message.contains("`design.spreadsheets`"),
        "{losses:?}"
    );

    ir.model.spreadsheets.push(v10_sheet());
    let losses = coverage_losses(&ir, omitted);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].message.contains("`spreadsheets`"), "{losses:?}");
}

/// No v10 record restates an evaluation, so an omitted `design.evaluations`
/// arena is charged while the document holds v10 design records.
#[test]
fn an_omitted_evaluation_arena_is_charged_beside_v10_design_records() {
    use crate::design::evaluation::{Evaluation, EvaluationStatus};

    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    ir.design.spreadsheets.push(design_sheet("v11"));
    ir.design.evaluations.push(Evaluation {
        id: crate::design::ids::EvaluationId::mint("test:design:evaluation#1").unwrap(),
        scope: crate::ids::ProductDefinitionId::mint("test:model:definition#1").unwrap(),
        configuration: None,
        of: crate::design::refs::Evaluated::Tool(
            crate::features::FeatureId::mint("test:design:feature#v10").unwrap(),
        ),
        status: EvaluationStatus::Stale,
        content: None,
        native_ref: None,
    });
    let losses = coverage_losses(
        &ir,
        ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Omitted)),
    );
    assert_eq!(losses.len(), 2, "{losses:?}");
    for (loss, arena) in losses.iter().zip(["spreadsheets", "design.evaluations"]) {
        assert_eq!(loss.code.namespace(), "export");
        assert_eq!(loss.code.local_code(), "arena.omitted");
        assert!(loss.message.contains(&format!("`{arena}`")), "{losses:?}");
    }
}

/// An omitted sketch arena of the design home is charged as an omitted
/// parametric record, and a written one is not.
#[test]
fn an_omitted_design_sketch_arena_is_charged() {
    use crate::design::evaluation::EvaluationStatus;
    use crate::design::ops::sketch::{Sketch, SketchPlane, SketchSolve, SpatialSketch};
    use crate::design::options::NativeOptions;

    let solve = || SketchSolve {
        state: None,
        status: EvaluationStatus::Current,
    };
    let owner = crate::features::FeatureId::mint("test:design:feature#1").unwrap();
    let mut ir = CadIr::empty();
    ir.design.sketches.push(Sketch {
        id: crate::sketches::SketchId::mint("test:design:sketch#1").unwrap(),
        owner: owner.clone(),
        name: None,
        plane: SketchPlane::Block,
        solve: solve(),
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    });
    ir.design.spatial_sketches.push(SpatialSketch {
        id: crate::sketches::SpatialSketchId::mint("test:design:spatial-sketch#1").unwrap(),
        owner,
        name: None,
        solve: solve(),
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    });
    let mut dispositions = uniform_dispositions(ArenaDisposition::Omitted);
    let losses = coverage_losses(&ir, ArenaCoverage::Declared(dispositions));
    assert_eq!(losses.len(), 2, "{losses:?}");
    for (loss, arena) in losses
        .iter()
        .zip(["design.sketches", "design.spatial_sketches"])
    {
        assert!(loss.message.contains(&format!("`{arena}`")), "{losses:?}");
        assert_eq!(
            loss.code.taxonomy(),
            crate::report::loss::LossTaxonomy::ParametricRecordOmitted
        );
    }
    dispositions.design.spatial_sketches = ArenaDisposition::Written;
    let losses = coverage_losses(&ir, ArenaCoverage::Declared(dispositions));
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(
        losses[0].message.contains("`design.sketches`"),
        "{losses:?}"
    );
}

#[test]
fn an_omitted_process_arena_is_charged_as_a_process_record() {
    let mut ir = CadIr::empty();
    ir.process
        .groups
        .push(crate::process::groups::ProcessGroup {
            id: crate::design::ids::ProcessGroupId::try_from("test:process:group#1".to_owned())
                .unwrap(),
            kind: crate::process::groups::GroupKind::Method,
            name: None,
            parent: None,
            native_ref: None,
        });
    let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
    dispositions.process.groups = ArenaDisposition::Omitted;
    let losses = coverage_losses(&ir, ArenaCoverage::Declared(dispositions));
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(losses[0].code.local_code(), "arena.omitted");
    assert_eq!(
        losses[0].code.taxonomy(),
        crate::report::loss::LossTaxonomy::ProcessRecordOmitted
    );
    assert!(losses[0].message.contains("`process.groups`"), "{losses:?}");
    assert!(coverage_losses(
        &ir,
        ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Written))
    )
    .is_empty());
}

#[test]
fn the_wrapper_charges_one_loss_per_omitted_nonempty_arena() {
    let ir = unit_cube().expect("valid unit cube fixture");
    assert_eq!(ir.geometry.faces.len(), 6);
    assert!(ir.geometry.subds.is_empty());
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

/// An `arena.omitted` loss lists every omitted id, sorted, after the count
/// (§3.2).
#[test]
fn an_arena_omitted_loss_lists_the_sorted_ids_after_the_count() {
    let mut ir = CadIr::empty();
    for ordinal in [2, 10, 1] {
        ir.process
            .groups
            .push(crate::process::groups::ProcessGroup {
                id: crate::design::ids::ProcessGroupId::try_from(format!(
                    "test:process:group#{ordinal}"
                ))
                .unwrap(),
                kind: crate::process::groups::GroupKind::Method,
                name: None,
                parent: None,
                native_ref: None,
            });
    }
    let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
    dispositions.process.groups = ArenaDisposition::Omitted;
    let losses = coverage_losses(&ir, ArenaCoverage::Declared(dispositions));
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].message,
        "the cadir write does not represent model arena `process.groups`: 3 record(s) omitted: \
         test:process:group#1, test:process:group#10, test:process:group#2"
    );
}

#[test]
fn a_dropped_edit_is_an_error_named_by_its_record_kind() {
    assert!(super::edit_dropped(
        EntityKind::SourceAttribute,
        "TEST",
        "keeps",
        Vec::<String>::new()
    )
    .is_none());
    let loss = super::edit_dropped(
        EntityKind::SourceAttribute,
        "TEST",
        "keeps these records as stored",
        ["test:attribute#b", "test:attribute#a", "test:attribute#b"],
    )
    .unwrap();
    assert_eq!(loss.code.namespace(), "export");
    assert_eq!(loss.code.local_code(), "edit.dropped");
    assert_eq!(loss.severity, Severity::Error);
    assert_eq!(
        loss.code.taxonomy(),
        crate::report::loss::LossTaxonomy::AttributesNotTransferred
    );
    assert_eq!(
        loss.message,
        "the TEST write keeps these records as stored (2): test:attribute#a, test:attribute#b"
    );
}

#[test]
fn the_unwritable_and_residual_codes_are_listed_warnings() {
    let note = super::value_unwritable("x", "cuts texts", ["b", "a", "b"]).unwrap();
    assert_eq!(note.code.local_code(), "value.unwritable");
    assert_eq!(note.severity, Severity::Warning);
    assert_eq!(note.message, "the x write cuts texts (2): a, b");
    let note = super::record_residual_unavailable("x", "states no tokens", ["f"]).unwrap();
    assert_eq!(note.code.local_code(), "record.residual-unavailable");
    assert_eq!(note.severity, Severity::Warning);
    assert!(super::value_unwritable("x", "r", Vec::<String>::new()).is_none());
}

#[test]
fn an_ignored_arena_charges_no_loss_when_it_holds_a_record() {
    let ir = crate::examples::unit_cube_with_history_body().expect("valid fixture");
    assert_eq!(ir.model.history_bodies.len(), 1);
    let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
    dispositions.history_bodies = ArenaDisposition::Ignored;
    assert!(
        coverage_losses(&ir, ArenaCoverage::Declared(dispositions)).is_empty(),
        "an ignored arena is not an omission"
    );
    assert!(!ArenaDisposition::Ignored.carries());
    dispositions.history_bodies = ArenaDisposition::Omitted;
    let losses = coverage_losses(&ir, ArenaCoverage::Declared(dispositions));
    assert_eq!(losses.len(), 1, "{losses:?}");
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
        rule: crate::rule_name!("test.fixture-reference"),
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
    let face = ir.geometry.faces[0].identity();
    let edge = ir.geometry.edges[0].identity();
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
        (ArenaDisposition::Ignored, false),
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
    let f3d_id = "f3d:test:asm-history#0";
    let nx_id = "nx:test:record#0";
    let record = |id: &str| {
        vec![crate::NativeRecord::new(
            crate::ids::Identity::new(id).expect("valid native identity"),
            serde_json::Map::default(),
        )
        .expect("valid native record")]
    };
    ir.native
        .namespace_mut(crate::codec_id!("f3d"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("asm_histories"),
            record(f3d_id),
        );
    ir.native
        .namespace_mut(crate::codec_id!("nx"))
        .arenas_mut()
        .insert(cadmpeg_core::nonblank_literal!("records"), record(nx_id));
    ir.finalize();
    let report = validation_report(vec![
        finding(Some(f3d_id), "invalid native record", Severity::Error),
        finding(Some(nx_id), "invalid native record", Severity::Error),
    ]);
    let declared = ArenaCoverage::Declared(uniform_dispositions(ArenaDisposition::Written));
    let f3d = F3dCoverageEncoder(declared)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert_eq!(f3d.carried_errors(&ir, &report), vec![&report.findings[0]]);
    let cadir = CoverageEncoder(declared)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let complete = CoverageEncoder(ArenaCoverage::Complete)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for plan in [cadir, complete] {
        assert_eq!(
            plan.carried_errors(&ir, &report),
            report.findings.iter().collect::<Vec<_>>()
        );
    }
}

struct LossyEncoder(Vec<crate::report::loss::LossNote>);

impl EncoderBackend for LossyEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(Vec::new(), input.ir, ArenaCoverage::Complete);
        body.losses.clone_from(&self.0);
        Ok(body)
    }
}

fn empty_output() -> LossyEncoder {
    LossyEncoder(vec![super::output_empty("the test output holds nothing")])
}

fn refused_write(error: EncodeError) -> super::StrictWriteRejection {
    match error {
        EncodeError::StrictRejected(rejection) => rejection,
        other => panic!("expected a strict refusal, got {other}"),
    }
}

#[test]
fn the_default_strictness_writes_a_report_with_losses() {
    let ir = CadIr::empty();
    let plan = empty_output()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| code_name(&loss.code) == "export:output.empty"));
    plan.write_to(&mut Vec::new()).unwrap();
}

#[test]
fn a_severity_floor_refuses_the_write_before_any_byte() {
    let ir = CadIr::empty();
    let strictness = Strictness {
        floor: Some(Severity::Info),
        ..Strictness::default()
    };
    let rejection = refused_write(
        empty_output()
            .plan(
                EncodeInput::new(&ir, None).with_strictness(&strictness),
                TargetRequest::Inherit,
            )
            .unwrap_err(),
    );
    assert_eq!(code_name(&rejection.loss().code), "export:output.empty");
    assert_eq!(
        rejection.report().losses[rejection.entry()],
        *rejection.loss()
    );
}

#[test]
fn a_severity_floor_above_every_loss_admits_the_write() {
    let ir = CadIr::empty();
    let encoder = empty_output();
    assert!(encoder.0[0].severity < Severity::Blocking);
    let strictness = Strictness {
        floor: Some(Severity::Blocking),
        ..Strictness::default()
    };
    encoder
        .plan(
            EncodeInput::new(&ir, None).with_strictness(&strictness),
            TargetRequest::Inherit,
        )
        .unwrap();
}

#[test]
fn a_refused_code_refuses_the_write_and_another_code_admits_it() {
    let ir = CadIr::empty();
    let refusing = Strictness::default().with_code("export:output.empty");
    let rejection = refused_write(
        empty_output()
            .plan(
                EncodeInput::new(&ir, None).with_strictness(&refusing),
                TargetRequest::Inherit,
            )
            .unwrap_err(),
    );
    assert_eq!(code_name(&rejection.loss().code), "export:output.empty");

    let other = Strictness::default().with_code("export:record.invalid");
    empty_output()
        .plan(
            EncodeInput::new(&ir, None).with_strictness(&other),
            TargetRequest::Inherit,
        )
        .unwrap();
}

#[test]
fn a_pinned_strictness_refuses_a_loss_that_reaches_its_strict_floor() {
    let ir = CadIr::empty();
    let kind: crate::report::loss::LossKind = crate::report::loss::NamespacedLossKind::new(
        crate::report::loss::LossNamespace::new("test").unwrap(),
        "synthetic.reject-floor",
        crate::report::loss::LossTaxonomy::TopologyNotTransferred,
    )
    .with_strict_floor(Some(Severity::Warning))
    .into();
    let encoder = LossyEncoder(vec![crate::report::loss::LossNote::new(
        kind.clone(),
        "synthetic reject floor",
    )]);
    let pinned = Strictness {
        pinned: true,
        ..Strictness::default()
    };
    let rejection = refused_write(
        encoder
            .plan(
                EncodeInput::new(&ir, None).with_strictness(&pinned),
                TargetRequest::Inherit,
            )
            .unwrap_err(),
    );
    assert_eq!(rejection.loss().code, kind);

    let unpinned = crate::report::loss::LossKind::shared(
        crate::report::loss::LossTaxonomy::ParametricRecordOmitted,
    );
    assert_eq!(unpinned.strict_floor(), None);
    LossyEncoder(vec![crate::report::loss::LossNote::new(
        unpinned,
        "a loss with no strict floor",
    )])
    .plan(
        EncodeInput::new(&ir, None).with_strictness(&pinned),
        TargetRequest::Inherit,
    )
    .unwrap();
}

#[test]
fn the_wrapper_charges_each_carried_error_of_the_validation() {
    let ir = CadIr::empty();
    let validation = validation_report(vec![
        finding(None, "warning", Severity::Warning),
        finding(None, "error", Severity::Error),
    ]);
    let plan = LossyEncoder(Vec::new())
        .plan(
            EncodeInput::new(&ir, None).with_validation(&validation),
            TargetRequest::Inherit,
        )
        .unwrap();
    let charged = plan
        .report()
        .losses
        .iter()
        .filter(|loss| code_name(&loss.code) == "export:record.invalid")
        .count();
    assert_eq!(charged, 1);

    let refusing = Strictness::default().with_code("export:record.invalid");
    let rejection = refused_write(
        LossyEncoder(Vec::new())
            .plan(
                EncodeInput::new(&ir, None)
                    .with_validation(&validation)
                    .with_strictness(&refusing),
                TargetRequest::Inherit,
            )
            .unwrap_err(),
    );
    assert!(rejection.loss().message.contains("error"));
}
