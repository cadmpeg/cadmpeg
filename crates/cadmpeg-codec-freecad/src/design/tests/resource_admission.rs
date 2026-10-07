// SPDX-License-Identifier: Apache-2.0
//! Design feature and spreadsheet admission tests.

use cadmpeg_ir::topology::BodyKind;
use std::collections::BTreeMap;

#[test]
fn design_distinct_feature_dependencies_refuse_at_collection_limit() {
    let source = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Source".into(),
            "Source".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let dependent = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Dependent".into(),
            "Dependent".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::default(),
        dependencies: vec![source.id().clone()],
        dependency_allow_partial: None,
        order: 1,
        data: None,
    };
    crate::test_support::assert_collection_refusal_at(
        &[],
        "fcstd distinct feature dependencies",
        |ctx| {
            super::super::transfer(
                ctx,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &[source.clone(), dependent.clone()],
                &[],
                &[],
                &[],
                None,
            )
        },
    );
}

#[test]
fn design_distinct_feature_outputs_refuse_at_collection_limit() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Shape".into(),
            "Shape".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Shape:Shape".into(),
        owner: object.id().clone(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let payload = crate::brep::ShapePayloadRecord {
        id: "fcstd:native:shape-payload#Shape:Shape".into(),
        property: property.id.clone(),
        entry: "shape.brp".into(),
        payload: crate::brep::ShapePayload::Empty,
    };
    let body_id = cadmpeg_ir::ids::BodyId::mint(format!(
        "{}1",
        crate::native::model_id("body", &payload.id, "")
    ))
    .expect("valid body ID");
    crate::test_support::assert_collection_refusal_at(
        &[],
        "validate distinct decoded members",
        |ctx| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.model.bodies.push(cadmpeg_ir::topology::Body {
                id: body_id.clone(),
                kind: BodyKind::default(),
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            });
            super::super::transfer(
                ctx,
                &mut ir,
                std::slice::from_ref(&object),
                std::slice::from_ref(&property),
                std::slice::from_ref(&payload),
                &[],
                None,
            )
        },
    );
}

#[test]
fn design_census_missing_projection_diagnostic_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Shape".into(),
            "Shape".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    crate::test_support::assert_retained_refusal_at(&[], "fcstd design diagnostic", |ctx| {
        super::super::census(ctx, std::slice::from_ref(&object), &[])
    });
}

#[test]
fn design_boolean_scalar_keeps_case_insensitive_values_without_copy() {
    for (value, expected) in [("TrUe", true), ("FaLsE", false)] {
        let property = crate::native::PropertyRecord {
            id: "flag".into(),
            owner: "owner".into(),
            name: "Flag".into(),
            type_name: "App::PropertyBool".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Transient,
            order: 0,
            xml: crate::native::RetainedXml::from_text(
                format!("<Property><Bool value=\"{value}\"/></Property>"),
                0,
            )
            .expect("valid XML span"),
        };
        assert_eq!(
            super::super::bool_property(
                &cadmpeg_test_support::service_decode_context(),
                &[&property],
                "Flag"
            )
            .expect("admitted XML"),
            Some(expected)
        );
    }
}

#[test]
fn design_spreadsheet_value_diagnostic_refuses_at_retained_limit() {
    let xml = roxmltree::Document::parse("<Property/>").expect("valid XML");
    crate::test_support::assert_retained_refusal_at(&[], "fcstd design diagnostic", |ctx| {
        super::super::direct_spreadsheet_value(ctx, &xml, "Cells", "spreadsheet-property")
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
            super::super::append_spreadsheet(ctx, &mut Vec::new(), &object, &[&property])
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
            super::super::append_spreadsheet(
                ctx,
                &mut Vec::new(),
                &object,
                &[&cells, &columns, &rows],
            )
        });
    }
}

#[test]
fn design_boolean_scalar_propagates_tree_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let property = super::bool_property("owner", "Refine", true);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bool_property(&ctx, &[&property], "Refine").unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("tree admission must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "FreeCAD direct property XML tree");
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn design_duplicate_property_search_stops_at_second_match() {
    use cadmpeg_core::decode::ResourceDimension;
    let property = super::scalar_property("owner", "Refine", "1");
    let after = "test after duplicate property search";
    let run = |count| {
        let properties: Vec<_> = (0..count).map(|_| &property).collect();
        crate::test_support::refusal_at(ResourceDimension::WorkUnits, &[], after, |ctx| {
            assert!(matches!(
                super::super::unique_named_property(ctx, &properties, "Refine")?,
                super::super::NamedProperty::Duplicate
            ));
            ctx.copy_retained_text("probe", after)
        })
    };
    let small = run(2);
    let large = run(1000);
    let (
        cadmpeg_core::CodecError::ResourceLimit(small),
        cadmpeg_core::CodecError::ResourceLimit(large),
    ) = (small, large)
    else {
        panic!("work refusal required");
    };
    assert_eq!(small.used, large.used);
    assert_eq!(small.additional, large.additional);
}

#[test]
fn design_operation_parameters_require_numeric_scalar_carriers() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Feature".into(),
            "Feature".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let numeric = super::scalar_property(object.id(), "Length", "3");
    crate::test_support::with_service_context(&[], |ctx| {
        let mut parameters = Vec::new();
        for (type_name, tag) in [
            ("App::PropertyString", "String"),
            ("App::PropertyBool", "Bool"),
        ] {
            let mut invalid = numeric.clone();
            invalid.type_name = type_name.into();
            invalid.xml = crate::native::RetainedXml::from_text(
                format!("<Property><{tag} value=\"3\"/></Property>"),
                0,
            )
            .expect("valid XML");
            super::super::append_operation_parameters(ctx, &mut parameters, &object, &[&invalid])
                .expect("parameter projection");
            assert!(parameters.is_empty());
        }
        super::super::append_operation_parameters(ctx, &mut parameters, &object, &[&numeric])
            .expect("numeric parameter");
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].expression, "3");
    });
}
