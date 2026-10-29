// SPDX-License-Identifier: Apache-2.0
//! World placement of shape payloads in containers and link uses.

use std::collections::HashMap;
use std::fmt::Write;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};
use cadmpeg_ir::transform::Transform;

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;

/// An object's placement, written both as its `Placement` property and as the
/// root location of its stored shape, which carries the same frame.
struct Frame {
    position: [f64; 3],
    quarter_turn: bool,
}

impl Frame {
    fn property(&self, name: &str) -> String {
        let [x, y, z] = self.position;
        let qz = usize::from(self.quarter_turn);
        format!(
            r#"<Property name="{name}" type="App::PropertyPlacement"><PropertyPlacement Px="{x}" Py="{y}" Pz="{z}" Q0="0" Q1="0" Q2="{qz}" Q3="1"/></Property>"#
        )
    }

    fn location_rows(&self) -> String {
        let [x, y, z] = self.position;
        let (cos, sin) = if self.quarter_turn { (0, 1) } else { (1, 0) };
        format!("{cos} {} 0 {x}\n{sin} {cos} 0 {y}\n0 0 1 {z}\n", -sin)
    }
}

fn located_vertex(frame: &Frame, local: [f64; 3]) -> String {
    let [x, y, z] = local;
    format!(
        "CASCADE Topology V1, (c) Matra-Datavision\nLocations 1\n1\n{}Curve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nVe 0.001 {x} {y} {z} 0 0 1001000 *\n+1 1",
        frame.location_rows()
    )
}

fn shape(object: &str) -> String {
    format!(
        r#"<Property name="Shape" type="Part::PropertyPartShape"><Part file="{object}.Shape.brp"/></Property>"#
    )
}

fn decode(objects: &[(&str, &str, Vec<String>)], payloads: &[(String, String)]) -> CadIr {
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
    let mut entries = vec![("Document.xml", document.as_bytes())];
    entries.extend(
        payloads
            .iter()
            .map(|(name, brep)| (name.as_str(), brep.as_bytes())),
    );
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&entries)),
            &DecodeOptions::default(),
        )
        .expect("placed shape archive")
        .ir()
        .clone()
}

/// Every world position of each payload's vertex: occurrence chain, then body
/// transform, then the stored point.
fn world_vertices(ir: &CadIr) -> HashMap<String, Vec<[f64; 3]>> {
    let occurrences = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.clone(), occurrence))
        .collect::<HashMap<_, _>>();
    let chain = |mut id| {
        let mut transform = Transform::identity();
        while let Some(occurrence) = occurrences.get(id) {
            let local = occurrence
                .effective_transform()
                .expect("occurrence transform")
                .expect("known occurrence placement");
            transform = local.compose(transform).expect("composed transform");
            match &occurrence.parent {
                OccurrenceParent::Root {} => break,
                OccurrenceParent::Occurrence { occurrence } => id = occurrence,
            }
        }
        transform
    };
    let mut uses = HashMap::<_, Vec<Transform>>::new();
    for occurrence in &ir.model.occurrences {
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            continue;
        };
        let definition = ir
            .model
            .product_definitions
            .iter()
            .find(|candidate| &candidate.id == definition)
            .expect("local prototype");
        for body in &definition.bodies {
            uses.entry(body.clone())
                .or_default()
                .push(chain(&occurrence.id));
        }
    }
    let mut world = HashMap::new();
    for body in &ir.model.bodies {
        let payload = body
            .id
            .as_str()
            .split(':')
            .find(|part| part.contains(".brp"))
            .expect("payload name")
            .to_owned();
        let own = body.transform.unwrap_or_else(Transform::identity);
        let point = ir.model.points.iter().find(|point| {
            ir.model
                .vertices
                .iter()
                .any(|vertex| vertex.point == point.id && vertex.id.as_str().contains(&payload))
        });
        let local = point.expect("payload vertex").position().get();
        let placements = uses
            .get(&body.id)
            .cloned()
            .unwrap_or_else(|| vec![Transform::identity()]);
        let mut positions = placements
            .into_iter()
            .map(|chain| {
                let p = chain
                    .compose(own)
                    .expect("body transform")
                    .apply_point(Point3::new(local.x, local.y, local.z))
                    .expect("finite point")
                    .get();
                [p.x, p.y, p.z]
            })
            .collect::<Vec<_>>();
        positions.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        world.insert(payload, positions);
    }
    world
}

fn assert_world(world: &HashMap<String, Vec<[f64; 3]>>, payload: &str, expected: &[[f64; 3]]) {
    let actual = &world[payload];
    let mut expected = expected.to_vec();
    expected.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    assert_eq!(actual.len(), expected.len(), "{payload}: {actual:?}");
    for (actual, expected) in actual.iter().zip(&expected) {
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a - e).abs() < 1.0e-9,
                "{payload}: {actual:?} != {expected:?}"
            );
        }
    }
}

#[test]
fn shape_features_apply_their_placement_once() {
    let outer = Frame {
        position: [10.0, 0.0, 0.0],
        quarter_turn: true,
    };
    let member = Frame {
        position: [1.0, 2.0, 0.0],
        quarter_turn: false,
    };
    let free = Frame {
        position: [5.0, 0.0, 0.0],
        quarter_turn: true,
    };
    let source = Frame {
        position: [0.0, 0.0, 7.0],
        quarter_turn: false,
    };
    let link = Frame {
        position: [0.0, 30.0, 0.0],
        quarter_turn: false,
    };
    let linked = |transform: bool| {
        vec![
            r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Source"/></Property>"#.to_owned(),
            format!(r#"<Property name="LinkTransform" type="App::PropertyBool"><Bool value="{transform}"/></Property>"#),
            link.property("LinkPlacement"),
        ]
    };
    for link_transform in [false, true] {
        let ir = decode(
            &[
                (
                    "Outer",
                    "App::Part",
                    vec![
                        r#"<Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Member"/></LinkList></Property>"#.to_owned(),
                        outer.property("Placement"),
                    ],
                ),
                ("Member", "Part::Feature", vec![shape("Member"), member.property("Placement")]),
                ("Free", "Part::Feature", vec![shape("Free"), free.property("Placement")]),
                ("Source", "Part::Feature", vec![shape("Source"), source.property("Placement")]),
                ("Link", "App::Link", linked(link_transform)),
            ],
            &[
                ("Member.Shape.brp".to_owned(), located_vertex(&member, [1.0, 0.0, 0.0])),
                ("Free.Shape.brp".to_owned(), located_vertex(&free, [1.0, 0.0, 0.0])),
                ("Source.Shape.brp".to_owned(), located_vertex(&source, [1.0, 0.0, 0.0])),
            ],
        );
        let world = world_vertices(&ir);
        // Outer turns (2, 2, 0) a quarter about z, then moves it by 10 along x.
        assert_world(&world, "Member.Shape.brp", &[[8.0, 2.0, 0.0]]);
        assert_world(&world, "Free.Shape.brp", &[[5.0, 1.0, 0.0]]);
        let link_use = if link_transform {
            [1.0, 30.0, 7.0]
        } else {
            [1.0, 30.0, 0.0]
        };
        assert_world(&world, "Source.Shape.brp", &[link_use, [1.0, 0.0, 7.0]]);
    }
}

fn group(name: &str, members: &[&str]) -> String {
    let mut links = String::new();
    for member in members {
        write!(links, r#"<Link value="{member}"/>"#).unwrap();
    }
    format!(
        r#"<Property name="{name}" type="App::PropertyLinkList"><LinkList count="{}">{links}</LinkList></Property>"#,
        members.len()
    )
}

#[test]
fn shape_feature_placement_is_its_shape_root_location() {
    let stored = Frame {
        position: [0.0, 0.0, 0.0],
        quarter_turn: false,
    };
    let location = Frame {
        position: [5.0, 0.0, 0.0],
        quarter_turn: true,
    };
    let ir = decode(
        &[
            (
                "Outer",
                "App::Part",
                vec![group("Group", &["Member"]), stored.property("Placement")],
            ),
            (
                "Member",
                "Part::Feature",
                vec![shape("Member"), stored.property("Placement")],
            ),
        ],
        &[(
            "Member.Shape.brp".to_owned(),
            located_vertex(&location, [1.0, 0.0, 0.0]),
        )],
    );
    assert_world(&world_vertices(&ir), "Member.Shape.brp", &[[5.0, 1.0, 0.0]]);
}

#[test]
fn link_group_element_keeps_its_own_placement() {
    let origin = Frame {
        position: [0.0, 0.0, 0.0],
        quarter_turn: false,
    };
    let member = Frame {
        position: [1.0, 2.0, 0.0],
        quarter_turn: true,
    };
    let ir = decode(
        &[
            (
                "Group",
                "App::LinkGroup",
                vec![
                    group("ElementList", &["Member"]),
                    origin.property("Placement"),
                ],
            ),
            (
                "Member",
                "Part::Feature",
                vec![shape("Member"), member.property("Placement")],
            ),
        ],
        &[(
            "Member.Shape.brp".to_owned(),
            located_vertex(&member, [1.0, 0.0, 0.0]),
        )],
    );
    assert_world(&world_vertices(&ir), "Member.Shape.brp", &[[1.0, 3.0, 0.0]]);
}

#[test]
fn listed_link_prototype_is_shown_only_through_its_links() {
    let feature = Frame {
        position: [0.0, 0.0, 7.0],
        quarter_turn: false,
    };
    let link = Frame {
        position: [0.0, 30.0, 0.0],
        quarter_turn: false,
    };
    let ir = decode(
        &[
            ("Body", "PartDesign::Body", vec![group("Group", &["Feature"])]),
            (
                "Feature",
                "Part::Feature",
                vec![shape("Feature"), feature.property("Placement")],
            ),
            (
                "Link",
                "App::Link",
                vec![
                    r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Feature"/></Property>"#.to_owned(),
                    r#"<Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>"#.to_owned(),
                    link.property("LinkPlacement"),
                ],
            ),
        ],
        &[(
            "Feature.Shape.brp".to_owned(),
            located_vertex(&feature, [1.0, 0.0, 0.0]),
        )],
    );
    assert_world(
        &world_vertices(&ir),
        "Feature.Shape.brp",
        &[[1.0, 30.0, 7.0]],
    );
}

#[test]
fn python_group_member_link_prototype_keeps_its_own_use() {
    let feature = Frame {
        position: [0.0, 0.0, 7.0],
        quarter_turn: false,
    };
    let link = Frame {
        position: [0.0, 30.0, 0.0],
        quarter_turn: false,
    };
    let ir = decode(
        &[
            (
                "Holder",
                "App::DocumentObjectGroupPython",
                vec![group("Group", &["Feature"])],
            ),
            (
                "Feature",
                "Part::Feature",
                vec![shape("Feature"), feature.property("Placement")],
            ),
            (
                "Link",
                "App::Link",
                vec![
                    r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Feature"/></Property>"#.to_owned(),
                    r#"<Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>"#.to_owned(),
                    link.property("LinkPlacement"),
                ],
            ),
        ],
        &[(
            "Feature.Shape.brp".to_owned(),
            located_vertex(&feature, [1.0, 0.0, 0.0]),
        )],
    );
    assert_world(
        &world_vertices(&ir),
        "Feature.Shape.brp",
        &[[1.0, 0.0, 7.0], [1.0, 30.0, 7.0]],
    );
}

#[test]
fn only_the_shape_property_is_a_definition_body() {
    let feature = Frame {
        position: [0.0, 0.0, 7.0],
        quarter_turn: false,
    };
    let origin = Frame {
        position: [0.0, 0.0, 0.0],
        quarter_turn: false,
    };
    let link = Frame {
        position: [0.0, 30.0, 0.0],
        quarter_turn: false,
    };
    let ir = decode(
        &[
            (
                "Feature",
                "Part::Feature",
                vec![
                    shape("Feature"),
                    r#"<Property name="AddSubShape" type="Part::PropertyPartShape"><Part file="Feature.AddSubShape.brp"/></Property>"#.to_owned(),
                    feature.property("Placement"),
                ],
            ),
            (
                "Link",
                "App::Link",
                vec![
                    r#"<Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Feature"/></Property>"#.to_owned(),
                    r#"<Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>"#.to_owned(),
                    link.property("LinkPlacement"),
                ],
            ),
        ],
        &[
            (
                "Feature.Shape.brp".to_owned(),
                located_vertex(&feature, [1.0, 0.0, 0.0]),
            ),
            (
                "Feature.AddSubShape.brp".to_owned(),
                located_vertex(&origin, [2.0, 0.0, 0.0]),
            ),
        ],
    );
    assert_world(
        &world_vertices(&ir),
        "Feature.Shape.brp",
        &[[1.0, 0.0, 7.0], [1.0, 30.0, 7.0]],
    );
    assert!(ir
        .model
        .product_definitions
        .iter()
        .flat_map(|definition| &definition.bodies)
        .all(|body| !body.as_str().contains("AddSubShape")));
}
