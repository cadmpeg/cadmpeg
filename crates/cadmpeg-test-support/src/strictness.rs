// SPDX-License-Identifier: Apache-2.0
//! Every loss code of a codec has one name that `Strictness.codes` holds.

use std::collections::BTreeMap;

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::strictness::{code_name, Strictness};

/// Asserts that the name `code_name` gives each note's code spells its local
/// code, that a `Strictness` naming it refuses the note at every depth, and
/// that no two codes share a name.
///
/// # Panics
///
/// Panics on the first code that breaks one of these.
pub fn assert_named_by_strictness(notes: impl IntoIterator<Item = LossNote>) {
    let mut names: BTreeMap<String, (String, String)> = BTreeMap::new();
    for note in notes {
        let name = code_name(&note.code);
        let identity = (
            note.code.namespace().to_owned(),
            note.code.local_code().to_owned(),
        );
        assert!(
            name.ends_with(note.code.local_code()),
            "{name} does not spell {}",
            note.code
        );
        let named = Strictness::default().with_code(&name);
        assert!(
            named.refuses(&note, false) && named.refuses(&note, true),
            "naming {name} does not refuse {}",
            note.code
        );
        if let Some(previous) = names.insert(name.clone(), identity.clone()) {
            assert_eq!(previous, identity, "two codes share the name {name}");
        }
    }
}

/// The codec error of a write planned under the default strictness, which
/// refuses nothing.
///
/// # Panics
///
/// Panics on a strict refusal.
#[must_use]
pub fn codec_error(error: cadmpeg_ir::codec::write::EncodeError) -> cadmpeg_core::CodecError {
    match error {
        cadmpeg_ir::codec::write::EncodeError::Codec(error) => error,
        error => panic!("the default strictness refused a write: {error}"),
    }
}
