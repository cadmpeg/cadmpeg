// SPDX-License-Identifier: Apache-2.0
//! The saved document-tree expansion marks the view presentations it names.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::presentation::PresentationStateKind;
use std::io::Cursor;

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;

fn decode(expand: &str) -> cadmpeg_ir::DecodeResult {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="App::DocumentObjectGroup" name="Body"/>
 <Object type="App::DocumentObjectGroup" name="Pad"/>
 <Object type="App::DocumentObjectGroup" name="Sketch"/>
</Objects>
<ObjectData Count="3">
 <Object name="Body"><Properties Count="0"/></Object>
 <Object name="Pad"><Properties Count="0"/></Object>
 <Object name="Sketch"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let gui = format!(
        r#"<Document SchemaVersion="1"><ViewProviderData Count="3"><ViewProvider name="Body"><Properties Count="0"/></ViewProvider><ViewProvider name="Pad"><Properties Count="0"/></ViewProvider><ViewProvider name="Sketch"><Properties Count="0"/></ViewProvider></ViewProviderData>{expand}<Camera settings=""/></Document>"#
    );
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_slice()),
                ("GuiDocument.xml", gui.as_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect("GUI document decodes")
}

fn expanded(result: &cadmpeg_ir::DecodeResult, name: &str) -> Option<bool> {
    let suffix = format!("#{name}");
    result
        .ir()
        .model
        .view_presentations
        .iter()
        .find(|view| {
            view.native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with(&suffix))
        })
        .unwrap_or_else(|| panic!("view presentation for {name}"))
        .expanded
}

fn native_expand_states(result: &cadmpeg_ir::DecodeResult) -> usize {
    result
        .ir()
        .model
        .presentation_documents
        .iter()
        .flat_map(cadmpeg_ir::PresentationDocument::states)
        .filter(
            |state| matches!(&state.kind, PresentationStateKind::Native(kind) if kind == "Expand"),
        )
        .count()
}

fn unresolved_gui_losses(result: &cadmpeg_ir::DecodeResult) -> Vec<String> {
    let code = crate::loss::FreecadLossCode::SourceGuiMetadataUnresolved
        .note(String::new())
        .code;
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code)
        .map(|loss| loss.message.clone())
        .collect()
}

#[test]
fn nested_expand_items_mark_their_view_presentations_expanded() {
    let result = decode(
        r#"<Expand count="1"><Expand name="Body" count="1"><Expand name="Pad"/></Expand></Expand>"#,
    );
    assert_eq!(expanded(&result, "Body"), Some(true));
    assert_eq!(expanded(&result, "Pad"), Some(true));
    assert_eq!(expanded(&result, "Sketch"), None);
    assert_eq!(native_expand_states(&result), 0);
    assert!(unresolved_gui_losses(&result).is_empty());
}

#[test]
fn an_empty_expand_marks_nothing_and_emits_no_state() {
    let result = decode("<Expand/>");
    for name in ["Body", "Pad", "Sketch"] {
        assert_eq!(expanded(&result, name), None, "{name}");
    }
    assert_eq!(native_expand_states(&result), 0);
}

#[test]
fn an_expand_name_without_a_view_provider_charges_a_loss() {
    let result = decode(r#"<Expand count="1"><Expand name="Ghost"/></Expand>"#);
    for name in ["Body", "Pad", "Sketch"] {
        assert_eq!(expanded(&result, name), None, "{name}");
    }
    let losses = unresolved_gui_losses(&result);
    assert!(
        losses.iter().any(|message| message.contains("Ghost")),
        "{losses:?}"
    );
}
