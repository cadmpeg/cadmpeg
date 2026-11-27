// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::design::feature::OperationFamily;

use super::{function_family, has_feature_objects, item_names, navigator, Kind, Listed, Navigator};
use crate::psm::{Cluster, SpaceMap, TypeTable};
use crate::test_support::{
    cluster_and_map, feature_document, names_stream, navigator_body, part_body, type_table,
    Feature, Object,
};

fn listed_of(features: &[Feature<'_>]) -> Vec<Listed> {
    match read(features) {
        Navigator::Features { listed, .. } => listed,
        other => panic!("expected features, found {other:?}"),
    }
}

/// The PSM streams of a document built by `feature_document`.
fn streams(file: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    use cadmpeg_container::compound::CompoundSnapshot;
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(file, &arena, &policy).unwrap();
    let snapshot = CompoundSnapshot::new(&ctx, root).unwrap();
    let read = |path: &str| {
        let entry = snapshot.stream(path).unwrap();
        snapshot.open(&ctx, entry).unwrap().window().to_vec()
    };
    let streams = (
        read("PSMcluster0"),
        read("PSMspacemap/0x00000000"),
        read("PSMtypetable"),
    );
    ctx.finish_session().unwrap();
    streams
}

fn read(features: &[Feature<'_>]) -> Navigator {
    let (cluster, map, types) = streams(&feature_document(features));
    let map = SpaceMap::parse(&map).unwrap();
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    navigator(&cluster, &TypeTable::parse(&types).unwrap())
}

fn kinds(navigator: Navigator) -> Vec<Kind> {
    match navigator {
        Navigator::Features { listed, .. } => listed.into_iter().map(|entry| entry.kind).collect(),
        other => panic!("expected features, found {other:?}"),
    }
}

#[test]
fn navigator_with_no_feature_lists_none() {
    assert_eq!(
        read(&[]),
        Navigator::Features {
            last: 0,
            listed: Vec::new()
        }
    );
}

#[test]
fn navigator_with_one_feature_reads_its_function() {
    assert_eq!(
        read(&[Feature::Part("JMProtrusionFunction")]),
        Navigator::Features {
            last: 10,
            listed: vec![Listed {
                object: 10,
                kind: Kind::Function(Some("JMProtrusionFunction".into())),
                suppressed: Some(false),
            }]
        }
    );
}

#[test]
fn navigator_lists_features_in_tree_order() {
    let found = kinds(read(&[
        Feature::Part("JMCutoutFunction"),
        Feature::Group,
        Feature::Part("JMRoundFunction"),
    ]));
    assert_eq!(
        found,
        vec![
            Kind::Function(Some("JMCutoutFunction".into())),
            Kind::Group,
            Kind::Function(Some("JMRoundFunction".into())),
        ]
    );
}

#[test]
fn feature_of_another_class_keeps_its_class_name() {
    assert_eq!(
        kinds(read(&[Feature::Other("CLSID_JMSketch")])),
        vec![Kind::Other("CLSID_JMSketch".into())]
    );
}

#[test]
fn folder_entry_reads_as_a_folder() {
    assert_eq!(
        kinds(read(&[
            Feature::Part("JMProtrusionFunction"),
            Feature::Other("CLSID_JMFeatureGroup"),
            Feature::Part("JMCutoutFunction"),
        ])),
        vec![
            Kind::Function(Some("JMProtrusionFunction".into())),
            Kind::Folder(Some(Vec::new())),
            Kind::Function(Some("JMCutoutFunction".into())),
        ]
    );
}

#[test]
fn function_names_map_to_families() {
    let table = [
        ("JMProtrusionFunction", OperationFamily::Extrude),
        ("JMCutoutFunction", OperationFamily::Extrude),
        ("JMRevolvedProtrusionFunction", OperationFamily::Revolve),
        ("JMMultiSweepProtrusionFunction", OperationFamily::Sweep),
        ("JMLoftedProtrusionFunction", OperationFamily::Loft),
        ("JMHoleFunction", OperationFamily::Hole),
        ("JMRoundFunction", OperationFamily::Fillet),
        ("JMChamferFunction", OperationFamily::Chamfer),
        ("JMThinwallAssocFunctionNew", OperationFamily::Shell),
        ("JMBaseTabFunction", OperationFamily::SheetMetal),
        ("JMFlangeFunction", OperationFamily::SheetMetal),
    ];
    for (name, family) in table {
        assert_eq!(function_family(name), Some(family), "{name}");
    }
    assert_eq!(function_family("JMOtherFunction"), None);
}

#[test]
fn unreadable_function_block_leaves_the_kind_unread() {
    // A function block whose name count runs past the object.
    let mut body = part_body("JMHoleFunction");
    let count_at = 132 - 8 + 4 + 6;
    body[count_at..count_at + 4].copy_from_slice(&900_u32.to_le_bytes());
    let classes = ["ReferencePlaneObject", "CLSID_JMPart", "Nav"];
    let objects = [
        Object {
            id: 1,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 2,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 3,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 4,
            form: 1,
            flags: 0,
            body,
        },
        Object {
            id: 5,
            form: 2,
            flags: 0,
            body: navigator_body([1, 2, 3], &[4]),
        },
    ];
    let (cluster, map) = cluster_and_map(&objects);
    let map = SpaceMap::parse(&map).unwrap();
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    let types = TypeTable::parse(&type_table(&classes, &[])).unwrap();
    assert_eq!(
        kinds(navigator(&cluster, &types)),
        vec![Kind::Function(None)]
    );
}

fn damaged(edit: impl FnOnce(&mut Vec<u8>)) -> Navigator {
    let (cluster, map, types) = streams(&feature_document(&[Feature::Group]));
    let map = SpaceMap::parse(&map).unwrap();
    let nav = map
        .items()
        .iter()
        .find(|item| item.id == 5000)
        .unwrap()
        .offset;
    let mut cluster = cluster;
    let mut object = cluster[nav..].to_vec();
    edit(&mut object);
    cluster.truncate(nav);
    cluster.extend_from_slice(&object);
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    navigator(&cluster, &TypeTable::parse(&types).unwrap())
}

#[test]
fn malformed_navigators_are_unread_not_absent() {
    // The name differs at its grammar position: after the frame, the flag word,
    // the GUID, two lists of 2 and 1 identifiers, the 147-byte block and the
    // length word.
    // An object with the navigator GUID and another name is not a part
    // navigator.
    let name_at = 8 + 4 + 16 + (4 + 8) + (4 + 4) + 147 + 4;
    assert_eq!(damaged(|object| object[name_at] ^= 1), Navigator::Absent);
    // The entry list runs past the object.
    assert_eq!(
        damaged(|object| object.truncate(object.len() - 12)),
        Navigator::Unread
    );
    // An entry pair does not start with zero.
    let last = |object: &mut Vec<u8>| {
        let at = object.len() - 4 - 8;
        object[at] = 1;
    };
    assert_eq!(damaged(last), Navigator::Unread);
    // More than one word after the final word.
    assert_eq!(
        damaged(|object| object.extend_from_slice(&[0; 8])),
        Navigator::Unread
    );
    // One more word after the final word is admitted.
    assert!(matches!(
        damaged(|object| object.extend_from_slice(&[0; 4])),
        Navigator::Features { .. }
    ));
}

#[test]
fn listed_object_the_cluster_lacks_reads_as_unreadable() {
    let classes = ["ReferencePlaneObject", "Nav"];
    let objects = [
        Object {
            id: 1,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 2,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 3,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 5,
            form: 1,
            flags: 0,
            body: navigator_body([1, 2, 3], &[44]),
        },
    ];
    let (cluster, map) = cluster_and_map(&objects);
    let map = SpaceMap::parse(&map).unwrap();
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    let types = TypeTable::parse(&type_table(&classes, &[])).unwrap();
    assert_eq!(kinds(navigator(&cluster, &types)), vec![Kind::Unreadable]);
}

#[test]
fn document_without_navigator_is_absent() {
    let classes = ["ReferencePlaneObject", "CLSID_JMPart"];
    let objects = [
        Object {
            id: 1,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 2,
            form: 1,
            flags: 0,
            body: part_body("JMHoleFunction"),
        },
    ];
    let (cluster, map) = cluster_and_map(&objects);
    let map = SpaceMap::parse(&map).unwrap();
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    let types = TypeTable::parse(&type_table(&classes, &[])).unwrap();
    assert_eq!(navigator(&cluster, &types), Navigator::Absent);
    assert!(has_feature_objects(&cluster, &types));
    let none = TypeTable::parse(&type_table(&classes[..1], &[])).unwrap();
    assert!(!has_feature_objects(&cluster, &none));
}

#[test]
fn part_state_bit_0x10_states_suppression() {
    let states = [(0x10, true), (0x00, false), (0x02, false), (0x12, true)];
    for (state, suppressed) in states {
        let listed = listed_of(&[Feature::PartState("JMHoleFunction", state)]);
        assert_eq!(listed[0].suppressed, Some(suppressed), "{state:#x}");
    }
}

#[test]
fn group_states_no_suppression() {
    assert_eq!(listed_of(&[Feature::Group])[0].suppressed, None);
}

#[test]
fn group_suppressed_state_is_the_u32_at_object_offset_12() {
    let states = [(0, Some(false)), (1, Some(true)), (2, None), (0x10, None)];
    for (state, suppressed) in states {
        let listed = listed_of(&[Feature::GroupState(state)]);
        assert_eq!(listed[0].suppressed, suppressed, "{state:#x}");
    }
}

fn wanted(ids: &[u32]) -> std::collections::BTreeSet<u32> {
    ids.iter().copied().collect()
}

#[test]
fn item_names_reads_the_records_of_the_wanted_objects() {
    let stream = names_stream(&[(11, "Housing"), (12, "Hole 1"), (13, "Group 1")]);
    let names = item_names(&stream, &wanted(&[11, 13]));
    assert_eq!(names.len(), 2);
    assert_eq!(names[&11], "Housing");
    assert_eq!(names[&13], "Group 1");
}

#[test]
fn item_names_reads_non_ascii_names() {
    let stream = names_stream(&[(11, "Gruppe \u{e4}\u{3b1}")]);
    assert_eq!(
        item_names(&stream, &wanted(&[11]))[&11],
        "Gruppe \u{e4}\u{3b1}"
    );
}

#[test]
fn item_names_drops_an_object_that_two_records_name_differently() {
    let stream = names_stream(&[(11, "First"), (11, "Second"), (12, "Same"), (12, "Same")]);
    let names = item_names(&stream, &wanted(&[11, 12]));
    assert_eq!(names.get(&11), None);
    assert_eq!(names[&12], "Same");
}

#[test]
fn item_names_ignores_a_name_without_its_zero_unit() {
    // The count claims three units; the last is not zero.
    let mut stream = 3_u32.to_le_bytes().to_vec();
    for unit in [0x41_u16, 0x42, 0x43] {
        stream.extend_from_slice(&unit.to_le_bytes());
    }
    stream.extend_from_slice(&11_u32.to_le_bytes());
    assert!(item_names(&stream, &wanted(&[11])).is_empty());
}

#[test]
fn item_names_ignores_a_name_with_a_control_unit_or_an_unwanted_identifier() {
    let mut stream = 3_u32.to_le_bytes().to_vec();
    for unit in [0x41_u16, 0x09, 0] {
        stream.extend_from_slice(&unit.to_le_bytes());
    }
    stream.extend_from_slice(&11_u32.to_le_bytes());
    stream.extend_from_slice(&names_stream(&[(12, "Other")]));
    assert!(item_names(&stream, &wanted(&[11])).is_empty());
}

#[test]
fn item_names_of_a_truncated_stream_is_empty() {
    let stream = names_stream(&[(11, "Housing")]);
    assert!(item_names(&stream[..stream.len() - 12], &wanted(&[11])).is_empty());
}

#[test]
fn navigator_reads_the_last_feature_identifier() {
    let (cluster, map, types) = streams(&crate::test_support::feature_document_last(
        &[Feature::Group, Feature::Group],
        Some(10),
    ));
    let map = SpaceMap::parse(&map).unwrap();
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    let found = navigator(&cluster, &TypeTable::parse(&types).unwrap());
    assert!(matches!(found, Navigator::Features { last: 10, .. }));
}

#[test]
fn folder_lists_its_members() {
    let listed = listed_of(&[Feature::Folder(&[11, 12]), Feature::Group, Feature::Group]);
    assert_eq!(listed[0].kind, Kind::Folder(Some(vec![11, 12])));
}

#[test]
fn folder_with_a_pair_marker_other_than_one_is_unread() {
    let listed = listed_of(&[Feature::FolderBadMarker(&[11]), Feature::Group]);
    assert_eq!(listed[0].kind, Kind::Folder(None));
}

#[test]
fn folder_count_beyond_its_bytes_is_unread() {
    // The count claims two pairs; one pair follows.
    let mut body = 2_u32.to_le_bytes().to_vec();
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&11_u32.to_le_bytes());
    let object = Object {
        id: 4,
        form: 1,
        flags: 0,
        body,
    };
    let objects = [
        Object {
            id: 1,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 2,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        Object {
            id: 3,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        },
        object,
        Object {
            id: 5,
            form: 2,
            flags: 0,
            body: navigator_body([1, 2, 3], &[4]),
        },
    ];
    let classes = ["ReferencePlaneObject", "CLSID_JMFeatureGroup", "Nav"];
    let (cluster, map) = cluster_and_map(&objects);
    let map = SpaceMap::parse(&map).unwrap();
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    let types = TypeTable::parse(&type_table(&classes, &[])).unwrap();
    assert_eq!(kinds(navigator(&cluster, &types)), vec![Kind::Folder(None)]);
}

#[test]
fn an_unreadable_listed_object_leaves_the_others_readable() {
    let classes = ["ReferencePlaneObject", "CLSID_JMPart", "Nav"];
    let plane = |id| Object {
        id,
        form: 0,
        flags: 0,
        body: vec![0; 8],
    };
    let objects = [
        plane(1),
        plane(2),
        plane(3),
        Object {
            id: 10,
            form: 1,
            flags: 0,
            body: part_body("JMProtrusionFunction"),
        },
        Object {
            id: 11,
            form: 1,
            flags: 0,
            body: part_body("JMHoleFunction"),
        },
        Object {
            id: 5,
            form: 2,
            flags: 0,
            body: navigator_body([1, 2, 3], &[10, 44, 11]),
        },
    ];
    let (cluster, map) = cluster_and_map(&objects);
    let map = SpaceMap::parse(&map).unwrap();
    let cluster = Cluster::parse(&cluster, &map).unwrap();
    let types = TypeTable::parse(&type_table(&classes, &[])).unwrap();
    assert_eq!(
        kinds(navigator(&cluster, &types)),
        vec![
            Kind::Function(Some("JMProtrusionFunction".into())),
            Kind::Unreadable,
            Kind::Function(Some("JMHoleFunction".into())),
        ]
    );
}

#[test]
fn the_timeline_states_the_readable_features_around_an_unreadable_one() {
    let listed = [
        Listed {
            object: 10,
            kind: Kind::Function(Some("JMProtrusionFunction".into())),
            suppressed: Some(false),
        },
        Listed {
            object: 44,
            kind: Kind::Unreadable,
            suppressed: None,
        },
        Listed {
            object: 11,
            kind: Kind::Function(Some("JMHoleFunction".into())),
            suppressed: Some(false),
        },
    ];
    let mut losses = Vec::new();
    let timeline =
        super::timeline(&listed, &std::collections::BTreeMap::new(), &mut losses).unwrap();
    assert_eq!(timeline.features.len(), 2);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "feature.storage-unread");
}

#[test]
fn a_folder_crossing_an_earlier_folder_is_not_stated() {
    let part = |object, name: &str| Listed {
        object,
        kind: Kind::Function(Some(name.into())),
        suppressed: Some(false),
    };
    let folder = |object, members: &[u32]| Listed {
        object,
        kind: Kind::Folder(Some(members.to_vec())),
        suppressed: None,
    };
    let listed = [
        folder(20, &[10, 11]),
        part(10, "JMProtrusionFunction"),
        folder(21, &[11, 12]),
        part(11, "JMHoleFunction"),
        part(12, "JMRoundFunction"),
    ];
    let mut losses = Vec::new();
    let timeline =
        super::timeline(&listed, &std::collections::BTreeMap::new(), &mut losses).unwrap();
    assert_eq!(timeline.features.len(), 3);
    assert_eq!(
        timeline
            .folders
            .iter()
            .map(|folder| (folder.first, folder.last))
            .collect::<Vec<_>>(),
        vec![(0, 1)]
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code.local_code() == "feature.folder-unread"));
}
