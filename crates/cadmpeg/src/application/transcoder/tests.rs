#[cfg(feature = "rhino")]
use cadmpeg_test_support::wire;

use std::path::{Path, PathBuf};

#[cfg(any(feature = "iges", feature = "rhino", feature = "step", feature = "usd"))]
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::CadIr;
use cadmpeg_registry::Format;

use crate::application::artifact_store::FileDestination;
use crate::application::document::LoadedDocument;
#[cfg(any(feature = "iges", feature = "rhino", feature = "step"))]
use crate::application::refusal::ConversionRefusal;

#[cfg(any(feature = "iges", feature = "rhino", feature = "step"))]
use super::export_target;
use super::{
    DestinationPolicy, LossyFloor, LossyScope, PreparedConversion, ResolvedDestination,
    StrictnessFlags, TargetSelection, Validation,
};

/// `--reject-lossy=export` at the default floor.
const EXPORT_LOSSY: StrictnessFlags = StrictnessFlags {
    reject_errors: false,
    reject_empty: false,
    reject_lossy: Some(LossyScope::Export),
    lossy_floor: LossyFloor::Info,
    no_salvage: false,
};

/// The flags of `--reject-errors` alone, or of no flag.
fn reject_errors(reject: bool) -> StrictnessFlags {
    StrictnessFlags {
        reject_errors: reject,
        ..StrictnessFlags::default()
    }
}

/// The report of a check that ran to its end.
fn complete(validation: &mut Validation) -> &mut cadmpeg_ir::report::check::ValidationReport {
    match validation {
        Validation::Complete(report) => report,
        Validation::Incomplete(reason) => panic!("the check did not run to its end: {reason}"),
    }
}

/// The number of `export:record.invalid` entries in a refused write's report.
#[cfg(feature = "fcstd")]
fn record_invalid_count(rejection: &cadmpeg_ir::codec::write::StrictWriteRejection) -> usize {
    use cadmpeg_ir::report::strictness::code_name;
    rejection
        .report()
        .losses
        .iter()
        .filter(|loss| code_name(&loss.code) == "export:record.invalid")
        .count()
}

#[test]
fn strictness_flags_set_the_decode_and_the_write_strictness() {
    use cadmpeg_ir::report::{strictness::Strictness, Severity};

    assert_eq!(StrictnessFlags::default().decode(), Strictness::default());
    assert_eq!(StrictnessFlags::default().write(), Strictness::default());
    let lossy = |scope| StrictnessFlags {
        reject_lossy: Some(scope),
        lossy_floor: LossyFloor::Warning,
        ..StrictnessFlags::default()
    };
    assert_eq!(
        [LossyScope::Decode, LossyScope::Export, LossyScope::Any]
            .map(|scope| (lossy(scope).decode().floor, lossy(scope).write().floor)),
        [
            (Some(Severity::Warning), None),
            (None, Some(Severity::Warning)),
            (Some(Severity::Warning), Some(Severity::Warning)),
        ]
    );
    let flags = StrictnessFlags {
        reject_errors: true,
        reject_empty: true,
        no_salvage: true,
        ..StrictnessFlags::default()
    };
    assert!(flags.decode().pinned);
    assert!(!flags.write().pinned);
    assert_eq!(
        flags
            .write()
            .codes
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["export:output.empty", "export:record.invalid"]
    );
    assert!(flags.decode().codes.is_empty());
}

#[test]
fn convert_file_destination_discards_binary_stdout_at_clap_admission() {
    use clap::Parser;
    let command = crate::Cli::try_parse_from([
        "cadmpeg",
        "convert",
        "input.step",
        "-o",
        "out.step",
        "--binary-stdout",
    ])
    .unwrap()
    .command;
    let crate::Command::Convert { destinations, .. } = command else {
        panic!("expected convert command");
    };
    assert_eq!(
        destinations.destination,
        DestinationPolicy::File(FileDestination {
            path: PathBuf::from("out.step"),
            overwrite: false,
        })
    );
}

fn prepared(
    ir: CadIr,
    format: Format,
    encoder: Box<dyn Encoder>,
    flags: StrictnessFlags,
) -> PreparedConversion {
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    PreparedConversion {
        document: LoadedDocument::neutral(ir, 0),
        validation: Validation::Complete(validation),
        encoder,
        selection: TargetSelection::new(format, None),
        assets: crate::loader::companions::Directory::beside(Path::new("input")),
        destination: ResolvedDestination::Stdout,
        write_strictness: flags.write(),
        missing_states: cadmpeg_ir::codec::write::MissingStateForm::GeometryTrue,
    }
}

#[cfg(feature = "iges")]
#[test]
fn encoder_planning_owns_unknown_explicit_target_admission() {
    let mut conversion = prepared(
        CadIr::empty(),
        Format::Iges,
        Box::new(cadmpeg_codec_iges::IgesCodec),
        StrictnessFlags::default(),
    );
    conversion.selection.request = Some("nonesuch".into());

    let Err(error) = conversion.plan() else {
        panic!("the encoder must reject an unknown target");
    };
    assert!(matches!(
        error.refusal(),
        Some(ConversionRefusal::UnsupportedTarget { .. })
    ));
}

#[cfg(feature = "step")]
fn step_ir_with_unrepresentable_native_content() -> CadIr {
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("asm_histories"),
            vec![cadmpeg_ir::NativeRecord::new(
                cadmpeg_ir::ids::Identity::new("f3d:test:asm-history#0").expect("valid identity"),
                serde_json::Map::default(),
            )
            .expect("valid native identity")],
        );
    ir.finalize();
    ir
}

/// STEP planning returns concrete loss rows, and the shared application
/// gate names them when export-loss rejection is active.
#[cfg(feature = "step")]
#[test]
fn step_export_losses_are_rejected_by_the_shared_plan_gate() {
    let conversion = prepared(
        step_ir_with_unrepresentable_native_content(),
        Format::Step,
        Box::new(cadmpeg_codec_step::StepCodec::default()),
        EXPORT_LOSSY,
    );
    let Err(error) = conversion.plan() else {
        panic!("the plan loss must refuse");
    };
    let refusal = error
        .refusal()
        .expect("the shared gate returns a typed refusal");
    assert!(
        matches!(refusal, ConversionRefusal::StrictExportRejected { .. }),
        "{refusal:?}"
    );
    let message = refusal.evidence().message;
    assert!(
        message.contains("the write's strictness refuses"),
        "{message}"
    );
    let report = refusal
        .evidence()
        .reports
        .export
        .expect("the refusal retains the completed export plan report");
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains("source-native record(s)")),
        "{:?}",
        report.losses
    );
}

/// Without export-loss rejection the same STEP plan succeeds and retains
/// the loss rows for the caller and report.
#[cfg(feature = "step")]
#[test]
fn step_export_losses_remain_on_the_plan_without_rejection() {
    let conversion = prepared(
        step_ir_with_unrepresentable_native_content(),
        Format::Step,
        Box::new(cadmpeg_codec_step::StepCodec::default()),
        StrictnessFlags::default(),
    );
    let planned = conversion.plan().expect("loss reporting is not refusal");
    assert!(
        planned
            .plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("source-native record(s)")),
        "{:?}",
        planned.plan.report().losses
    );
}

/// A Rhino write omits product structure it cannot represent. The wrapper
/// charges that omission as an export loss, so the shared gate refuses it.
#[cfg(feature = "rhino")]
#[test]
fn rhino_omitted_product_structure_is_rejected_by_the_shared_plan_gate() {
    let mut ir = CadIr::empty();
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#pin").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            0.0, 0.0, 0.0,
        ))
        .expect("finite position"),
        None,
    ));
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            sketches: Vec::default(),

            id: "cadir:model:product#group"
                .try_into()
                .expect("valid identity"),
            kind: Some(cadmpeg_ir::products::ProductDefinitionKind::Group),
            source_name: Some("Group".into()),
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            bodies: Vec::new(),
            native_ref: None,
            generated_from: None,
            storage: cadmpeg_ir::products::Storage::Embedded,
            occurrences: Vec::new(),
            instance_bodies: Vec::new(),
            representations: Vec::new(),
            units: None,
            general_tolerance: None,
            active: None,
        });
    let conversion = prepared(
        ir,
        Format::Rhino,
        Box::new(cadmpeg_codec_rhino::RhinoCodec),
        EXPORT_LOSSY,
    );
    let Err(error) = conversion.plan() else {
        panic!("the omitted product definition must refuse");
    };
    let refusal = error
        .refusal()
        .expect("the shared gate returns a typed refusal");
    let ConversionRefusal::StrictExportRejected { rejection, .. } = refusal else {
        panic!("{refusal:?}");
    };
    let export_report = rejection.report();
    assert!(
        export_report
            .losses
            .iter()
            .any(|loss| loss.message.contains("model arena `product_definitions`")),
        "{:?}",
        export_report.losses
    );
}

struct NotImplementedEncoder;

impl cadmpeg_ir::codec::write::EncoderBackend for NotImplementedEncoder {
    const FORMAT: cadmpeg_ir::codec::FormatId =
        cadmpeg_ir::codec::FormatId::new("notimplementedtest");
    const REQUEST: cadmpeg_core::decode::DecodeDepth = cadmpeg_core::decode::DecodeDepth::Full;
    type Target = cadmpeg_ir::codec::write::target::DialectFree;
    const TARGET: Self::Target = cadmpeg_ir::codec::write::target::DialectFree;

    fn plan_resolved(
        &self,
        _input: EncodeInput<'_>,
        _target: (),
    ) -> Result<cadmpeg_ir::codec::write::ExportBody, cadmpeg_core::CodecError> {
        Err(cadmpeg_core::CodecError::NotImplemented(
            "writer path is not implemented".into(),
        ))
    }
}

/// A writer implementation failure is not evidence of an export loss,
/// even when the application would reject real plan losses.
#[test]
fn not_implemented_plan_failure_is_not_reclassified_as_export_loss() {
    let conversion = prepared(
        CadIr::empty(),
        Format::Cadir,
        Box::new(NotImplementedEncoder),
        EXPORT_LOSSY,
    );
    let Err(error) = conversion.plan() else {
        panic!("the synthetic writer is not implemented");
    };
    assert_eq!(
        error.to_string(),
        "not implemented yet: writer path is not implemented"
    );
    assert!(error.refusal().is_none());
}

/// A failed plan returns its own refusal, whatever the check found and
/// whatever the write's strictness.
#[test]
fn a_failed_plan_returns_its_own_refusal_under_reject_errors() {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };

    for reject in [false, true] {
        let mut conversion = prepared(
            CadIr::empty(),
            Format::Cadir,
            Box::new(NotImplementedEncoder),
            reject_errors(reject),
        );
        complete(&mut conversion.validation).findings.push(Finding {
            rule: cadmpeg_ir::rule_name!("cadmpeg/test.synthetic-document-wide-error"),
            related: Vec::new(),
            check: Check::Counts,
            severity: Severity::Error,
            message: "synthetic document-wide error".into(),
            entity: None,
        });
        let Err(error) = conversion.plan() else {
            panic!("the synthetic writer is not implemented");
        };
        assert_eq!(
            error.to_string(),
            "not implemented yet: writer path is not implemented"
        );
        assert!(error.refusal().is_none());
    }
}

/// Flag absence is a request that names no target; the encoder decides what
/// that means.
///
/// The selection is an owned-string adapter and nothing else. The
/// cross-format catalog default is owned by write resolution, so the command
/// line does not decide it a second time.
#[cfg(feature = "iges")]
#[test]
fn flag_absence_is_a_request_that_names_no_target() {
    assert_eq!(
        TargetSelection::new(Format::Iges, None).request(),
        TargetRequest::Unnamed
    );
    let named = TargetSelection::new(Format::Iges, Some("iges:5.1-fixed-ascii".to_owned()));
    assert_eq!(
        named.request(),
        TargetRequest::Explicit("iges:5.1-fixed-ascii")
    );
}

#[cfg(feature = "iges")]
#[test]
fn target_selection_owns_format_and_dialect_grammar() {
    let format_only = TargetSelection::resolve(Some("iges"), None).expect("format resolves");
    assert_eq!(format_only.format, Format::Iges);
    assert_eq!(format_only.request(), TargetRequest::Unnamed);

    let qualified = TargetSelection::resolve(Some("iges:5.1"), None).expect("alias resolves");
    assert_eq!(qualified.format, Format::Iges);
    assert_eq!(qualified.request(), TargetRequest::Explicit("5.1"));

    let inferred = TargetSelection::resolve(Some("5.1"), Some(Path::new("part.iges")))
        .expect("output extension supplies the format");
    assert_eq!(inferred.format, Format::Iges);
    assert_eq!(inferred.request(), TargetRequest::Explicit("5.1"));
}

#[cfg(feature = "step")]
#[test]
fn a_format_alias_does_not_become_a_dialect_request() {
    let selection = TargetSelection::resolve(Some("stp"), Some(Path::new("part.stp")))
        .expect("the identity registry owns the STEP alias");
    assert_eq!(selection.format, Format::Step);
    assert_eq!(selection.request(), TargetRequest::Unnamed);
}

#[test]
fn target_selection_rejects_an_empty_qualified_dialect() {
    let error = TargetSelection::resolve(Some("cadir:"), None).unwrap_err();
    assert!(error.to_string().contains("nothing after the colon"));
    assert!(error.refusal().is_none());
}

/// Export-loss rejection is not a target.
///
/// A loss flag that also named a target would turn `convert a.step -o
/// b.step --reject-lossy=export` into an explicit AP214 request and lose
/// the identity default. Only `--to` may say what to write.
#[cfg(feature = "step")]
#[test]
fn rejecting_export_losses_does_not_name_a_target() {
    let target = export_target(TargetSelection::new(Format::Step, None));

    assert!(target.selection.request.is_none(), "{:?}", target.selection);
    assert_eq!(target.selection.request(), TargetRequest::Unnamed);

    let named = export_target(TargetSelection::new(
        Format::Step,
        Some("step:ap242-e3".to_owned()),
    ));
    assert_eq!(
        named.selection.request(),
        TargetRequest::Explicit("step:ap242-e3")
    );
}

/// `--to` carries the dialect half verbatim; the encoder resolves it.
///
/// Both spellings the grammar admits reach `plan` unchanged, and both
/// resolve, because catalog lookup matches a row by id or by alias.
/// Resolving here instead would put a second copy of every catalog in the
/// CLI, which is the drift the registries exist to kill.
#[cfg(feature = "rhino")]
#[test]
fn an_alias_and_an_id_reach_the_encoder_unresolved_and_both_resolve() {
    use cadmpeg_core::dialect::DialectId;

    let ir = CadIr::empty();
    for spelling in ["archive-60", "60"] {
        let target = export_target(TargetSelection::new(
            Format::Rhino,
            Some(spelling.to_owned()),
        ));
        assert_eq!(
            target.selection.request(),
            TargetRequest::Explicit(spelling)
        );
        let plan = target
            .encoder
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(spelling),
            )
            .expect("the catalog carries the row under both spellings");
        assert_eq!(
            wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
                plan.report(),
                "identity/target"
            )
            .as_ref()
            .map(DialectId::as_str),
            Some("rhino:archive-60")
        );
    }
}

/// A dialect outside the catalog reaches the encoder and is refused by the
/// sealed planning boundary.
#[cfg(feature = "iges")]
#[test]
fn an_unknown_dialect_is_refused_by_plan_with_its_catalog() {
    let target = export_target(TargetSelection::new(
        Format::Iges,
        Some("ap242e3".to_owned()),
    ));
    let error = target
        .encoder
        .plan(
            EncodeInput::new(&CadIr::empty(), None),
            target.selection.request(),
        )
        .expect_err("a STEP alias is not an IGES target");
    let message = error.to_string();
    assert!(message.contains("ap242e3"), "{message}");
    assert!(message.contains("iges:5.3-fixed-ascii"), "{message}");
}

#[test]
fn reject_lossy_cli_preserves_omission_and_rejects_allow_spelling() {
    use clap::Parser;
    for (flag, expected) in [
        (None, None),
        (Some("--reject-lossy"), Some(LossyScope::Any)),
        (Some("--reject-lossy=decode"), Some(LossyScope::Decode)),
        (Some("--reject-lossy=export"), Some(LossyScope::Export)),
        (Some("--reject-lossy=any"), Some(LossyScope::Any)),
    ] {
        let mut args = vec!["cadmpeg", "convert", "input.step"];
        args.extend(flag);
        let crate::Command::Convert {
            reject_lossy,
            reject_lossy_floor,
            ..
        } = crate::Cli::try_parse_from(args).unwrap().command
        else {
            panic!("expected convert command");
        };
        assert_eq!(reject_lossy, expected);
        assert_eq!(reject_lossy_floor, LossyFloor::Info);
    }
    let error =
        crate::Cli::try_parse_from(["cadmpeg", "convert", "input.step", "--reject-lossy=allow"])
            .unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
}

#[test]
fn missing_states_and_reject_empty_parse() {
    use clap::Parser;
    let crate::Command::Convert {
        missing_states,
        reject_empty,
        ..
    } = crate::Cli::try_parse_from(["cadmpeg", "convert", "input.step"])
        .unwrap()
        .command
    else {
        panic!("expected convert command");
    };
    assert_eq!(missing_states, crate::MissingStates::GeometryTrue);
    assert!(!reject_empty);
    let crate::Command::Convert {
        missing_states,
        reject_empty,
        ..
    } = crate::Cli::try_parse_from([
        "cadmpeg",
        "convert",
        "input.step",
        "--missing-states=history-true",
        "--reject-empty",
    ])
    .unwrap()
    .command
    else {
        panic!("expected convert command");
    };
    assert_eq!(missing_states, crate::MissingStates::HistoryTrue);
    assert!(reject_empty);
    let crate::Command::Measure { reject_empty, .. } =
        crate::Cli::try_parse_from(["cadmpeg", "measure", "input.step", "--reject-empty"])
            .unwrap()
            .command
    else {
        panic!("expected measure command");
    };
    assert!(reject_empty);
}

#[test]
fn reject_lossy_floor_names_a_severity_and_needs_reject_lossy() {
    use clap::Parser;
    let crate::Command::Convert {
        reject_lossy_floor, ..
    } = crate::Cli::try_parse_from([
        "cadmpeg",
        "convert",
        "input.step",
        "--reject-lossy=export",
        "--reject-lossy-floor=error",
    ])
    .unwrap()
    .command
    else {
        panic!("expected convert command");
    };
    assert_eq!(reject_lossy_floor, LossyFloor::Error);
    let error = crate::Cli::try_parse_from([
        "cadmpeg",
        "convert",
        "input.step",
        "--reject-lossy-floor=error",
    ])
    .unwrap_err();
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[cfg(feature = "fcstd")]
fn checked_design_history() -> (LoadedDocument, cadmpeg_ir::report::check::ValidationReport) {
    let inputs = cadmpeg_registry::InputCatalog::with_builtins();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures/design_history.FCStd");
    let options = cadmpeg_ir::codec::DecodeOptions::default();
    let document = crate::loader::load_artifact(&inputs, &path, &options, None)
        .expect("load the FreeCAD design-history fixture");
    let validation = crate::application::validators::validate_loaded(
        &inputs,
        &document,
        &options.policy,
        cadmpeg_measure::WorkProfile::Desktop,
    )
    .expect("validate the FreeCAD design-history fixture");
    assert_eq!(validation.error_count(), 0, "{validation:?}");
    (document, validation)
}

#[cfg(all(feature = "fcstd", feature = "step"))]
fn prepare_cadir_for_step(ir: &CadIr, path: &Path, reject_errors: bool) -> PreparedConversion {
    use cadmpeg_ir::codec::write::CadirEncoder;
    use cadmpeg_ir::codec::DecodeOptions;
    use cadmpeg_registry::InputCatalog;

    CadirEncoder
        .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
        .expect("plan the mutated CADIR document")
        .write_to(&mut std::fs::File::create(path).expect("create CADIR input"))
        .expect("write the mutated CADIR document");
    super::prepare(
        &InputCatalog::with_builtins(),
        &super::SourceRequest {
            path,
            forced: None,
            options: DecodeOptions::default(),
            work: cadmpeg_measure::WorkProfile::Desktop,
        },
        export_target(TargetSelection::new(Format::Step, None)),
        &super::ConversionPolicy {
            strictness: self::reject_errors(reject_errors),
            missing_states: cadmpeg_ir::codec::write::MissingStateForm::GeometryTrue,
            destination: DestinationPolicy::Stdout {
                allow_binary: false,
            },
        },
    )
    .expect("prepare must retain validation errors for the export plan gate")
}

#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn a_design_arena_error_does_not_refuse_a_step_export_under_reject_errors() {
    use cadmpeg_ir::ids::ProductDefinitionId;
    use cadmpeg_ir::report::Severity;

    let (mut document, _) = checked_design_history();
    let parameter_id = document.ir.design.parameters[0].id.to_string();
    document.ir.design.parameters[0].scope =
        ProductDefinitionId::mint("fcstd:test:definition#missing")
            .expect("valid absent definition identity");
    let directory = tempfile::tempdir().expect("temporary CADIR directory");
    let conversion =
        prepare_cadir_for_step(&document.ir, &directory.path().join("design.json"), true);
    let planned = conversion.plan().expect("STEP drops the design arenas");
    let report = planned
        .prepared()
        .validation
        .report()
        .expect("the check runs to its end");
    assert!(
        report.findings.iter().any(|finding| {
            finding.severity >= Severity::Error
                && finding.entity.as_deref() == Some(parameter_id.as_str())
        }),
        "{report:?}"
    );
    assert!(
        report
            .findings
            .iter()
            .filter(|finding| finding.severity >= Severity::Error)
            .all(|finding| document
                .ir
                .design
                .parameters
                .iter()
                .any(|parameter| { finding.entity.as_deref() == Some(parameter.id.as_str()) })),
        "{report:?}"
    );
    assert_eq!(planned.carried_errors(), 0);
}

#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn a_brep_error_refuses_a_step_export_under_reject_errors() {
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::report::Severity;

    let (mut document, _) = checked_design_history();
    let face_id = document.ir.geometry.faces[0].id.to_string();
    document.ir.geometry.faces[0].surface =
        SurfaceId::mint("fcstd:test:surface#missing").expect("valid absent surface identity");
    let directory = tempfile::tempdir().expect("temporary CADIR directory");
    let conversion =
        prepare_cadir_for_step(&document.ir, &directory.path().join("brep.json"), true);
    let Err(error) = conversion.plan() else {
        panic!("STEP must refuse an error in the B-rep it writes");
    };
    let Some(ConversionRefusal::StrictExportRejected {
        validation: Some(validation),
        rejection,
        ..
    }) = error.refusal()
    else {
        panic!("expected a strict export refusal, got {error:?}");
    };
    let carried = record_invalid_count(rejection);
    assert!(carried >= 1);
    assert_eq!(carried, validation.error_count());
    assert!(
        validation.findings.iter().any(|finding| {
            finding.severity >= Severity::Error
                && finding.entity.as_deref() == Some(face_id.as_str())
        }),
        "{validation:?}"
    );
    assert!(
        validation
            .findings
            .iter()
            .filter(|finding| finding.severity >= Severity::Error)
            .all(|finding| finding.entity.as_deref().is_none_or(|id| {
                document
                    .ir
                    .geometry
                    .faces
                    .iter()
                    .any(|face| face.id.as_str() == id)
                    || document
                        .ir
                        .geometry
                        .surfaces
                        .iter()
                        .any(|surface| surface.id.as_str() == id)
            })),
        "{validation:?}"
    );
}

#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn an_error_without_an_entity_refuses_a_step_export_under_reject_errors() {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };

    let (document, _) = checked_design_history();
    let feature_id = document.ir.model.features[0].id.to_string();
    for include_design_error in [false, true] {
        let mut conversion = prepared(
            document.ir.clone(),
            Format::Step,
            export_target(TargetSelection::new(Format::Step, None)).encoder,
            reject_errors(true),
        );
        let validation = complete(&mut conversion.validation);
        assert_eq!(validation.error_count(), 0);
        validation.findings.push(Finding {
            rule: cadmpeg_ir::rule_name!("cadmpeg/test.synthetic-document-wide-error"),
            related: Vec::new(),
            check: Check::Counts,
            severity: Severity::Error,
            message: "synthetic document-wide error".into(),
            entity: None,
        });
        if include_design_error {
            validation.findings.push(Finding {
                rule: cadmpeg_ir::rule_name!("cadmpeg/test.synthetic-design-error"),
                related: Vec::new(),
                check: Check::Counts,
                severity: Severity::Error,
                message: "synthetic design error".into(),
                entity: Some(feature_id.clone()),
            });
        }
        let Err(error) = conversion.plan() else {
            panic!("an unlocated error must refuse the export");
        };
        let Some(ConversionRefusal::StrictExportRejected { rejection, .. }) = error.refusal()
        else {
            panic!("expected a strict export refusal, got {error:?}");
        };
        assert_eq!(record_invalid_count(rejection), 1, "{error:?}");
        assert!(
            rejection
                .loss()
                .message
                .contains("cadmpeg/test.synthetic-document-wide-error"),
            "{error}"
        );
    }
}

#[cfg(feature = "fcstd")]
#[test]
fn a_design_arena_error_refuses_a_freecad_round_trip_under_reject_errors() {
    use crate::application::refusal::ConversionRefusal;
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };

    let (document, validation) = checked_design_history();
    let feature_id = document.ir.model.features[0].id.to_string();
    for inject_error in [false, true] {
        let target = super::export_target(TargetSelection::new(Format::Fcstd, None));
        let mut conversion = PreparedConversion {
            document: document.clone(),
            validation: Validation::Complete(validation.clone()),
            encoder: target.encoder,
            selection: target.selection,
            assets: crate::loader::companions::Directory::beside(Path::new("input")),
            destination: ResolvedDestination::Stdout,
            write_strictness: reject_errors(true).write(),
            missing_states: cadmpeg_ir::codec::write::MissingStateForm::GeometryTrue,
        };
        if inject_error {
            complete(&mut conversion.validation).findings.push(Finding {
                rule: cadmpeg_ir::rule_name!("cadmpeg/test.synthetic-design-error"),
                related: Vec::new(),
                check: Check::Counts,
                severity: Severity::Error,
                message: "synthetic design error".into(),
                entity: Some(feature_id.clone()),
            });
            let Err(error) = conversion.plan() else {
                panic!("FreeCAD carries design errors through complete coverage");
            };
            let Some(ConversionRefusal::StrictExportRejected { rejection, .. }) = error.refusal()
            else {
                panic!("expected a strict export refusal, got {error:?}");
            };
            assert_eq!(record_invalid_count(rejection), 1, "{error:?}");
        } else {
            let planned = conversion
                .plan()
                .expect("the unchanged FreeCAD fixture must plan");
            assert_eq!(planned.carried_errors(), 0);
        }
    }
}

#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn a_carried_error_does_not_refuse_the_export_by_default() {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };

    let (document, _) = checked_design_history();
    let mut conversion = prepared(
        document.ir,
        Format::Step,
        export_target(TargetSelection::new(Format::Step, None)).encoder,
        StrictnessFlags::default(),
    );
    let validation = complete(&mut conversion.validation);
    assert_eq!(validation.error_count(), 0);
    validation.findings.push(Finding {
        rule: cadmpeg_ir::rule_name!("cadmpeg/test.synthetic-document-wide-error"),
        related: Vec::new(),
        check: Check::Counts,
        severity: Severity::Error,
        message: "synthetic document-wide error".into(),
        entity: None,
    });
    let planned = conversion
        .plan()
        .expect("a carried error is reported, and the export proceeds");
    assert_eq!(planned.carried_errors(), 1);
}

#[cfg(feature = "step")]
#[test]
fn an_f3d_native_design_record_error_does_not_refuse_a_step_export_under_reject_errors() {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };

    let mut conversion = prepared(
        step_ir_with_unrepresentable_native_content(),
        Format::Step,
        export_target(TargetSelection::new(Format::Step, None)).encoder,
        reject_errors(true),
    );
    let validation = complete(&mut conversion.validation);
    assert_eq!(validation.error_count(), 0);
    let finding = Finding {
        rule: cadmpeg_ir::rule_name!("cadmpeg/test.synthetic-native-design-record-error"),
        related: Vec::new(),
        check: Check::Counts,
        severity: Severity::Error,
        message: "synthetic native design record error".into(),
        entity: Some("f3d:test:asm-history#0".into()),
    };
    validation.findings.push(finding.clone());
    let planned = conversion
        .plan()
        .expect("STEP drops the foreign native namespace");
    assert_eq!(planned.carried_errors(), 0);
    assert!(planned
        .prepared()
        .validation
        .report()
        .expect("the check runs to its end")
        .findings
        .contains(&finding));
}

#[cfg(feature = "usd")]
#[test]
fn an_output_extension_that_names_a_dialect_selects_it() {
    let package = Some(Path::new("assembly.usdz"));
    let inferred = TargetSelection::resolve(None, package).expect("format from the extension");
    assert_eq!(inferred.format, Format::Usd);
    assert_eq!(inferred.request(), TargetRequest::Explicit("usdz"));
    let format_only = TargetSelection::resolve(Some("usd"), package).expect("format resolves");
    assert_eq!(format_only.request(), TargetRequest::Explicit("usdz"));
    let explicit =
        TargetSelection::resolve(Some("usd:usda-1.0"), package).expect("dialect resolves");
    assert_eq!(explicit.request(), TargetRequest::Explicit("usda-1.0"));
    let layer = TargetSelection::resolve(None, Some(Path::new("assembly.usda")))
        .expect("format from the extension");
    assert_eq!(layer.request(), TargetRequest::Unnamed);
    let binary = TargetSelection::resolve(None, Some(Path::new("assembly.usdc")))
        .expect("format from the extension");
    assert_eq!(binary.request(), TargetRequest::Explicit("usdc"));
}

/// A decode report that states nothing, for a sidecar.
fn empty_decode_report() -> cadmpeg_ir::report::decode::DecodeReport {
    cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
        "identity": {"kind": "unclassified", "format": "test"},
        "transfer": {"kind": "full", "geometry_transferred": false},
        "coverage": {},
        "losses": [],
        "notes": [],
        "transfer_ledger": {"entries": []},
    }))
    .unwrap()
}

/// Writes `ir` as CADIR to `path` from `origin`, with the origin's fidelity,
/// and returns the provenance keys of the sidecar the write leaves.
fn sidecar_keys_of_cadir_write(
    path: &Path,
    ir: &CadIr,
    origin: &crate::application::document::LoadOrigin,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
) -> Vec<String> {
    use cadmpeg_ir::codec::write::CadirEncoder;
    let plan = CadirEncoder
        .plan(
            EncodeInput::new(ir, fidelity),
            cadmpeg_ir::codec::write::target::TargetRequest::Unnamed,
        )
        .unwrap();
    super::emit_export_plan(
        plan,
        Format::Cadir,
        &ResolvedDestination::File(path.to_owned()),
        origin,
    )
    .unwrap();
    let sidecar = cadmpeg_ir::DecodeSidecar::from_json(
        &std::fs::read_to_string(cadmpeg_ir::decode_sidecar_path(path).unwrap()).unwrap(),
    )
    .unwrap();
    let mut keys: Vec<String> = sidecar
        .fidelity
        .annotations
        .provenance
        .keys()
        .map(|key| key.as_str().to_owned())
        .collect();
    keys.sort_unstable();
    keys
}

/// The sidecar of a CADIR document written from a read that gave up a record
/// holds no annotation of the record or of a record dropped with it (§3.3).
#[test]
fn the_sidecar_of_a_document_written_after_a_give_up_names_no_removed_record() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("part.cadir.json");
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let given_up = ir.geometry.points[0].id.as_str().to_owned();
    let kept = ir.geometry.points[1].id.as_str().to_owned();
    let mut value = cadmpeg_core::serde_erase::json::to_value(&ir).unwrap();
    value["geometry"]["points"][0]["position"] = serde_json::json!("not a point");
    let text = value.to_string();
    std::fs::write(&input, &text).unwrap();
    let stream =
        cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("test:source"));
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    annotations.note(
        cadmpeg_ir::ids::Identity::new(given_up.as_str()).expect("identity grammar"),
        &stream,
        0,
        None,
    );
    annotations.note(
        cadmpeg_ir::ids::Identity::new(kept.as_str()).expect("identity grammar"),
        &stream,
        0,
        None,
    );
    let mut fidelity =
        cadmpeg_ir::SourceFidelity::new(cadmpeg_ir::hash::digest::Sha256Digest::digest(b"source"));
    fidelity.annotations = annotations.build();
    let sidecar = cadmpeg_ir::DecodeSidecar::bind_sha256(
        cadmpeg_ir::hash::digest::Sha256Digest::digest(text.as_bytes()),
        empty_decode_report(),
        fidelity,
    );
    std::fs::write(
        cadmpeg_ir::decode_sidecar_path(&input).unwrap(),
        sidecar.to_canonical_json().unwrap(),
    )
    .unwrap();
    let loaded = crate::loader::load_artifact(
        &cadmpeg_registry::InputCatalog::with_builtins(),
        &input,
        &cadmpeg_ir::codec::DecodeOptions::default(),
        Some(cadmpeg_registry::ForcedInput::Cadir),
    )
    .unwrap();

    let keys = sidecar_keys_of_cadir_write(
        &directory.path().join("written.cadir.json"),
        &loaded.ir,
        &loaded.origin,
        loaded.fidelity(),
    );

    assert_eq!(keys, [kept]);
}

/// A CADIR write leaves the annotation of a record of an arena the wire
/// omits out of the sidecar, and keeps that of a written design record that
/// shares its id with one (§3.2, §3.4).
#[test]
fn the_sidecar_keeps_the_annotations_of_a_written_record_that_shares_an_omitted_id() {
    use cadmpeg_ir::design::feature::{
        Feature, FeatureDefinition, FeatureOperation, OperationFamily,
    };
    use cadmpeg_ir::features::FeatureId;

    const SHARED: &str = "test:writer:feature#shared";
    const OMITTED: &str = "test:writer:feature#omitted";
    let legacy = |id: &str| cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::StoredGeometry {},
            ),
        ),
        native_ref: None,
    };
    let directory = tempfile::tempdir().unwrap();
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.set_scope(cadmpeg_core::decode::ContentScope::full());
    let point = ir.geometry.points[0].id.as_str().to_owned();
    ir.model.features.push(legacy(OMITTED));
    ir.model.features.push(legacy(SHARED));
    ir.design.features.push(Feature {
        id: FeatureId::mint(SHARED).unwrap(),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved {
                family: OperationFamily::Extrude,
            },
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    });
    ir.finalize();
    let stream =
        cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("test:source"));
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    for key in [point.as_str(), SHARED, OMITTED] {
        annotations.note(
            cadmpeg_ir::ids::Identity::new(key).expect("identity grammar"),
            &stream,
            0,
            None,
        );
    }
    let mut fidelity =
        cadmpeg_ir::SourceFidelity::new(cadmpeg_ir::hash::digest::Sha256Digest::digest(b"source"));
    fidelity.annotations = annotations.build();
    let origin = crate::application::document::LoadOrigin::Restored {
        report: empty_decode_report(),
        fidelity: Box::new(fidelity.clone()),
    };

    let keys = sidecar_keys_of_cadir_write(
        &directory.path().join("written.cadir.json"),
        &ir,
        &origin,
        Some(&fidelity),
    );

    let mut expected = vec![point, SHARED.to_owned()];
    expected.sort_unstable();
    assert_eq!(keys, expected);
}
