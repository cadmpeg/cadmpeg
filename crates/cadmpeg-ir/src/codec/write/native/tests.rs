// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{NativeDisposition, NativeDispositions};
use crate::codec::write::target::{DialectFree, TargetRequest};
use crate::codec::write::{ArenaCoverage, EncodeInput, Encoder, EncoderBackend, ExportBody};
use crate::codec::FormatId;
use crate::report::loss::LossNote;
use crate::CadIr;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

struct NativeEncoder(NativeDispositions);

impl EncoderBackend for NativeEncoder {
    const FORMAT: FormatId = FormatId::new("test");
    const NAME: &'static str = "test";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            self.0.clone(),
        ))
    }
}

/// A document with `f3d` arenas `history` (two records, given out of order)
/// and `unknowns`, and an `nx` arena `records`.
fn document() -> CadIr {
    let record = |id: &str| {
        crate::NativeRecord::new(
            crate::ids::Identity::new(id).unwrap(),
            serde_json::Map::default(),
        )
        .unwrap()
    };
    let arena = |name: &str| NonBlankString::new(name).unwrap();
    let mut ir = CadIr::empty();
    let f3d = ir
        .native
        .namespace_mut(crate::codec_id!("f3d"))
        .arenas_mut();
    f3d.insert(
        arena("history"),
        vec![record("f3d:test:history#1"), record("f3d:test:history#0")],
    );
    f3d.insert(arena("unknowns"), vec![record("f3d:test:unknown#0")]);
    ir.native
        .namespace_mut(crate::codec_id!("nx"))
        .arenas_mut()
        .insert(arena("records"), vec![record("nx:test:record#0")]);
    ir.finalize();
    ir
}

fn omissions(native: NativeDispositions) -> Vec<LossNote> {
    let ir = document();
    let plan = NativeEncoder(native)
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    plan.report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "export/native.records-omitted")
        .cloned()
        .collect()
}

fn messages(native: NativeDispositions) -> Vec<String> {
    omissions(native)
        .into_iter()
        .map(|loss| loss.message)
        .collect()
}

/// The default reason of [`NativeDispositions::dropped`] in these tests.
const UNWRITTEN: &str = "which the test writer does not write";

#[test]
fn an_undeclared_namespace_is_charged_with_every_id_sorted_and_the_default_reason() {
    assert_eq!(
        messages(NativeDispositions::dropped(UNWRITTEN)),
        [
            "the test write omits 2 native record(s) of `f3d` arena `history` which the test \
             writer does not write: f3d:test:history#0, f3d:test:history#1",
            "the test write omits 1 native record(s) of `nx` arena `records` which the test \
             writer does not write: nx:test:record#0",
        ]
    );
}

#[test]
fn given_up_stubs_are_never_charged() {
    let mut native = NativeDispositions::dropped(UNWRITTEN);
    native.arena("f3d", "unknowns", NativeDisposition::Dropped);
    assert!(messages(native)
        .iter()
        .all(|message| !message.contains("unknown")));
}

#[test]
fn a_written_or_regenerated_namespace_is_not_charged() {
    for disposition in [NativeDisposition::Written, NativeDisposition::Regenerated] {
        let mut native = NativeDispositions::dropped(UNWRITTEN);
        native.namespace("f3d", disposition);
        assert_eq!(
            messages(native),
            [
                "the test write omits 1 native record(s) of `nx` arena `records` which the test \
              writer does not write: nx:test:record#0"
            ]
        );
    }
    assert!(omissions(NativeDispositions::written()).is_empty());
}

#[test]
fn a_record_drop_wins_over_its_arena_and_namespace() {
    let mut native = NativeDispositions::written();
    native
        .arena("f3d", "history", NativeDisposition::Regenerated)
        .drop_records("f3d", "that name an omitted body", ["f3d:test:history#1"]);
    assert_eq!(
        messages(native.clone()),
        [
            "the test write omits 1 native record(s) of `f3d` arena `history` that name an \
          omitted body: f3d:test:history#1"
        ]
    );
    assert_eq!(
        native.disposition("f3d", "history", "f3d:test:history#0"),
        NativeDisposition::Regenerated
    );
    assert_eq!(
        native.disposition("nx", "records", "nx:test:record#0"),
        NativeDisposition::Written
    );
}

#[test]
fn an_arena_with_drops_of_several_reasons_is_one_loss_that_states_each() {
    let mut native = NativeDispositions::written();
    native
        .drop_records("f3d", "that name an omitted body", ["f3d:test:history#1"])
        .drop_arena("f3d", "history", "which the test format cannot hold")
        .drop_arena("nx", "records", "which the test format cannot hold");
    assert_eq!(
        messages(native),
        [
            "the test write omits 2 native record(s) of `f3d` arena `history`: \
             f3d:test:history#0, f3d:test:history#1; that name an omitted body: \
             f3d:test:history#1; which the test format cannot hold: f3d:test:history#0",
            "the test write omits 1 native record(s) of `nx` arena `records` which the test \
             format cannot hold: nx:test:record#0",
        ]
    );
}

#[test]
fn a_drop_declared_without_a_reason_takes_the_default_reason() {
    let mut native = NativeDispositions::dropped(UNWRITTEN);
    native
        .namespace("f3d", NativeDisposition::Written)
        .arena("f3d", "history", NativeDisposition::Dropped)
        .namespace("nx", NativeDisposition::Written);
    assert_eq!(
        messages(native),
        [
            "the test write omits 2 native record(s) of `f3d` arena `history` which the test \
          writer does not write: f3d:test:history#0, f3d:test:history#1"
        ]
    );
}
