// SPDX-License-Identifier: Apache-2.0
//! Scene collection contracts.

use cadmpeg_core::convert::truncate_f64_to_u8;
use cadmpeg_core::decode::View;

use crate::dialect::Grammar;

use super::{diffuse_colour, option_fields, Scene};
use crate::test_support::hsf::{
    box_shells, translation, BodySpec, Item, ModelSpec, NodeSpec, Stream,
};
use crate::test_support::models::{
    solidworks_assembly, SW_DEEP_SEGMENT, SW_DEFAULT_COLOUR, SW_DEFAULT_HIDDEN_SEGMENT,
    SW_HIDDEN_SUB_SEGMENT,
};

fn part(segment: &str, name: &str, body: usize, user_name: bool) -> NodeSpec {
    NodeSpec::Part {
        segment: segment.into(),
        name: name.into(),
        body,
        matrix: Some(translation([0.001, 0.0, 0.0])),
        user_name,
    }
}

fn spec() -> ModelSpec {
    let cube = BodySpec {
        colour: Some([10, 20, 30]),
        subs: vec![box_shells([0.01; 3])],
    };
    ModelSpec {
        document: "doc.sldasm".into(),
        bodies: vec![cube.clone(), cube],
        tree: vec![
            NodeSpec::Sub {
                segment: "subassem1".into(),
                name: "inner".into(),
                children: vec![part("assempart2", "second", 1, false)],
            },
            part("assempart0", "first", 0, true),
        ],
        state: Some((
            "disp0".into(),
            vec![("config0/includes/assempart0".into(), false)],
        )),
    }
}

#[test]
fn option_strings_split_on_unquoted_commas() {
    assert_eq!(
        option_fields("path=a/b,vis=off,color=1 2 3"),
        [("path", "a/b"), ("vis", "off"), ("color", "1 2 3")]
    );
    assert_eq!(
        option_fields("name=`a,b`,x=1"),
        [("name", "a,b"), ("x", "1")]
    );
    assert_eq!(option_fields("junk"), []);
    assert_eq!(option_fields("k=`open"), [("k", "open")]);
}

#[test]
fn the_tree_bodies_names_and_slots_are_collected() {
    let bytes = spec().stream();
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    assert_eq!(scene.bodies.len(), 2);
    assert_eq!(scene.bodies.first().unwrap().colour, Some([10, 20, 30]));
    assert_eq!(
        scene
            .bodies
            .iter()
            .map(|b| b.slots.len())
            .collect::<Vec<_>>(),
        [6, 6]
    );
    let nodes: Vec<_> = scene
        .nodes
        .iter()
        .map(|n| (n.segment.as_str(), n.parent, n.is_instance))
        .collect();
    assert_eq!(
        nodes,
        [
            ("subassem1", None, false),
            ("assempart2", Some(0), true),
            ("assempart0", None, true)
        ]
    );
    let name = |index: usize| {
        let node = scene.nodes.get(index).unwrap();
        scene
            .texts
            .get(&node.refname_slot.unwrap())
            .unwrap()
            .as_str()
    };
    assert_eq!((name(0), name(1), name(2)), ("inner", "second", "first"));
    let inner = scene.nodes.first().unwrap();
    assert_eq!(scene.texts.get(&inner.name_slot.unwrap()).unwrap(), "inner");
    assert_eq!(inner.user_name.as_deref(), Some("inner"));
    assert_eq!(scene.nodes.get(1).unwrap().user_name, None);
    assert_eq!(
        scene.texts.get(&scene.document_slot.unwrap()).unwrap(),
        "doc.sldasm"
    );
    assert!(scene.nodes.get(1).unwrap().matrix.is_some());
    assert_eq!(
        scene.nodes.get(1).unwrap().body_paths,
        ["/include library/model_1_include/shared/body1"]
    );
    assert_eq!(scene.current_state.as_deref(), Some("disp0"));
    assert_eq!(scene.components.len(), 1);
    assert!(!scene.components.first().unwrap().visible);
    assert_eq!(scene.orphan_slots, 0);
    assert_eq!(scene.duplicate_objects, 0);
}

#[test]
fn shared_bodies_are_defined_once() {
    let mut s = spec();
    s.tree = vec![
        part("assempart0", "a", 0, false),
        part("assempart1", "b", 0, false),
    ];
    let bytes = s.stream();
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    assert_eq!(scene.bodies.len(), 1);
    assert_eq!(scene.bodies.first().unwrap().slots.len(), 6);
    assert_eq!(
        scene
            .nodes
            .iter()
            .filter(|n| !n.body_paths.is_empty())
            .count(),
        2
    );
}

#[test]
fn slots_outside_a_body_and_repeated_ids_are_counted() {
    let mut s = Stream::new();
    s.shell_placeholder().slot();
    s.shell(0, &[[0.0; 3]; 3], &[vec![0, 1, 2]]);
    s.shell(0, &[[0.0; 3]; 3], &[vec![0, 1, 2]]);
    s.raw(b"z");
    let scene = Scene::collect(View::over_retained(s.bytes()), Grammar::Resaved).unwrap();
    assert_eq!((scene.orphan_slots, scene.duplicate_objects), (1, 1));
    assert_eq!(scene.objects.len(), 1);
}

#[test]
fn walk_failures_carry_the_offset() {
    let mut s = Stream::new();
    s.open("a").raw(&[0x99]);
    let error = Scene::collect(View::over_retained(s.bytes()), Grammar::Resaved).unwrap_err();
    assert_eq!(error.offset, 3);
}

#[test]
fn instance_ids_beyond_32_bits_stop_the_collection() {
    let mut s = Stream::new();
    s.instance(0, u64::MAX, &[0.0; 12]).raw(b"z");
    let error = Scene::collect(View::over_retained(s.bytes()), Grammar::Resaved).unwrap_err();
    assert_eq!(error.reason, "shell instance id exceeds 32 bits");
}

#[test]
fn an_unused_item_type_still_walks() {
    let item = Item::Instance {
        source: 0,
        matrix: [0.0; 12],
    };
    let body = BodySpec {
        colour: None,
        subs: vec![vec![item]],
    };
    let bytes = ModelSpec {
        document: "d".into(),
        bodies: vec![body],
        tree: vec![part("instance0", "p", 0, false)],
        state: None,
    }
    .stream();
    assert!(Scene::collect(View::over_retained(&bytes), Grammar::Resaved).is_ok());
}

#[test]
fn material_colours_read_in_both_forms() {
    assert_eq!(
        diffuse_colour("diffuse color=(r=0.294118 g=0.294118 b=0.294118)"),
        Some([75, 75, 75])
    );
    assert_eq!(
        diffuse_colour(
            "diffuse color=`r=0.866667 g=0.909804 b=1.000000`,diffuse texture=(thread-mod)"
        ),
        Some([221, 232, 255])
    );
    assert_eq!(diffuse_colour(""), None);
    assert_eq!(diffuse_colour("diffuse color=(r=0.5 g=0.5)"), None);
    assert_eq!(diffuse_colour("diffuse color=(r=2.0 g=0.5 b=0.5)"), None);
    assert_eq!(diffuse_colour("diffuse color=(r=x g=0.5 b=0.5)"), None);
}

#[test]
fn a_solidworks_scene_collects_materials_bodies_and_states() {
    let bytes = solidworks_assembly().stream();
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::SolidWorks).unwrap();
    assert_eq!(scene.bodies.len(), 3);
    let [r, g, b] = SW_DEFAULT_COLOUR;
    let byte = |v: f32| truncate_f64_to_u8((f64::from(v) * 255.0).round()).unwrap();
    let first = scene.bodies.first().unwrap();
    assert_eq!(first.colour, Some([byte(r), byte(g), byte(b)]));
    assert_eq!(first.sub_colours, [Some([255, 0, 0])]);
    assert!(scene.bodies.get(1).unwrap().sub_colours.is_empty());
    assert_eq!(scene.bodies.get(1).unwrap().slots.len(), 6);

    let node = |segment: &str| scene.nodes.iter().find(|n| n.segment == segment).unwrap();
    assert_eq!(
        node(SW_DEFAULT_HIDDEN_SEGMENT).body_paths,
        [
            "/include library/shared/body0",
            "/include library/shared/body2"
        ]
    );
    assert!(node(SW_DEFAULT_HIDDEN_SEGMENT).hidden_by_default);
    assert!(!node(SW_DEEP_SEGMENT).hidden_by_default);
    assert_eq!(node("assempart1").colour, Some([255, 0, 0]));
    assert_eq!(node("assempart1").default_colour, Some([255, 0, 0]));
    assert_eq!(
        scene
            .nodes
            .iter()
            .find(|n| n.segment == "subassem3")
            .unwrap()
            .parent,
        scene.nodes.iter().position(|n| n.segment == "subassem2")
    );
    assert!(!node(SW_HIDDEN_SUB_SEGMENT).is_instance);

    // No `current=` key: the default state applies. The `name` slots of the
    // states are no components.
    assert_eq!(scene.current_state.as_deref(), Some("disp0"));
    assert_eq!(scene.components.len(), 5);
    assert_eq!(scene.orphan_slots, 0);
    assert_eq!(scene.duplicate_objects, 0);
}

#[test]
fn a_solidworks_stream_without_a_default_state_has_no_current_state() {
    let mut model = solidworks_assembly();
    model.default_state = None;
    let bytes = model.stream();
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::SolidWorks).unwrap();
    assert_eq!(scene.current_state, None);
}

#[test]
fn a_cross_body_instance_resolves_through_the_global_object_table() {
    let bytes = solidworks_assembly().stream();
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::SolidWorks).unwrap();
    let source_body = scene.bodies.first().unwrap();
    let instancing = scene.bodies.get(1).unwrap();
    let Some(super::Object::Instance(instance)) = instancing
        .slots
        .iter()
        .filter_map(|id| scene.objects.get(id))
        .find(|object| matches!(object, super::Object::Instance(_)))
    else {
        panic!("no instance in the second body");
    };
    let source = u32::try_from(instance.source).unwrap();
    assert!(source_body.slots.contains(&source));
    assert!(matches!(
        scene.objects.get(&source),
        Some(super::Object::Shell(_))
    ));
}

#[test]
fn the_re_saved_grammar_ignores_materials_and_default_states() {
    let mut s = Stream::new();
    s.open("/include library/model_1_include/shared/body0")
        .material("diffuse color=(r=1.0 g=0.0 b=0.0)")
        .close();
    s.raw(b"z");
    let scene = Scene::collect(View::over_retained(s.bytes()), Grammar::Resaved).unwrap();
    assert_eq!(scene.bodies.first().unwrap().colour, None);
    assert_eq!(scene.current_state, None);
}

/// The name option of an `assempart` with several keys.
const MULTI_KEY_OPTION: &str =
    "name=`\u{c9}l\u{e9}ment,\u{65e5}`,mbd_comp_name=`a=b`,showbits0=1,componentdescription=`x`";

#[test]
fn a_wide_and_a_narrow_option_name_a_node_alike() {
    let mut narrow = Stream::new();
    narrow
        .open("assempart0")
        .user_option("name=`part,one`,mbd_comp_name=`m`,showbits0=1")
        .close()
        .raw(b"z");
    let mut wide = Stream::new();
    wide.open("assempart0")
        .wide_user_option(MULTI_KEY_OPTION)
        .close()
        .raw(b"z");
    let name = |stream: &Stream| {
        let scene = Scene::collect(View::over_retained(stream.bytes()), Grammar::Resaved).unwrap();
        scene.nodes.first().unwrap().user_name.clone()
    };
    assert_eq!(name(&narrow).as_deref(), Some("part,one"));
    assert_eq!(name(&wide).as_deref(), Some("\u{c9}l\u{e9}ment,\u{65e5}"));
}

#[test]
fn geometry_side_objects_take_one_object_id_each() {
    let mut point = vec![b'X'];
    point.extend([0_u8; 12]);
    let mut s = Stream::new();
    s.open("assempart0")
        .open("hole markers")
        .open("axis99")
        .user_option("diameter=0.01")
        .line_pattern("edw-chain")
        .edge_pattern("edw-chain")
        .colour([191, 191, 191])
        .open("star")
        .marker(9)
        .close()
        .side_geometry(0, &[point.clone(), point].concat())
        .close()
        .slot()
        .close()
        .side_text(1, "after")
        .close()
        .slot()
        .close()
        .close()
        .raw(b"z");
    let scene = Scene::collect(View::over_retained(s.bytes()), Grammar::Resaved).unwrap();
    assert_eq!(scene.texts.get(&1).map(String::as_str), Some("after"));
    assert_eq!(scene.duplicate_objects, 0);
    assert_eq!(scene.orphan_slots, 0);
}

#[test]
fn the_late_solidworks_grammar_keeps_the_solidworks_tree_rules() {
    let bytes = solidworks_assembly().stream();
    let early = Scene::collect(View::over_retained(&bytes), Grammar::SolidWorks).unwrap();
    let late = Scene::collect(View::over_retained(&bytes), Grammar::SolidWorksLate).unwrap();
    assert_eq!(early.nodes.len(), late.nodes.len());
    assert_eq!(early.bodies.len(), late.bodies.len());
    assert_eq!(early.current_state, late.current_state);
    assert_eq!(early.components.len(), late.components.len());
    assert!(late.nodes.iter().any(|node| node.hidden_by_default));
}

#[test]
fn relative_library_paths_define_bodies_and_a_part_keeps_every_include() {
    let first = "../../../model_4_include/shared/body3";
    let second = "../../../model_4_include/shared/body4";
    let mut s = Stream::new();
    s.open("assempart0")
        .open("part")
        .modelling_matrix(&translation([0.001, 0.0, 0.0]));
    for path in [first, second] {
        s.open(path)
            .colour([1, 2, 3])
            .open("sub0")
            .shell_placeholder()
            .slot()
            .close()
            .close()
            .include(path);
    }
    s.close().close().raw(b"z");
    let scene = Scene::collect(View::over_retained(s.bytes()), Grammar::Resaved).unwrap();
    assert_eq!(scene.orphan_slots, 0);
    assert_eq!(scene.bodies.len(), 2);
    assert_eq!(scene.bodies.first().map(|b| b.slots.len()), Some(1));
    assert_eq!(scene.bodies.first().unwrap().colour, Some([1, 2, 3]));
    assert_eq!(scene.nodes.first().unwrap().body_paths, [first, second]);
}
