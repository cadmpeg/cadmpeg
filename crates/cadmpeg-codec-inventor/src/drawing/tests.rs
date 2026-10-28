// SPDX-License-Identifier: Apache-2.0

use super::{Family, Inventory};

#[test]
fn drawing_registry_types_have_exact_family_names() {
    for name in [
        "DlDocDcSegmentType",
        "DlBRxSegmentType",
        "DlDirectorySegmentType",
        "AppSegmentType",
        "DlSheetDcSegmentType",
        "DlSheetSmSegmentType",
        "DlSheetDlSegmentType",
    ] {
        assert_eq!(
            Family::from_type_name(name).map(Family::type_name),
            Some(name)
        );
    }
    for name in [
        "DlSheetDc",
        "dlsheetdcsegmenttype",
        "FBAttributeSegment",
        "PmDcSegmentType",
    ] {
        assert!(Family::from_type_name(name).is_none());
    }
}

#[test]
fn unresolved_drawing_records_emit_the_pinned_native_only_loss() {
    let inventory = Inventory {
        segment_count: 7,
        record_count: 3,
        ..Inventory::default()
    };
    let losses = inventory.losses();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::DrawingNativeOnly
            .note("")
            .code
    );
    assert_eq!(losses[0].code.to_string(), "inventor/drawing.native-only");
    assert!(Inventory::default().losses().is_empty());
}

#[test]
fn refused_typed_drawing_records_emit_the_pinned_malformed_loss() {
    let inventory = Inventory {
        issues: vec![crate::native::StructuralIssueRecord {
            id: "inventor:drawing:record-issue#generated-0".into(),
            scope: "generated-0".into(),
            detail: "unsupported prefix".into(),
        }],
        ..Inventory::default()
    };
    let losses = inventory.losses();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::DrawingRecordMalformed
            .note("")
            .code
    );
    assert_eq!(
        losses[0].code.to_string(),
        "inventor/drawing.record-malformed"
    );
}
