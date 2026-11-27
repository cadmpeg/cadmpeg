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
}

#[test]
fn a_link_with_an_unreadable_prototype_is_an_unresolved_place_at_its_placement() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Occurrence"/></Objects>
<ObjectData Count="2">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="2">
  <Property name="LinkedObject" type="App::PropertyLink"><Link value="Prototype"/></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="4" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = decode_document(document);
    assert!(gives_up(&result, "Occurrence", "LinkedObject"));
    assert!(product_node(&result, "Occurrence").is_none());
    let feature = result
        .ir()
        .design
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("fcstd:native:object#Occurrence"))
        .unwrap_or_else(|| panic!("{:?}", result.ir().design.features));
    assert_eq!(feature.name.as_deref(), Some("Occurrence"));
    assert_eq!(
        feature.definition.operation,
        cadmpeg_ir::design::feature::FeatureOperation::Unresolved {
            family: cadmpeg_ir::design::feature::OperationFamily::Place
        }
    );
    let placement = feature.placement.as_ref().expect("the link's placement");
    let Some(cadmpeg_ir::design::construction::PostStep::Fixed(transform)) =
        placement.after.first()
    else {
        panic!("{placement:?}");
    };
    let rows = transform.affine_rows();
    assert_eq!([rows[0][3], rows[1][3], rows[2][3]], [0.0, 4.0, 0.0]);
    assert_valid_document(result.ir());
}
