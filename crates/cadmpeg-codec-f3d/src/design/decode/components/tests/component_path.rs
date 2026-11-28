// SPDX-License-Identifier: Apache-2.0
//! Component-path carriers of any length.

use crate::records::component_path::{
    ComponentPathTransform, DesignComponentPath, DesignComponentPathElement,
    DesignComponentPathElementKind as Kind,
};
use crate::test_support::{cross_document_reference, local_reference};

/// A `CommonData` component-path carrier with the given local path elements.
fn path_carrier(entity_id: u64, elements: &[(u64, u32)], transform: [[f64; 4]; 4]) -> Vec<u8> {
    path_carrier_with_tail(entity_id, elements, transform, true)
}

/// A carrier whose closing reference run has the tagged u32 run, or the two
/// closing references alone.
fn path_carrier_with_tail(
    entity_id: u64,
    elements: &[(u64, u32)],
    transform: [[f64; 4]; 4],
    tagged: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"297");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&u32::try_from(elements.len()).unwrap().to_le_bytes());
    for (record, discriminator) in elements {
        bytes.extend(local_reference(*record));
        bytes.extend_from_slice(&discriminator.to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    if tagged {
        bytes.push(2);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&42_u32.to_le_bytes());
        bytes.extend(local_reference(8));
    }
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn translation(x: f64) -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

#[test]
fn a_path_of_any_length_keeps_each_element_and_its_discriminator() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    for elements in [
        vec![(159, 61), (200, 0)],
        vec![(159, 61), (300, 0), (400, 1), (500, 2), (600, 0)],
        vec![
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0),
            (8, 3),
        ],
    ] {
        let bytes = path_carrier(11, &elements, translation(0.5));
        let carrier = crate::xref::component_path_carrier(&bytes, magic).expect("path");
        assert_eq!(carrier.carrier_record_index, 11);
        assert_eq!(carrier.elements, elements);
        assert_eq!(carrier.transform, Some(translation(0.5)));
    }
}

#[test]
fn a_current_stream_admits_the_closing_run_with_or_without_the_tagged_run() {
    let elements = [(578, 0), (582, 0), (592, 1), (624, 0)];
    let modern = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    for tagged in [true, false] {
        let bytes = path_carrier_with_tail(11, &elements, translation(0.5), tagged);
        let carrier = crate::xref::component_path_carrier(&bytes, modern).expect("path");
        assert_eq!(carrier.elements, elements);
        assert_eq!(carrier.transform, Some(translation(0.5)));
        assert!(!crate::xref::local_component_path_unadmitted(
            &bytes, modern
        ));
    }
    // The tagged run belongs to the current serializer magic alone.
    let tagged = path_carrier_with_tail(11, &elements, translation(0.5), true);
    assert!(crate::xref::component_path_carrier(&tagged, Some(1)).is_none());
    assert!(crate::xref::component_path_carrier(&tagged, None).is_none());
    let untagged = path_carrier_with_tail(11, &elements, translation(0.5), false);
    assert!(crate::xref::component_path_carrier(&untagged, Some(1)).is_some());
}

#[test]
fn a_mirrored_matrix_is_a_valid_placement_and_a_skewed_one_is_not() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let mut mirrored = translation(0.5);
    mirrored[0][0] = -1.0;
    let bytes = path_carrier(11, &[(159, 0), (200, 0)], mirrored);
    let carrier = crate::xref::component_path_carrier(&bytes, magic).expect("mirrored");
    assert_eq!(carrier.transform, Some(mirrored));
    // The strict two-element parser keeps its proper-rigid admission.
    assert!(crate::xref::local_component_path_pose(&bytes, magic).is_none());
    let mut skewed = translation(0.5);
    skewed[0][1] = 0.5;
    let bytes = path_carrier(11, &[(159, 0), (200, 0)], skewed);
    assert!(crate::xref::component_path_carrier(&bytes, magic).is_none());
}

#[test]
fn a_path_with_a_cross_document_element_is_not_a_local_path() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let mut bytes = path_carrier(11, &[(159, 0), (200, 0), (300, 0)], translation(0.5));
    let first = local_reference(159);
    let at = bytes
        .windows(first.len())
        .position(|window| window == first.as_slice())
        .unwrap();
    bytes.splice(
        at..at + first.len(),
        cross_document_reference(100, "aaaabbbb-cccc-dddd-eeee-ffff00001111"),
    );
    assert!(crate::xref::component_path_carrier(&bytes, magic).is_none());
}

#[test]
fn a_retained_path_is_checked_against_its_record() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = path_carrier(11, &[(159, 4), (200, 1)], translation(0.5));
    let carrier = crate::xref::component_path_carrier(&bytes, magic).unwrap();
    let path = DesignComponentPath {
        id: "f3d:Design/BulkStream.dat:design-component-path#11".to_owned(),
        carrier_record_index: carrier.carrier_record_index,
        elements: vec![
            DesignComponentPathElement {
                record_index: 159,
                kind: Kind::Component,
                discriminator: 4,
            },
            DesignComponentPathElement {
                record_index: 200,
                kind: Kind::Occurrence,
                discriminator: 1,
            },
        ],
        transform: Some(ComponentPathTransform::try_from(translation(0.5)).unwrap()),
        frame_bytes: bytes.into(),
    };
    assert!(crate::xref::component_path_matches(&path));
    let mut changed = path.clone();
    changed.elements[1].discriminator = 2;
    assert!(!crate::xref::component_path_matches(&changed));
    let mut moved = path;
    moved.transform = Some(ComponentPathTransform::try_from(translation(0.7)).unwrap());
    assert!(!crate::xref::component_path_matches(&moved));
}

/// The `CommonData` component-path type of the synthetic Design stream.
const PATH_TYPE: &str = "3B6814F4-D87B-46BD-AD9D-0BE07880F7BB";
const DESIGN_BULK: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

/// A path record whose class tag is the registered ordinal `256` and whose
/// primary entity is `entity_id`.
fn registered_path(entity_id: u64, tagged: bool) -> Vec<u8> {
    let mut bytes =
        path_carrier_with_tail(entity_id, &[(10, 0), (11, 0)], translation(0.5), tagged);
    bytes[4..7].copy_from_slice(b"256");
    bytes
}

/// The unread-path count that `decode_local_components` stores for a Design
/// stream holding `records` as component-path records.
fn unread_paths_of(records: &[Vec<u8>]) -> usize {
    use std::io::{Cursor, Write};

    let mut bulk = Vec::new();
    let mut index = Vec::new();
    for (ordinal, record) in records.iter().enumerate() {
        index.push((
            100 + u64::try_from(ordinal).unwrap(),
            cadmpeg_core::decode::u64_from_index(bulk.len()),
        ));
        bulk.extend_from_slice(record);
    }
    let entities = index.iter().map(|(entity, _)| *entity).collect::<Vec<_>>();
    let meta = crate::test_support::streams_fixture::design_metastream_with_records(
        &[(PATH_TYPE, "", 0, "CommonData", &entities)],
        &index,
    );
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_fixture::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(DESIGN_BULK, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let space = crate::records::recipes::DesignComponentNamingSpace {
        id: crate::ids::native_design_component_naming_space_id(DESIGN_BULK, 0),
        byte_offset: 0,
        component_record_index: 10,
        context_uuid: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned(),
        )
        .unwrap(),
        context_uuid_offset: 12,
        name: None,
        name_offset: None,
    };
    let streams = crate::test_support::zip_fixture::with_scan(&archive, |scan| {
        super::super::decode_local_components(scan, std::slice::from_ref(&space)).unwrap()
    });
    streams
        .get(&crate::ids::native_scope(DESIGN_BULK))
        .expect("a stream with a naming space")
        .unadmitted_paths
}

#[test]
fn the_decoder_counts_every_component_path_record_that_no_local_form_reads() {
    // Both closing forms are read.
    assert_eq!(unread_paths_of(&[registered_path(100, true)]), 0);
    assert_eq!(
        unread_paths_of(&[registered_path(100, true), registered_path(101, false)]),
        0
    );
    // A record that does not parse as a path is counted.
    let mut junk = Vec::new();
    junk.extend_from_slice(&3_u32.to_le_bytes());
    junk.extend_from_slice(b"256");
    junk.extend_from_slice(&101_u64.to_le_bytes());
    junk.extend_from_slice(&[0xff; 12]);
    assert_eq!(unread_paths_of(&[registered_path(100, true), junk]), 1);
    // A path with a cross-document first element is counted.
    let mut foreign = registered_path(101, true);
    let first = local_reference(10);
    let at = foreign
        .windows(first.len())
        .position(|window| window == first.as_slice())
        .unwrap();
    foreign.splice(
        at..at + first.len(),
        cross_document_reference(10, "aaaabbbb-cccc-dddd-eeee-ffff00001111"),
    );
    assert_eq!(unread_paths_of(&[registered_path(100, true), foreign]), 1);
}

#[test]
fn a_tag_byte_of_zero_or_one_after_the_counted_references_is_not_the_tagged_run() {
    let modern = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let elements = [(10, 0), (11, 0)];
    let tagged = path_carrier_with_tail(11, &elements, translation(0.5), true);
    let at = tagged.len() - (local_reference(3).len() * 2 + local_reference(8).len() + 4 + 4 + 1);
    assert_eq!(tagged[at], 2);
    for tag in [0_u8, 1] {
        let mut bytes = tagged.clone();
        bytes[at] = tag;
        assert!(crate::xref::component_path_carrier(&bytes, modern).is_none());
        assert!(crate::xref::local_component_path_unadmitted(&bytes, modern));
    }
}
