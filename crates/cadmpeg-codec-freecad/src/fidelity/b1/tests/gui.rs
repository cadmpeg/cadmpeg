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
    assert!(prepare(&ctx, root, &scan)
        .unwrap()
        .is_complete(&ctx)
        .unwrap());
    let source = crate::gui::source::parse(&ctx, scan.data["GuiDocument.xml"]).unwrap();
    let inventory = prepare(&ctx, root, &scan).unwrap();
    let extent = ctx
        .capture_part_extent(source.scanner(&ctx).unwrap().view())
        .unwrap();
    assert_eq!(
        inventory.0[extent.stream()].byte_len,
        u64::try_from(gui.len()).unwrap()
    );
    let framing: Vec<_> = source
        .scanner(&ctx)
        .unwrap()
        .elements()
        .iter()
        .flat_map(|element| element.framing())
        .map(|view| view.window())
        .collect();
    assert_eq!(
        framing,
        [
            b"<Document".as_slice(),
            b">",
            b"</Document>",
            b"<ViewProviderData",
            b"/>"
        ]
    );
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
        let inventory = prepare(&ctx, root, &scan).unwrap();
        assert!(inventory.is_complete(&ctx).unwrap(), "{gui}");
        assert_eq!(source.scanner(&ctx).unwrap().view().window(), gui.as_bytes());
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
    let inventory = prepare(&ctx, root, &scan).unwrap();
    let foreign = ctx
        .capture_part_extent(source.scanner(&ctx).unwrap().view())
        .unwrap();
    let selected = ctx
        .capture_part_extent(scan.data["GuiDocument.xml"])
        .unwrap();
    assert_ne!(foreign.stream(), selected.stream());
    assert_eq!(
        inventory.0[foreign.stream()].byte_len,
        u64::try_from(gui.len()).unwrap()
    );
    assert_eq!(
        inventory.0[selected.stream()].byte_len,
        u64::try_from(gui.len()).unwrap()
    );
}
