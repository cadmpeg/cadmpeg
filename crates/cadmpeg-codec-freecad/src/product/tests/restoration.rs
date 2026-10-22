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
