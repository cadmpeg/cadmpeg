use super::*;
use cadmpeg_ir::codec::CadirEncoder;
use cadmpeg_ir::CadIr;

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
fn cli_admission_flags_resolve_to_distinct_modes() {
    assert_eq!(
        [
            ValidationAdmission::new(false, false),
            ValidationAdmission::new(true, false),
            ValidationAdmission::new(false, true),
            ValidationAdmission::new(true, true),
        ],
        [
            ValidationAdmission::Strict,
            ValidationAdmission::AllowErrors,
            ValidationAdmission::AllowEmpty,
            ValidationAdmission::AllowErrorsAndEmpty,
        ]
    );
}

#[test]
fn destination_policy_discards_flags_irrelevant_to_the_destination() {
    assert_eq!(
        DestinationPolicy::new(Some(PathBuf::from("part.step")), true, true),
        DestinationPolicy::File {
            path: PathBuf::from("part.step"),
            overwrite: true,
        }
    );
    assert_eq!(
        DestinationPolicy::new(None, true, false),
        DestinationPolicy::Stdout {
            allow_binary: false,
        }
    );
}

fn prepared(
    ir: CadIr,
    format: Format,
    encoder: Box<dyn Encoder>,
    reject_export_losses: bool,
) -> PreparedConversion {
    PreparedConversion {
        document: LoadedDocument::neutral(ir),
        notices: Vec::new(),
        validation: None,
        encoder,
        selection: TargetSelection::new(format, None),
        destination: ResolvedDestination::Stdout,
        plan_policy: if reject_export_losses {
            PlanPolicy::RejectLosses
        } else {
            PlanPolicy::PermitLosses
        },
    }
}

#[cfg(feature = "iges")]
#[test]
fn encoder_planning_owns_unknown_explicit_target_admission() {
    let mut conversion = prepared(
        CadIr::empty(cadmpeg_ir::units::Units::default()),
        Format::Iges,
        Box::new(cadmpeg_codec_iges::IgesEncoder),
        false,
    );
    conversion.selection.request = Some("nonesuch".into());

    let Err(error) = conversion.plan() else {
        panic!("the encoder must reject an unknown target");
    };
    assert!(matches!(
        error.downcast_ref::<ConversionRefusal>(),
        Some(ConversionRefusal::UnsupportedTarget { .. })
    ));
}

#[cfg(feature = "step")]
fn step_ir_with_unrepresentable_native_content() -> CadIr {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.native.namespace_mut("f3d").arenas.insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            "asm-history-0",
            serde_json::Map::default(),
        )],
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
        true,
    );
    let Err(error) = conversion.plan() else {
        panic!("the plan loss must refuse");
    };
    let refusal = error
        .downcast_ref::<ConversionRefusal>()
        .expect("the shared gate returns a typed refusal");
    assert!(
        matches!(refusal, ConversionRefusal::ExportLossRejected { .. }),
        "{refusal:?}"
    );
    assert!(
        refusal.message().contains("source-native record(s)"),
        "{}",
        refusal.message()
    );
    let report = refusal
        .export_report()
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
        false,
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

struct NotImplementedEncoder;

impl Encoder for NotImplementedEncoder {
    fn id(&self) -> &'static str {
        "not-implemented-test"
    }

    fn targets(&self) -> &'static [cadmpeg_ir::codec::TargetDescriptor] {
        &[]
    }

    fn plan<'a>(
        &self,
        _input: EncodeInput<'a>,
        _request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, cadmpeg_core::CodecError> {
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
        CadIr::empty(cadmpeg_ir::units::Units::default()),
        Format::Cadir,
        Box::new(NotImplementedEncoder),
        true,
    );
    let Err(error) = conversion.plan() else {
        panic!("the synthetic writer is not implemented");
    };
    assert!(
        matches!(
            error.downcast_ref::<cadmpeg_core::CodecError>(),
            Some(cadmpeg_core::CodecError::NotImplemented(message))
                if message == "writer path is not implemented"
        ),
        "{error:#}"
    );
    assert!(error.downcast_ref::<ConversionRefusal>().is_none());
}

/// Flag absence is always `Inherit`; the encoder decides what that means.
///
/// The selection is an owned-string adapter and nothing else. The
/// cross-format catalog default is owned by write resolution, so the command
/// line does not decide it a second time.
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

#[test]
fn target_selection_rejects_an_empty_qualified_dialect() {
    let error = TargetSelection::resolve(Some("cadir:"), None).unwrap_err();
    assert!(error.to_string().contains("nothing after the colon"));
}

/// The cross-format default still lands on the catalog default.
///
/// A source of another format has nothing to inherit, so write resolution
/// selects the catalog default.
#[cfg(feature = "iges")]
#[test]
fn a_cross_format_convert_writes_the_catalog_default() {
    use cadmpeg_ir::codec::{resolve_write_request, SourceRelation, WriteRequest};

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.source = Some(cadmpeg_ir::SourceMeta::unclassified(
        "step",
        std::collections::BTreeMap::new(),
    ));
    let encoder = cadmpeg_codec_iges::IgesEncoder;
    let WriteRequest::Catalog {
        entry,
        source: SourceRelation::None,
    } = resolve_write_request(&ir, TargetRequest::Inherit, encoder.id(), encoder.targets())
        .expect("the fallback resolves")
    else {
        panic!("a cross-format request resolves to the catalog")
    };
    assert_eq!(entry.id, "iges:5.3-fixed-ascii");
}

/// CADIR has no catalog, so it takes `Inherit` either way.
#[test]
fn cadir_takes_inherit() {
    let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("CADIR identity resolves through the empty catalog");
    assert_eq!(plan.report().target(), None);
}

/// `convert old.3dm -o new.3dm` with no target flag writes the archive
/// version the file already is.
///
/// The whole chain the command line owns, minus argv parsing: no flag makes
/// [`export_target`] build a Rhino encoder and an `Unstated` selection, the
/// selection resolves to `Inherit` because the source is Rhino too, and the
/// encoder resolves `Inherit` against the source's dialect. The source is
/// archive 50 and the catalog default is archive 80, so the assertion cannot
/// pass by coincidence.
///
/// Until this change `export_target` substituted archive 80 for flag
/// absence, so the round trip handed a Rhino 5 user a file their own Rhino
/// cannot open. `cadmpeg-codec-rhino`'s `writer/tests/targets.rs` covers the
/// explicit flag, the cross-format default, and the refusal.
#[cfg(feature = "rhino")]
#[test]
fn a_same_format_rhino_convert_keeps_the_source_archive_version() {
    use cadmpeg_core::dialect::DialectId;
    use cadmpeg_ir::codec::Codec;

    let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut archive_50 = Vec::new();
    cadmpeg_codec_rhino::RhinoEncoder
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("rhino:archive-50"),
        )
        .expect("archive 50 is a target")
        .write_to(&mut archive_50)
        .expect("the plan writes");
    let decoded = cadmpeg_codec_rhino::RhinoCodec
        .decode(
            &mut std::io::Cursor::new(archive_50),
            &DecodeOptions::default(),
        )
        .expect("the archive decodes");

    let target = export_target(TargetSelection::new(Format::Rhino, None));
    let request = target.selection.request();
    assert_eq!(request, TargetRequest::Inherit);

    let plan = target
        .encoder
        .plan(EncodeInput::new(decoded.ir(), None), request)
        .expect("the inherited target is writable");
    assert_eq!(
        plan.report().target().map(DialectId::as_str),
        Some("rhino:archive-50")
    );
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
/// resolve, because `find_target` matches a catalog row by id or by alias.
/// Resolving here instead would put a second copy of every catalog in the
/// CLI, which is the drift the registries exist to kill.
#[cfg(feature = "rhino")]
#[test]
fn an_alias_and_an_id_reach_the_encoder_unresolved_and_both_resolve() {
    use cadmpeg_core::dialect::DialectId;

    let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
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
            plan.report().target().map(DialectId::as_str),
            Some("rhino:archive-60")
        );
    }
}

/// A dialect outside the catalog is refused by the encoder, with the
/// catalog in the message. The CLI writes no vocabulary of its own.
#[cfg(feature = "iges")]
#[test]
fn an_unknown_dialect_is_refused_by_the_encoder_with_its_catalog() {
    let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let target = export_target(TargetSelection::new(
        Format::Iges,
        Some("ap242e3".to_owned()),
    ));
    let Err(error) = target.encoder.plan(
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("ap242e3"),
    ) else {
        panic!("a STEP alias is not an IGES target");
    };
    let message = error.to_string();
    assert!(message.contains("ap242e3"), "{message}");
    assert!(message.contains("iges:5.3-fixed-ascii"), "{message}");
}
