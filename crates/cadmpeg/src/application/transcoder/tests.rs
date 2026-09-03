use super::*;
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
    let file = DestinationPolicy::new(Some(PathBuf::from("part.step")), true, true);
    assert_eq!(
        file,
        DestinationPolicy::File {
            path: PathBuf::from("part.step"),
            overwrite: true,
        }
    );
    assert_eq!(file.path(), Some(Path::new("part.step")));
    let stdout = DestinationPolicy::new(None, true, false);
    assert_eq!(
        stdout,
        DestinationPolicy::Stdout {
            allow_binary: false,
        }
    );
    assert_eq!(stdout.path(), None);
}

fn prepared(
    ir: CadIr,
    format: Format,
    encoder: Box<dyn Encoder>,
    loss_policy: LossPolicy,
) -> PreparedConversion {
    let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new());
    PreparedConversion {
        document: LoadedDocument::neutral(ir),
        validation,
        encoder,
        selection: TargetSelection::new(format, None),
        destination: ResolvedDestination::Stdout,
        loss_policy,
    }
}

#[cfg(feature = "iges")]
#[test]
fn encoder_planning_owns_unknown_explicit_target_admission() {
    let mut conversion = prepared(
        CadIr::empty(cadmpeg_ir::units::Units::default()),
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

struct NotImplementedEncoder;

impl cadmpeg_ir::codec::write::EncoderBackend for NotImplementedEncoder {
    const FORMAT: &'static str = "not-implemented-test";
    type Target = cadmpeg_ir::codec::write::DialectFree;
    const TARGET: Self::Target = cadmpeg_ir::codec::write::DialectFree;

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
        CadIr::empty(cadmpeg_ir::units::Units::default()),
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

#[test]
fn an_unwritable_format_is_a_typed_plan_refusal() {
    let error = TargetSelection::resolve(Some("catia:v5"), None).unwrap_err();
    let refusal = error
        .refusal()
        .expect("unsupported output formats are semantic plan refusals");
    assert!(matches!(
        refusal,
        ConversionRefusal::UnsupportedOutputFormat { .. }
    ));
    assert_eq!(
        refusal.code(),
        crate::application::refusal::RefusalCode::UnsupportedOutputFormat
    );
    assert_eq!(
        serde_json::to_value(refusal.report()).unwrap()["stage"],
        "plan"
    );
    assert_eq!(refusal.exit_code(), 1);
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
            EncodeInput::new(&CadIr::empty(cadmpeg_ir::units::Units::default()), None),
            target.selection.request(),
        )
        .expect_err("a STEP alias is not an IGES target");
    let message = error.to_string();
    assert!(message.contains("ap242e3"), "{message}");
    assert!(message.contains("iges:5.3-fixed-ascii"), "{message}");
}
