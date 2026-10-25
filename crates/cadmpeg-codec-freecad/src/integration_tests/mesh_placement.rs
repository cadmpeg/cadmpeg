// SPDX-License-Identifier: Apache-2.0
//! World-coordinate application mesh contracts.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::fmt::Write;

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;

fn placement(name: &str, position: [f64; 3], quarter_turn: bool) -> String {
    let [x, y, z] = position;
    let qz = usize::from(quarter_turn);
    format!(
        r#"<Property name="{name}" type="App::PropertyPlacement"><PropertyPlacement Px="{x}" Py="{y}" Pz="{z}" Q0="0" Q1="0" Q2="{qz}" Q3="1"/></Property>"#
    )
}

fn members(names: &[&str]) -> String {
    let mut links = String::new();
    for name in names {
        write!(links, r#"<Link value="{name}"/>"#).unwrap();
    }
    format!(
        r#"<Property name="Group" type="App::PropertyLinkList"><LinkList count="{}">{links}</LinkList></Property>"#,
        names.len()
    )
}

fn linked(target: &str, include_prototype_placement: bool) -> Vec<String> {
    vec![
        format!(
            r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="{target}"/></Property>"#
        ),
        format!(
            r#"<Property name="LinkTransform" type="App::PropertyBool"><Bool value="{include_prototype_placement}"/></Property>"#
        ),
    ]
}

fn mesh_properties(position: [f64; 3], quarter_turn: bool) -> Vec<String> {
    vec![
        r#"<Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file="triangle"/></Property>"#.to_owned(),
        placement("Placement", position, quarter_turn),
    ]
}

fn decode(
    objects: &[(&str, &str, Vec<String>)],
    entries: &[(&str, &[u8])],
) -> cadmpeg_ir::DecodeResult {
    let mut declarations = String::new();
    let mut data = String::new();
    for (name, kind, properties) in objects {
        write!(declarations, r#"<Object name="{name}" type="{kind}"/>"#).unwrap();
        write!(
            data,
            r#"<Object name="{name}"><Properties Count="{}">{}</Properties></Object>"#,
            properties.len(),
            properties.concat()
        )
        .unwrap();
    }
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="{}">{declarations}</Objects><ObjectData Count="{}">{data}</ObjectData></Document>"#,
        objects.len(),
        objects.len()
    );
    let mut triangle = Vec::new();
    triangle.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
    triangle.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    triangle.extend_from_slice(&[0; 256]);
    triangle.extend_from_slice(&3_u32.to_le_bytes());
    triangle.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0] {
        triangle.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
        triangle.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f32, 2.0, 0.0, 3.0, 0.0, 0.0] {
        triangle.extend_from_slice(&value.to_le_bytes());
    }
    let mut members = vec![
        ("Document.xml", document.as_bytes()),
        ("triangle", triangle.as_slice()),
    ];
    members.extend_from_slice(entries);
    FcstdCodec
        .decode(
            &mut std::io::Cursor::new(archive_entries(&members)),
            &DecodeOptions::default(),
        )
        .expect("mesh archive")
}

fn assert_vertices(result: &cadmpeg_ir::DecodeResult, index: usize, expected: [[f64; 3]; 3]) {
    let meshes = &result.ir().model.tessellations;
    let is_source = |mesh: &&cadmpeg_ir::tessellation::Tessellation| {
        mesh.source_object
            .as_ref()
            .is_some_and(|source| source.instance_path.is_empty())
    };
    let mesh = if index == 0 {
        meshes.iter().find(is_source)
    } else {
        meshes.iter().filter(|mesh| !is_source(mesh)).nth(index - 1)
    }
    .expect("placed mesh use");
    assert_eq!(mesh.triangles(), [[0, 1, 2]]);
    assert_eq!(mesh.vertex_count(), 3);
    for (point, expected) in mesh.vertices().iter().zip(expected) {
        let point = point.get();
        for (actual, expected) in [point.x, point.y, point.z].into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1.0e-10,
                "{actual} != {expected}"
            );
        }
    }
}

#[test]
fn application_mesh_applies_own_translation_and_rotation_once() {
    let result = decode(
        &[(
            "Mesh",
            "Mesh::Feature",
            mesh_properties([100.0, 200.0, 300.0], true),
        )],
        &[],
    );
    assert_vertices(
        &result,
        0,
        [
            [100.0, 200.0, 300.0],
            [100.0, 202.0, 300.0],
            [97.0, 200.0, 300.0],
        ],
    );
    assert!(result.report().losses.is_empty());
}

#[test]
fn application_mesh_composes_nested_container_placements() {
    let result = decode(
        &[
            (
                "Outer",
                "App::Part",
                vec![
                    members(&["Inner"]),
                    placement("Placement", [10.0, 0.0, 0.0], true),
                ],
            ),
            (
                "Inner",
                "App::Part",
                vec![
                    members(&["Mesh"]),
                    placement("Placement", [2.0, 0.0, 0.0], false),
                ],
            ),
            (
                "Mesh",
                "Mesh::Feature",
                mesh_properties([1.0, 0.0, 0.0], false),
            ),
        ],
        &[],
    );
    assert_vertices(
        &result,
        0,
        [[10.0, 3.0, 0.0], [10.0, 5.0, 0.0], [7.0, 3.0, 0.0]],
    );
    assert!(result.report().losses.is_empty());
}

#[test]
fn application_mesh_links_select_prototype_placement_once() {
    for prototype_transform in [false, true] {
        let mut link = linked("Mesh", prototype_transform);
        link.push(placement("LinkPlacement", [10.0, 20.0, 30.0], false));
        let result = decode(
            &[
                (
                    "Mesh",
                    "Mesh::Feature",
                    mesh_properties([100.0, 200.0, 300.0], true),
                ),
                ("Link", "App::Link", link),
            ],
            &[],
        );
        assert_eq!(result.ir().model.tessellations.len(), 2);
        assert_vertices(
            &result,
            0,
            [
                [100.0, 200.0, 300.0],
                [100.0, 202.0, 300.0],
                [97.0, 200.0, 300.0],
            ],
        );
        let expected = if prototype_transform {
            [
                [110.0, 220.0, 330.0],
                [110.0, 222.0, 330.0],
                [107.0, 220.0, 330.0],
            ]
        } else {
            [[10.0, 20.0, 30.0], [12.0, 20.0, 30.0], [10.0, 23.0, 30.0]]
        };
        assert_vertices(&result, 1, expected);
        assert_eq!(
            result
                .ir()
                .model
                .tessellations
                .iter()
                .filter_map(|mesh| mesh.source_object.as_ref())
                .filter(|source| !source.instance_path.is_empty())
                .map(|source| source.instance_path.len())
                .next()
                .unwrap(),
            1
        );
        assert!(result.report().losses.is_empty());
    }
}

#[test]
fn application_mesh_link_array_restores_element_placements() {
    for expanded in [false, true] {
        let mut link = linked("Mesh", false);
        link.extend([
            placement("LinkPlacement", [10.0, 0.0, 0.0], false),
            r#"<Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>"#.to_owned(),
            format!(r#"<Property name="ShowElement" type="App::PropertyBool"><Bool value="{expanded}"/></Property>"#),
            r#"<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="placements"/></Property>"#.to_owned(),
        ]);
        let mut objects = vec![(
            "Mesh",
            "Mesh::Feature",
            mesh_properties([100.0, 0.0, 0.0], false),
        )];
        if expanded {
            link.push(r#"<Property name="ElementList" type="App::PropertyLinkList"><LinkList count="2"><Link value="A"/><Link value="B"/></LinkList></Property>"#.to_owned());
            objects.push((
                "A",
                "App::LinkElement",
                vec![placement("Placement", [1.0, 0.0, 0.0], false)],
            ));
            objects.push((
                "B",
                "App::LinkElement",
                vec![placement("Placement", [4.0, 0.0, 0.0], true)],
            ));
        }
        objects.push(("Array", "App::Link", link));
        let mut placements = 2_u32.to_le_bytes().to_vec();
        for value in [
            1.0_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0,
        ] {
            placements.extend_from_slice(&value.to_le_bytes());
        }
        let result = decode(&objects, &[("placements", &placements)]);
        assert_eq!(result.ir().model.tessellations.len(), 3);
        assert_vertices(
            &result,
            1,
            [[11.0, 0.0, 0.0], [13.0, 0.0, 0.0], [11.0, 3.0, 0.0]],
        );
        assert_vertices(
            &result,
            2,
            [[14.0, 0.0, 0.0], [14.0, 2.0, 0.0], [11.0, 0.0, 0.0]],
        );
        assert!(result.report().losses.is_empty());
    }
}

#[test]
fn application_mesh_linked_container_uses_relative_child_placement() {
    for prototype_transform in [false, true] {
        let mut link = linked("Part", prototype_transform);
        link.push(placement("LinkPlacement", [10.0, 0.0, 0.0], false));
        let result = decode(
            &[
                (
                    "Part",
                    "App::Part",
                    vec![
                        members(&["Mesh"]),
                        placement("Placement", [4.0, 0.0, 0.0], false),
                    ],
                ),
                (
                    "Mesh",
                    "Mesh::Feature",
                    mesh_properties([1.0, 0.0, 0.0], false),
                ),
                ("Link", "App::Link", link),
            ],
            &[],
        );
        assert_eq!(result.ir().model.tessellations.len(), 2);
        let x = if prototype_transform { 15.0 } else { 11.0 };
        assert_vertices(
            &result,
            1,
            [[x, 0.0, 0.0], [x + 2.0, 0.0, 0.0], [x, 3.0, 0.0]],
        );
        assert!(result.report().losses.is_empty());
    }
}

#[test]
fn application_mesh_nested_link_reports_withheld_placement() {
    let result = decode(
        &[
            (
                "Mesh",
                "Mesh::Feature",
                mesh_properties([1.0, 0.0, 0.0], false),
            ),
            ("Inner", "App::Link", linked("Mesh", true)),
            ("Outer", "App::Link", linked("Inner", true)),
        ],
        &[],
    );
    assert_eq!(result.ir().model.tessellations.len(), 2);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "fcstd/geometry.mesh-placement-not-transferred"));
}

#[test]
fn application_mesh_link_scale_transforms_prototype_translation_and_vertices() {
    let mut link = linked("Mesh", true);
    link.push(placement("LinkPlacement", [10.0, 0.0, 0.0], false));
    link.push(r#"<Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="-2" valueY="3" valueZ="1"/></Property>"#.to_owned());
    let result = decode(
        &[
            (
                "Mesh",
                "Mesh::Feature",
                mesh_properties([2.0, 0.0, 0.0], false),
            ),
            ("Link", "App::Link", link),
        ],
        &[],
    );
    assert_vertices(
        &result,
        1,
        [[6.0, 0.0, 0.0], [2.0, 0.0, 0.0], [6.0, 9.0, 0.0]],
    );
    assert!(result.report().losses.is_empty());
}

#[test]
fn application_mesh_subelement_link_withholds_unresolved_selection() {
    let mut link = linked("Mesh", true);
    link[0] = r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Mesh" count="1"><Sub value="Facet1"/></XLink></Property>"#.to_owned();
    let result = decode(
        &[
            (
                "Mesh",
                "Mesh::Feature",
                mesh_properties([1.0, 0.0, 0.0], false),
            ),
            ("Link", "App::Link", link),
        ],
        &[],
    );
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "fcstd/geometry.mesh-placement-not-transferred"));
}

#[test]
fn application_mesh_nested_link_fanout_has_a_shared_work_bound() {
    let child_names = (0..300)
        .map(|index| format!("Child{index}"))
        .collect::<Vec<_>>();
    let group_members = child_names.iter().map(String::as_str).collect::<Vec<_>>();
    let mut objects = vec![
        (
            "Mesh".to_owned(),
            "Mesh::Feature",
            mesh_properties([0.0; 3], false),
        ),
        ("Inner".to_owned(), "App::Link", linked("Mesh", true)),
        (
            "Part".to_owned(),
            "App::Part",
            vec![members(&group_members)],
        ),
    ];
    for name in &child_names {
        objects.push((name.clone(), "App::Link", linked("Inner", true)));
    }
    for index in 0..300 {
        objects.push((format!("Outer{index}"), "App::Link", linked("Part", true)));
    }
    let object_refs = objects
        .iter()
        .map(|(name, kind, properties)| (name.as_str(), *kind, properties.clone()))
        .collect::<Vec<_>>();
    let result = decode(&object_refs, &[]);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "fcstd/geometry.mesh-placement-bounded"));
    assert_eq!(result.ir().model.tessellations.len(), 2);
    assert_vertices(
        &result,
        0,
        [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
    );
}
