// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet value, cell, dimension, and address admission tests.

use super::{cell_address, merged_range, offset_cell_address, range_contains_address};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::spreadsheets::{CellAddress, SpreadsheetRange};
use std::collections::BTreeMap;

#[test]
fn ignores_nonpositive_spans_in_the_neutral_spreadsheet_projection() {
    for xml in [
        r#"<Cell address="A1" rowSpan="0" colSpan="2"/>"#,
        r#"<Cell address="A1" rowSpan="2" colSpan="-7"/>"#,
    ] {
        let document = roxmltree::Document::parse(xml).expect("cell XML");
        crate::test_support::with_service_context(&[], |ctx| {
            assert_eq!(merged_range(ctx, document.root_element()).unwrap(), None);
        });
    }
}

#[test]
fn detects_cells_covered_by_a_merged_range() {
    let range = SpreadsheetRange::new(
        CellAddress::parse("A1").expect("A1"),
        CellAddress::parse("I2").expect("I2"),
    )
    .expect("A1:I2");

    assert!(range_contains_address(&range, "B1"));
    assert!(range_contains_address(&range, "I2"));
    assert!(!range_contains_address(&range, "J1"));
    assert!(!range_contains_address(&range, "A3"));
}

#[test]
fn spreadsheet_column_label_holds_maximum_u32_index() {
    crate::test_support::with_service_context(&[], |ctx| {
        let address = offset_cell_address(ctx, "A1", 0, u32::MAX - 1)
            .expect("address admission")
            .expect("valid column offset");
        assert_eq!(
            cell_address(ctx, &address).expect("address admission"),
            Some((1, u32::MAX))
        );
    });
}

#[test]
fn design_spreadsheet_value_diagnostic_refuses_at_retained_limit() {
    let xml = roxmltree::Document::parse("<Property/>").expect("valid XML");
    crate::test_support::assert_retained_refusal_at(&[], "fcstd design diagnostic", |ctx| {
        super::direct_spreadsheet_value(ctx, &xml, "Cells", "spreadsheet-property")
    });
}

#[test]
fn design_spreadsheet_cell_properties_refuse_at_collection_limits() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Sheet".into(),
            "Sheet".into(),
        )
        .expect("object identity"),
        type_name: "Spreadsheet::Sheet".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "cells-property".into(),
        owner: object.id().clone(),
        name: "cells".into(),
        type_name: "Spreadsheet::PropertySheet".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Cells Count=\"1\"><Cell address=\"A1\" content=\"5\" alias=\"Length\"/></Cells></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    for operation in [
        "fcstd spreadsheet cell properties",
        "fcstd spreadsheet distinct parameter IDs",
        "fcstd spreadsheet distinct addresses",
    ] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::append_spreadsheet(ctx, &mut Vec::new(), &object, &[&property])
        });
    }
}

#[test]
fn design_spreadsheet_dimensions_refuse_at_distinct_collection_limits() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Sheet".into(),
            "Sheet".into(),
        )
        .expect("object identity"),
        type_name: "Spreadsheet::Sheet".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property =
        |id: &str, name: &str, type_name: &str, xml: &str| crate::native::PropertyRecord {
            id: id.into(),
            owner: object.id().clone(),
            name: name.into(),
            type_name: type_name.into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Transient,
            order: 0,
            xml: crate::native::RetainedXml::from_text(xml.into(), 0).expect("valid XML span"),
        };
    let cells = property(
        "cells",
        "cells",
        "Spreadsheet::PropertySheet",
        "<Property><Cells Count=\"0\"/></Property>",
    );
    let columns = property("columns", "columnWidths", "Spreadsheet::PropertyColumnWidths",
        "<Property><ColumnInfo Count=\"1\"><Column name=\"A\" width=\"120\"/></ColumnInfo></Property>");
    let rows = property(
        "rows",
        "rowHeights",
        "Spreadsheet::PropertyRowHeights",
        "<Property><RowInfo Count=\"1\"><Row name=\"2\" height=\"45\"/></RowInfo></Property>",
    );
    for operation in [
        "fcstd spreadsheet distinct column widths",
        "fcstd spreadsheet distinct row heights",
    ] {
        crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
            super::append_spreadsheet(ctx, &mut Vec::new(), &object, &[&cells, &columns, &rows])
        });
    }
}

#[test]
fn spreadsheet_cells_refuse_at_caller_limit() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Sheet".into(),
            "Sheet".into(),
        )
        .expect("object identity"),
        type_name: "Spreadsheet::Sheet".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "property".into(),
        owner: object.id().clone(),
        name: "cells".into(),
        type_name: "Spreadsheet::PropertySheet".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Cells Count=\"1\"><Cell address=\"A1\" content=\"5\"/></Cells></Property>"
                .into(),
            0,
        )
        .expect("valid XML span"),
    };
    crate::test_support::assert_collection_refusal_at(&[], "FreeCAD spreadsheet cells", |ctx| {
        super::append_spreadsheet(ctx, &mut Vec::new(), &object, &[&property])
    });
}

#[test]
fn spreadsheet_value_visits_exact_descendants_without_end_probe() {
    let document = roxmltree::Document::parse("<Property><Cells/></Property>")
        .expect("valid spreadsheet XML");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 27;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");

    let value = super::direct_spreadsheet_value(&ctx, &document, "Cells", "cells-property")
        .expect("the exact descendant visits fit the work cap");
    assert_eq!(value.tag_name().name(), "Cells");
}

#[test]
fn spreadsheet_cell_address_visits_only_exact_column_bytes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert_eq!(
        cell_address(&ctx, "A1").expect("cell address fits the exact work cap"),
        Some((1, 1))
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert_eq!(
        cell_address(&ctx, "1").expect("empty column scan fits the exact work cap"),
        None
    );
}
