// SPDX-License-Identifier: Apache-2.0
//! Resource admission for product property diagnostics.

use super::super::{
    bool_list, bool_property, copy_on_change_property, integer_property, link_list, parse_finite,
    require_root, scale_property, side_bytes, single_link, single_value, vector_property,
};
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml, ValueRecord};
use crate::test_support::assert_retained_refusal_at;
use std::collections::BTreeMap;

fn property(
    name: &str,
    type_name: &str,
    tags: &[&str],
    attributes: &[(&str, &str)],
    sides: &[&str],
    link_count: usize,
) -> PropertyRecord {
    PropertyRecord {
        id: format!("fcstd:native:property#Owner:{name}"),
        owner: "fcstd:native:object#Owner".into(),
        name: name.into(),
        type_name: type_name.into(),
        family: PropertyFamily::Unknown,
        status: None,
        body: PropertyBody::Persisted {
            values: tags.iter().enumerate().map(|(order, tag)| ValueRecord {
                tag: (*tag).into(),
                order,
                attributes: attributes.iter().map(|(key, value)| ((*key).into(), (*value).into())).collect(),
                text: None,
                raw_xml: format!("<{tag}/>")
            }).collect(),
            links: vec![None; link_count],
            side_entries: sides.iter().map(|side| (*side).into()).collect(),
            dynamic: None,
        },
        order: 0,
        xml: RetainedXml::from_text("<Property/>".into(), 0).expect("valid XML span"),
    }
}

macro_rules! refusal {
    ($name:ident, $operation:literal, $property:expr, |$ctx:ident, $item:ident| $decode:expr) => {
        #[test]
        fn $name() {
            let $item = $property;
            assert_retained_refusal_at(&[], $operation, |$ctx| $decode);
        }
    };
}

refusal!(multiple_side_entries_refuse_before_diagnostic, "fcstd product side entry count",
    property("PlacementList", "App::PropertyPlacementList", &["PlacementList"], &[], &["one", "two"], 0),
    |ctx, item| side_bytes(ctx, &item, "App::PropertyPlacementList", "PlacementList", &BTreeMap::new()));

refusal!(missing_side_entry_refuses_before_diagnostic, "fcstd product missing side entry",
    property("PlacementList", "App::PropertyPlacementList", &["PlacementList"], &[], &["missing"], 0),
    |ctx, item| side_bytes(ctx, &item, "App::PropertyPlacementList", "PlacementList", &BTreeMap::new()));

refusal!(link_target_count_refuses_before_diagnostic, "fcstd product link target count",
    property("LinkedObject", "App::PropertyXLink", &["XLink"], &[], &[], 0),
    |ctx, item| single_link(ctx, &item, "App::PropertyXLink", "XLink", "LinkedObject"));

refusal!(link_list_child_refuses_before_diagnostic, "fcstd product link list child",
    property("Group", "App::PropertyLinkList", &["LinkList", "Other"], &[], &[], 0),
    |ctx, item| link_list(ctx, &item, "App::PropertyLinkList", "Group"));

refusal!(runtime_type_refuses_before_diagnostic, "fcstd product runtime type",
    property("Group", "App::PropertyBool", &["LinkList"], &[], &[], 0),
    |ctx, item| require_root(ctx, &item, "App::PropertyLinkList", "Group", "LinkList"));

refusal!(root_value_refuses_before_diagnostic, "fcstd product root value",
    property("Group", "App::PropertyLinkList", &["Wrong"], &[], &[], 0),
    |ctx, item| require_root(ctx, &item, "App::PropertyLinkList", "Group", "LinkList"));

refusal!(value_count_refuses_before_diagnostic, "fcstd product value count",
    property("LinkTransform", "App::PropertyBool", &["Bool", "Other"], &[], &[], 0),
    |ctx, item| single_value(ctx, &item, "App::PropertyBool", "LinkTransform", "Bool"));

refusal!(missing_boolean_refuses_before_diagnostic, "fcstd product missing boolean",
    property("LinkTransform", "App::PropertyBool", &["Bool"], &[], &[], 0),
    |ctx, item| bool_property(ctx, &[&item], "LinkTransform"));

refusal!(invalid_boolean_refuses_before_diagnostic, "fcstd product invalid boolean",
    property("LinkTransform", "App::PropertyBool", &["Bool"], &[("value", "maybe")], &[], 0),
    |ctx, item| bool_property(ctx, &[&item], "LinkTransform"));

refusal!(missing_integer_refuses_before_diagnostic, "fcstd product missing integer",
    property("ElementCount", "App::PropertyIntegerConstraint", &["Integer"], &[], &[], 0),
    |ctx, item| integer_property(ctx, &[&item], "ElementCount"));

refusal!(invalid_integer_refuses_before_diagnostic, "fcstd product invalid integer",
    property("ElementCount", "App::PropertyIntegerConstraint", &["Integer"], &[("value", "many")], &[], 0),
    |ctx, item| integer_property(ctx, &[&item], "ElementCount"));

refusal!(missing_enumeration_refuses_before_diagnostic, "fcstd product missing enumeration",
    property("LinkCopyOnChange", "App::PropertyEnumeration", &["Integer"], &[], &[], 0),
    |ctx, item| copy_on_change_property(ctx, &[&item]));

refusal!(invalid_enumeration_refuses_before_diagnostic, "fcstd product invalid enumeration",
    property("LinkCopyOnChange", "App::PropertyEnumeration", &["Integer"], &[("value", "many")], &[], 0),
    |ctx, item| copy_on_change_property(ctx, &[&item]));

refusal!(missing_scale_refuses_before_diagnostic, "fcstd product missing scale",
    property("Scale", "App::PropertyFloat", &["Float"], &[], &[], 0),
    |ctx, item| scale_property(ctx, &[&item]));

refusal!(missing_scale_component_refuses_before_diagnostic, "fcstd product missing scale component",
    property("ScaleVector", "App::PropertyVector", &["PropertyVector"], &[], &[], 0),
    |ctx, item| vector_property(ctx, &item));

refusal!(invalid_finite_scale_refuses_before_diagnostic, "fcstd product invalid finite scale",
    property("Scale", "App::PropertyFloat", &["Float"], &[], &[], 0),
    |ctx, item| parse_finite(ctx, "NaN", &item, "Scale"));

refusal!(missing_visibility_list_refuses_before_diagnostic, "fcstd product missing visibility list",
    property("VisibilityList", "App::PropertyBoolList", &["BoolList"], &[], &[], 0),
    |ctx, item| bool_list(ctx, &[&item], "VisibilityList"));

refusal!(invalid_visibility_list_refuses_before_diagnostic, "fcstd product invalid visibility list",
    property("VisibilityList", "App::PropertyBoolList", &["BoolList"], &[("value", "2")], &[], 0),
    |ctx, item| bool_list(ctx, &[&item], "VisibilityList"));
