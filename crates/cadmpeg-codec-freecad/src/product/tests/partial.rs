// SPDX-License-Identifier: Apache-2.0
//! Product records the decoder states as far as it reads them (§3.3).

use super::{decode_document, gives_up, occurrence, owned_suffixes, product_node};
use crate::test_support::test_archive::assert_valid_document;
use cadmpeg_test_support::assembly;

#[test]
fn a_link_group_shows_an_object_a_part_holds_again_at_its_own_placement() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="App::Part" name="Holder"/><Object type="App::LinkGroup" name="Folder"/><Object type="Part::Feature" name="Member"/></Objects>
<ObjectData Count="3">
 <Object name="Holder"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="4" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Folder"><Properties Count="2"><Property name="ElementList" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="7" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Member"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()));
    assert_eq!(
        owned_suffixes(result.ir(), "#Holder:definition"),
        ["#Member:container"]
    );
    assert_eq!(
        owned_suffixes(result.ir(), "#Folder:definition"),
        ["#Member:container/Folder"]
    );
    for (suffix, world) in [
        ("#Member:container", [1.0, 4.0, 0.0]),
        ("#Member:container/Folder", [1.0, 0.0, 7.0]),
    ] {
        let rows = assembly::resolved_transform(result.ir(), &occurrence(&result, suffix).id)
            .expect("a placed member")
            .affine_rows();
        assert_eq!([rows[0][3], rows[1][3], rows[2][3]], world, "{suffix}");
    }
    assert_valid_document(result.ir());
}

#[test]
fn a_part_with_an_unreadable_member_list_keeps_its_placement_and_occurrence() {
    for group in [
        r#"<Property name="Group" type="App::PropertyLink"><Link value="Member"/></Property>"#,
        r#"<Property name="Group" type="App::PropertyLinkList"><LinkList count="x"><Link value="Member"/></LinkList></Property>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="App::Part" name="Holder"/><Object type="Part::Feature" name="Member"/></Objects>
<ObjectData Count="2">
 <Object name="Holder"><Properties Count="2">{group}<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="4" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Member"><Properties Count="0"/></Object>
</ObjectData></Document>"#
        );
        let result = decode_document(&document);
        assert!(gives_up(&result, "Holder", "Group"), "{group}");
        assert!(
            result.report().losses.iter().any(|loss| loss
                .message
                .contains("placed without the placement of fcstd:native:object#Holder")),
            "{:?}",
            result.report().losses
        );
        assert!(product_node(&result, "Holder").is_some_and(|node| node.members().is_empty()));
        let rows = occurrence(&result, "#Holder:container")
            .transform
            .expect("the Part's placement")
            .affine_rows();
        assert_eq!([rows[0][3], rows[1][3], rows[2][3]], [0.0, 4.0, 0.0]);
        assert_valid_document(result.ir());
    }
}

#[test]
fn a_placement_give_up_names_the_occurrences_left_with_no_transform() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="App::Part" name="Holder"/><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Follower"/></Objects>
<ObjectData Count="3">
 <Object name="Holder"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Prototype"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="NaN"/></Property></Properties></Object>
 <Object name="Prototype"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="NaN"/></Property></Properties></Object>
 <Object name="Follower"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    for (object, occurrences) in [
        (
            "Holder",
            "occurrence fcstd:model:occurrence#Holder:container states",
        ),
        (
            "Prototype",
            "occurrences fcstd:model:occurrence#Follower:instance, \
             fcstd:model:occurrence#Prototype:container state",
        ),
    ] {
        let head = format!("record fcstd:native:property#{object}:Placement given up");
        let losses = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.message.starts_with(&head))
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), 1, "{object}: {:?}", result.report().losses);
        assert!(
            losses[0]
                .message
                .contains(&format!("; {occurrences} no transform")),
            "{}",
            losses[0].message
        );
    }
    assert_eq!(occurrence(&result, "#Follower:instance").transform, None);
    assert_valid_document(result.ir());
}

#[test]
fn a_nested_link_cycle_gives_up_only_the_links_in_it() {
    let link = |name: &str, target: &str| {
        format!(
            r#"<Object name="{name}"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="{target}"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>"#
        )
    };
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object type="App::Link" name="Entry"/><Object type="App::Link" name="First"/><Object type="App::Link" name="Second"/><Object type="App::Link" name="Plain"/></Objects>
<ObjectData Count="4">
 {}
 {}
 {}
 <Object name="Plain"><Properties Count="1"><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="3" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#,
        link("Entry", "First"),
        link("First", "Second"),
        link("Second", "First"),
    );
    let result = decode_document(&document);
    let given_up = |name: &str| {
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.message
                    .starts_with(&format!("record fcstd:native:object#{name} given up"))
            })
            .count()
    };
    assert_eq!(given_up("First"), 1, "{:?}", result.report().losses);
    assert_eq!(given_up("Second"), 1);
    assert_eq!(given_up("Entry"), 0);
    for name in ["Entry", "First", "Second"] {
        assert_eq!(
            occurrence(&result, &format!("#{name}:instance")).transform,
            None,
            "{name}"
        );
    }
    let rows = occurrence(&result, "#Plain:instance")
        .transform
        .expect("a link outside the cycle")
        .affine_rows();
    assert_eq!(rows[0][3], 3.0);
    assert_valid_document(result.ir());
    let findings = crate::test_support::validate_native(result.ir());
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_link_cycle_is_given_up_whatever_the_placements_on_it_hold() {
    let link = |name: &str, target: &str, q3: &str| {
        format!(
            r#"<Object name="{name}"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="{target}"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="{q3}"/></Property></Properties></Object>"#
        )
    };
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="App::Link" name="First"/><Object type="App::Link" name="Second"/></Objects>
<ObjectData Count="2">
 {}
 {}
</ObjectData></Document>"#,
        link("First", "Second", "1"),
        link("Second", "First", "NaN"),
    );
    let result = decode_document(&document);
    let given_up = |name: &str| {
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.message
                    .starts_with(&format!("record fcstd:native:object#{name} given up"))
            })
            .count()
    };
    assert_eq!(given_up("First"), 1, "{:?}", result.report().losses);
    assert_eq!(given_up("Second"), 1);
    assert_valid_document(result.ir());
}

#[test]
fn native_validation_keeps_membership_cycles_when_it_omits_followed_edges() {
    let link = |name: &str, target: &str| {
        format!(
            r#"<Object name="{name}"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="{target}"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="{name}"/></LinkList></Property></Properties></Object>"#
        )
    };
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="App::Link" name="First"/><Object type="App::Link" name="Second"/></Objects>
<ObjectData Count="2">{}{}</ObjectData></Document>"#,
        link("First", "Second"),
        link("Second", "First"),
    );
    let result = decode_document(&document);
    assert_eq!(
        crate::test_support::validate_native(result.ir())
            .iter()
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>(),
        [
            "fcstd:native:product#First participates in a product-structure cycle",
            "fcstd:native:product#Second participates in a product-structure cycle",
        ]
    );
    let given_up = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.as_str())
        .filter(|message| {
            message.contains("its chain of links that follow their linked objects returns to it")
        })
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 2);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "feature.link-placement-unresolved")
            .count(),
        0
    );
}

#[test]
fn an_object_a_body_holds_has_no_occurrence_of_a_part_that_lists_it_later() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="PartDesign::Body" name="Body"/><Object type="App::Part" name="Holder"/><Object type="Part::Feature" name="Member"/></Objects>
<ObjectData Count="3">
 <Object name="Body"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Holder"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="9" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Member"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="3" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    assert!(!result
        .ir()
        .model
        .occurrences
        .iter()
        .any(|occurrence| occurrence.native_ref.as_deref() == Some("fcstd:native:object#Member")));
    assert_eq!(
        owned_suffixes(result.ir(), "#Holder:definition"),
        Vec::<&str>::new()
    );
    let messages = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .starts_with("record fcstd:native:property#Holder:Group given up")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let [message] = messages[..] else {
        panic!("{:?}", result.report().losses)
    };
    assert!(
        message.ends_with(
            "it lists fcstd:native:object#Member, which fcstd:native:object#Body holds; \
             FreeCAD keeps each in the group that holds it, and this list places none of them; dropped=[]"
        ),
        "{message}"
    );
    assert_valid_document(result.ir());
}

#[test]
fn a_body_held_link_does_not_take_an_ordinal_from_the_part_that_lists_it() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object type="PartDesign::Body" name="Body"/><Object type="App::Part" name="Part"/><Object type="App::Link" name="Held"/><Object type="Part::Feature" name="Member"/></Objects>
<ObjectData Count="4">
 <Object name="Body"><Properties Count="1"><Property name="Model" type="App::PropertyLinkList"><LinkList count="1"><Link value="Held"/></LinkList></Property></Properties></Object>
 <Object name="Part"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Held"/><Link value="Member"/></LinkList></Property></Properties></Object>
 <Object name="Held"><Properties Count="0"/></Object>
 <Object name="Member"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    assert_eq!(
        owned_suffixes(result.ir(), "#Part:definition"),
        ["#Member:container"]
    );
    assert_eq!(
        result
            .ir()
            .model
            .occurrences
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        [
            "fcstd:model:occurrence#Member:container",
            "fcstd:model:occurrence#Part:container",
        ]
    );
    assert_valid_document(result.ir());
}

#[test]
fn a_link_with_an_unreadable_prototype_keeps_its_elements_under_it() {
    let element = |name: &str, x: u32| {
        format!(
            r#"<Object name="{name}"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="{x}" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>"#
        )
    };
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Array"/><Object type="App::LinkElement" name="First"/><Object type="App::LinkElement" name="Second"/></Objects>
<ObjectData Count="4">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Array"><Properties Count="5">
  <Property name="LinkedObject" type="App::PropertyLink"><Link value="Prototype"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>
  <Property name="ShowElement" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="ElementList" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
 </Properties></Object>
 {}
 {}
</ObjectData></Document>"#,
        element("First", 1),
        element("Second", 2),
    );
    let result = decode_document(&document);
    let messages = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .starts_with("record fcstd:native:property#Array:LinkedObject given up")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1, "{:?}", result.report().losses);
    assert!(
        messages[0].ends_with(
            "; the occurrences of fcstd:native:object#Array place an unresolved definition; dropped=[]"
        ),
        "{}",
        messages[0]
    );
    let node = product_node(&result, "Array").expect("the link keeps its record");
    assert_eq!(node.prototype(), None);
    assert_eq!(
        result
            .ir()
            .model
            .occurrences
            .iter()
            .map(|occurrence| {
                let rows = occurrence
                    .transform
                    .expect("a placed element")
                    .affine_rows();
                (
                    &occurrence.id.as_str()
                        [occurrence.id.as_str().rfind('#').unwrap_or_default()..],
                    [rows[0][3], rows[1][3], rows[2][3]],
                )
            })
            .collect::<Vec<_>>(),
        [
            ("#Array:0", [11.0, 0.0, 0.0]),
            ("#Array:1", [12.0, 0.0, 0.0])
        ]
    );
    assert_valid_document(result.ir());
}

#[test]
fn a_link_with_an_unreadable_element_list_is_given_up_with_its_elements() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5"><Object type="Part::Feature" name="Prototype" id="1"/><Object type="App::Link" name="Array" id="2"/><Object type="App::LinkElement" name="Listed" id="3"/><Object type="App::LinkElement" name="Owned" id="4"/><Object type="App::Part" name="Holder" id="5"/></Objects>
<ObjectData Count="5">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Array"><Properties Count="3">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="4" Pz="0" Q0="0" Q1="0" Q2="0" Q3="NaN"/></Property>
  <Property name="ElementList" type="App::PropertyLink"><Link value="Listed"/></Property>
 </Properties></Object>
 <Object name="Listed"><Properties Count="0"/></Object>
 <Object name="Owned"><Properties Count="1"><Property name="_LinkOwner" type="App::PropertyInteger"><Integer value="2"/></Property></Properties></Object>
 <Object name="Holder"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Array"/></LinkList></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    for name in ["Array", "Listed", "Owned"] {
        assert!(product_node(&result, name).is_none(), "{name}");
    }
    assert_eq!(
        result
            .ir()
            .model
            .occurrences
            .iter()
            .map(|occurrence| occurrence.id.as_str())
            .collect::<Vec<_>>(),
        ["fcstd:model:occurrence#Holder:container"]
    );
    let given_up = |name: &str| {
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.message.starts_with(&format!(
                    "record fcstd:native:property#Array:{name} given up"
                ))
            })
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>()
    };
    let [element_list] = given_up("ElementList")[..] else {
        panic!("{:?}", result.report().losses)
    };
    assert!(
        element_list.ends_with(
            "; dropped=[fcstd:native:product#Array, fcstd:native:product#Listed, \
             fcstd:native:product#Owned]"
        ),
        "{element_list}"
    );
    assert_eq!(given_up("Placement").len(), 1);
    let feature = result
        .ir()
        .design
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("fcstd:native:object#Array"))
        .unwrap_or_else(|| panic!("{:?}", result.ir().design.features));
    assert_eq!(
        feature.definition.operation,
        cadmpeg_ir::design::feature::FeatureOperation::Unresolved {
            family: cadmpeg_ir::design::feature::OperationFamily::Place
        }
    );
    assert_eq!(feature.placement, None);
    assert_valid_document(result.ir());
}

#[test]
fn a_miscounted_array_list_gives_up_its_elements_and_keeps_a_dependent_link() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="App::Link" name="Array"/><Object type="App::LinkElement" name="Element"/><Object type="App::Link" name="Dependent"/></Objects>
<ObjectData Count="3">
 <Object name="Array"><Properties Count="1"><Property name="ElementList" type="App::PropertyLinkList"><LinkList count="2"><Link value="Element"/></LinkList></Property></Properties></Object>
 <Object name="Element"><Properties Count="0"/></Object>
 <Object name="Dependent"><Properties Count="2">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Element"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    let occurrence = occurrence(&result, "#Dependent:instance");
    assert_eq!(occurrence.transform, None);
    assert_eq!(
        result
            .ir()
            .model
            .occurrences
            .iter()
            .map(|row| &row.id)
            .collect::<Vec<_>>(),
        [&occurrence.id]
    );
    let prototype = result
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|row| row.id == occurrence.prototype)
        .expect("unresolved prototype of dependent link");
    assert_eq!(
        prototype.storage,
        cadmpeg_ir::products::Storage::External(cadmpeg_ir::products::ExternalDocument::default())
    );
    assert_eq!(prototype.native_ref, None);
    let message = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.message
                .starts_with("record fcstd:native:property#Array:ElementList given up")
        })
        .expect("array membership give-up");
    assert!(
        message.message.ends_with(concat!(
            "occurrence fcstd:model:occurrence#Dependent:instance states no transform; ",
            "dropped=[fcstd:native:product#Array, fcstd:native:product#Element]"
        )),
        "{message:?}"
    );
    assert_valid_document(result.ir());
}

#[test]
fn a_link_with_an_unreadable_copy_on_change_source_keeps_its_occurrence() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Copy"/></Objects>
<ObjectData Count="2">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Copy"><Properties Count="4">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="4" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="LinkCopyOnChange" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="LinkCopyOnChangeSource" type="App::PropertyLink"><Link value="Prototype"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    let messages = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .starts_with("record fcstd:native:property#Copy:LinkCopyOnChangeSource given up")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1, "{:?}", result.report().losses);
    assert!(
        messages[0].ends_with(
            "; the copy-on-change state of fcstd:native:object#Copy is not stated; dropped=[]"
        ),
        "{}",
        messages[0]
    );
    let node = product_node(&result, "Copy").expect("the link keeps its record");
    assert_eq!(node.prototype(), Some("fcstd:native:object#Prototype"));
    assert!(node.copy_on_change_source().is_none());
    let rows = occurrence(&result, "#Copy:instance")
        .transform
        .expect("the link's placement")
        .affine_rows();
    assert_eq!([rows[0][3], rows[1][3], rows[2][3]], [0.0, 4.0, 0.0]);
    assert_valid_document(result.ir());
}

#[test]
fn a_member_list_give_up_names_eight_objects_and_counts_the_rest() {
    let names = (0..10).map(|index| format!("M{index}")).collect::<Vec<_>>();
    let links = names
        .iter()
        .map(|name| [r#"<Link value=""#, name, r#""/>"#].concat())
        .collect::<String>();
    let declarations = names
        .iter()
        .map(|name| [r#"<Object type="Part::Feature" name=""#, name, r#""/>"#].concat())
        .collect::<String>();
    let data = names
        .iter()
        .map(|name| {
            [
                r#"<Object name=""#,
                name,
                r#""><Properties Count="0"/></Object>"#,
            ]
            .concat()
        })
        .collect::<String>();
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="11"><Object type="App::Part" name="Holder"/>{declarations}</Objects>
<ObjectData Count="11">
 <Object name="Holder"><Properties Count="1"><Property name="Group" type="App::PropertyLinkListHidden"><LinkList count="10">{links}</LinkList></Property></Properties></Object>
 {data}
</ObjectData></Document>"#
    );
    let result = decode_document(&document);
    let named = names[..8]
        .iter()
        .map(|name| format!("fcstd:native:object#{name}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.ends_with(&format!(
                "; the objects it lists ({named} and 2 more) are placed without the placement of \
             fcstd:native:object#Holder; dropped=[]"
            ))),
        "{:?}",
        result.report().losses
    );
    assert_valid_document(result.ir());
}
