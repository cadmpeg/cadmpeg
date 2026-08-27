// SPDX-License-Identifier: Apache-2.0
//! Resolution of a write request against the source: the synthesis catalog and
//! the two refusals.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{
    default_target, find_target, Codec, DecodeOptions, EncodeInput, Encoder, ExportPlan,
    TargetRequest,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::units::Units;

use crate::codec::StepCodec;
use crate::options::{StepSchema, StepWriteOptions};

/// A decoded STEP document whose `FILE_SCHEMA` declares `identifier`.
///
/// The DATA section is the same cube in every case, written at AP242 edition 3
/// and then re-declared, because the reader has one Part 21 grammar and never
/// branches on `FILE_SCHEMA`. That isolates the axis under test: these documents
/// differ in declared identity and in nothing else.
fn source_declaring(identifier: &str) -> cadmpeg_ir::codec::DecodeResult {
    let written = StepSchema::Ap242Edition3;
    let mut bytes = Vec::new();
    crate::write_step(
        &unit_cube(),
        &mut bytes,
        &StepWriteOptions {
            schema: written,
            ..StepWriteOptions::default()
        },
    )
    .expect("write the cube at AP242 edition 3");
    let text = String::from_utf8(bytes).expect("the writer emits 7-bit output");
    assert_eq!(
        text.matches(written.file_schema()).count(),
        1,
        "the schema identifier must occur once, so the re-declaration is exact"
    );
    let text = text.replace(written.file_schema(), identifier);
    StepCodec::default()
        .decode(
            &mut Cursor::new(text.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode the re-declared exchange")
}

fn inherit<'a>(
    encoder: &StepCodec,
    ir: &'a CadIr,
) -> Result<ExportPlan<'a>, cadmpeg_core::CodecError> {
    encoder.plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
}

fn written_text(plan: ExportPlan<'_>) -> String {
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write the planned export");
    String::from_utf8(bytes).expect("the writer emits 7-bit output")
}

fn target_of(plan: &ExportPlan<'_>) -> Option<String> {
    plan.report().target.as_ref().map(ToString::to_string)
}

fn refusal(error: &CodecError) -> (&str, &str, &str) {
    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = error
    else {
        panic!("expected a target refusal, got {error}");
    };
    (format, requested, available)
}

/// The flagship case: `convert in.step -o out.step` on a file that is not the
/// catalog default keeps the schema it was handed.
///
/// The encoder's constructor schema is AP214 here, exactly as the command line
/// builds it when no `--step-target` is given. Reading it under `Inherit` is the
/// defect: this AP203 edition 1 file would come back declaring
/// `AUTOMOTIVE_DESIGN`, a different application protocol, with the report
/// claiming success. The resolution reads the source instead, and the emitted
/// `FILE_SCHEMA` and the reported target are checked separately so a report that
/// agreed with neither the request nor the bytes still fails.
#[test]
fn inherit_synthesizes_the_source_schema_not_the_constructor_schema() {
    let decoded = source_declaring(StepSchema::Ap203Edition1.file_schema());
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect
            .as_ref()
            .map(ToString::to_string),
        Some("step:ap203-e1".to_owned())
    );

    let encoder = StepCodec::default();
    assert_eq!(encoder.options.schema, StepSchema::Ap214);
    let plan = inherit(&encoder, decoded.ir()).expect("an AP203 edition 1 source is a catalog row");

    assert_eq!(target_of(&plan), Some("step:ap203-e1".to_owned()));
    let text = written_text(plan);
    assert!(
        text.contains("FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'))"),
        "{text}"
    );
    assert!(!text.contains("AUTOMOTIVE_DESIGN"), "{text}");
}

/// Every catalog row inherits itself, so the flagship is not one lucky schema.
#[test]
fn inherit_round_trips_the_declaration_of_every_catalog_row() {
    for schema in StepSchema::ALL {
        let decoded = source_declaring(schema.file_schema());
        let plan = inherit(&StepCodec::default(), decoded.ir())
            .unwrap_or_else(|error| panic!("{schema:?} is a catalog row, got {error}"));
        assert_eq!(target_of(&plan), Some(schema.target().to_owned()));
        assert!(
            written_text(plan).contains(schema.file_schema()),
            "{schema:?}"
        );
    }
}

/// An AP242 source that declares no edition refuses under `Inherit`, naming both
/// its own dialect and the catalog.
///
/// The object identifier is optional in Part 21, so this file's declaration is
/// complete and says the edition is unspecified. Every schema this writer emits
/// stamps arcs, so synthesizing edition 3 would make the output declare an
/// edition the input never did — a re-decode of it classifies `step:ap242-e3`,
/// a different registry row. That is the silent change of what the file is that
/// the resolution forbids.
#[test]
fn inherit_refuses_an_edition_unspecified_ap242_source() {
    let decoded = source_declaring("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF");
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect
            .as_ref()
            .map(ToString::to_string),
        Some("step:ap242".to_owned())
    );

    let error = inherit(&StepCodec::default(), decoded.ir())
        .err()
        .expect("an edition-unspecified AP242 source has no write target");
    let (format, requested, available) = refusal(&error);
    assert_eq!(format, "step");
    assert_eq!(requested, "step:ap242");
    for schema in StepSchema::ALL {
        assert!(available.contains(schema.target()), "{available}");
    }
}

/// A declaration the registry does not name lands on `step:unknown`, the
/// totality row, which is also read-side only. `Inherit` refuses it for the same
/// reason: there is no schema to preserve.
#[test]
fn inherit_refuses_an_unrecognized_source_declaration() {
    let decoded = source_declaring("NOT_A_DECLARED_SCHEMA");
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect
            .as_ref()
            .map(ToString::to_string),
        Some("step:unknown".to_owned())
    );

    let error = inherit(&StepCodec::default(), decoded.ir())
        .err()
        .expect("step:unknown has no write target");
    let (_, requested, available) = refusal(&error);
    assert_eq!(requested, "step:unknown");
    assert!(available.contains("step:ap214"), "{available}");
}

/// A STEP source that records no dialect refuses as well. Nothing names an
/// identity to preserve, and picking one would be inventing it.
#[test]
fn inherit_refuses_a_step_source_that_records_no_dialect() {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(SourceMeta {
        format: "step".to_owned(),
        dialect: None,
        ..SourceMeta::default()
    });

    let error = inherit(&StepCodec::default(), &ir)
        .err()
        .expect("a STEP source with no dialect has nothing to preserve");
    let (format, requested, _) = refusal(&error);
    assert_eq!(format, "step");
    assert_eq!(requested, "step");
}

/// The same source writes the moment the caller names a target: an explicit
/// request is always the escape from an inherit refusal, and it is how an
/// edition gets chosen for an edition-unspecified file — deliberately, by the
/// caller.
#[test]
fn an_explicit_target_writes_a_source_the_catalog_cannot_inherit() {
    let decoded = source_declaring("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF");
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("step:ap242-e3"),
        )
        .expect("an explicit catalog row writes");

    assert_eq!(target_of(&plan), Some("step:ap242-e3".to_owned()));
    assert!(
        written_text(plan).contains(StepSchema::Ap242Edition3.file_schema()),
        "the explicit edition is declared with its arcs"
    );
}

/// An explicit target beats the source's own schema, so a deliberate schema
/// change is still one flag away.
#[test]
fn an_explicit_target_overrides_the_source_schema() {
    let decoded = source_declaring(StepSchema::Ap203Edition1.file_schema());
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("step:ap242-e2"),
        )
        .expect("an explicit catalog row writes");

    assert_eq!(target_of(&plan), Some("step:ap242-e2".to_owned()));
}

/// Cross-format conversion into STEP writes the catalog default. The command
/// line builds `Explicit(default)` when the formats differ, because there is
/// nothing to inherit; the default is AP214.
#[test]
fn a_cross_format_conversion_writes_the_catalog_default() {
    let encoder = StepCodec::default();
    let default = default_target(Encoder::targets(&encoder)).expect("the catalog has a default");
    assert_eq!(default, "step:ap214");

    let mut ir = unit_cube();
    ir.source = Some(SourceMeta {
        format: "rhino".to_owned(),
        dialect: Some(cadmpeg_core::dialect::DialectId::pinned("rhino:archive-50")),
        ..SourceMeta::default()
    });
    let plan = encoder
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(default),
        )
        .expect("the catalog default writes");

    assert_eq!(target_of(&plan), Some("step:ap214".to_owned()));
    assert!(written_text(plan).contains("AUTOMOTIVE_DESIGN"));
}

/// With nothing STEP to inherit — no source at all, or one of another format —
/// the constructor schema is the target. This is the only path that reads it,
/// and the command line never takes it: it builds `Inherit` for a STEP source
/// alone.
#[test]
fn nothing_to_inherit_falls_to_the_constructor_schema() {
    let encoder = StepCodec {
        options: StepWriteOptions {
            schema: StepSchema::Ap242Edition1,
            ..StepWriteOptions::default()
        },
    };

    let sourceless = unit_cube();
    let plan = inherit(&encoder, &sourceless).expect("a sourceless document takes the constructor");
    assert_eq!(target_of(&plan), Some("step:ap242-e1".to_owned()));

    let mut foreign = unit_cube();
    foreign.source = Some(SourceMeta {
        format: "iges".to_owned(),
        dialect: Some(cadmpeg_core::dialect::DialectId::pinned(
            "iges:5.3-fixed-ascii",
        )),
        ..SourceMeta::default()
    });
    let plan = inherit(&encoder, &foreign).expect("a foreign source takes the constructor");
    assert_eq!(target_of(&plan), Some("step:ap242-e1".to_owned()));
}

/// The catalog is exactly the six schemas the writer emits, in both directions,
/// and each is reachable by its alias as well as its id. The read-side identity
/// rows — `step:ap242`, `step:unknown`, and the three alternate encodings — are
/// deliberately absent: no request makes this writer produce them.
#[test]
fn the_catalog_is_the_schemas_the_writer_emits() {
    let encoder = StepCodec::default();
    let targets = Encoder::targets(&encoder);
    assert_eq!(targets.len(), StepSchema::ALL.len());
    for schema in StepSchema::ALL {
        let target = find_target(targets, schema.target())
            .unwrap_or_else(|| panic!("{schema:?} has no catalog row"));
        assert!(!target.aliases.is_empty(), "{schema:?}");
        for alias in target.aliases {
            assert_eq!(
                find_target(targets, alias).map(|row| row.id),
                Some(target.id)
            );
        }
    }
    for target in targets {
        assert!(
            StepSchema::ALL
                .into_iter()
                .any(|schema| schema.target() == target.id),
            "catalog row {} names no schema",
            target.id
        );
    }
    for id in [
        "step:ap242",
        "step:unknown",
        "step:part28-xml",
        "step:ap242-bo-model-xml",
        "step:part26-hdf5",
    ] {
        assert!(find_target(targets, id).is_none(), "{id}");
    }
}

/// An explicit target this writer does not produce is refused by `plan` itself,
/// with the catalog in the message.
///
/// The check runs before any synthesis, so an empty document is enough: what is
/// under test is that the request reaches the encoder at all. A `plan` that
/// dropped the guard would emit AP214 and report success for a schema nobody
/// asked for.
#[test]
fn plan_refuses_an_explicit_target_outside_the_catalog() {
    let ir = CadIr::empty(Units::default());
    let encoder = StepCodec::default();
    let error = Encoder::plan(
        &encoder,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("step:nonesuch"),
    )
    .err()
    .expect("an id outside the catalog is refused");

    let (format, requested, available) = refusal(&error);
    assert_eq!(format, "step");
    assert_eq!(requested, "step:nonesuch");
    for target in Encoder::targets(&encoder) {
        assert!(available.contains(target.id), "{available}");
    }
}
