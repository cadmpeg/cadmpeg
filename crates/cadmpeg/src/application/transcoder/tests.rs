#[cfg(feature = "rhino")]
use cadmpeg_test_support::wire;

#[cfg(any(feature = "iges", feature = "step"))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(any(feature = "iges", feature = "rhino", feature = "step"))]
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
    DestinationPolicy, LossPolicy, PreparedConversion, ResolvedDestination, TargetSelection,
};

#[test]
fn loss_policy_assigns_each_refusal_phase() {
    assert_eq!(
        [
            (LossPolicy::Allow, false, false),
            (LossPolicy::RejectDecode, true, false),
            (LossPolicy::RejectExport, false, true),
            (LossPolicy::RejectAny, true, true),
        ],
        [
            (
                LossPolicy::Allow,
                LossPolicy::Allow.rejects_decode(),
                LossPolicy::Allow.rejects_export(),
            ),
            (
                LossPolicy::RejectDecode,
                LossPolicy::RejectDecode.rejects_decode(),
                LossPolicy::RejectDecode.rejects_export(),
            ),
            (
                LossPolicy::RejectExport,
                LossPolicy::RejectExport.rejects_decode(),
                LossPolicy::RejectExport.rejects_export(),
            ),
            (
                LossPolicy::RejectAny,
                LossPolicy::RejectAny.rejects_decode(),
                LossPolicy::RejectAny.rejects_export(),
            ),
        ]
    );
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
    loss_policy: LossPolicy,
) -> PreparedConversion {
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    PreparedConversion {
        document: LoadedDocument::neutral(ir, 0),
        validation,
        encoder,
        selection: TargetSelection::new(format, None),
        destination: ResolvedDestination::Stdout,
        loss_policy,
        allow_errors: false,
        allow_empty: false,
    }
}

#[cfg(feature = "iges")]
#[test]
fn encoder_planning_owns_unknown_explicit_target_admission() {
    let mut conversion = prepared(
        CadIr::empty(),
        Format::Iges,
        Box::new(cadmpeg_codec_iges::IgesCodec),
        LossPolicy::Allow,
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
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "asm_histories".into(),
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
        LossPolicy::RejectExport,
    );
    let Err(error) = conversion.plan() else {
        panic!("the plan loss must refuse");
    };
    let refusal = error
        .refusal()
        .expect("the shared gate returns a typed refusal");
    assert!(
        matches!(refusal, ConversionRefusal::ExportLossRejected { .. }),
        "{refusal:?}"
    );
    let message = refusal.evidence().message;
    assert!(message.contains("source-native record(s)"), "{message}");
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
        LossPolicy::Allow,
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
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#pin").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
            .expect("finite position"),
        None,
    ));
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: "cadir:model:product#group"
                .try_into()
                .expect("valid identity"),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Group,
            source_name: Some("Group".into()),
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            bodies: Vec::new(),
            native_ref: None,
        });
    let mut conversion = prepared(
        ir,
        Format::Rhino,
        Box::new(cadmpeg_codec_rhino::RhinoCodec),
        LossPolicy::RejectExport,
    );
    // The lone point is an orphan carrier, a check error the Rhino payload
    // carries. This test covers the export-loss gate, so it permits errors.
    conversion.allow_errors = true;
    let Err(error) = conversion.plan() else {
        panic!("the omitted product definition must refuse");
    };
    let refusal = error
        .refusal()
        .expect("the shared gate returns a typed refusal");
    let ConversionRefusal::ExportLossRejected { export_report, .. } = refusal else {
        panic!("{refusal:?}");
    };
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
        cadmpeg_ir::codec::FormatId::new("not-implemented-test");
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
        LossPolicy::RejectExport,
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

/// Flag absence is always `Inherit`; the encoder decides what that means.
///
/// The selection is an owned-string adapter and nothing else. The
/// cross-format catalog default is owned by write resolution, so the command
/// line does not decide it a second time.
#[cfg(feature = "iges")]
#[test]
fn flag_absence_is_always_an_inherit_request() {
    assert_eq!(
        TargetSelection::new(Format::Iges, None).request(),
        TargetRequest::Inherit
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
    assert_eq!(format_only.request(), TargetRequest::Inherit);

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
    assert_eq!(selection.request(), TargetRequest::Inherit);
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
    assert_eq!(target.selection.request(), TargetRequest::Inherit);

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
fn loss_policy_cli_preserves_omission_and_rejects_allow_spelling() {
    use clap::Parser;
    for (flag, expected) in [
        (None, LossPolicy::Allow),
        (Some("--reject-lossy"), LossPolicy::RejectAny),
        (Some("--reject-lossy=decode"), LossPolicy::RejectDecode),
        (Some("--reject-lossy=export"), LossPolicy::RejectExport),
        (Some("--reject-lossy=any"), LossPolicy::RejectAny),
    ] {
        let mut args = vec!["cadmpeg", "convert", "input.step"];
        args.extend(flag);
        let crate::Command::Convert { reject_lossy, .. } =
            crate::Cli::try_parse_from(args).unwrap().command
        else {
            panic!("expected convert command");
        };
        assert_eq!(reject_lossy.unwrap_or_default(), expected);
    }
    let error =
        crate::Cli::try_parse_from(["cadmpeg", "convert", "input.step", "--reject-lossy=allow"])
            .unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
}

#[cfg(feature = "fcstd")]
fn checked_design_history() -> (LoadedDocument, cadmpeg_ir::report::check::ValidationReport) {
    let inputs = cadmpeg_registry::InputCatalog::with_builtins();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures/design_history.FCStd");
    let options = cadmpeg_ir::codec::DecodeOptions::default();
    let document = crate::loader::load_artifact(&inputs, &path, options, None)
        .expect("load the FreeCAD design-history fixture");
    let validation =
        crate::application::validators::validate_loaded(&inputs, &document, &options.policy)
            .expect("validate the FreeCAD design-history fixture");
    assert_eq!(validation.error_count(), 0, "{validation:?}");
    (document, validation)
}

#[cfg(all(feature = "fcstd", feature = "step"))]
fn prepare_cadir_for_step(ir: &CadIr, path: &Path) -> PreparedConversion {
    use cadmpeg_ir::codec::write::CadirEncoder;
    use cadmpeg_ir::codec::DecodeOptions;
    use cadmpeg_registry::InputCatalog;

    CadirEncoder
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .expect("plan the mutated CADIR document")
        .write_to(&mut std::fs::File::create(path).expect("create CADIR input"))
        .expect("write the mutated CADIR document");
    super::prepare(
        &InputCatalog::with_builtins(),
        &super::SourceRequest {
            path,
            forced: None,
            options: DecodeOptions::default(),
        },
        export_target(TargetSelection::new(Format::Step, None)),
        &super::ConversionPolicy {
            losses: LossPolicy::Allow,
            allow_errors: false,
            allow_empty: false,
            destination: DestinationPolicy::Stdout {
                allow_binary: false,
            },
        },
    )
    .expect("prepare must retain validation errors for the export plan gate")
}

#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn a_design_arena_error_does_not_block_a_step_export() {
    use cadmpeg_ir::features::FeatureId;
    use cadmpeg_ir::report::{check::Check, Severity};

    let (mut document, _) = checked_design_history();
    let parameter_id = document.ir.model.parameters[0].id.to_string();
    document.ir.model.parameters[0].owner =
        Some(FeatureId::mint("fcstd:test:feature#missing").expect("valid absent feature identity"));
    let directory = tempfile::tempdir().expect("temporary CADIR directory");
    let conversion = prepare_cadir_for_step(&document.ir, &directory.path().join("design.json"));
    let planned = conversion.plan().expect("STEP drops the design arenas");
    let report = &planned.prepared().validation;
    assert!(
        report.findings.iter().any(|finding| {
            finding.severity >= Severity::Error
                && finding.check == Check::ReferentialIntegrity
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
                .model
                .parameters
                .iter()
                .any(|parameter| { finding.entity.as_deref() == Some(parameter.id.as_str()) })),
        "{report:?}"
    );
    assert_eq!(planned.carried_errors(), 0);
}

#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn a_brep_error_blocks_a_step_export() {
    use crate::application::refusal::CheckOperation;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::report::Severity;

    let (mut document, _) = checked_design_history();
    let face_id = document.ir.model.faces[0].id.to_string();
    document.ir.model.faces[0].surface =
        SurfaceId::mint("fcstd:test:surface#missing").expect("valid absent surface identity");
    let directory = tempfile::tempdir().expect("temporary CADIR directory");
    let conversion = prepare_cadir_for_step(&document.ir, &directory.path().join("brep.json"));
    let Err(error) = conversion.plan() else {
        panic!("STEP must refuse an error in the B-rep it writes");
    };
    let Some(ConversionRefusal::CheckFailed {
        operation: CheckOperation::Export { carried },
        validation,
        ..
    }) = error.refusal()
    else {
        panic!("expected an export check refusal, got {error:?}");
    };
    assert!(*carried >= 1);
    assert_eq!(*carried, validation.error_count());
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
                    .model
                    .faces
                    .iter()
                    .any(|face| face.id.as_str() == id)
                    || document
                        .ir
                        .model
                        .surfaces
                        .iter()
                        .any(|surface| surface.id.as_str() == id)
            })),
        "{validation:?}"
    );
}

#[cfg(all(feature = "fcstd", feature = "step"))]
#[test]
fn an_error_without_an_entity_blocks_a_step_export() {
    use crate::application::refusal::CheckOperation;
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
            LossPolicy::Allow,
        );
        assert_eq!(conversion.validation.error_count(), 0);
        conversion.validation.findings.push(Finding {
            related: Vec::new(),
            check: Check::Counts,
            severity: Severity::Error,
            message: "synthetic document-wide error".into(),
            entity: None,
        });
        if include_design_error {
            conversion.validation.findings.push(Finding {
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
        assert!(
            matches!(
                error.refusal(),
                Some(ConversionRefusal::CheckFailed {
                    operation: CheckOperation::Export { carried: 1 },
                    ..
                })
            ),
            "{error:?}"
        );
        if include_design_error {
            assert!(
                error
                    .to_string()
                    .contains("2 error(s), 1 in what the export writes"),
                "{error}"
            );
        }
    }
}

#[cfg(feature = "fcstd")]
#[test]
fn a_design_arena_error_blocks_a_freecad_round_trip() {
    use crate::application::refusal::{CheckOperation, ConversionRefusal};
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
            validation: validation.clone(),
            encoder: target.encoder,
            selection: target.selection,
            destination: ResolvedDestination::Stdout,
            loss_policy: LossPolicy::Allow,
            allow_errors: false,
            allow_empty: false,
        };
        if inject_error {
            conversion.validation.findings.push(Finding {
                related: Vec::new(),
                check: Check::Counts,
                severity: Severity::Error,
                message: "synthetic design error".into(),
                entity: Some(feature_id.clone()),
            });
            let Err(error) = conversion.plan() else {
                panic!("FreeCAD carries design errors through complete coverage");
            };
            assert!(
                matches!(
                    error.refusal(),
                    Some(ConversionRefusal::CheckFailed {
                        operation: CheckOperation::Export { carried: 1 },
                        ..
                    })
                ),
                "{error:?}"
            );
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
fn allow_errors_writes_past_a_carried_error() {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };

    let (document, _) = checked_design_history();
    let mut conversion = prepared(
        document.ir,
        Format::Step,
        export_target(TargetSelection::new(Format::Step, None)).encoder,
        LossPolicy::Allow,
    );
    assert_eq!(conversion.validation.error_count(), 0);
    conversion.validation.findings.push(Finding {
        related: Vec::new(),
        check: Check::Counts,
        severity: Severity::Error,
        message: "synthetic document-wide error".into(),
        entity: None,
    });
    conversion.allow_errors = true;
    let planned = conversion
        .plan()
        .expect("allow-errors permits a carried error");
    assert_eq!(planned.carried_errors(), 1);
}

#[cfg(feature = "step")]
#[test]
fn an_f3d_native_design_record_error_does_not_block_a_step_export() {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };

    let mut conversion = prepared(
        step_ir_with_unrepresentable_native_content(),
        Format::Step,
        export_target(TargetSelection::new(Format::Step, None)).encoder,
        LossPolicy::Allow,
    );
    assert_eq!(conversion.validation.error_count(), 0);
    let finding = Finding {
        related: Vec::new(),
        check: Check::Counts,
        severity: Severity::Error,
        message: "synthetic native design record error".into(),
        entity: Some("f3d:test:asm-history#0".into()),
    };
    conversion.validation.findings.push(finding.clone());
    let planned = conversion
        .plan()
        .expect("STEP drops the foreign native namespace");
    assert_eq!(planned.carried_errors(), 0);
    assert!(planned.prepared().validation.findings.contains(&finding));
}
