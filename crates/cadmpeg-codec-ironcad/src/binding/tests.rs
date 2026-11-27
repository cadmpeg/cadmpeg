// SPDX-License-Identifier: Apache-2.0

use super::{bind, Place, ResultSpan};
use crate::placement::{Role, SceneRecord};
use crate::shape_contents::{ObjectKind, SceneObject};
use crate::tree::Assembly;

fn record(name: &str, role: Role, id: Option<u32>) -> SceneRecord {
    SceneRecord {
        name: name.to_owned(),
        role,
        id,
        copy: false,
        paths: Vec::new(),
        transform: None,
        matrix_searched: false,
        version_unrecognized: false,
        embedded: None,
        properties: Vec::new(),
        unread_property: None,
    }
}

fn copy(name: &str, id: Option<u32>, paths: &[&[u32]]) -> SceneRecord {
    SceneRecord {
        copy: true,
        paths: paths.iter().map(|path| path.to_vec()).collect(),
        ..record(name, Role::Part, id)
    }
}

fn object(id: u32, name: &str, end: u64, kind: ObjectKind) -> SceneObject {
    SceneObject {
        id,
        name: name.to_owned(),
        end,
        kind,
    }
}

fn span(child_start: usize, end: usize, assembly: Option<usize>) -> ResultSpan {
    ResultSpan {
        child_start,
        end,
        assembly,
    }
}

#[test]
fn records_bind_by_name_and_identifier_and_copies_share_a_body() {
    let objects = [
        object(1, "Block", 100, ObjectKind::Body),
        object(0x31, "Part", 200, ObjectKind::Body),
        object(0x32, "Part", 200, ObjectKind::Body),
        object(0x33, "Part", 300, ObjectKind::Body),
    ];
    let records = [
        record("Part", Role::Part, None),
        copy("Part", Some(0x32), &[]),
        record("Part", Role::Part, Some(0x33)),
    ];
    let results = [span(50, 200, None), span(210, 300, None)];
    let binding = bind(&records, &objects, &results, &[]);
    assert_eq!(
        binding.places,
        [
            Some(Place::Part(0)),
            Some(Place::Part(0)),
            Some(Place::Part(1))
        ]
    );
}

#[test]
fn an_identifier_of_another_kind_falls_back_to_the_name() {
    let objects = [
        object(0x45, "Brewer", 200, ObjectKind::Body),
        object(0x53, "Brewer", 200, ObjectKind::Assembly),
    ];
    let records = [
        record("Brewer", Role::Part, Some(0x53)),
        record("Brewer", Role::Assembly, None),
    ];
    let results = [span(50, 200, Some(0))];
    let assemblies = [Assembly {
        parent: None,
        end: 200,
    }];
    let binding = bind(&records, &objects, &results, &assemblies);
    assert_eq!(
        binding.places,
        [Some(Place::Part(0)), Some(Place::Assembly(0))]
    );
}

#[test]
fn a_copy_at_an_empty_node_takes_the_body_its_path_names() {
    let objects = [
        object(0x56, "Stool", 208, ObjectKind::Body),
        object(0x57, "Stool", 300, ObjectKind::Body),
        object(0x57, "Stool", 400, ObjectKind::Body),
        object(0x40, "Scene", 500, ObjectKind::Assembly),
    ];
    let records = [
        copy("Stool", Some(0x56), &[&[0x40, 0x57]]),
        record("Stool", Role::Part, Some(0x57)),
    ];
    let results = [span(210, 300, None), span(310, 400, Some(0))];
    let assemblies = [Assembly {
        parent: None,
        end: 500,
    }];
    let binding = bind(&records, &objects, &results, &assemblies);
    assert_eq!(binding.places[0], Some(Place::Part(1)));
}

#[test]
fn an_unnamed_first_child_takes_the_next_object_and_a_repeat_binds_nothing() {
    let objects = [
        object(0x49, "Knob", 100, ObjectKind::Body),
        object(0x47, "Door", 200, ObjectKind::Body),
        object(0x4f, "Drawers", 200, ObjectKind::Assembly),
    ];
    let records = [
        record("", Role::Part, Some(0x4f)),
        record("", Role::Part, Some(0x47)),
        copy("", Some(0x47), &[]),
    ];
    let results = [span(10, 100, Some(0)), span(110, 200, Some(0))];
    let assemblies = [Assembly {
        parent: None,
        end: 200,
    }];
    let binding = bind(&records, &objects, &results, &assemblies);
    assert_eq!(
        binding.places,
        [Some(Place::Part(0)), Some(Place::Part(1)), None]
    );
    assert_eq!(binding.repeats, [false, false, true]);
}

#[test]
fn assemblies_that_end_together_take_their_nodes_in_closing_order() {
    let objects = [
        object(0x4d, "Drawer", 600, ObjectKind::Assembly),
        object(0x4e, "Drawer", 600, ObjectKind::Assembly),
        object(0x4f, "Drawers", 600, ObjectKind::Assembly),
    ];
    let records = [
        record("Drawer", Role::Assembly, None),
        copy("Drawer", None, &[]),
        record("Drawers", Role::Assembly, None),
    ];
    let assemblies = [
        Assembly {
            parent: Some(1),
            end: 600,
        },
        Assembly {
            parent: None,
            end: 600,
        },
    ];
    let binding = bind(&records, &objects, &[], &assemblies);
    assert_eq!(
        binding.places,
        [
            Some(Place::Assembly(0)),
            Some(Place::Assembly(0)),
            Some(Place::Assembly(1))
        ]
    );
}

#[test]
fn a_feature_object_inside_a_child_node_binds_no_record() {
    let objects = [
        object(1, "Block", 150, ObjectKind::Body),
        object(0x31, "Part", 200, ObjectKind::Body),
    ];
    let records = [record("Block", Role::Part, Some(1))];
    let results = [span(100, 200, None)];
    let binding = bind(&records, &objects, &results, &[]);
    assert_eq!(binding.places, [None]);
    assert_eq!(binding.repeats, [false]);
}
