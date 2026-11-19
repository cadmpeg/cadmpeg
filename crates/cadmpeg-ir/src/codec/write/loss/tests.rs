// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ArenaClass;

use super::{carried_source_losses, TargetLossCode, WriterLossCode};
use crate::codec::write::{Consumption, PatchConsumption, WritePath};
use crate::report::Severity;
use crate::source_fidelity::{RetainedSourceRecord, SourceFidelity};

#[test]
fn each_shared_writer_code_has_its_section_3_2_spelling_and_severity_and_no_strict_floor() {
    let codes = [
        (
            WriterLossCode::SourceCarried,
            "source.carried",
            Severity::Info,
        ),
        (
            WriterLossCode::SourceUnchecked,
            "source.unchecked",
            Severity::Warning,
        ),
        (
            WriterLossCode::SourceDropped,
            "source.dropped",
            Severity::Warning,
        ),
        (WriterLossCode::EditDropped, "edit.dropped", Severity::Error),
        (
            WriterLossCode::EditUnverified,
            "edit.unverified",
            Severity::Warning,
        ),
        (
            WriterLossCode::LinkUnwritten,
            "link.unwritten",
            Severity::Error,
        ),
        (
            WriterLossCode::RecordInvalid,
            "record.invalid",
            Severity::Error,
        ),
        (
            WriterLossCode::ValueUnwritable,
            "value.unwritable",
            Severity::Warning,
        ),
        (
            WriterLossCode::DefinitionPlaced,
            "definition.placed",
            Severity::Warning,
        ),
    ];
    for (code, spelling, severity) in codes {
        let note = code.note("message");
        assert_eq!(note.code.namespace(), "export");
        assert_eq!(note.code.local_code(), spelling);
        assert_eq!(note.severity, severity, "{spelling}");
        assert_eq!(note.code.strict_floor(), None, "{spelling}");
    }
    let note = TargetLossCode::RequestDisplaced.note(crate::loss_namespace!("f3d"), "message");
    assert_eq!(note.code.namespace(), "f3d");
    assert_eq!(note.code.local_code(), "target.request-displaced");
    assert_eq!(note.severity, Severity::Warning);
    assert_eq!(note.code.strict_floor(), None);
}

fn marked_fidelity() -> SourceFidelity {
    let mut fidelity = SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(b"source"));
    fidelity
        .insert_retained_record(
            "test:image#0".to_owned().try_into().unwrap(),
            RetainedSourceRecord::whole("", b"image".to_vec())
                .with_untyped([ArenaClass::Design, ArenaClass::Model]),
        )
        .unwrap();
    fidelity
        .insert_retained_record(
            "test:typed#0".to_owned().try_into().unwrap(),
            RetainedSourceRecord::whole("", b"typed".to_vec()),
        )
        .unwrap();
    fidelity
}

#[test]
fn a_replay_carries_and_a_patch_also_leaves_unchecked_the_untyped_records_it_holds() {
    let fidelity = marked_fidelity();
    let codes = |path: &WritePath| {
        carried_source_losses(Some(&fidelity), path)
            .into_iter()
            .map(|note| {
                assert!(
                    note.message.contains("test:image#0 (model, design)")
                        && !note.message.contains("test:typed#0"),
                    "{}",
                    note.message
                );
                note.code.local_code().to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(&WritePath::VerbatimReplay), ["source.carried"]);
    assert_eq!(
        codes(&WritePath::Patched {
            consumption: PatchConsumption::Replayed
        }),
        ["source.carried", "source.unchecked"]
    );
    assert!(codes(&WritePath::Synthesized {
        consumption: Consumption::NotConsumed
    })
    .is_empty());
}

#[test]
fn a_file_with_no_untyped_record_carries_nothing_to_report() {
    let mut fidelity = marked_fidelity();
    fidelity.remove_retained_record("test:image#0");
    assert!(carried_source_losses(Some(&fidelity), &WritePath::VerbatimReplay).is_empty());
    assert!(carried_source_losses(None, &WritePath::VerbatimReplay).is_empty());
}
