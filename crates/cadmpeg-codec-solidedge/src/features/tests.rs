// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::design::feature::OperationFamily;

use super::{function_family, has_feature_objects, navigator, Kind, Listed, Navigator};
use crate::psm::{Cluster, SpaceMap, TypeTable};
use crate::test_support::{
    cluster_and_map, feature_document, navigator_body, part_body, type_table, Feature, Object,
};

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
        Navigator::Features(listed) => listed.into_iter().map(|entry| entry.kind).collect(),
        other => panic!("expected features, found {other:?}"),
    }
}

#[test]
fn navigator_with_no_feature_lists_none() {
    assert_eq!(read(&[]), Navigator::Features(Vec::new()));
}

#[test]
fn navigator_with_one_feature_reads_its_function() {
    assert_eq!(
        read(&[Feature::Part("JMProtrusionFunction")]),
        Navigator::Features(vec![Listed {
            object: 10,
            kind: Kind::Function(Some("JMProtrusionFunction".into())),
        }])
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
            Kind::Folder,
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
        ("JMMultiFlangeFunction", OperationFamily::SheetMetal),
        ("JMBaseContourFlangeFunction", OperationFamily::SheetMetal),
        ("JMHemAssocFunction", OperationFamily::SheetMetal),
        ("JMUnfoldFunction", OperationFamily::SheetMetal),
        ("JMRefoldFunction", OperationFamily::SheetMetal),
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
        Navigator::Features(_)
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
        },
        Listed {
            object: 44,
            kind: Kind::Unreadable,
        },
        Listed {
            object: 11,
            kind: Kind::Function(Some("JMHoleFunction".into())),
        },
    ];
    let mut losses = Vec::new();
    let features = super::timeline(&listed, &mut losses).unwrap();
    assert_eq!(features.len(), 2);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "feature.storage-unread");
}
