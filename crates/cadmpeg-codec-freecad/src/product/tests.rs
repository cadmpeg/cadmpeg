// SPDX-License-Identifier: Apache-2.0
//! Product-structure transfer unit tests.

#![allow(unused_imports)]

use crate::native;
use crate::product::{product_cycle_nodes, product_kind, product_record_index};
use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::collections::HashSet;
use std::io::Cursor;

#[test]
pub(crate) fn recovers_product_prototypes_occurrences_and_placements() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="App::Part" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="Prototype" id="2"/>
 <Object type="App::Link" name="Occurrence" id="3"/>
 <Object type="Part::Feature" name="ElementA" id="4"/>
 <Object type="Part::Feature" name="ElementB" id="5"/>
 <Object type="App::Part" name="Outer" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="Assembly"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Occurrence"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Prototype"><Properties Count="3"><Property name="Label" type="App::PropertyString"><String value="Drive gear"/></Property><Property name="Description" type="App::PropertyString"><String value="Hardened drive gear"/></Property><Property name="PartNumber" type="App::PropertyString"><String value="GEAR-42"/></Property></Properties></Object>
 <Object name="Occurrence"><Properties Count="14">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype" count="1"><Sub value="Face1"/></XLink></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="PlacementList"/></Property>
  <Property name="ScaleList" type="App::PropertyVectorList"><VectorList file="ScaleList"/></Property>
  <Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="3" valueZ="4"/></Property>
  <Property name="VisibilityList" type="App::PropertyBoolList"><BoolList value="01"/></Property>
  <Property name="ElementList" type="App::PropertyLinkList"><LinkList count="2"><Link value="ElementA"/><Link value="ElementB"/></LinkList></Property>
  <Property name="LinkClaimChild" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="LinkCopyOnChange" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="LinkCopyOnChangeSource" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkCopyOnChangeGroup" type="App::PropertyLink"><Link value="Assembly"/></Property>
  <Property name="LinkCopyOnChangeTouched" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="ElementA"><Properties Count="0"/></Object>
 <Object name="ElementB"><Properties Count="0"/></Object>
 <Object name="Outer"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Assembly"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="100" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let mut placements = 2_u32.to_le_bytes().to_vec();
    for value in [
        1.0_f64, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 4.0, 5.0, 6.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        placements.extend_from_slice(&value.to_le_bytes());
    }
    let mut scales = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f64, 1.0, 1.0, 2.0, 2.0, 2.0] {
        scales.extend_from_slice(&value.to_le_bytes());
    }
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("PlacementList", &placements),
        ("ScaleList", &scales),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("product structure");
    let nodes = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .expect("product nodes");
    assert_eq!(nodes.len(), 3);
    let assembly = nodes
        .iter()
        .find(|node| node.object.ends_with("Assembly"))
        .expect("assembly part");
    let occurrence = nodes
        .iter()
        .find(|node| node.kind == "occurrence")
        .expect("occurrence");
    assert_eq!(assembly.members, vec![occurrence.object.clone()]);
    assert_eq!(
        occurrence.prototype.as_deref(),
        Some("fcstd:native:object#Prototype")
    );
    assert_eq!(occurrence.local_transform.expect("placement")[0][3], 4.0);
    assert_eq!(occurrence.element_count, Some(2));
    assert_eq!(occurrence.link_transform, Some(true));
    assert_eq!(occurrence.element_transforms.len(), 2);
    assert_eq!(occurrence.element_transforms[1][0][3], 4.0);
    assert_eq!(occurrence.element_scales, vec![[1.0; 3], [2.0; 3]]);
    assert_eq!(result.ir().model.product_definitions.len(), 5);
    let component = result
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|component| {
            component
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("Assembly"))
        })
        .expect("neutral assembly component");
    let assembly_occurrence = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.as_deref() == component.native_ref.as_deref())
        .expect("assembly occurrence");
    let link_occurrences = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("Occurrence"))
        })
        .collect::<Vec<_>>();
    assert_eq!(assembly_occurrence.transform.rows[0][3], 10.0);
    assert_eq!(link_occurrences.len(), 2);
    assert_eq!(link_occurrences[0].ordinal, 0);
    assert_eq!(link_occurrences[0].transform.rows[0][3], 5.0);
    assert_eq!(link_occurrences[1].transform.rows[0][3], 8.0);
    let graph = cadmpeg_ir::AssemblyGraph::new(&result.ir().model.occurrences)
        .expect("valid assembly graph");
    assert_eq!(
        graph
            .resolved_transform(&link_occurrences[0].id)
            .unwrap()
            .rows[0][3],
        115.0
    );
    assert_eq!(
        graph
            .resolved_transform(&link_occurrences[1].id)
            .unwrap()
            .rows[0][3],
        118.0
    );
    assert_eq!(link_occurrences[0].scale, [2.0, 3.0, 4.0]);
    assert_eq!(link_occurrences[1].scale, [4.0, 6.0, 8.0]);
    assert_eq!(link_occurrences[0].linked_subelements, ["Face1"]);
    assert_eq!(link_occurrences[0].visible, None);
    assert_eq!(link_occurrences[1].visible, None);
    assert!(link_occurrences[0].element_component.is_some());
    assert_eq!(link_occurrences[0].claim_child, Some(true));
    assert_eq!(
        link_occurrences[0].copy_on_change,
        Some(cadmpeg_ir::CopyOnChangePolicy::Owned)
    );
    assert!(link_occurrences[0].copy_on_change_source.is_some());
    assert!(link_occurrences[0].copy_on_change_group.is_some());
    assert_eq!(link_occurrences[0].copy_on_change_touched, Some(true));
    assert!(matches!(
        &link_occurrences[0].prototype,
        cadmpeg_ir::PrototypeReference::Local { definition }
            if definition.0.contains("Prototype")
    ));
    let prototype = result
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|component| component.source_name.as_deref() == Some("Prototype"))
        .expect("prototype component identity");
    assert_eq!(prototype.label.as_deref(), Some("Drive gear"));
    assert_eq!(
        prototype.description.as_deref(),
        Some("Hardened drive gear")
    );
    assert_eq!(prototype.part_number.as_deref(), Some("GEAR-42"));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
    let mut corrupted = result.ir().clone();
    corrupted.model.occurrences[0].prototype = cadmpeg_ir::PrototypeReference::Local {
        definition: cadmpeg_ir::ids::ProductDefinitionId("fcstd:model:component#missing".into()),
    };
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid occurrence reference")));
}

#[test]
fn selects_the_active_link_placement_carrier() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Feature" name="Prototype" id="1"/>
 <Object type="App::Link" name="Propagating" id="2"/>
 <Object type="App::Link" name="LocalOnly" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Propagating"><Properties Count="4">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="20" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
 <Object name="LocalOnly"><Properties Count="4">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="3" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="30" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("active link placement carrier");
    let nodes = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .expect("product nodes");
    let x = |name: &str| {
        nodes
            .iter()
            .find(|node| node.object.ends_with(name))
            .and_then(|node| node.local_transform)
            .map(|matrix| matrix[0][3])
            .expect("link placement")
    };
    assert_eq!(x("Propagating"), 2.0);
    assert_eq!(x("LocalOnly"), 30.0);
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn accepts_axis_angle_placement_values() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="Prototype" id="1"/>
 <Object type="App::Link" name="Occurrence" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="2">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="3" Pz="4" A="1.5707963267948966" Ox="0" Oy="0" Oz="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("axis-angle placement");
    let nodes = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .expect("product nodes");
    let occurrence = nodes
        .iter()
        .find(|node| node.object.ends_with("Occurrence"))
        .expect("occurrence");
    let matrix = occurrence.local_transform.expect("placement");
    assert_eq!(matrix[0][3], 2.0);
    assert_eq!(matrix[1][3], 3.0);
    assert_eq!(matrix[2][3], 4.0);
    assert!((matrix[0][0]).abs() < f64::EPSILON * 16.0);
    assert!((matrix[0][1] + 1.0).abs() < f64::EPSILON * 16.0);
    assert!((matrix[1][0] - 1.0).abs() < f64::EPSILON * 16.0);
    assert!((matrix[1][1]).abs() < f64::EPSILON * 16.0);
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn rejects_ambiguous_link_placement_without_policy() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="Prototype" id="1"/>
 <Object type="App::Link" name="Occurrence" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="3">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="20" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("ambiguous placement carriers");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn rejects_ambiguous_link_prototype_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Feature" name="First" id="1"/>
 <Object type="Part::Feature" name="Second" id="2"/>
 <Object type="App::Link" name="Occurrence" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="1">
  <Property name="LinkedObject" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("multiple linked-object carriers");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn rejects_duplicate_product_carriers() {
    for document in [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Feature" name="First" id="1"/>
 <Object type="Part::Feature" name="Second" id="2"/>
 <Object type="App::Link" name="Occurrence" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="2">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="First"/></Property>
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Second"/></Property>
 </Properties></Object>
</ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="Prototype" id="1"/>
 <Object type="App::Link" name="Occurrence" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="4">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="20" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="Prototype" id="1"/>
 <Object type="App::Link" name="Occurrence" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="2">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/><PropertyPlacement Px="20" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#,
    ] {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect_err("duplicate product carrier");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn rejects_invalid_product_placement_values() {
    for placement in [
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="NaN"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="0"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" A="1" Ox="0" Oy="0"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyString"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Occurrence"/></Objects>
<ObjectData Count="2"><Object name="Prototype"><Properties Count="0"/></Object><Object name="Occurrence"><Properties Count="2">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
{placement}
</Properties></Object></ObjectData></Document>"#
        );
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn rejects_overlapping_product_membership_for_neutral_projection() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="App::Part" name="First"/><Object type="App::Part" name="Second"/><Object type="Part::Feature" name="Member"/></Objects>
<ObjectData Count="3">
 <Object name="First"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property></Properties></Object>
 <Object name="Second"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property></Properties></Object>
 <Object name="Member"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[("Document.xml", document)])),
            &DecodeOptions::default(),
        )
        .expect_err("overlapping product membership");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn product_runtime_dispatch_requires_exact_registered_types() {
    for (runtime_type, expected) in [
        ("Assembly::AssemblyObject", Some("part")),
        ("Assembly::AssemblyLink", Some("part")),
        ("App::Part", Some("part")),
        ("App::DocumentObjectGroup", Some("group")),
        ("App::LinkGroup", Some("link_group")),
        ("App::Link", Some("occurrence")),
        ("App::LinkElement", Some("occurrence")),
    ] {
        assert_eq!(product_kind(runtime_type), expected, "{runtime_type}");
    }
    for runtime_type in [
        "Vendor::AssemblyObject",
        "Vendor::LinkGroup",
        "App::LinkPython",
        "App::DocumentObjectGroupPython",
        "Assembly::AssemblyObjectExtension",
    ] {
        assert_eq!(product_kind(runtime_type), None, "{runtime_type}");
    }
}

#[test]
fn rejects_wrong_runtime_type_for_copy_on_change_policy() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Occurrence"/></Objects>
<ObjectData Count="2">
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="2">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkCopyOnChange" type="App::PropertyInteger"><Integer value="2"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("wrong copy-on-change carrier type");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn rejects_wrong_runtime_types_for_named_product_carriers() {
    for carrier in [
        r#"<Property name="LinkedObject" type="App::PropertyLinkList"><LinkList count="1"><Link value="Prototype"/></LinkList></Property>"#,
        r#"<Property name="LinkTransform" type="App::PropertyInteger"><Integer value="1"/></Property>"#,
        r#"<Property name="ElementCount" type="App::PropertyInteger"><Integer value="1"/></Property>"#,
        r#"<Property name="ScaleVector" type="App::PropertyFloat"><Float value="1"/></Property>"#,
        r#"<Property name="VisibilityList" type="App::PropertyString"><String value="1"/></Property>"#,
        r#"<Property name="ElementList" type="App::PropertyLink"><Link value="Prototype"/></Property>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Occurrence"/></Objects>
<ObjectData Count="2"><Object name="Prototype"><Properties Count="0"/></Object><Object name="Occurrence"><Properties Count="2">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
{carrier}
</Properties></Object></ObjectData></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect_err("wrong named product carrier type");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn product_record_identity_rejects_duplicates() {
    let records = [node("A", &[]), node("A", &[])];
    assert!(matches!(
        product_record_index(&records),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn rejects_populated_link_arrays_when_element_count_is_zero() {
    let decode = |array_property: &str, entry: Option<(&str, Vec<u8>)>| {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Occurrence"/></Objects>
<ObjectData Count="2"><Object name="Prototype"><Properties Count="0"/></Object><Object name="Occurrence"><Properties Count="3">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="0"/></Property>
{array_property}
</Properties></Object></ObjectData></Document>"#
        );
        let bytes = entry.map_or_else(
            || archive(&document),
            |(name, content)| {
                archive_entries(&[("Document.xml", document.as_bytes()), (name, &content)])
            },
        );
        FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default())
    };

    let mut placement = 1_u32.to_le_bytes().to_vec();
    for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        placement.extend_from_slice(&value.to_le_bytes());
    }
    let mut scale = 1_u32.to_le_bytes().to_vec();
    for value in [1.0_f64, 1.0, 1.0] {
        scale.extend_from_slice(&value.to_le_bytes());
    }
    let cases = [
        (
            r#"<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="PlacementList"/></Property>"#,
            Some(("PlacementList", placement)),
        ),
        (
            r#"<Property name="ScaleList" type="App::PropertyVectorList"><VectorList file="ScaleList"/></Property>"#,
            Some(("ScaleList", scale)),
        ),
        (
            r#"<Property name="VisibilityList" type="App::PropertyBoolList"><BoolList value="1"/></Property>"#,
            None,
        ),
        (
            r#"<Property name="ElementList" type="App::PropertyLinkList"><LinkList count="1"><Link value="Prototype"/></LinkList></Property>"#,
            None,
        ),
    ];

    for (array_property, entry) in cases {
        assert!(matches!(
            decode(array_property, entry),
            Err(cadmpeg_core::CodecError::Malformed(message))
                if message.contains("inconsistent link-array counts")
        ));
    }

    let zero = decode(
        r#"<Property name="VisibilityList" type="App::PropertyBoolList"><BoolList value=""/></Property>"#,
        None,
    )
    .expect("empty zero-count link array");
    assert_eq!(
        zero.ir()
            .model
            .occurrences
            .iter()
            .filter(|occurrence| {
                occurrence
                    .native_ref
                    .as_deref()
                    .is_some_and(|native_ref| native_ref.ends_with("Occurrence"))
            })
            .count(),
        1
    );
}

#[test]
fn composes_nested_link_prototype_placements_once_by_policy() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="App::Part" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="Prototype" id="2"/>
 <Object type="App::Link" name="Inner" id="3"/>
 <Object type="App::Link" name="Outer" id="4"/>
 <Object type="App::Link" name="Override" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Assembly"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Outer"/><Link value="Override"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Prototype"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Inner"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="3" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Outer"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Inner"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Override"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Inner"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="false"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("nested links");
    let occurrence = |name: &str| {
        result
            .ir()
            .model
            .occurrences
            .iter()
            .find(|occurrence| {
                occurrence
                    .native_ref
                    .as_deref()
                    .is_some_and(|id| id.ends_with(name))
                    && !occurrence.id.0.ends_with(":container")
            })
            .expect("named occurrence")
    };
    assert_eq!(occurrence("Inner").prototype_transform.rows[0][3], 5.0);
    assert_eq!(occurrence("Outer").prototype_transform.rows[0][3], 8.0);
    assert_eq!(occurrence("Override").prototype_transform.rows[0][3], 0.0);
    let graph = cadmpeg_ir::AssemblyGraph::new(&result.ir().model.occurrences)
        .expect("valid assembly graph");
    assert_eq!(
        graph
            .resolved_transform(&occurrence("Inner").id)
            .unwrap()
            .rows[0][3],
        8.0
    );
    assert_eq!(
        graph
            .resolved_transform(&occurrence("Outer").id)
            .unwrap()
            .rows[0][3],
        20.0
    );
    assert_eq!(
        graph
            .resolved_transform(&occurrence("Override").id)
            .unwrap()
            .rows[0][3],
        14.0
    );
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn transfers_external_product_paths_and_targets() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1">
 <Object type="App::Link" name="ByPath" id="1"/>
</Objects>
<ObjectData Count="1">
 <Object name="ByPath"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="parts/widget.FCStd" name="Body"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("external products");
    assert_eq!(result.ir().model.occurrences.len(), 1);
    let by_path = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("ByPath"))
        })
        .expect("path occurrence");
    let cadmpeg_ir::PrototypeReference::External { document, object } = &by_path.prototype else {
        panic!("path prototype is external");
    };
    assert_eq!(document.path.as_deref(), Some("parts/widget.FCStd"));
    assert_eq!(document.document_id, None);
    assert_eq!(object.as_deref(), Some("Body"));
    assert_eq!(
        document.resolution,
        cadmpeg_ir::ExternalResolution::Unresolved
    );

    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
    let mut corrupted = result.ir().clone();
    let cadmpeg_ir::PrototypeReference::External { document, .. } =
        &mut corrupted.model.occurrences[0].prototype
    else {
        panic!("external prototype");
    };
    document.path = Some("also-a-path.FCStd".into());
    document.document_id = Some("also-an-id".into());
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid occurrence reference")));
}

#[test]
fn rejects_non_schema_link_carrier_aliases() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Link" name="Link"/></Objects>
<ObjectData Count="1"><Object name="Link"><Properties Count="1">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink document="document-7" name="Gear"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("unsupported XLink document alias");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("unsupported link carrier document")
    ));
}

#[test]
fn restores_shadowed_link_subelement_name() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Owner"/><Object type="Part::Feature" name="Target"/></Objects>
<ObjectData Count="2"><Object name="Owner"><Properties Count="1">
<Property name="Support" type="App::PropertyLinkSub"><LinkSub value="Target" count="1"><Sub value="Face1" shadowed="Face7"/></LinkSub></Property>
</Properties></Object><Object name="Target"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("shadowed subelement");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let support = properties
        .iter()
        .find(|property| property.name == "Support")
        .expect("support");
    assert_eq!(support.links[0].subelements, ["Face7"]);
}

#[test]
fn rejects_conflicting_xlink_subelement_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Link" name="Link"/></Objects>
<ObjectData Count="1"><Object name="Link"><Properties Count="1">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink name="Gear" sub="Face1" count="1"><Sub value="Face2"/></XLink></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("conflicting XLink subelement carriers");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("both sub and count carriers")
    ));
}

fn node(object: &str, members: &[&str]) -> native::ProductNodeRecord {
    native::ProductNodeRecord {
        id: format!("product:{object}"),
        object: object.into(),
        kind: "group".into(),
        members: members.iter().map(|member| (*member).into()).collect(),
        prototype: None,
        external_document: None,
        external_document_attribute: None,
        local_transform: None,
        placement_property: None,
        element_count: None,
        link_transform: None,
        element_transforms: Vec::new(),
        element_scales: Vec::new(),
        linked_subelements: Vec::new(),
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        scale: None,
        element_visibility: Vec::new(),
        element_objects: Vec::new(),
    }
}

#[test]
fn reconvergent_product_graph_is_not_a_cycle() {
    let records = [node("A", &["C", "B"]), node("B", &["C"]), node("C", &[])];
    let nodes = records
        .iter()
        .map(|record| (record.object.as_str(), record))
        .collect();
    assert!(product_cycle_nodes(&nodes).is_empty());
}

#[test]
fn product_cycle_marks_only_the_strongly_connected_component() {
    let records = [node("A", &["B"]), node("B", &["C"]), node("C", &["B"])];
    let nodes = records
        .iter()
        .map(|record| (record.object.as_str(), record))
        .collect();
    assert_eq!(product_cycle_nodes(&nodes), HashSet::from(["B", "C"]));
}
