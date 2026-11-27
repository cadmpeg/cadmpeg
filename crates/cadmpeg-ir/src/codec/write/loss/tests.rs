// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ArenaClass;

use super::{carried_source_losses, edit_unverified};
use crate::codec::write::{Consumption, PatchConsumption, WritePath};
use crate::report::Severity;
use crate::schema::EntityKind;
use crate::source_fidelity::{RetainedSourceRecord, SourceFidelity};

#[test]
fn the_carried_source_and_unverified_edit_codes_have_their_section_3_2_spelling_and_severity() {
    let mut notes = carried_source_losses(
        Some(&marked_fidelity()),
        &WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        },
    );
    notes.push(edit_unverified("message"));
    let codes: Vec<_> = notes
        .iter()
        .map(|note| {
            assert_eq!(note.code.namespace(), "export");
            assert_eq!(note.code.strict_floor(), None, "{}", note.code.local_code());
            (note.code.local_code(), note.severity)
        })
        .collect();
    assert_eq!(
        codes,
        [
            ("source.carried", Severity::Info),
            ("source.unchecked", Severity::Warning),
            ("edit.unverified", Severity::Warning),
        ]
    );
}

/// §3.2 states no strict floor for an `export:` code, whatever the category
/// of the content it names.
#[test]
fn no_export_code_pins_a_strict_floor() {
    use super::ExportLossCode as Code;
    let mut codes = vec![
        Code::NativeRecordsOmitted,
        Code::OutputEmpty,
        Code::FacePartial,
        Code::SolidPartial,
        Code::EvaluationMissing,
        Code::EvaluationStale,
        Code::ParameterFlattened,
        Code::ParameterRenamed,
        Code::RecordResidualUnavailable,
        Code::ValueUnwritable,
        Code::RuleInvalid,
        Code::SourceCarried,
        Code::SourceUnchecked,
        Code::EditUnverified,
        Code::ValueQuantized,
        Code::SourceDropped,
        Code::LinkUnwritten,
        Code::DefinitionPlaced,
        Code::FitRestated,
        Code::FitDropped,
        Code::GeneralToleranceRestated,
        Code::GeneralToleranceDropped,
        Code::FeatureLowered,
        Code::DependencyCycleBroken,
    ];
    for kind in EntityKind::ALL {
        codes.extend([
            Code::ArenaOmitted(kind),
            Code::EditDropped(kind),
            Code::RecordInvalid(kind),
        ]);
    }
    for code in codes {
        assert_eq!(
            code.note("message").code.strict_floor(),
            None,
            "{}",
            code.code()
        );
    }
}

fn marked_fidelity() -> SourceFidelity {
    let mut fidelity = SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(b"source"));
    fidelity
        .insert_retained_record(
            "test:source:image#0".to_owned().try_into().unwrap(),
            RetainedSourceRecord::whole("", b"image".to_vec())
                .with_untyped([ArenaClass::Design, ArenaClass::Model]),
        )
        .unwrap();
    fidelity
        .insert_retained_record(
            "test:source:typed#0".to_owned().try_into().unwrap(),
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
                    note.message.contains("test:source:image#0 (model, design)")
                        && !note.message.contains("test:source:typed#0"),
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
    fidelity.remove_retained_record("test:source:image#0");
    assert!(carried_source_losses(Some(&fidelity), &WritePath::VerbatimReplay).is_empty());
    assert!(carried_source_losses(None, &WritePath::VerbatimReplay).is_empty());
}
