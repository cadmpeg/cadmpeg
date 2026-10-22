// SPDX-License-Identifier: Apache-2.0
//! GUI admission uses the early DOM and the selected original member.

use super::*;

#[test]
fn selected_gui_dom_supplies_early_complete_inventory_but_missing_dom_does_not() {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='0'/></Document>";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", gui),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (scan, _) = crate::container::scan_for_decode(&ctx, root).unwrap();
    assert!(matches!(
        prepare(&ctx, root, &scan, None).unwrap(),
        Admission::Pending
    ));
    let source = crate::gui::source::parse(&ctx, scan.data["GuiDocument.xml"]).unwrap();
    let Admission::Complete(inventory) = prepare(&ctx, root, &scan, Some(&source)).unwrap() else {
        panic!("known XML and stored ZIP grammars have a complete actual inventory");
    };
    let extent = ctx.capture_part_extent(source.view()).unwrap();
    assert!(!inventory.0[extent.stream()].framing.is_empty());
    assert!(inventory.is_complete(&ctx).unwrap());
    crate::test_support::test_archive::assert_only_document_file_version_capture(&ctx, &scan);
}

#[test]
fn unsupported_or_unreadable_gui_stays_pending_before_legacy_evidence_decision() {
    for gui in [
        "<Document><ViewProviderData><Vendor><ViewProvider/></Vendor></ViewProviderData></Document>",
        "<Document><ViewProviderData><ViewProvider><Properties><Property type='App::PropertyFloatList'><FloatList file='values'/></Property></Properties></ViewProvider></ViewProviderData></Document>",
        "<Document><ViewProviderData></Document>",
    ] {
        let bytes = crate::test_support::test_archive::archive_entries(&[
            ("Document.xml", b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>"),
            ("GuiDocument.xml", gui.as_bytes()),
        ]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let (scan, _) = crate::container::scan_for_decode(&ctx, root).unwrap();
        let source = crate::gui::source::parse(&ctx, scan.data["GuiDocument.xml"]).unwrap();
        assert!(matches!(
            prepare(&ctx, root, &scan, Some(&source)).unwrap(),
            Admission::Pending
        ), "{gui}");
    }
}

#[test]
fn equal_xml_in_another_actual_member_cannot_replace_selected_gui_source() {
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='0'/></Document>";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>"),
        ("GuiDocument.xml", gui),
        ("other.xml", gui),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (scan, _) = crate::container::scan_for_decode(&ctx, root).unwrap();
    let source = crate::gui::source::parse(&ctx, scan.data["other.xml"]).unwrap();
    assert!(matches!(
        prepare(&ctx, root, &scan, Some(&source)),
        Err(CodecError::Malformed(_))
    ));
}
