#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, ExtrusionDirectionSource, FeatureDefinition,
    InnerWireTaper, Length, PathRef, RevolveExtent, ShellJoin, ShellMode, SweepOrientation,
    SweepTransformation, SweepTransition, Termination,
};
use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind as Kind;
use cadmpeg_ir::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

pub(crate) use crate::test_support::*;

#[test]
fn recovers_product_prototypes_occurrences_and_placements() {
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
  <Property name="LinkedObject" type="App::PropertyXLinkSub"><XLink file="" name="Prototype" count="1"><Sub value="Face1"/></XLink></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="PlacementList"/></Property>
  <Property name="ScaleList" type="App::PropertyVectorList"><VectorList file="ScaleList"/></Property>
  <Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="3" valueZ="4"/></Property>
  <Property name="VisibilityList" type="App::PropertyBoolList"><BoolList count="2"><Bool value="true"/><Bool value="false"/></BoolList></Property>
  <Property name="ElementList" type="App::PropertyLinkList"><LinkList count="2"><Link value="ElementA"/><Link value="ElementB"/></LinkList></Property>
  <Property name="LinkClaimChild" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="LinkCopyOnChange" type="App::PropertyEnumeration"><Integer value="2"/><CustomEnumList count="4"><Enum value="Disabled"/><Enum value="Enabled"/><Enum value="Owned"/><Enum value="Tracking"/></CustomEnumList></Property>
  <Property name="LinkCopyOnChangeSource" type="App::PropertyLink"><Link value="Prototype"/></Property>
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
    assert_eq!(link_occurrences[0].visible, Some(true));
    assert_eq!(link_occurrences[1].visible, Some(false));
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
fn retains_overlapping_native_product_membership_with_one_neutral_parent() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="App::Part" name="First"/><Object type="App::Part" name="Second"/><Object type="Part::Feature" name="Member"/></Objects>
<ObjectData Count="3">
 <Object name="First"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property></Properties></Object>
 <Object name="Second"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property></Properties></Object>
 <Object name="Member"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[("Document.xml", document)])),
            &DecodeOptions::default(),
        )
        .expect("overlapping product membership");
    let nodes = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .expect("product nodes");
    let member_id = "fcstd:native:object#Member";
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.members.iter().any(|member| member == member_id))
            .count(),
        2
    );
    let member = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.as_deref() == Some(member_id))
        .expect("member occurrence");
    assert!(matches!(
        &member.parent,
        cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence }
            if occurrence.0.contains("First")
    ));
    assert_valid_document(result.ir());
}

#[test]
fn rejects_populated_link_arrays_when_element_count_is_zero() {
    let decode = |array_property: &str, entry: Option<(&str, Vec<u8>)>| {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Occurrence"/></Objects>
<ObjectData Count="2"><Object name="Prototype"><Properties Count="0"/></Object><Object name="Occurrence"><Properties Count="3">
<Property name="LinkedObject" type="App::PropertyLink"><Link value="Prototype"/></Property>
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
            r#"<Property name="VisibilityList" type="App::PropertyBoolList"><BoolList count="1"><Bool value="true"/></BoolList></Property>"#,
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
        r#"<Property name="VisibilityList" type="App::PropertyBoolList"><BoolList count="0"/></Property>"#,
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
fn recovers_assembly_joint_operands_frames_and_state() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="App::FeaturePython" name="Joint" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Assembly"><Properties Count="0"/></Object>
 <Object name="Joint"><Properties Count="14">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Revolute"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="2"><Sub value="A.Face1"/><Sub value="A.Edge2"/></XLink></Property>
  <Property name="Reference2" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="1"><Sub value="B.Edge3"/></XLink></Property>
  <Property name="Placement1" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement2" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="15"/></Property>
  <Property name="AngleMin" type="App::PropertyAngle"><Float value="-30"/></Property>
  <Property name="AngleMax" type="App::PropertyAngle"><Float value="45"/></Property>
  <Property name="EnableAngleMin" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="EnableAngleMax" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Detach1" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Offset1" type="App::PropertyPlacement"><PropertyPlacement Px="0.5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Offset2" type="App::PropertyPlacement"><PropertyPlacement Px="1.5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("joint");
    let joints = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::JointRecord>("joints")
        .expect("joints");
    assert_eq!(joints.len(), 1);
    assert_eq!(joints[0].kind, "Revolute");
    assert_eq!(joints[0].references.len(), 2);
    assert_eq!(
        joints[0].references[0].object.as_deref(),
        Some("fcstd:native:object#Assembly")
    );
    assert_eq!(joints[0].references[0].subelements, ["A.Face1", "A.Edge2"]);
    assert_eq!(joints[0].placements[1][0][3], 2.0);
    assert_eq!(
        joints[0].parameters.get("Suppressed").map(String::as_str),
        Some("true")
    );
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
    let joint = &result.ir().model.assembly_joints[0];
    assert_eq!(joint.kind, cadmpeg_ir::JointKind::Revolute);
    assert_eq!(joint.operands.len(), 2);
    assert!(joint
        .operands
        .iter()
        .all(|operand| operand.occurrence.is_some()));
    assert_eq!(joint.frames[1].rows[0][3], 2.0);
    assert_eq!(joint.offset_frames.len(), 2);
    assert_eq!(joint.offset_frames[0].rows[0][3], 0.5);
    assert_eq!(joint.offset_frames[1].rows[0][3], 1.5);
    assert!(joint.suppressed);
    assert_eq!(joint.detached, [true, false]);
    assert!((joint.angle.expect("angle") - 15_f64.to_radians()).abs() < 1e-12);
    let limits = joint.angular_limits.as_ref().expect("angular limits");
    assert!((limits.minimum.expect("minimum") - (-30_f64).to_radians()).abs() < 1e-12);
    assert!((limits.maximum.expect("maximum") - 45_f64.to_radians()).abs() < 1e-12);
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
    let mut corrupted = result.ir().clone();
    let limits = corrupted.model.assembly_joints[0]
        .angular_limits
        .as_mut()
        .expect("limits");
    limits.minimum = Some(2.0);
    limits.maximum = Some(1.0);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid assembly joint")));
    let mut corrupted = result.ir().clone();
    corrupted.model.assembly_joints[0].operands[0].external_document =
        Some(cadmpeg_ir::ExternalDocumentReference {
            path: Some("external.FCStd".into()),
            document_id: None,
            resolution: cadmpeg_ir::ExternalResolution::Unresolved,
        });
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid assembly joint operands")));
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
 <Object name="Inner"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyLink"><Link value="Prototype"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="3" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Outer"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyLink"><Link value="Inner"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Override"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyLink"><Link value="Inner"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="false"/></Property></Properties></Object>
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

#[test]
fn transfers_grounded_assembly_state_with_resolved_component() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="BasePlate" id="1"/>
 <Object type="App::FeaturePython" name="Ground" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="BasePlate"><Properties Count="0"/></Object>
 <Object name="Ground"><Properties Count="2">
  <Property name="ObjectToGround" type="App::PropertyLink"><Link value="BasePlate"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="7" Py="8" Pz="9" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("grounded assembly object");
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
    let joint = &result.ir().model.assembly_joints[0];
    assert_eq!(joint.kind, cadmpeg_ir::JointKind::Grounded);
    assert_eq!(joint.operands.len(), 1);
    assert!(joint.operands[0].occurrence.is_some());
    assert_eq!(joint.frames.len(), 1);
    assert_eq!(joint.frames[0].rows[0][3], 7.0);
    assert_eq!(joint.frames[0].rows[1][3], 8.0);
    assert_eq!(joint.frames[0].rows[2][3], 9.0);
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn censuses_application_domains_and_keeps_python_payloads_inert() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5" Dependencies="1">
 <ObjectDeps Name="Mesh" Count="1"><Dep Name="Points"/></ObjectDeps>
 <ObjectDeps Name="Points" Count="0"/>
 <ObjectDeps Name="Analysis" Count="0"/>
 <ObjectDeps Name="Toolpath" Count="0"/>
 <ObjectDeps Name="Local" Count="0"/>
 <Object type="Mesh::Feature" name="Mesh" id="1"/>
 <Object type="Points::Feature" name="Points" id="2"/>
 <Object type="Fem::FemAnalysis" name="Analysis" id="3"/>
 <Object type="Path::FeaturePython" name="Toolpath" id="4"/>
 <Object type="LocalType" name="Local" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Mesh"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Points"/></Property></Properties></Object>
 <Object name="Points"><Properties Count="0"/></Object>
 <Object name="Analysis"><Properties Count="1"><Property name="Report" type="App::PropertyFileIncluded"><FileIncluded file="analysis.dat"/></Property></Properties></Object>
 <Object name="Toolpath"><Properties Count="1"><Property name="Proxy" type="App::PropertyPythonObject"><PythonObject class="ToolController">serialized-but-inert</PythonObject></Property></Properties></Object>
 <Object name="Local"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("analysis.dat", b"finite-element-results"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("application census");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let records = namespace
        .arena_as::<crate::native::ApplicationRecord>("applications")
        .expect("applications");
    assert_eq!(records.len(), 5);
    let by_domain = records
        .iter()
        .map(|record| (record.domain.as_str(), record))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_domain["Mesh"].dependencies,
        ["fcstd:native:object#Points"]
    );
    assert_eq!(by_domain["Fem"].side_entries, ["analysis.dat"]);
    assert!(by_domain["Path"].inert_payload);
    assert!(!by_domain["Mesh"].inert_payload);
    assert_eq!(by_domain["Unqualified"].type_name, "LocalType");
    let report = &by_domain["Fem"].property_records[0];
    assert_eq!(report.object, by_domain["Fem"].object);
    assert!(report.byte_start < report.byte_end);
    assert_eq!(report.byte_len, report.data.len() as u64);
    assert_eq!(report.sha256, cadmpeg_ir::hash::sha256_hex(&report.data));
    assert_eq!(report.payloads.len(), 1);
    assert_eq!(report.payloads[0].name, "analysis.dat");
    assert_eq!(report.payloads[0].data, b"finite-element-results");
    assert_eq!(
        report.payloads[0].sha256,
        cadmpeg_ir::hash::sha256_hex(&report.payloads[0].data)
    );
    let python = &by_domain["Path"].property_records[0];
    assert!(python.inert);
    assert!(String::from_utf8_lossy(&python.data).contains("serialized-but-inert"));
    assert!(records.iter().all(|record| {
        record.byte_start < record.byte_end
            && record.byte_len == record.data.len() as u64
            && record.sha256 == cadmpeg_ir::hash::sha256_hex(&record.data)
    }));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    let mut stale_records = records.clone();
    stale_records[0].property_records[0].sha256 = "0".repeat(64);
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("applications", &stale_records)
        .expect("replace application records");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding
            .message
            .contains("application preservation records do not match authoritative bytes")));
}

#[test]
fn transfers_application_mesh_and_transformed_point_cloud_payloads() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Mesh::Feature" name="Mesh" id="1"/>
 <Object type="Points::Feature" name="Cloud" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Mesh"><Properties Count="1"><Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file="MeshKernel.bms"/></Property></Properties></Object>
 <Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud" mtrx="1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let mut mesh = Vec::new();
    mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
    mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    mesh.extend_from_slice(&[0; 256]);
    mesh.extend_from_slice(&3_u32.to_le_bytes());
    mesh.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f32, 1.0, 0.0, 1.0, 0.0, 0.0] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    let mut points = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
        points.extend_from_slice(&value.to_le_bytes());
    }
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("MeshKernel.bms", &mesh),
                ("Cloud", &points),
            ])),
            &DecodeOptions::default(),
        )
        .expect("application geometry");
    assert_eq!(result.ir().model.tessellations.len(), 1);
    let mesh = &result.ir().model.tessellations[0];
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert_eq!(
        mesh.source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("fcstd:native:object#Mesh")
    );
    assert_eq!(result.ir().model.points.len(), 2);
    assert_eq!(
        result.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(11.0, 22.0, 33.0)
    );
    assert_eq!(
        result.ir().model.points[1].position,
        cadmpeg_ir::math::Point3::new(9.0, 18.0, 27.0)
    );
    assert!(result.report().geometry_transferred);
    assert!(result.report().losses.is_empty());
}

#[test]
fn rejects_multiple_side_entries_for_one_typed_geometry_property() {
    for (object_type, property_type, values) in [
        (
            "Mesh::Feature",
            "Mesh::PropertyMeshKernel",
            r#"<Mesh file="first"/><Mesh file="second"/>"#,
        ),
        (
            "Points::Feature",
            "Points::PropertyPointKernel",
            r#"<Points file="first"/><Points file="second"/>"#,
        ),
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="{object_type}" name="Geometry"/></Objects>
<ObjectData Count="1"><Object name="Geometry"><Properties Count="1"><Property name="Geometry" type="{property_type}">{values}</Property></Properties></Object></ObjectData>
</Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("first", b""),
                    ("second", b""),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("multiple typed geometry entries");

        assert!(matches!(
            error,
            cadmpeg_core::CodecError::Malformed(message)
                if message.contains("references more than one side entry")
        ));
    }
}

#[test]
fn retains_ordered_document_level_gui_state() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1" active="UnrecognizedRootState">
 <ViewProviderData Count="0"/>
 <Camera settings="OrthographicCamera { position 1 2 3 }"/>
 <ClipPlane enabled="true" file="section.bin"/>
</Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("section.bin", b"section-state"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("GUI state");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let documents = namespace
        .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
        .expect("GUI documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].schema_version, Some(1));
    assert_eq!(documents[0].attributes["active"], "UnrecognizedRootState");
    assert_eq!(
        documents[0]
            .states
            .iter()
            .map(|state| state.kind.as_str())
            .collect::<Vec<_>>(),
        ["Camera", "ClipPlane"]
    );
    assert_eq!(
        documents[0].states[0].attributes["settings"],
        "OrthographicCamera { position 1 2 3 }"
    );
    assert_eq!(documents[0].states[1].side_entries, ["section.bin"]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let section = entries
        .iter()
        .find(|entry| entry.name == "section.bin")
        .expect("section asset");
    assert_eq!(section.referenced_by, [documents[0].states[1].id.clone()]);
    assert_eq!(result.ir().model.presentation_documents.len(), 1);
    let presentation = &result.ir().model.presentation_documents[0];
    assert_eq!(presentation.schema_version, Some(1));
    assert_eq!(presentation.active_view, None);
    let camera = presentation.camera.as_ref().expect("camera state");
    assert_eq!(camera.position, None);
    assert_eq!(camera.orientation, None);
    assert_eq!(
        camera.properties["settings"],
        "OrthographicCamera { position 1 2 3 }"
    );
    assert_eq!(presentation.states[1].assets.len(), 1);
    assert!(presentation.states[1].assets[0].ends_with("section.bin"));
    assert!(result.ir().model.view_presentations.is_empty());
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted.model.presentation_documents[0]
        .camera
        .as_mut()
        .expect("camera state")
        .orientation = Some([0.0; 4]);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid document presentation state"));
}

#[test]
fn requires_one_camera_in_schema_one_gui_document() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    for camera_records in [
        "",
        r#"<Camera settings="first"/><Camera settings="second"/>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/>{camera_records}</Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("schema-one camera cardinality");

        assert!(matches!(
            error,
            cadmpeg_core::CodecError::Malformed(message)
                if message.contains("schema 1 requires one Camera record")
        ));
    }
}

#[test]
fn gui_property_counts_ignore_nested_extension_properties() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="0"/><Extension name="Nested"><Properties Count="1"><Property name="NestedValue" type="App::PropertyString"><String value="kept by extension"/></Property></Properties></Extension></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("nested extension properties do not alter the provider's direct count");
    let native = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("FCStd namespace");
    assert_eq!(
        native
            .arena_as::<crate::native::ObjectRecord>("objects")
            .expect("objects")
            .len(),
        1
    );
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_malformed_registered_gui_property_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="LineWidth" type="App::PropertyFloatConstraint"><Integer value="2"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("mismatched GUI value tag");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn accepts_and_validates_gui_custom_enumerations() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="2"><Property name="Pattern" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="None"/><Enum value="Cross"/></CustomEnumList></Property><Property name="ChildViewProvider" type="App::PropertyPersistentObject"><String value=""/><PersistentObject><State value="retained"/></PersistentObject></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("custom GUI enumeration");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let enumeration = properties
        .iter()
        .find(|property| property.name == "Pattern")
        .expect("enumeration");
    let persistent = properties
        .iter()
        .find(|property| property.name == "ChildViewProvider")
        .expect("persistent object");
    assert_eq!(enumeration.values.len(), 4);
    assert_eq!(persistent.values[0].tag, "String");
    assert_eq!(persistent.values[1].tag, "PersistentObject");
    assert_eq!(persistent.values[2].tag, "State");

    for invalid in [
        br#"<Integer value="1"/><CustomEnumList count="0"/>"#.as_slice(),
        br#"<Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="None"/></CustomEnumList>"#.as_slice(),
    ] {
        let gui = [
            br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="Pattern" type="App::PropertyEnumeration">"#.as_slice(),
            invalid,
            br#"</Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        ]
        .concat();
        assert!(FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", &gui),
                ])),
                &DecodeOptions::default(),
            )
            .is_err());
    }
}

#[test]
fn rejects_truncated_gui_material_list_payload() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="3"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("ShapeAppearance", &1_u32.to_le_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("truncated GUI material list");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn retains_unregistered_gui_property_values_without_semantic_dispatch() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ExtensionState" type="Vendor::PropertyState"><VendorState mode="custom"><Nested value="kept"/></VendorState></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("unregistered GUI property");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties[0].type_name, "Vendor::PropertyState");
    assert_eq!(properties[0].values[0].tag, "VendorState");
    assert_eq!(properties[0].values[1].tag, "Nested");
}

#[test]
fn recovers_techdraw_page_template_and_view_graph() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawPage" name="Page" id="2"/>
 <Object type="TechDraw::DrawSVGTemplate" name="Template" id="3"/>
 <Object type="TechDraw::DrawViewPart" name="View" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="Page"><Properties Count="2">
  <Property name="Template" type="App::PropertyLink"><Link value="Template"/></Property>
  <Property name="Views" type="App::PropertyLinkList"><LinkList count="1"><Link value="View"/></LinkList></Property>
 </Properties></Object>
 <Object name="Template"><Properties Count="1"><Property name="Template" type="App::PropertyFileIncluded"><FileIncluded file="page.svg"/></Property></Properties></Object>
 <Object name="View"><Properties Count="5">
  <Property name="Source" type="App::PropertyLink"><Link value="Model"/></Property>
  <Property name="X" type="App::PropertyDistance"><Float value="25"/></Property>
  <Property name="Y" type="App::PropertyDistance"><Float value="40"/></Property>
  <Property name="Scale" type="App::PropertyFloatConstraint"><Float value="2"/></Property>
  <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("page.svg", b"<svg/>"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("TechDraw");
    let drawings = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::DrawingRecord>("drawings")
        .expect("drawings");
    assert_eq!(drawings.len(), 3);
    let page = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Page"))
        .expect("page");
    let template = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Template"))
        .expect("template");
    let view = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("view");
    assert_eq!(
        page.template.as_deref(),
        Some("fcstd:native:object#Template")
    );
    assert_eq!(page.views, ["fcstd:native:object#View"]);
    assert_eq!(template.side_entries, ["page.svg"]);
    assert_eq!(
        view.sources[0].object.as_deref(),
        Some("fcstd:native:object#Model")
    );
    assert!(view.parameters.contains_key("Direction"));
    assert_eq!(result.ir().model.drawings.len(), 3);
    let neutral_page = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Page"))
        .expect("neutral page");
    let neutral_template = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Template"))
        .expect("neutral template");
    let neutral_view = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view");
    assert_eq!(neutral_page.kind, cadmpeg_ir::drawings::DrawingKind::Page);
    assert_eq!(
        neutral_page.template.as_deref(),
        Some(neutral_template.id.0.as_str())
    );
    assert_eq!(
        neutral_page.relationships["Views"][0].target.as_deref(),
        Some(neutral_view.id.0.as_str())
    );
    assert_eq!(neutral_template.assets.len(), 1);
    assert_eq!(neutral_view.position, Some([25.0, 40.0]));
    assert_eq!(neutral_view.scale, Some(2.0));
    assert_eq!(neutral_view.direction, Some([0.0, 0.0, 1.0]));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted
        .model
        .drawings
        .iter_mut()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view")
        .scale = Some(0.0);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid drawing reference, order, or numeric state"));
}

#[test]
fn transfers_app_annotation_text_and_position_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="App::Annotation" name="Note"/>
 <Object type="App::AnnotationLabel" name="Label"/>
</Objects>
<ObjectData Count="2">
 <Object name="Note"><Properties Count="2">
  <Property name="LabelText" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
  <Property name="Position" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
 </Properties></Object>
 <Object name="Label"><Properties Count="2">
  <Property name="LabelText" type="App::PropertyStringList"><StringList count="1"><String value="LABEL"/></StringList></Property>
  <Property name="TextPosition" type="App::PropertyVector"><PropertyVector valueX="4" valueY="5" valueZ="6"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("App annotations");

    let annotations = &result.ir().model.semantic_annotations;
    assert_eq!(annotations.len(), 2);
    let note = annotations
        .iter()
        .find(|annotation| annotation.runtime_type == "App::Annotation")
        .expect("note annotation");
    assert_eq!(note.text, ["NOTE"]);
    assert_eq!(note.position, Some([1.0, 2.0, 3.0]));
    let label = annotations
        .iter()
        .find(|annotation| annotation.runtime_type == "App::AnnotationLabel")
        .expect("label annotation");
    assert_eq!(label.text, ["LABEL"]);
    assert_eq!(label.position, Some([4.0, 5.0, 6.0]));
}

#[test]
fn rejects_incomplete_annotation_position_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewAnnotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="2">
<Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
<Property name="X" type="App::PropertyDistance"><Float value="10"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("incomplete annotation position");

    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("position requires both X and Y")
    ));
}

#[test]
fn separates_semantic_annotations_from_drawing_relationships() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawViewPart" name="View" id="2"/>
 <Object type="TechDraw::DrawViewDimension" name="Dimension" id="3"/>
 <Object type="TechDraw::DrawViewAnnotation" name="Note" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="View"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Model"/></Property></Properties></Object>
 <Object name="Dimension"><Properties Count="5">
  <Property name="BaseView" type="App::PropertyLink"><Link value="View"/></Property>
  <Property name="References2D" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Model" sub="Edge1"/></LinkSubList></Property>
  <Property name="FormatSpec" type="App::PropertyString"><String value="12.5 mm"/></Property>
  <Property name="X" type="App::PropertyDistance"><Float value="10"/></Property>
  <Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property>
 </Properties></Object>
 <Object name="Note"><Properties Count="2">
  <Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="INSPECT"/></StringList></Property>
  <Property name="View" type="App::PropertyLink"><Link value="View"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("semantic annotations");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let annotations = namespace
        .arena_as::<crate::native::SemanticAnnotationRecord>("annotations")
        .expect("annotations");
    let drawings = namespace
        .arena_as::<crate::native::DrawingRecord>("drawings")
        .expect("drawings");
    assert_eq!(annotations.len(), 2);
    let dimension = annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Dimension"))
        .expect("dimension");
    assert_eq!(dimension.text, ["12.5 mm"]);
    assert_eq!(
        dimension.references["References2D"][0].subelements,
        ["Edge1"]
    );
    let note = annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .expect("note");
    assert_eq!(note.text, ["INSPECT"]);
    let drawing_dimension = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Dimension"))
        .expect("drawing dimension");
    assert_eq!(
        drawing_dimension.relationships["BaseView"][0]
            .object
            .as_deref(),
        Some("fcstd:native:object#View")
    );
    let neutral_dimension = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Dimension"))
        .expect("neutral drawing dimension");
    assert_eq!(
        neutral_dimension.kind,
        cadmpeg_ir::drawings::DrawingKind::Dimension
    );
    assert!(neutral_dimension.relationships.contains_key("BaseView"));
    assert!(neutral_dimension.relationships.contains_key("References2D"));
    assert_eq!(result.ir().model.semantic_annotations.len(), 2);
    let semantic_dimension = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Dimension"))
        .expect("semantic dimension");
    assert_eq!(
        semantic_dimension.kind,
        cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Dimension
    );
    assert_eq!(semantic_dimension.text, ["12.5 mm"]);
    assert_eq!(semantic_dimension.format.as_deref(), Some("12.5 mm"));
    assert_eq!(semantic_dimension.value, None);
    assert_eq!(semantic_dimension.position, Some([10.0, 20.0, 0.0]));
    assert_eq!(
        semantic_dimension.references["References2D"][0].subelements,
        ["Edge1"]
    );
    let semantic_note = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .expect("semantic note");
    assert_eq!(
        semantic_note.kind,
        cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text
    );
    assert_eq!(semantic_note.text, ["INSPECT"]);
    let neutral_view = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view");
    assert_eq!(
        semantic_note.references["View"][0].target.as_deref(),
        Some(neutral_view.id.0.as_str())
    );
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted.model.semantic_annotations[0].value = Some(f64::INFINITY);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message
            == "invalid semantic annotation reference, order, or numeric state"));
}

#[test]
fn transfers_remaining_semantic_annotation_families_and_assets() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawViewBalloon" name="Balloon" id="2"/>
 <Object type="TechDraw::DrawLeaderLine" name="Leader" id="3"/>
 <Object type="TechDraw::DrawViewSymbol" name="Symbol" id="4"/>
 <Object type="TechDraw::DrawViewDatum" name="Datum" id="5"/>
 <Object type="TechDraw::DrawViewTolerance" name="Tolerance" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="Balloon"><Properties Count="2">
  <Property name="Text" type="App::PropertyString"><String value="7"/></Property>
  <Property name="Source" type="App::PropertyLinkSub"><LinkSub value="Model" count="1"><Sub value="Face1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="Leader"><Properties Count="1"><Property name="Text" type="App::PropertyString"><String value="LEAD"/></Property></Properties></Object>
 <Object name="Symbol"><Properties Count="1"><Property name="Symbol" type="App::PropertyFileIncluded"><FileIncluded file="symbol.svg"/></Property></Properties></Object>
 <Object name="Datum"><Properties Count="1"><Property name="LabelText" type="App::PropertyString"><String value="A"/></Property></Properties></Object>
 <Object name="Tolerance"><Properties Count="1"><Property name="Text" type="App::PropertyString"><String value="0.1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("symbol.svg", b"<svg/>"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("annotation families");
    let kinds = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .map(|annotation| annotation.kind.clone())
        .collect::<Vec<_>>();
    assert_eq!(kinds, [Kind::Balloon, Kind::Leader, Kind::Symbol]);
    assert!(result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .all(|annotation| !annotation.object.ends_with("#Datum")
            && !annotation.object.ends_with("#Tolerance")));
    let balloon = &result.ir().model.semantic_annotations[0];
    assert_eq!(balloon.text, ["7"]);
    assert_eq!(balloon.references["Source"][0].subelements, ["Face1"]);
    let symbol = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.kind == Kind::Symbol)
        .expect("semantic symbol");
    assert_eq!(symbol.assets.len(), 1);
    assert!(symbol.assets[0].ends_with("symbol.svg"));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn retains_support_attachment_and_distinct_offset_frame() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Feature" name="Support" id="1"/>
 <Object type="Sketcher::SketchObject" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Support"><Properties Count="0"/></Object>
 <Object name="Sketch"><Properties Count="5">
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
  <Property name="Support" type="App::PropertyLinkSub"><LinkSub value="Support" count="1"><Sub value="Face1"/></LinkSub></Property>
  <Property name="MapMode" type="App::PropertyString"><String value="FlatFace"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="AttachmentOffset" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("attachment");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let attachments = namespace
        .arena_as::<crate::native::AttachmentRecord>("attachments")
        .expect("attachments");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].map_mode.as_deref(), Some("FlatFace"));
    assert_eq!(
        attachments[0].supports[0].object.as_deref(),
        Some("fcstd:native:object#Support")
    );
    assert_eq!(attachments[0].supports[0].subelements, ["Face1"]);
    assert_eq!(attachments[0].placement.expect("placement")[0][3], 10.0);
    assert_eq!(attachments[0].offset.expect("offset")[0][3], 2.0);
    assert_eq!(attachments[0].effective_frame[0][3], 10.0);
    let sketch = result.ir().model.sketches.first().expect("sketch");
    assert_eq!(
        sketch
            .resolved_placement()
            .expect("resolved sketch placement")
            .0
            .x,
        10.0
    );
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn transfers_recursive_exact_parameter_curve_geometry() {
    let source = crate::brep::TextCurve2d::Offset {
        distance: 0.25,
        basis: Box::new(crate::brep::TextCurve2d::Trimmed {
            parameter_range: [0.0, std::f64::consts::PI],
            basis: Box::new(crate::brep::TextCurve2d::Circle {
                center: cadmpeg_ir::math::Point2::new(1.0, 2.0),
                x_axis: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                y_axis: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                radius: 3.0,
            }),
        }),
    };
    let cadmpeg_ir::geometry::PcurveGeometry::Offset { distance, basis } =
        crate::topology_transfer::pcurve_geometry(&source)
    else {
        panic!("expected offset pcurve");
    };
    assert_eq!(distance, 0.25);
    assert!(matches!(
        basis.as_ref(),
        cadmpeg_ir::geometry::PcurveGeometry::Trimmed { basis, .. }
            if matches!(basis.as_ref(), cadmpeg_ir::geometry::PcurveGeometry::Circle { radius: 3.0, .. })
    ));
}

#[test]
fn transfers_binary_exact_curve_and_surface_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.bin"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let mut brep = b"\nOpen CASCADE Topology V3 (c)\nLocations 0\nCurve2ds 0\nCurves 1\n".to_vec();
    brep.push(1);
    for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n");
    brep.push(1);
    for value in [
        0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Triangulations 1\n");
    brep.extend_from_slice(&3_i32.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.push(0);
    for value in [0.01_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    for node in [1_i32, 2, 3] {
        brep.extend_from_slice(&node.to_le_bytes());
    }
    brep.extend_from_slice(b"TShapes 7\n");
    let flags = |brep: &mut Vec<u8>| brep.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0]);
    let child = |brep: &mut Vec<u8>, orientation: u8, reverse_index: i32| {
        brep.push(orientation);
        brep.extend_from_slice(&reverse_index.to_le_bytes());
        brep.extend_from_slice(&0_i32.to_le_bytes());
    };
    brep.push(7);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.push(0);
    flags(&mut brep);
    brep.push(b'*');
    brep.push(6);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&[1, 1, 1, 0]);
    flags(&mut brep);
    child(&mut brep, 0, 7);
    child(&mut brep, 1, 7);
    brep.push(b'*');
    brep.push(5);
    flags(&mut brep);
    child(&mut brep, 0, 6);
    brep.push(b'*');
    brep.push(4);
    brep.push(0);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.push(2);
    brep.extend_from_slice(&1_i32.to_le_bytes());
    flags(&mut brep);
    child(&mut brep, 0, 5);
    brep.push(b'*');
    for (kind, reverse_index) in [(3_u8, 4_i32), (2, 3), (0, 2)] {
        brep.push(kind);
        flags(&mut brep);
        child(&mut brep, 0, reverse_index);
        brep.push(b'*');
    }
    brep.extend_from_slice(&7_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.bin", &brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("binary curve carrier");
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Line { .. }
    ));
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].triangles, [[0, 1, 2]]);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(
        result.ir().model.tessellations[0].body.as_ref(),
        Some(&result.ir().model.bodies[0].id)
    );
    assert_eq!(
        result.ir().model.tessellations[0].faces,
        [result.ir().model.faces[0].id.clone()]
    );
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert!(result.report().geometry_transferred);
}

#[test]
fn transfers_connected_text_brep_topology() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Shape" expanded="1"><Properties Count="8">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3368601600"/></Property>
<Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="3"/></Property>
<Property name="LineColor" type="App::PropertyColor"><PropertyColor value="4278190335"/></Property>
<Property name="LineWidth" type="App::PropertyFloatConstraint"><Float value="2.5"/></Property>
<Property name="PointColor" type="App::PropertyColor"><PropertyColor value="16711935"/></Property>
<Property name="PointSize" type="App::PropertyFloatConstraint"><Float value="4"/></Property>
<Property name="Transparency" type="App::PropertyPercent"><Integer value="25"/></Property>
<Property name="Visibility" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let mut shape_appearance = Vec::new();
    shape_appearance.extend_from_slice(&1_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x3333_33ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x3366_99ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x1111_11ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x0000_00ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0.75_f32.to_le_bytes());
    shape_appearance.extend_from_slice(&0.25_f32.to_le_bytes());
    for _ in 0..3 {
        shape_appearance.extend_from_slice(&0_u32.to_le_bytes());
    }
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("ShapeAppearance", &shape_appearance),
        ("Shape.brp", brep),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("connected topology");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 2);
    assert_eq!(result.ir().model.edges.len(), 2);
    assert_eq!(result.ir().model.vertices.len(), 2);
    assert_eq!(result.ir().model.pcurves.len(), 2);
    assert_eq!(result.ir().model.appearances.len(), 3);
    assert_eq!(result.ir().model.appearance_bindings.len(), 5);
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
            ))
            .count(),
        2
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
            ))
            .count(),
        2
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.schema.as_deref() == Some("FCStd ViewProvider line style"))
            .and_then(|appearance| appearance.properties.get("line_width")),
        Some(&2.5)
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearances
            .iter()
            .find(
                |appearance| appearance.schema.as_deref() == Some("FCStd ViewProvider point style")
            )
            .and_then(|appearance| appearance.properties.get("point_size")),
        Some(&4.0)
    );
    assert_eq!(result.ir().model.bodies[0].visible, Some(false));
    assert_eq!(result.ir().model.presentation_documents.len(), 1);
    assert_eq!(result.ir().model.view_presentations.len(), 1);
    let view = &result.ir().model.view_presentations[0];
    assert!(view
        .object
        .as_deref()
        .is_some_and(|id| id.ends_with("Shape")));
    assert_eq!(view.order, 0);
    assert_eq!(view.expanded, Some(true));
    assert_eq!(view.visible, Some(false));
    assert_eq!(view.line_width, Some(2.5));
    assert_eq!(view.point_size, Some(4.0));
    let color = result.ir().model.bodies[0].color.expect("shape color");
    assert!((color.r - 0x33 as f32 / 255.0).abs() < 1e-6);
    assert!((color.g - 0x66 as f32 / 255.0).abs() < 1e-6);
    assert!((color.b - 0x99 as f32 / 255.0).abs() < 1e-6);
    assert!((color.a - 0.75).abs() < 1e-6);
    let shape_material = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.schema.as_deref() == Some("FCStd ShapeAppearance"))
        .expect("shape material");
    assert_eq!(shape_material.properties.get("shininess"), Some(&0.75));
    assert_eq!(shape_material.properties.get("transparency"), Some(&0.25));
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    assert_eq!(namespace.version, 22);
    let census = namespace
        .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
        .expect("carrier census");
    assert_eq!(census.len(), 1);
    assert_eq!(census[0].topology_version, 1);
    assert_eq!(census[0].curves_2d["line"], 2);
    assert_eq!(census[0].curves_3d["line"], 2);
    assert_eq!(census[0].surfaces["plane"], 1);
    assert_eq!(census[0].topology["edge"], 2);
    assert_eq!(census[0].topology["vertex"], 2);
    let gui_providers = namespace
        .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")
        .expect("GUI providers");
    let gui_properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(gui_providers.len(), 1);
    assert_eq!(
        gui_providers[0].object.as_deref(),
        Some("fcstd:native:object#Shape")
    );
    assert_eq!(gui_properties.len(), 8);
    assert!(gui_properties
        .iter()
        .all(|property| property.raw_xml.starts_with("<Property")));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted.model.view_presentations[0].line_width = Some(f64::NAN);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid view presentation reference, order, or size"));
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::Severity::Error),
        "{:#?}",
        report.findings
    );
}

#[test]
fn transfers_triangulation_only_face_and_indexed_edge_polygon() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects>
<ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade
Locations 1
1 1 0 0 10 0 1 0 0 0 0 1 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 1
2 1 2 p 0.01 1 0 1
Surfaces 0
Triangulations 1
3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3
TShapes 7
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 6 1 1 0 0 1001000 +7 0 -6 0 *
Wi 1001000 +5 0 *
Fa 0 0.001 0 1 2 1 1001000 +4 0 *
Sh 1001000 +3 0 *
So 1001000 +2 0 *
+1 0 *";
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Shape.brp", brep),
            ])),
            &DecodeOptions::default(),
        )
        .expect("triangulation-only topology");
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices[0].x, 0.0);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Polygonal {
            chordal_deflection: 0.02,
            ..
        }
    ));
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Polyline {
            chordal_deflection: 0.01,
            ..
        }
    ));
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 1.0]));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.findings.iter().all(|finding| {
            finding.severity < cadmpeg_ir::Severity::Error
                || finding.check == cadmpeg_ir::Check::Identity
        }),
        "{:#?}",
        validation.findings
    );
}

#[test]
fn connects_persistent_element_names_to_neutral_topology() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1" StringHasher="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="2">
<Property name="AuxShape" type="Part::PropertyPartShape">
<Part HasherIndex="0" SaveHasher="1" ElementMap="1.0" file="AuxShape.brp"/>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="5">
41 PostfixCount 0 MapCount 1
ElementMap 1 41 3
Face ChildCount 0 NameCount 2
0
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 3
0
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property>
<Property name="Shape" type="Part::PropertyPartShape">
<Part HasherIndex="0" SaveHasher="1" ElementMap="1.0" file="Shape.brp"/>
<StringHasher saveall="0" threshold="16" count="0" new="1"/>
<StringHasher2 count="1">
a.c PersistentSource
</StringHasher2>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="5">
41 PostfixCount 0 MapCount 1
ElementMap 1 41 3
Face ChildCount 0 NameCount 2
0
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 3
0
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property></Properties></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Shape"><Properties Count="4">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3435973632"/></Property>
<Property name="DiffuseColor" type="App::PropertyColorList"><ColorList file="DiffuseColor"/></Property>
<Property name="LineColorArray" type="App::PropertyColorList"><ColorList file="LineColorArray"/></Property>
<Property name="PointColorArray" type="App::PropertyColorList"><ColorList file="PointColorArray"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let face_colors = [1_u8, 0, 0, 0, 0, 0, 0, 255];
    let edge_colors = [2_u8, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255];
    let point_colors = [2_u8, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 255];
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("DiffuseColor", &face_colors),
        ("LineColorArray", &edge_colors),
        ("PointColorArray", &point_colors),
        ("AuxShape.brp", brep),
        ("Shape.brp", brep),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("persistent element map");
    let namespace = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("required invariant");
    let tables = namespace
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .expect("required invariant");
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].entries[0].string_id, 10);
    let maps = namespace
        .arena_as::<crate::native::ElementMapRecord>("element_maps")
        .expect("required invariant");
    assert_eq!(maps.len(), 2);
    let shape_map = maps
        .iter()
        .find(|map| map.property.ends_with("#Shape:Shape"))
        .expect("displayed Shape element map");
    assert_eq!(shape_map.hasher_index, Some(0));
    let groups = &shape_map.maps[0].groups;
    assert_eq!(groups[0].names[1][0].topology_ids.len(), 1);
    assert_eq!(groups[1].names[1][0].topology_ids.len(), 1);
    assert_eq!(groups[1].names[2][0].topology_ids.len(), 1);
    assert_eq!(groups[2].names[1][0].topology_ids.len(), 1);
    assert_eq!(groups[2].names[2][0].topology_ids.len(), 1);
    let shape_face_ids = groups[0]
        .names
        .iter()
        .flatten()
        .flat_map(|name| &name.topology_ids)
        .collect::<std::collections::HashSet<_>>();
    assert!(result.ir().model.appearance_bindings.iter().any(|binding| {
        matches!(
            &binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Face(face) if shape_face_ids.contains(&face.0)
        ) && binding.channels.get("precedence").map(String::as_str) == Some("face_over_object")
    }));
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
                ) && binding.channels.get("precedence").map(String::as_str)
                    == Some("edge_array_over_line")
            })
            .count(),
        2
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
                ) && binding.channels.get("precedence").map(String::as_str)
                    == Some("vertex_array_over_point")
            })
            .count(),
        2
    );
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn binds_both_seam_pcurves_and_closes_the_radial_pair() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 0 1
1 6.283185307179586 0 0 1
Curves 1
1 1 0 0 0 0 1
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
2 0 0 0 0 0 1 1 0 0 0 1 0 1
Triangulations 0
TShapes 8
Ve 0.001 1 0 0 0 0 1001000 *
Ve 0.001 1 0 1 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 3 1 2 C0 1 0 0 1 0 1001000 +8 0 -7 0 *
Wi 1001000 +6 0 -6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("cylindrical seam");
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 2);
    let first = &result.ir().model.coedges[0];
    let second = &result.ir().model.coedges[1];
    assert_eq!(first.radial_next, second.id);
    assert_eq!(second.radial_next, first.id);
    assert_ne!(first.pcurves, second.pcurves);
    assert!(!first.pcurves.is_empty() && !second.pcurves.is_empty());
    let errors = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .findings
        .into_iter()
        .filter(|finding| finding.severity == cadmpeg_ir::Severity::Error)
        .filter(|finding| finding.check != cadmpeg_ir::Check::Identity)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn preserves_a_free_edge_as_a_wire_body() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 3
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("free edge");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.shells[0].wire_edges.len(), 1);
    assert!(result.ir().model.shells[0].faces.is_empty());
}

#[test]
fn repeated_shape_roots_have_distinct_occurrence_identity() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape">
<Part ElementMap="1.0" file="Shape.brp"/>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="4">
1 PostfixCount 0 MapCount 1
ElementMap 1 1 2
Edge ChildCount 0 NameCount 3
0
;EdgeStable.0.a 0
;DeletedEdgeStable.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 3
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *
+1 0 +1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("repeated roots");

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.edges.len(), 2);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_ne!(
        result.ir().model.bodies[0].id,
        result.ir().model.bodies[1].id
    );
    let maps = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ElementMapRecord>("element_maps")
        .expect("element maps");
    let groups = &maps[0].maps[0].groups;
    assert_eq!(groups[0].names[1][0].topology_ids.len(), 2);
    assert_eq!(groups[1].names[1][0].topology_ids.len(), 2);
    assert_eq!(groups[1].names[2][0].topology_ids.len(), 2);
    assert_valid_document(result.ir());
}

#[test]
fn preserves_an_unbounded_edge_as_a_free_exact_curve() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::Line" name="Axis" id="1"/></Objects>
<ObjectData Count="1"><Object name="Axis"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Axis.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 0 0 1
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Axis.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("unbounded datum axis");
    assert!(result.ir().model.bodies.is_empty());
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.ir().model.curves[0].source_object.is_some());
    assert_valid_document(result.ir());
}

#[test]
fn preserves_compound_ownership_and_composes_nested_mirrored_locations_once() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 3
1 1 0 0 10 0 1 0 0 0 0 1 0
1 -2 0 0 0 0 2 0 5 0 0 2 0
1 1 0 0 20 0 1 0 0 0 0 1 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 2 *
So 1001000 +3 0 *
Co 1001000 +2 1 +2 3 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("located topology");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(result.ir().model.bodies[0].regions.len(), 2);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 4);
    let mut positions = result
        .ir()
        .model
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .map(|vertex| {
            let vertex = result
                .ir()
                .model
                .vertices
                .iter()
                .find(|candidate| &candidate.id == vertex)
                .expect("required invariant");
            result
                .ir()
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .expect("required invariant")
                .position
        })
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| left.x.total_cmp(&right.x));
    positions.dedup();
    assert_eq!(positions.len(), 4);
    assert_eq!([positions[0].x, positions[0].y], [8.0, 5.0]);
    assert_eq!([positions[1].x, positions[1].y], [10.0, 5.0]);
    assert_eq!([positions[2].x, positions[2].y], [18.0, 5.0]);
    assert_eq!([positions[3].x, positions[3].y], [20.0, 5.0]);
    let face = &result.ir().model.faces[0];
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .expect("required invariant");
    let cadmpeg_ir::geometry::SurfaceGeometry::Transformed { basis, transform } = &surface.geometry
    else {
        panic!("located face must retain its exact transformed basis");
    };
    assert!(matches!(
        basis.as_ref(),
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
    assert_eq!(transform.rows[0][0], -2.0);
    assert_eq!(transform.rows[1][1], 2.0);
    let origin =
        cadmpeg_ir::eval::surface_point(&surface.geometry, 0.0, 0.0).expect("required invariant");
    assert_eq!([origin.x, origin.y], [10.0, 5.0]);
    for edge in &result.ir().model.edges {
        let curve = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve.as_ref())
            .expect("required invariant");
        let range = edge.param_range.expect("located edge parameter range");
        let start =
            cadmpeg_ir::eval::curve_point(&curve.geometry, range[0]).expect("required invariant");
        let end =
            cadmpeg_ir::eval::curve_point(&curve.geometry, range[1]).expect("required invariant");
        assert_eq!((start.x - end.x).abs(), 2.0);
    }
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::Severity::Error
                || finding.check == cadmpeg_ir::Check::Identity),
        "{:#?}",
        report.findings
    );
}

#[test]
fn unregistered_application_payloads_remain_whole_named_opaque_entries() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Vendor::Feature" name="Owner"/></Objects>
<ObjectData Count="1"><Object name="Owner"><Properties Count="1">
<Property name="Payload" type="Vendor::PropertyMeshLike"><Mesh file="Payload.bin"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let payload = [
        0xd0, 0xc0, 0xb0, 0xa0, 0x00, 0x00, 0x01, 0x00, 0x7f, 0x45, 0x4c, 0x46,
    ];
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Payload.bin", &payload),
            ])),
            &DecodeOptions::default(),
        )
        .expect("opaque vendor payload");
    assert!(result.ir().model.tessellations.is_empty());
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(properties[0].type_name, "Vendor::PropertyMeshLike");
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.name == "Payload.bin")
        .expect("payload entry");
    let spans = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = spans
        .iter()
        .find(|span| span.entry == entry.name)
        .expect("payload span");
    assert_eq!(span.start, 0);
    assert_eq!(span.end, payload.len() as u64);
    assert_eq!(span.classification, "named_opaque");
    assert_eq!(span.owner.as_deref(), Some(entry.id.as_str()));
    assert_eq!(entry.data, payload);
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_interleaved_new_string_hasher_payload() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape"><Part file=""/>
<StringHasher new="1" count="0"/><Interleaved/><StringHasher2 count="0"/>
</Property></Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("interleaved string table must fail");

    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[path = "integration_tests.rs"]
mod integration_tests;
