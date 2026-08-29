use super::*;
use cadmpeg_ir::codec::CadirEncoder;
use cadmpeg_ir::CadIr;

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
        format,
        encoder,
        selection: TargetSelection::Unstated,
        destination: None,
        reject_export_losses,
    }
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
    assert_eq!(TargetSelection::Unstated.request(), TargetRequest::Inherit);
    let named = TargetSelection::Explicit("iges:5.1-fixed-ascii".to_owned());
    assert_eq!(
        named.request(),
        TargetRequest::Explicit("iges:5.1-fixed-ascii")
    );
}

/// The cross-format default still lands on the catalog default.
///
/// A source of another format has nothing to inherit, so write resolution
/// selects the catalog default.
#[cfg(feature = "iges")]
#[test]
fn a_cross_format_convert_writes_the_catalog_default() {
    use cadmpeg_ir::codec::{resolve_write_request, WriteRequest};

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.source = Some(cadmpeg_ir::SourceMeta {
        format: "step".into(),
        ..Default::default()
    });
    let encoder = cadmpeg_codec_iges::IgesEncoder;
    let WriteRequest::Catalog { entry, displaced } =
        resolve_write_request(&ir, TargetRequest::Inherit, encoder.id(), encoder.targets())
            .expect("the fallback resolves")
    else {
        panic!("a cross-format request resolves to the catalog")
    };
    assert_eq!(entry.id, "iges:5.3-fixed-ascii");
    assert_eq!(displaced, None);
}

/// CADIR has no catalog, so it takes `Inherit` either way.
#[test]
fn cadir_takes_inherit() {
    let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    assert!(CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .is_ok());
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

    let target = export_target(Format::Rhino, None);
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
    let target = export_target(Format::Step, None);

    assert!(
        matches!(target.selection, TargetSelection::Unstated),
        "{:?}",
        target.selection
    );
    assert_eq!(target.selection.request(), TargetRequest::Inherit);

    let named = export_target(Format::Step, Some("step:ap242-e3"));
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
        let target = export_target(Format::Rhino, Some(spelling));
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
    let target = export_target(Format::Iges, Some("ap242e3"));
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
