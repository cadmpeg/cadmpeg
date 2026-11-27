// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

mod encode_sets;

use crate::test_support::wire;
use cadmpeg_core::serde_erase::json;

use super::{
    target::{Catalog, DialectFree, ResolvedWrite, TargetRequest},
    ArenaCoverage, ArenaDisposition, ArenaDispositions, CadirEncoder, Consumption, EncodeError,
    EncodeInput, Encoder, EncoderBackend, ExportBody, NativeDispositions, PatchConsumption,
    WritePath,
};
use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::codec::FormatId;
use crate::design::projection::restated::Restater;
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::check::{Check, Finding, ValidationReport};
use crate::report::export::FidelityResolution;
use crate::report::strictness::{code_name, Strictness};
use crate::report::Severity;
use crate::schema::{EntityKind, EntitySchema};
use crate::source_fidelity::SourceFidelity;
use crate::validate::validate_neutral;
use crate::CadIr;
use cadmpeg_core::decode::{DecodeContext, DecodeDepth};
use cadmpeg_core::CodecError;

mod dropped_records;

#[test]
fn cadir_encoder_streams_the_canonical_json_shape() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let mut encoded = Vec::new();
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
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

/// The CADIR writer drops a record nested past the document bound and
/// removes the stub link that names it; the record at the bound stays. The
/// dropped native record is charged only as an omitted native record.
#[test]
fn cadir_encoder_drops_a_record_past_the_document_bound() {
    const WITHIN: &str = "test:native:within#1";
    const PAST: &str = "test:native:past#1";
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let record = |id: &str, depth: usize| {
        let field = (0..depth).fold(serde_json::Value::Null, |inner, _| {
            serde_json::Value::Array(vec![inner])
        });
        crate::NativeRecord::new(
            crate::ids::Identity::new(id).unwrap(),
            [("field".to_owned(), field)].into_iter().collect(),
        )
        .unwrap()
    };
    let within = crate::bounds::MAX_DOCUMENT_DEPTH - 5;
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
        .set_arena_records(
            "deep",
            vec![record(WITHIN, within), record(PAST, within + 1)],
        )
        .unwrap();
    let stub = crate::NativeUnknownRecord {
        id: crate::ids::UnknownId::mint("test:source:part#holder").unwrap(),
        links: vec![crate::ids::Identity::new(PAST).unwrap()],
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    ir.set_native_unknowns(&ctx, "test", std::slice::from_ref(&stub))
        .unwrap();

    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("a record past the bound does not refuse the write");
    let messages = |code: &str| {
        plan.report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == code)
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        messages("native.records-omitted"),
        [format!(
            "the CADIR write omits 1 native record(s) of `test` arena `deep` which {}: {PAST}",
            crate::bounds::Bound::Document.describe()
        )]
    );
    assert_eq!(messages("value.unwritable"), Vec::<&str>::new());
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let written = CadIr::from_json(std::str::from_utf8(&encoded).unwrap()).unwrap();
    let kept = written
        .native
        .namespace("test")
        .unwrap()
        .arenas()
        .get("deep")
        .unwrap()
        .iter()
        .map(crate::NativeRecord::id)
        .collect::<Vec<_>>();
    assert_eq!(kept, [WITHIN]);
    let stubs = written.native_unknowns("test").unwrap();
    assert_eq!(stubs.len(), 1);
    assert!(stubs[0].links.is_empty());
    assert_eq!(written.geometry.faces.len(), ir.geometry.faces.len());
    assert_eq!(written.geometry.points.len(), ir.geometry.points.len());
}

/// A document whose scope excludes a class it holds records of writes
/// without those records. Where the scope excludes design, a model record's
/// reference into design is absent and an attribute whose target is a
/// design record is left out (§2.1). Each loss names what the write leaves
/// out, and the strict reader reads the output.
#[test]
fn cadir_encoder_drops_the_records_its_scope_excludes() {
    use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    const PARAMETER: &str = "synthetic:test:parameter#depth";
    const NAMES_PARAMETER: &str = "synthetic:test:attribute#depth-value";
    const ON_CONFIGURATION: &str = "synthetic:test:attribute#large";
    const ON_DOCUMENT: &str = "synthetic:test:attribute#part-number";
    let attribute = |id: &str, entity: AttributeTarget, values| SourceAttribute {
        id: crate::ids::AttributeId::mint(id).unwrap(),
        target: entity.into(),
        name: "Part Number".into(),
        values,
        native_ref: None,
    };
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.design
        .parameters
        .push(crate::design::parameter::DesignParameter {
            id: crate::features::ParameterId::mint(PARAMETER).unwrap(),
            scope: crate::ids::ProductDefinitionId::mint("synthetic:test:product-definition#root")
                .unwrap(),
            owner: None,
            name: None,
            kind: crate::design::units::ValueKind::Text,
            unit: None,
            definition: crate::design::parameter::ParameterDefinition::Driven,
            value: None,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: std::collections::BTreeMap::new(),
            source_settings: std::collections::BTreeMap::new(),
            native_ref: None,
        });
    ir.model.attributes.extend([
        attribute(
            NAMES_PARAMETER,
            AttributeTarget::Document,
            vec![Some(AttributeValue::Parameter(
                crate::features::ParameterId::mint(PARAMETER).unwrap(),
            ))],
        ),
        attribute(
            ON_CONFIGURATION,
            AttributeTarget::Configuration(
                crate::features::ConfigurationId::mint("synthetic:test:configuration#large")
                    .unwrap(),
            ),
            Vec::new(),
        ),
        attribute(ON_DOCUMENT, AttributeTarget::Document, Vec::new()),
    ]);
    let mut value = json::to_value(&ir).unwrap();
    value["scope"] = cadmpeg_core::json!({"model": true, "design": false, "process": false});
    let (read, _, _) = CadIr::read(
        value.to_string().as_bytes(),
        &crate::document::read::ReadOptions::default(),
    )
    .expect("the read keeps the scope as written")
    .without_sidecar();

    let plan = CadirEncoder
        .plan(EncodeInput::new(&read, None))
        .expect("a scope breach does not refuse the write");
    let messages = |code: &str| {
        plan.report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == code)
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        messages("arena.omitted"),
        [
            format!(
                "the cadir write drops the records of arena `attributes` that the document's \
                 scope excludes or that need one: 1 record(s) omitted: {ON_CONFIGURATION}"
            ),
            format!(
                "the cadir write drops the records of arena `design.parameters` that the \
                 document's scope excludes or that need one: 1 record(s) omitted: {PARAMETER}"
            ),
        ]
    );
    let unwritable = messages("value.unwritable");
    let [unwritable] = unwritable.as_slice() else {
        panic!("one value.unwritable: {unwritable:?}");
    };
    assert!(unwritable.contains(NAMES_PARAMETER), "{unwritable}");
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let written = CadIr::from_json(std::str::from_utf8(&encoded).unwrap()).unwrap();
    assert_eq!(
        written.scope(),
        cadmpeg_core::decode::ContentScope::geometry()
    );
    assert!(written.design.parameters.is_empty());
    let attributes = written
        .model
        .attributes
        .iter()
        .map(|attribute| attribute.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(attributes, [NAMES_PARAMETER, ON_DOCUMENT]);
    assert_eq!(written.model.attributes[0].values, [None]);
    assert_eq!(written.geometry.faces.len(), ir.geometry.faces.len());
    assert_eq!(written.geometry.points.len(), ir.geometry.points.len());
}

/// A CADIR write states each id of an arena once: equal copies write once
/// with no loss, and of copies that differ the first in arena order is
/// written, with an `export:record.invalid` naming the id. The strict reader
/// reads the output.
#[test]
fn cadir_encoder_writes_a_repeated_id_once() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let equal = ir.geometry.points[0].clone();
    let original = ir.geometry.points[1].clone();
    let differing =
        crate::topology::Point::new(original.id.clone(), ir.geometry.points[2].position(), None);
    ir.geometry.points.extend([equal.clone(), differing]);

    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("a repeated id does not refuse the write");
    let invalid = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.invalid")
        .collect::<Vec<_>>();
    let [loss] = invalid.as_slice() else {
        panic!("one loss for the id whose copies differ: {invalid:?}");
    };
    assert!(
        loss.message.contains(original.id.as_str()) && !loss.message.contains(equal.id.as_str()),
        "{}",
        loss.message
    );
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let written = CadIr::from_json(std::str::from_utf8(&encoded).unwrap()).unwrap();
    assert_eq!(written.geometry.points.len(), ir.geometry.points.len() - 2);
    let kept = written
        .geometry
        .points
        .iter()
        .find(|point| point.id == original.id)
        .unwrap();
    assert_eq!(kept, &original);
}

/// A native-record link to a record of an arena the CADIR wire omits names
/// no written record: the write states the native record without that link,
/// keeps its other links, and names the cleared link. The output checks with
/// the Errors of the fixture it extends only.
#[test]
fn cadir_encoder_clears_a_native_link_to_an_omitted_record() {
    use crate::features::{
        Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation,
        SketchFeatureBinding,
    };

    const FEATURE: &str = "synthetic:test:feature#detail-view";
    const ANNOTATION: &str = "test:native:annotation#1";
    const NOTE: &str = "test:native:annotation#2";
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint(FEATURE).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Unresolved,
            }),
        ),
        native_ref: None,
    });
    let links = |record: &crate::native::NativeRecord| record.field("links");
    let record = |id: &str, fields: serde_json::Value| {
        crate::native::NativeRecord::new(
            crate::ids::Identity::new(id).unwrap(),
            fields.as_object().cloned().unwrap(),
        )
        .unwrap()
    };
    let annotation = record(ANNOTATION, cadmpeg_core::json!({"links": [FEATURE, NOTE]}));
    let note = record(NOTE, cadmpeg_core::json!({}));
    ir.native
        .namespace_mut(crate::codec_id!("test"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("annotations"),
            vec![annotation, note],
        );

    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("plan CADIR export");
    let unwritable = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "value.unwritable")
        .map(|loss| loss.message.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        unwritable,
        [format!(
            "the cadir write writes each native record without its link to a record of an \
             arena the CADIR wire omits (1): {ANNOTATION} -> {FEATURE}"
        )]
    );
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let written = CadIr::from_json(std::str::from_utf8(&encoded).unwrap()).unwrap();
    let records = &written.native.namespace("test").unwrap().arenas()["annotations"];
    assert_eq!(links(&records[0]), Some(cadmpeg_core::json!([NOTE])));
    let errors = |ir: &CadIr| {
        validate_neutral(ir, Vec::new())
            .unwrap()
            .findings
            .into_iter()
            .filter(|finding| finding.severity == Severity::Error)
            .collect::<Vec<_>>()
    };
    let found = errors(&written);
    assert!(
        found
            .iter()
            .all(|finding| finding.check != Check::NativeLinks),
        "{found:?}"
    );
    assert_eq!(found, errors(&unit_cube().unwrap()));
}

#[test]
fn cadir_encoder_census_matches_validation_counts() {
    let ir = directed_subd_sum().unwrap();
    let validation_counts = validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .entity_counts;
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("plan CADIR export");

    assert_eq!(plan.report().census.counts, validation_counts);
}

struct NeutralEncoder;

impl EncoderBackend for NeutralEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        // The body carries no identity: whatever the backend does, the
        // wrapper stamps FORMAT.
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        ))
    }
}

#[test]
fn the_wrapper_stamps_cadir_on_a_dialect_free_plan() {
    let ir = CadIr::empty();
    let plan = NeutralEncoder.plan(EncodeInput::new(&ir, None)).unwrap();
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
        .plan(EncodeInput::new(&ir, None).with_target(TargetRequest::Explicit("any")))
        .unwrap_err();
    let EncodeError::UnsupportedTarget(refusal) = error else {
        panic!("a dialect-free encoder has no explicit targets")
    };
    assert_eq!(
        json::to_value(&refusal).expect("serialize refusal")["refusal"]["kind"],
        "unknown_explicit"
    );
    assert!(refusal.available().is_empty());
}

struct CatalogEncoder;

impl EncoderBackend for CatalogEncoder {
    const FORMAT: FormatId = FormatId::new("test");
    const NAME: &'static str = "test";
    const REQUEST: DecodeDepth = DecodeDepth::Geometry;
    const REUSES_SOURCE: bool = false;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(CATALOG_WRITE_TARGETS, Some(1));

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        );
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
        .plan(EncodeInput::new(&ir, None).with_target(TargetRequest::Explicit("new")))
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

/// A displaced write is written and reported under a pinned strictness:
/// §3.2 gives the `target.*` codes no strict floor.
#[test]
fn a_pinned_strictness_writes_a_displaced_target() {
    let mut ir = CadIr::empty();
    ir.source = Some(
        json::from_value(cadmpeg_core::json!({
            "identity": {"kind": "unclassified", "format": "test"},
            "attributes": {},
        }))
        .unwrap(),
    );
    let pinned = Strictness {
        pinned: true,
        ..Strictness::default()
    };
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None).with_strictness(&pinned))
        .expect("a pinned strictness writes a displaced target");
    let codes = plan
        .report()
        .losses
        .iter()
        .map(|loss| code_name(&loss.code))
        .filter(|code| code.starts_with("target."))
        .collect::<Vec<_>>();
    assert_eq!(codes, ["target.source-dialect-displaced"]);
}

fn recorded_at(depth: DecodeDepth) -> CadIr {
    let mut ir = CadIr::empty();
    ir.set_scope(depth.scope());
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
            .plan(EncodeInput::new(&ir, None))
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
        .plan(EncodeInput::new(&full, None))
        .unwrap();
    assert!(depth_notes(&plan).is_empty());
}

#[test]
fn a_geometry_request_encoder_plans_a_container_document_with_a_note() {
    let geometry = recorded_at(DecodeDepth::Geometry);
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&geometry, None).with_target(TargetRequest::Explicit("new")))
        .unwrap();
    assert!(depth_notes(&plan).is_empty());
    let container = recorded_at(DecodeDepth::Container);
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&container, None).with_target(TargetRequest::Explicit("new")))
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
        let plan = CadirEncoder.plan(EncodeInput::new(&ir, None)).unwrap();
        let mut encoded = Vec::new();
        plan.write_to(&mut encoded).unwrap();
        let written: serde_json::Value = json::from_slice(&encoded).unwrap();
        assert_eq!(written["scope"], json::to_value(depth.scope()).unwrap());
    }
}

#[test]
fn fidelity_resolution_is_not_provided_whenever_the_input_carries_none() {
    let ir = CadIr::empty();
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None).with_target(TargetRequest::Explicit("new")))
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
        .plan(EncodeInput::new(&ir, Some(&fidelity)).with_target(TargetRequest::Explicit("new")))
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
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            self.0,
            NativeDispositions::written(),
        ))
    }
}

fn coverage_losses(ir: &CadIr, coverage: ArenaCoverage) -> Vec<crate::report::loss::LossNote> {
    CoverageEncoder(coverage)
        .plan(EncodeInput::new(ir, None))
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
    model_sheet("v10")
}

fn model_sheet(key: &str) -> crate::spreadsheets::Spreadsheet {
    crate::spreadsheets::Spreadsheet::new(
        crate::spreadsheets::SpreadsheetId::mint(format!("test:design:spreadsheet#{key}")).unwrap(),
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

/// The `arena.omitted` losses of `ir` a write with `dispositions` charges on
/// the model arena `spreadsheets`.
fn sheet_charges(ir: &CadIr, dispositions: ArenaDispositions) -> Vec<String> {
    coverage_losses(ir, ArenaCoverage::Declared(dispositions))
        .into_iter()
        .filter(|loss| loss.message.contains("model arena `spreadsheets`"))
        .map(|loss| loss.message)
        .collect()
}

/// A CADIR write carries a record of an arena the wire omits that a written
/// `design` record restates under another id, and still charges a record no
/// written record restates; a write that omits the restater charges both.
#[test]
fn a_cadir_write_carries_a_record_a_written_design_record_restates_under_another_id() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    ir.model.spreadsheets.push(model_sheet("unrestated"));
    ir.design.spreadsheets.push(design_sheet("v11"));
    ir.model.restated_mut().record(
        "test:design:spreadsheet#v10",
        Restater::Record("test:design:spreadsheet#v11".to_owned()),
        [],
    );
    let cadir = super::coverage::cadir_dispositions();
    assert_eq!(
        sheet_charges(&ir, cadir),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) omitted: test:design:spreadsheet#unrestated"
        ]
    );
    let mut no_design = cadir;
    no_design.design.spreadsheets = ArenaDisposition::Omitted;
    assert_eq!(
        sheet_charges(&ir, no_design),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          2 record(s) omitted: test:design:spreadsheet#unrestated, test:design:spreadsheet#v10"
        ]
    );
}

/// A CADIR write charges a record a written `design` record restates under
/// its id without some fields once, naming the record and the fields.
#[test]
fn a_cadir_write_charges_a_record_restated_in_part_once_naming_its_fields() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    ir.model.spreadsheets.push(model_sheet("unrestated"));
    ir.design.spreadsheets.push(design_sheet("v10"));
    let by = || Restater::Record("test:design:spreadsheet#v10".to_owned());
    ir.model
        .restated_mut()
        .record("test:design:spreadsheet#v10", by(), ["row_heights"]);
    ir.model
        .restated_mut()
        .record("test:design:spreadsheet#v10", by(), ["merged_ranges"]);
    let cadir = super::coverage::cadir_dispositions();
    assert_eq!(
        sheet_charges(&ir, cadir),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) omitted: test:design:spreadsheet#unrestated; \
          1 record(s) stated in part: test:design:spreadsheet#v10 (merged_ranges, row_heights)"
        ]
    );
    ir.model.spreadsheets.pop();
    assert_eq!(
        sheet_charges(&ir, cadir),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) stated in part: test:design:spreadsheet#v10 (merged_ranges, row_heights)"
        ]
    );
    assert_eq!(cadir.omitted(&ir), ["spreadsheets"]);
}

/// The name table restates a record only for a write of every `design`
/// arena, the one write that writes the table.
#[test]
fn a_record_the_name_table_restates_is_carried_only_by_a_write_of_the_whole_design() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    ir.model
        .restated_mut()
        .record("test:design:spreadsheet#v10", Restater::Names, []);
    let cadir = super::coverage::cadir_dispositions();
    assert!(sheet_charges(&ir, cadir).is_empty());
    let mut reported = cadir;
    reported.design.parameters = ArenaDisposition::Reported;
    assert_eq!(
        sheet_charges(&ir, reported),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) omitted: test:design:spreadsheet#v10"
        ]
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
    const NAME: &'static str = "F3D";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            self.0,
            NativeDispositions::written(),
        ))
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
    .plan(EncodeInput::new(&ir, None))
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
            .plan(EncodeInput::new(&ir, None))
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
        .plan(EncodeInput::new(&ir, None))
        .unwrap();
    assert_eq!(f3d.carried_errors(&ir, &report), vec![&report.findings[0]]);
    let cadir = CoverageEncoder(declared)
        .plan(EncodeInput::new(&ir, None))
        .unwrap();
    let complete = CoverageEncoder(ArenaCoverage::Complete)
        .plan(EncodeInput::new(&ir, None))
        .unwrap();
    for plan in [cadir, complete] {
        assert_eq!(
            plan.carried_errors(&ir, &report),
            report.findings.iter().collect::<Vec<_>>()
        );
    }
}

#[test]
fn cadir_encoder_writes_a_name_past_the_document_bound_by_its_native_name() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.set_scope(cadmpeg_core::decode::ContentScope::full());
    let face = |at: usize| ir.geometry.faces[at].id.as_str().to_owned();
    let (named, unnamed) = (face(0), face(1));
    let deep = (0..crate::bounds::MAX_DOCUMENT_DEPTH / 2).fold(
        cadmpeg_core::json!({"kind": "generated", "value": {"kind": "face", "feature": "test:design:feature#f"}}),
        |from, _| {
            cadmpeg_core::json!({"kind": "modified", "value": {"feature": "test:design:feature#f", "from": from}})
        },
    );
    let native =
        cadmpeg_core::json!({"codec": "test", "value": {"kind": "text", "value": "Face1"}});
    ir.design.names = json::from_value(cadmpeg_core::json!([
        [{"kind": "face", "value": named}, {"name": deep, "native": native}],
        [{"kind": "face", "value": unnamed}, {"name": deep}]
    ]))
    .unwrap();

    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("a name past the bound does not refuse the write");
    let unwritable = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "value.unwritable")
        .collect::<Vec<_>>();
    assert_eq!(unwritable.len(), 2, "{unwritable:?}");
    assert!(unwritable
        .iter()
        .all(|loss| loss.message.contains("design.names")));
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let written = CadIr::from_json(std::str::from_utf8(&encoded).unwrap()).unwrap();
    assert_eq!(
        crate::hash::finite_json::to_json_value(&written.design.names).unwrap(),
        cadmpeg_core::json!([[{"kind": "face", "value": named}, {"native": native}]])
    );
    assert_eq!(written.geometry.faces.len(), ir.geometry.faces.len());
}

#[test]
fn cadir_encoder_writes_the_empty_document_when_every_record_is_dropped() {
    const PAST: &str = "test:native:past#1";
    let mut ir = CadIr::empty();
    let field = (0..crate::bounds::MAX_DOCUMENT_DEPTH).fold(serde_json::Value::Null, |inner, _| {
        serde_json::Value::Array(vec![inner])
    });
    let record = crate::NativeRecord::new(
        crate::ids::Identity::new(PAST).unwrap(),
        [("field".to_owned(), field)].into_iter().collect(),
    )
    .unwrap();
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
        .set_arena_records("deep", vec![record])
        .unwrap();

    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("a document past the bound does not refuse the write");
    assert!(
        plan.report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "output.empty"),
        "{:?}",
        plan.report().losses
    );
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let written = CadIr::from_json(std::str::from_utf8(&encoded).unwrap()).unwrap();
    assert_eq!(written, CadIr::empty());
}

struct LossyEncoder(Vec<crate::report::loss::LossNote>);

impl EncoderBackend for LossyEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        );
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
    let plan = empty_output().plan(EncodeInput::new(&ir, None)).unwrap();
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
            .plan(EncodeInput::new(&ir, None).with_strictness(&strictness))
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
        .plan(EncodeInput::new(&ir, None).with_strictness(&strictness))
        .unwrap();
}

#[test]
fn a_refused_code_refuses_the_write_and_another_code_admits_it() {
    let ir = CadIr::empty();
    let refusing = Strictness::default().with_code("export:output.empty");
    let rejection = refused_write(
        empty_output()
            .plan(EncodeInput::new(&ir, None).with_strictness(&refusing))
            .unwrap_err(),
    );
    assert_eq!(code_name(&rejection.loss().code), "export:output.empty");

    let other = Strictness::default().with_code("export:record.invalid");
    empty_output()
        .plan(EncodeInput::new(&ir, None).with_strictness(&other))
        .unwrap();
}

#[test]
fn a_pinned_strictness_refuses_a_loss_that_reaches_its_strict_floor() {
    let ir = CadIr::empty();
    let kind: crate::report::loss::LossKind = crate::report::loss::NamespacedLossKind::new(
        crate::loss_namespace!("test"),
        "synthetic.reject-floor",
        crate::report::loss::LossTaxonomy::TopologyNotTransferred,
    )
    .with_strict_floor(Some(crate::report::Severity::Warning))
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
            .plan(EncodeInput::new(&ir, None).with_strictness(&pinned))
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
    .plan(EncodeInput::new(&ir, None).with_strictness(&pinned))
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
        .plan(EncodeInput::new(&ir, None).with_validation(&validation))
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
            )
            .unwrap_err(),
    );
    assert!(rejection.loss().message.contains("error"));
}

#[derive(Default)]
struct LimitsEncoder(std::cell::Cell<Option<cadmpeg_core::decode::ResourceLimits>>);

impl EncoderBackend for LimitsEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        ctx: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        self.0.set(Some(ctx.policy().limits));
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        ))
    }
}

#[test]
fn a_writer_plans_under_the_callers_session_ceilings() {
    let ir = CadIr::empty();
    let encoder = LimitsEncoder::default();
    let limits = cadmpeg_core::decode::ResourceLimits::service();
    encoder
        .plan(EncodeInput::new(&ir, None).with_limits(limits))
        .unwrap();
    assert_eq!(encoder.0.get(), Some(limits));
}

/// A writer that enters one nesting level.
struct NestingEncoder;

impl EncoderBackend for NestingEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        ctx: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        let _level = ctx.enter_nested("test nesting")?;
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        ))
    }
}

/// The depth ceiling bounds one part of a decode, but a write gives up no
/// part: its refusal has session scope (§2.5).
#[test]
fn a_write_refusal_has_session_scope() {
    let ir = CadIr::empty();
    let mut limits = cadmpeg_core::decode::ResourceLimits::desktop();
    limits.max_recursion_depth = 0;
    let Err(EncodeError::ResourceLimit(limit)) =
        NestingEncoder.plan(EncodeInput::new(&ir, None).with_limits(limits))
    else {
        panic!("the depth ceiling refuses the write");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
    assert_eq!(limit.scope, cadmpeg_core::decode::LimitScope::Session);
}

/// A writer with a patch path: it patches where `patch` holds and
/// otherwise synthesizes, stating `cause` where it is given one.
struct ReusingEncoder {
    patch: bool,
    cause: Option<crate::report::export::SynthesisCause>,
}

impl EncoderBackend for ReusingEncoder {
    const FORMAT: FormatId = FormatId::new("test");
    const NAME: &'static str = "test";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = true;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        );
        if self.patch {
            body.write_path = WritePath::Patched {
                consumption: PatchConsumption::Replayed,
            };
        }
        body.synthesis_cause.clone_from(&self.cause);
        Ok(body)
    }
}

fn test_format_source() -> CadIr {
    let mut ir = CadIr::empty();
    ir.source = Some(
        json::from_value(cadmpeg_core::json!({
            "identity": {"kind": "unclassified", "format": "test"},
            "attributes": {},
        }))
        .unwrap(),
    );
    ir
}

fn declined(reason: &str) -> crate::report::export::SynthesisCause {
    crate::report::export::SynthesisCause::PatchDeclined {
        reason: reason.to_owned(),
    }
}

#[test]
fn a_source_reusing_writer_records_why_it_synthesized_a_same_format_file() {
    let ir = test_format_source();
    let encoder = ReusingEncoder {
        patch: false,
        cause: Some(declined("the edit has no patch")),
    };
    let plan = encoder.plan(EncodeInput::new(&ir, None)).unwrap();
    assert_eq!(
        plan.report().synthesis_cause(),
        Some(&declined("the edit has no patch"))
    );
}

#[test]
fn a_same_format_synthesis_with_no_stated_cause_records_the_fidelity_reason() {
    let ir = test_format_source();
    let encoder = ReusingEncoder {
        patch: false,
        cause: None,
    };
    let plan = encoder.plan(EncodeInput::new(&ir, None)).unwrap();
    assert_eq!(
        plan.report().synthesis_cause(),
        Some(&declined("the encode holds no source fidelity"))
    );
}

/// A catalog writer with a patch path that synthesizes every file and states
/// no cause.
struct ReusingCatalogEncoder;

impl EncoderBackend for ReusingCatalogEncoder {
    const FORMAT: FormatId = FormatId::new("test");
    const NAME: &'static str = "test";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = true;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(CATALOG_WRITE_TARGETS, Some(1));

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        _: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        ))
    }
}

#[test]
fn a_synthesis_that_displaces_the_source_dialect_records_the_displacement() {
    let ir = test_format_source();
    let plan = ReusingCatalogEncoder
        .plan(EncodeInput::new(&ir, None))
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| code_name(&loss.code) == "target.source-dialect-displaced"));
    assert_eq!(
        plan.report().synthesis_cause(),
        Some(&declined("the write target displaces the source dialect"))
    );
}

#[test]
fn a_patched_file_or_a_file_of_another_format_records_no_synthesis_cause() {
    let patched = ReusingEncoder {
        patch: true,
        cause: Some(declined("unused")),
    };
    let plan = patched
        .plan(EncodeInput::new(&test_format_source(), None))
        .unwrap();
    assert_eq!(plan.report().synthesis_cause(), None);

    let synthesized = ReusingEncoder {
        patch: false,
        cause: Some(declined("unused")),
    };
    let plan = synthesized
        .plan(EncodeInput::new(&CadIr::empty(), None))
        .unwrap();
    assert_eq!(plan.report().synthesis_cause(), None);
}

#[test]
fn a_writer_with_no_patch_or_replay_path_records_no_synthesis_cause() {
    let mut ir = test_format_source();
    ir.source = Some(
        json::from_value(cadmpeg_core::json!({
            "identity": {"kind": "unclassified", "format": "cadir"},
            "attributes": {},
        }))
        .unwrap(),
    );
    let plan = NeutralEncoder.plan(EncodeInput::new(&ir, None)).unwrap();
    assert_eq!(plan.report().synthesis_cause(), None);
}

struct ScratchFailureEncoder;

impl EncoderBackend for ScratchFailureEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        _: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        Err(CodecError::Io(std::io::Error::other("scratch file")))
    }
}

#[test]
fn an_io_failure_before_the_destination_is_written_is_no_destination_io() {
    let ir = CadIr::empty();
    let error = ScratchFailureEncoder
        .plan(EncodeInput::new(&ir, None))
        .unwrap_err();
    assert!(!matches!(error, EncodeError::Io(_)), "got {error:?}");
}

#[test]
fn a_failed_write_to_the_destination_is_io() {
    struct Refusing;
    impl std::io::Write for Refusing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("destination"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let ir = unit_cube().unwrap();
    let plan = CadirEncoder.plan(EncodeInput::new(&ir, None)).unwrap();
    assert!(matches!(
        plan.write_to(&mut Refusing),
        Err(EncodeError::Io(_))
    ));
}

/// A Boolean parameter whose expression is `depth - 1` negations of `true`:
/// a tree `depth` levels deep.
fn negated_parameter(key: &str, depth: usize) -> crate::design::parameter::DesignParameter {
    use crate::design::expr::{Expression, Neutral, Node, UnaryOp};
    crate::design::parameter::DesignParameter {
        id: format!("test:design:parameter#{key}").try_into().unwrap(),
        scope: "test:design:definition#part".to_owned().try_into().unwrap(),
        owner: None,
        name: Some(cadmpeg_core::text::NonBlankString::new(key).unwrap()),
        kind: crate::design::units::ValueKind::Boolean,
        unit: None,
        definition: crate::design::parameter::ParameterDefinition::Expression(Expression::Neutral(
            Neutral(
                (1..depth).fold(Node::Boolean(true), |operand, _| Node::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                }),
            ),
        )),
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: std::collections::BTreeMap::default(),
        source_settings: std::collections::BTreeMap::default(),
        native_ref: None,
    }
}

/// An expression past the bound of §2.5 is content CADIR cannot hold (§3.4):
/// the writer states it `Source` with no tokens, the partial form of §3.3,
/// and the bytes it writes read back.
#[test]
fn the_cadir_writer_restates_an_expression_past_its_bound() {
    let mut ir = unit_cube().unwrap();
    ir.design.parameters.push(negated_parameter(
        "within",
        crate::bounds::MAX_EXPRESSION_DEPTH,
    ));
    ir.design.parameters.push(negated_parameter(
        "past",
        crate::bounds::MAX_EXPRESSION_DEPTH + 1,
    ));
    assert!(matches!(
        ir.to_canonical_json(),
        Err(crate::hash::finite_json::CanonicalJsonError::PastBound {
            bound: crate::bounds::Bound::Expression,
            ..
        })
    ));
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("a record past a bound does not refuse the write");
    let unwritable = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "value.unwritable")
        .collect::<Vec<_>>();
    let [loss] = unwritable.as_slice() else {
        panic!("one loss for the one record: {unwritable:?}");
    };
    assert!(
        loss.message.contains("test:design:parameter#past")
            && loss.message.contains("design.parameters")
            && !loss.message.contains("#within"),
        "{}",
        loss.message
    );
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let read = CadIr::from_json(std::str::from_utf8(&written).unwrap())
        .expect("the written bytes read back");
    let definition = |key: &str| {
        read.design
            .parameters
            .iter()
            .find(|parameter| parameter.id.as_str() == format!("test:design:parameter#{key}"))
            .map(|parameter| parameter.definition.clone())
            .unwrap()
    };
    assert_eq!(
        definition("within"),
        negated_parameter("within", crate::bounds::MAX_EXPRESSION_DEPTH).definition
    );
    assert!(matches!(
        definition("past"),
        crate::design::parameter::ParameterDefinition::Expression(
            crate::design::expr::Expression::Source { tokens, .. }
        ) if tokens.is_empty()
    ));
}

/// `repro` outside every record is written unstated where it passes the
/// document bound.
#[test]
fn the_cadir_writer_writes_repro_past_the_document_bound_unstated() {
    let mut ir = unit_cube().unwrap();
    let finding = Finding {
        rule: crate::rule_name!("test.unresolved-link"),
        check: Check::NativeLinks,
        severity: Severity::Error,
        entity: None,
        related: Vec::new(),
        message: "unresolved link".into(),
    };
    ir.repro = Some(Box::new(crate::repro::BundleMetadata {
        signature: crate::repro::Signature::new(
            &finding,
            crate::codec_id!("cadir"),
            crate::repro::Stage::Check,
        ),
        roots: Vec::new(),
        context: (0..crate::bounds::MAX_DOCUMENT_DEPTH)
            .fold(crate::attributes::AttributeJson::Null, |inner, _| {
                crate::attributes::AttributeJson::Array(vec![inner])
            }),
        manifest: Vec::new(),
        incomplete_reason: None,
    }));
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .expect("repro past the bound does not refuse the write");
    assert!(
        plan.report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "value.unwritable"
                && loss.message.contains("repro"))
    );
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let read = CadIr::from_json(std::str::from_utf8(&written).unwrap())
        .expect("the written bytes read back");
    assert!(read.repro.is_none());
}
