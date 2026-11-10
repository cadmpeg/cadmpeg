// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn part_number_carrier_depends_on_runtime_not_string_contents() {
    for runtime in [
        "App::Part",
        "Assembly::AssemblyObject",
        "Assembly::AssemblyLink",
        "App::DocumentObjectGroup",
    ] {
        for (id, expected_id) in [
            ("", None),
            (
                r#"<Property name="Id" type="App::PropertyString"><String value=""/></Property>"#,
                None,
            ),
            (
                r#"<Property name="Id" type="App::PropertyString"><String value="ITEM-7"/></Property>"#,
                Some("ITEM-7"),
            ),
            (
                r#"<Property name="Id" type="App::PropertyInteger"><Integer value="7"/></Property>"#,
                None,
            ),
        ] {
            let count = if id.is_empty() { 1 } else { 2 };
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="{runtime}" name="Item"/></Objects>
<ObjectData Count="1"><Object name="Item"><Properties Count="{count}">
{id}<Property name="PartNumber" type="App::PropertyString"><String value="CUSTOM-8"/></Property>
</Properties></Object></ObjectData></Document>"#
            );
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let [definition] = decoded.ir().model.product_definitions.as_slice() else {
                panic!("one definition")
            };
            let is_part = runtime != "App::DocumentObjectGroup";
            assert_eq!(
                definition.part_number.as_deref(),
                if is_part {
                    expected_id
                } else {
                    Some("CUSTOM-8")
                },
                "{runtime}: {id}"
            );
            if is_part {
                assert_eq!(
                    definition
                        .bom_properties
                        .get("PartNumber")
                        .map(String::as_str),
                    Some("CUSTOM-8")
                );
            }
            assert_valid_document(decoded.ir());
        }
    }
}

#[test]
fn array_restore_uses_expansion_mode_and_element_objects() {
    for show in [None, Some(true), Some(false)] {
        for stated in [None, Some(0), Some(1), Some(3)] {
            let show_property = show.map_or(String::new(), |value| format!(r#"<Property name="ShowElement" type="App::PropertyBool"><Bool value="{value}"/></Property>"#));
            let count_property = stated.map_or(String::new(), |value| format!(r#"<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="{value}"/></Property>"#));
            let property_count = 7 + usize::from(show.is_some()) + usize::from(stated.is_some());
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object type="Part::Feature" name="Prototype"/><Object type="App::LinkElement" name="First"/><Object type="App::LinkElement" name="Second"/><Object type="App::Link" name="Instance"/></Objects>
<ObjectData Count="4"><Object name="Prototype"><Properties Count="0"/></Object>
<Object name="First"><Properties Count="2"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="11" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
<Object name="Second"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="22" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="4" valueY="5" valueZ="6"/></Property></Properties></Object>
<Object name="Instance"><Properties Count="{property_count}">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="100" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="3" valueZ="4"/></Property>
<Property name="ElementList" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="placements"/></Property>
<Property name="ScaleList" type="App::PropertyVectorList"><VectorList file="scales"/></Property>
<Property name="VisibilityList" type="App::PropertyBoolList"><BoolList value="0"/></Property>
{show_property}{count_property}</Properties></Object></ObjectData></Document>"#
            );
            let mut placements = 2_u32.to_le_bytes().to_vec();
            for x in [1.0_f64, 2.0] {
                placements.extend(
                    [x, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                        .into_iter()
                        .flat_map(f64::to_le_bytes),
                );
            }
            let mut scales = 1_u32.to_le_bytes().to_vec();
            scales.extend([2.0_f64, 2.0, 2.0].into_iter().flat_map(f64::to_le_bytes));
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(archive_entries(&[
                        ("Document.xml", document.as_bytes()),
                        ("placements", &placements),
                        ("scales", &scales),
                    ])),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let expanded = show != Some(false);
            let expected_count = if expanded {
                2
            } else {
                stated.unwrap_or(0).max(1)
            };
            let occurrences = &decoded.ir().model.occurrences;
            assert_eq!(
                occurrences.len(),
                expected_count,
                "show={show:?}, stated={stated:?}"
            );
            for (index, occurrence) in occurrences.iter().enumerate() {
                let array = expanded || stated.is_some_and(|value| value > 0);
                let x = if expanded {
                    [11.0, 22.0][index]
                } else if array {
                    [1.0, 2.0, 0.0][index]
                } else {
                    0.0
                };
                assert_eq!(occurrence.transform.rows()[0][3], 100.0 + 2.0 * x);
                let element_scale = if expanded && index == 1 {
                    [4.0, 5.0, 6.0]
                } else if !expanded && array && index == 0 {
                    [2.0; 3]
                } else {
                    [1.0; 3]
                };
                for (axis, parent_scale) in [2.0, 3.0, 4.0].into_iter().enumerate() {
                    assert_eq!(
                        occurrence.transform.rows()[axis][axis],
                        parent_scale * element_scale[axis]
                    );
                }
                assert_eq!(
                    occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get),
                    [1.0; 3]
                );
                assert_eq!(
                    occurrence
                        .link
                        .as_ref()
                        .and_then(|link| link.element_component())
                        .is_some(),
                    expanded
                );
            }
            let records = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<native::ProductNodeRecord>("product_nodes")
                .unwrap();
            let record = records
                .iter()
                .find(|record| record.object.ends_with("#Instance"))
                .unwrap();
            assert_eq!(
                record.element_count(),
                stated.map(|value| u64::try_from(value).unwrap())
            );
            assert_eq!(record.element_transforms().len(), 2);
            assert_eq!(record.element_scales().len(), 1);
            assert_eq!(record.element_visibility(), [false]);
            assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
            assert_valid_document(decoded.ir());
        }
    }
}

#[test]
fn parent_nonuniform_scale_precedes_rotated_array_element() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="App::Link" name="Instance"/></Objects>
<ObjectData Count="2"><Object name="Prototype"><Properties Count="0"/></Object>
<Object name="Instance"><Properties Count="6">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="6" Pz="7" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="3" valueZ="4"/></Property>
<Property name="ShowElement" type="App::PropertyBool"><Bool value="false"/></Property>
<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="1"/></Property>
<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="placements"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let mut placements = 1_u32.to_le_bytes().to_vec();
    placements.extend(
        [
            11.0_f64,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes),
    );
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("placements", &placements),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let [occurrence] = decoded.ir().model.occurrences.as_slice() else {
        panic!("one array element")
    };
    let expected = [
        [0.0, -2.0, 0.0, 27.0],
        [3.0, 0.0, 0.0, 6.0],
        [0.0, 0.0, 4.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (actual, expected) in occurrence
        .transform
        .rows()
        .into_iter()
        .flatten()
        .zip(expected.into_iter().flatten())
    {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert_eq!(
        occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.0; 3]
    );
    assert_valid_document(decoded.ir());
}

#[test]
fn link_scales_precede_the_linked_prototype_placement() {
    for (expanded, count) in [(true, 1), (false, 1), (false, 0)] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="Part::Feature" name="Prototype"/><Object type="App::LinkElement" name="Element"/><Object type="App::Link" name="Instance"/></Objects>
<ObjectData Count="3">
<Object name="Prototype"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="3" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
<Object name="Element"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="11" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="0.5" valueY="1" valueZ="1.5"/></Property></Properties></Object>
<Object name="Instance"><Properties Count="9">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="6" Pz="7" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="3" valueZ="4"/></Property>
<Property name="ShowElement" type="App::PropertyBool"><Bool value="{expanded}"/></Property>
<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="{count}"/></Property>
<Property name="ElementList" type="App::PropertyLinkList"><LinkList count="1"><Link value="Element"/></LinkList></Property>
<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="placements"/></Property>
<Property name="ScaleList" type="App::PropertyVectorList"><VectorList file="scales"/></Property>
</Properties></Object></ObjectData></Document>"#
        );
        let mut placements = 1_u32.to_le_bytes().to_vec();
        placements.extend(
            [11.0_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                .into_iter()
                .flat_map(f64::to_le_bytes),
        );
        let mut scales = 1_u32.to_le_bytes().to_vec();
        scales.extend([0.5_f64, 1.0, 1.5].into_iter().flat_map(f64::to_le_bytes));
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("placements", &placements),
                    ("scales", &scales),
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();
        let [occurrence] = decoded.ir().model.occurrences.as_slice() else {
            panic!("one link use, no duplicate element")
        };
        let effective = occurrence.effective_transform().unwrap().rows();
        assert_eq!(effective[0][3], if count == 0 { 11.0 } else { 30.0 });
        assert_eq!(effective[0][0], if count == 0 { 2.0 } else { 1.0 });
        assert_eq!(effective[1][1], 3.0);
        assert_eq!(effective[2][2], if count == 0 { 4.0 } else { 6.0 });
        assert_eq!(
            occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get),
            [1.0; 3]
        );
        assert_valid_document(decoded.ir());
    }
}

#[test]
fn element_list_membership_controls_link_element_occurrence_roots() {
    for runtime in ["App::Link", "App::LinkElement"] {
        for owner in [None, Some(0), Some(3), Some(17), Some(99)] {
            for expanded in [true, false] {
                let owner_property = owner.map_or(String::new(), |owner| format!(r#"<Property name="_LinkOwner" type="App::PropertyInteger"><Integer value="{owner}"/></Property>"#));
                let member_property_count = 4 + usize::from(owner.is_some());
                let document = format!(
                    r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object type="Part::Feature" name="Prototype" id="1"/><Object type="{runtime}" name="Member" id="2"/><Object type="App::FeaturePython" name="Owner" id="3"/><Object type="App::Link" name="Array" id="17"/></Objects>
<ObjectData Count="4"><Object name="Prototype"><Properties Count="0"/></Object>
<Object name="Member"><Properties Count="{member_property_count}">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="11" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="Scale" type="App::PropertyFloat"><Float value="3"/></Property>
<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="3" valueY="3" valueZ="3"/></Property>{owner_property}</Properties></Object>
<Object name="Owner"><Properties Count="0"/></Object>
<Object name="Array"><Properties Count="8">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="100" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="Scale" type="App::PropertyFloat"><Float value="2"/></Property>
<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="2" valueZ="2"/></Property>
<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="0"/></Property>
<Property name="ShowElement" type="App::PropertyBool"><Bool value="{expanded}"/></Property>
<Property name="ElementList" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property>
</Properties></Object></ObjectData></Document>"#
                );
                let decoded = FcstdCodec
                    .decode(
                        &mut Cursor::new(archive(&document)),
                        &DecodeOptions::default(),
                    )
                    .unwrap();
                let is_link_element_member = runtime == "App::LinkElement";
                let expected_count = if is_link_element_member { 1 } else { 2 };
                assert_eq!(
                    decoded.ir().model.occurrences.len(),
                    expected_count,
                    "{runtime}: owner={owner:?}, expanded={expanded}"
                );
                assert_eq!(
                    decoded
                        .ir()
                        .model
                        .occurrences
                        .iter()
                        .any(|occurrence| occurrence.native_ref.as_deref()
                            == Some("fcstd:native:object#Member")),
                    !is_link_element_member
                );
                let array = decoded
                    .ir()
                    .model
                    .occurrences
                    .iter()
                    .find(|occurrence| {
                        occurrence.native_ref.as_deref() == Some("fcstd:native:object#Array")
                    })
                    .unwrap();
                if is_link_element_member {
                    let rows = array.effective_transform().unwrap().rows();
                    let expected_x = if expanded { 122.0 } else { 100.0 };
                    let expected_scale = if expanded { 6.0 } else { 2.0 };
                    assert!(
                        (rows[0][3] - expected_x).abs() < 1e-12,
                        "{runtime}: owner={owner:?}, expanded={expanded}, transform={rows:?}"
                    );
                    assert!(
                        (rows[0][0] - expected_scale).abs() < 1e-12,
                        "{runtime}: owner={owner:?}, expanded={expanded}, transform={rows:?}"
                    );
                    assert!(
                        (rows[1][1] - expected_scale).abs() < 1e-12,
                        "{runtime}: owner={owner:?}, expanded={expanded}, transform={rows:?}"
                    );
                    assert!(
                        (rows[2][2] - expected_scale).abs() < 1e-12,
                        "{runtime}: owner={owner:?}, expanded={expanded}, transform={rows:?}"
                    );
                }
                if !is_link_element_member {
                    let member = decoded
                        .ir()
                        .model
                        .occurrences
                        .iter()
                        .find(|occurrence| {
                            occurrence.native_ref.as_deref() == Some("fcstd:native:object#Member")
                        })
                        .unwrap();
                    assert!(matches!(
                        member.parent,
                        cadmpeg_ir::products::OccurrenceParent::Root {}
                    ));
                }
                let namespace = decoded.ir().native.namespace("fcstd").unwrap();
                let records = namespace
                    .arena_as::<native::ProductNodeRecord>("product_nodes")
                    .unwrap();
                let objects = namespace
                    .arena_as::<native::ObjectRecord>("objects")
                    .unwrap();
                let properties = namespace
                    .arena_as::<native::PropertyRecord>("properties")
                    .unwrap();
                assert_eq!(
                    crate::product::link_occurrence_count(&records, &objects, &properties).unwrap(),
                    u64::try_from(expected_count).unwrap()
                );
                assert_valid_document(decoded.ir());
            }
        }
    }
}

#[test]
fn unlisted_link_element_with_live_owner_remains_an_occurrence() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="Part::Feature" name="Prototype" id="1"/><Object type="App::LinkElement" name="Member" id="2"/><Object type="App::Part" name="Owner" id="3"/></Objects>
<ObjectData Count="3">
<Object name="Prototype"><Properties Count="0"/></Object>
<Object name="Member"><Properties Count="2"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property><Property name="_LinkOwner" type="App::PropertyInteger"><Integer value="3"/></Property></Properties></Object>
<Object name="Owner"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let member = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.as_deref() == Some("fcstd:native:object#Member"))
        .unwrap();
    assert!(matches!(
        member.parent,
        cadmpeg_ir::products::OccurrenceParent::Root {}
    ));
    assert_valid_document(decoded.ir());
}

#[test]
fn each_link_array_uses_its_own_membership_for_a_shared_element() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5"><Object type="Part::Feature" name="Prototype" id="1"/><Object type="App::LinkElement" name="Member" id="2"/><Object type="App::Link" name="FirstArray" id="3"/><Object type="App::Link" name="SecondArray" id="4"/><Object type="App::Part" name="Assembly" id="5"/></Objects>
<ObjectData Count="5">
<Object name="Prototype"><Properties Count="0"/></Object>
<Object name="Member"><Properties Count="5">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="11" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="Scale" type="App::PropertyFloat"><Float value="2"/></Property>
<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="2" valueZ="2"/></Property>
<Property name="_LinkOwner" type="App::PropertyInteger"><Integer value="99"/></Property>
</Properties></Object>
<Object name="FirstArray"><Properties Count="6">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="100" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="0"/></Property>
<Property name="ShowElement" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="ElementList" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property>
</Properties></Object>
<Object name="SecondArray"><Properties Count="6">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="200" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="0"/></Property>
<Property name="ShowElement" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="ElementList" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property>
</Properties></Object>
<Object name="Assembly"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="FirstArray"/><Link value="SecondArray"/></LinkList></Property></Properties></Object>
</ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let occurrences = &decoded.ir().model.occurrences;
    let actual = occurrences
        .iter()
        .map(|occurrence| {
            (
                occurrence.native_ref.as_deref(),
                &occurrence.parent,
                occurrence.transform.rows(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(occurrences.len(), 3, "actual occurrences: {actual:?}");
    let assembly = occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.as_deref() == Some("fcstd:native:object#Assembly"))
        .unwrap();
    assert!(matches!(
        assembly.parent,
        cadmpeg_ir::products::OccurrenceParent::Root {}
    ));
    let first = occurrences
        .iter()
        .find(|occurrence| {
            occurrence.native_ref.as_deref() == Some("fcstd:native:object#FirstArray")
        })
        .unwrap();
    let second = occurrences
        .iter()
        .find(|occurrence| {
            occurrence.native_ref.as_deref() == Some("fcstd:native:object#SecondArray")
        })
        .unwrap();
    for array in [first, second] {
        assert!(matches!(
            &array.parent,
            cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence }
                if occurrence == &assembly.id
        ));
    }
    for (occurrence, expected_x) in [(first, 111.0), (second, 211.0)] {
        let rows = occurrence.effective_transform().unwrap().rows();
        assert!(
            (rows[0][3] - expected_x).abs() < 1e-12,
            "{}: transform={rows:?}",
            occurrence.native_ref.as_deref().unwrap_or_default()
        );
        assert!(
            (rows[0][0] - 2.0).abs() < 1e-12,
            "{}: transform={rows:?}",
            occurrence.native_ref.as_deref().unwrap_or_default()
        );
        assert!(
            (rows[1][1] - 2.0).abs() < 1e-12,
            "{}: transform={rows:?}",
            occurrence.native_ref.as_deref().unwrap_or_default()
        );
        assert!(
            (rows[2][2] - 2.0).abs() < 1e-12,
            "{}: transform={rows:?}",
            occurrence.native_ref.as_deref().unwrap_or_default()
        );
    }
    assert!(occurrences
        .iter()
        .all(|occurrence| occurrence.native_ref.as_deref() != Some("fcstd:native:object#Member")));
    assert_valid_document(decoded.ir());
}

#[test]
fn link_restore_migrates_uniform_scale_vector_from_scalar_scale() {
    for runtime in ["App::Link", "App::LinkElement"] {
        for (vector, scalar, expected) in [
            ([2.0, 2.0, 2.0], Some(3.0), [3.0, 3.0, 3.0]),
            ([2.0, 3.0, 4.0], Some(5.0), [2.0, 3.0, 4.0]),
            ([2.0, 2.0, 2.0], None, [1.0, 1.0, 1.0]),
        ] {
            let [x, y, z] = vector;
            let scalar_property = scalar.map_or(String::new(), |scalar| format!(r#"<Property name="Scale" type="App::PropertyFloat"><Float value="{scalar}"/></Property>"#));
            let count = 2 + usize::from(scalar.is_some());
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Prototype"/><Object type="{runtime}" name="Instance"/></Objects>
<ObjectData Count="2"><Object name="Prototype"><Properties Count="0"/></Object>
<Object name="Instance"><Properties Count="{count}">
<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="{x}" valueY="{y}" valueZ="{z}"/></Property>{scalar_property}
</Properties></Object></ObjectData></Document>"#
            );
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let [occurrence] = decoded.ir().model.occurrences.as_slice() else {
                panic!("one scalar link")
            };
            for (axis, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    occurrence.transform.rows()[axis][axis],
                    expected,
                    "{runtime}: vector={vector:?}, scalar={scalar:?}"
                );
            }
            assert_eq!(
                occurrence.scale.map(cadmpeg_ir::scalar::FiniteReal::get),
                [1.0; 3]
            );
            let properties = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<native::PropertyRecord>("properties")
                .unwrap();
            let raw_vector = properties
                .iter()
                .find(|property| property.name == "ScaleVector")
                .unwrap();
            assert!(raw_vector.xml.text().contains(&format!("valueX=\"{x}\"")));
            assert_eq!(
                properties.iter().any(|property| property.name == "Scale"),
                scalar.is_some()
            );
            assert_valid_document(decoded.ir());
        }
    }
}
