// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

use crate::test_support::*;

#[test]
fn om_multi_instance_output_lane_requires_consistent_counts_and_groups() {
    let mut payload = b"\xaa\x3a\x00\x00\x01\x00\x00\x00\x00\x25\x01\x07".to_vec();
    let mut row_index = 2;
    for selector in [2, 3, 4] {
        for ordinal in 2..=3 {
            payload.extend_from_slice(b"\x26\x27\x01\x02\x65\x01\x02");
            payload.extend_from_slice(&[selector, 0x28, ordinal, row_index]);
            row_index += 1;
        }
    }
    payload.extend_from_slice(b"\x00\x3b\x90\x3d\xea\x90\x3d\xeb\x01\x03\xbb");
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Multi Instance Output",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let lane = super::multi_instance_output_payload_lane(record).expect("complete output lane");
    assert_eq!(lane.offset, 209);
    assert_eq!(lane.declared_count, 7);
    assert_eq!(lane.selectors, [2, 2, 3, 3, 4, 4]);
    assert_eq!(lane.ordinals, [2, 3, 2, 3, 2, 3]);
    assert_eq!(lane.row_indices, [2, 3, 4, 5, 6, 7]);
    assert_eq!(lane.instance_count, 3);
    assert_eq!(lane.selector_offsets, [219, 230, 241, 252, 263, 274]);
    assert_eq!(
        lane.trailing_references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [15850, 15851]
    );
    assert_eq!(lane.trailing_references[0].offset, 280);
    assert_eq!(
        lane.trailing_references[0].raw_object_index,
        [0x90, 0x3d, 0xea]
    );

    let mut incomplete_group = payload.clone();
    incomplete_group[76] = 2;
    assert!(
        super::multi_instance_output_payload_lane(super::OperationRecord {
            bytes: &incomplete_group,
            payload: &incomplete_group,
            ..record
        })
        .is_none()
    );
    let mut wrong_row_index = payload.clone();
    wrong_row_index[77] = 6;
    assert!(
        super::multi_instance_output_payload_lane(super::OperationRecord {
            bytes: &wrong_row_index,
            payload: &wrong_row_index,
            ..record
        })
        .is_none()
    );
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::multi_instance_output_payload_lane(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_identical_instance_output_lane_requires_complete_ordered_rows() {
    let payload = b"\xaa\x34\x13\x01\x04\x14\x15\x01\x02\x16\x80\x20\x00\x02\
          \x14\x15\x01\x02\x16\x0f\x00\x03\
          \x14\x15\x01\x02\x16\x81\x23\x00\x04\
          \x00\x05\xe0\x7f\xff\xff\xff\x00\x00\xbb";
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "IDENTICAL INSTANCE OUTPUT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let lane = super::identical_instance_output_payload_lane(record)
        .expect("complete identical-instance lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.leading_schema_index, 0x34);
    assert_eq!(lane.count_schema_index, 0x13);
    assert_eq!(lane.row_schema_indices, [0x14, 0x15, 0x16]);
    assert_eq!(lane.declared_count, 4);
    assert_eq!(lane.selectors, [0x20, 0x0f, 0x123]);
    assert_eq!(
        lane.raw_selectors,
        [vec![0x80, 0x20], vec![0x0f], vec![0x81, 0x23]]
    );
    assert_eq!(lane.selector_offsets, [210, 219, 227]);

    let mut wrong_ordinal = payload.to_vec();
    wrong_ordinal[21] = 4;
    assert!(
        super::identical_instance_output_payload_lane(super::OperationRecord {
            bytes: &wrong_ordinal,
            payload: &wrong_ordinal,
            ..record
        })
        .is_none()
    );
    let mut wrong_terminal_count = payload.to_vec();
    wrong_terminal_count[32] = 4;
    assert!(
        super::identical_instance_output_payload_lane(super::OperationRecord {
            bytes: &wrong_terminal_count,
            payload: &wrong_terminal_count,
            ..record
        })
        .is_none()
    );
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::identical_instance_output_payload_lane(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_geometry_instance_reference_requires_one_complete_field() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Geometry Instance",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x44\x45\x00\xff\xff\xf1\x03\x21\x01\x02\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x01\x02";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::pattern_payload_references(record).expect("complete field");
    assert_eq!(field.references[0].object_index, 801);
    assert_eq!(field.references[0].offset, 205);

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(super::pattern_payload_references(super::OperationRecord {
        bytes: &ambiguous,
        payload: &ambiguous,
        ..record
    })
    .is_none());
}

#[test]
fn om_point_feature_header_requires_the_complete_leading_envelope() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "POINT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = super::point_feature_payload_header(record).expect("complete header");
    assert_eq!(header.reference.object_index, 7311);
    assert_eq!(header.reference.offset, 207);
    assert_eq!(header.mode, 0x02);

    let mut alternate_mode = payload.to_vec();
    alternate_mode[52] = 0x03;
    assert_eq!(
        super::point_feature_payload_header(super::OperationRecord {
            bytes: &alternate_mode,
            payload: &alternate_mode,
            ..record
        })
        .expect("alternate mode")
        .mode,
        0x03
    );

    for malformed_offset in [0, 10, 51, 72] {
        let mut malformed = payload.to_vec();
        malformed[malformed_offset] ^= 0x01;
        assert!(super::point_feature_payload_header(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none());
    }
    let mut unsupported_mode = payload.to_vec();
    unsupported_mode[52] = 0x04;
    assert!(super::point_feature_payload_header(super::OperationRecord {
        bytes: &unsupported_mode,
        payload: &unsupported_mode,
        ..record
    })
    .is_none());
    assert!(super::point_feature_payload_header(super::OperationRecord {
        bytes: &payload[..72],
        payload: &payload[..72],
        ..record
    })
    .is_none());
}

#[test]
fn om_point_feature_scalar_lane_spans_the_preceding_block_atomically() {
    let mut encoded = Vec::new();
    for value in [1.0_f64, -2.0, 3.5, 4.0, 5.25, -6.0] {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        encoded.extend_from_slice(&bytes);
    }
    let preceding = [vec![0xaa, 0xbb], encoded[..3].to_vec()].concat();
    let mut target = encoded[3..].to_vec();
    target.extend_from_slice(&[
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00,
    ]);
    target.push(0xcc);

    let lane = super::point_feature_scalar_lane(&preceding, &target).expect("complete lane");
    assert_eq!(lane.values, [1.0, -2.0, 3.5, 4.0, 5.25, -6.0]);
    assert_eq!(lane.raw_values.concat(), encoded);
    assert_eq!(lane.value_offsets, [2, 10, 18, 26, 34, 42]);

    let mut malformed = target.clone();
    malformed[45] = 0x01;
    assert!(super::point_feature_scalar_lane(&preceding, &malformed).is_none());
    assert!(super::point_feature_scalar_lane(&preceding[..2], &target).is_none());
    assert!(super::point_feature_scalar_lane(&preceding, &target[..63]).is_none());

    let mut nonfinite = target;
    nonfinite[5..13].copy_from_slice(&[0x6f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert!(super::point_feature_scalar_lane(&preceding, &nonfinite).is_none());
}

#[test]
fn om_draft_feature_references_require_one_complete_graph() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "DRAFT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let prefix = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49";
    let graph = b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff";
    let terminal = b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00";
    let payload = [prefix.as_slice(), graph.as_slice(), terminal.as_slice()].concat();
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = super::draft_feature_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .clone()
            .map(|reference| reference.object_index),
        [7036, 7037, 7038, 7039]
    );
    assert_eq!(
        field.references.map(|reference| reference.offset),
        [230, 235, 273, 280]
    );
    let lane = super::draft_feature_leading_index_lane(record).expect("complete index lane");
    assert_eq!(lane.declared_count, 3);
    assert_eq!(lane.indices, vec![(148, 224), (585, 226)]);
    assert_eq!(lane.raw_indices, vec![vec![0x80, 0x94], vec![0x82, 0x49]]);
    let terminal_lane = super::draft_feature_terminal_lane(record).expect("complete terminal lane");
    assert_eq!(terminal_lane.indices, [350, 184]);
    assert_eq!(terminal_lane.raw_indices, [[0x81, 0x5e], [0x80, 0xb8]]);
    assert_eq!(terminal_lane.index_offsets, [284, 286]);
    assert_eq!(terminal_lane.tail, [0x29, 0x29, 0x0c]);
    assert_eq!(terminal_lane.offset, 284);

    let mut malformed = payload.clone();
    malformed[53] = 0x00;
    assert!(
        super::draft_feature_payload_references(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
    let mut malformed_lane = payload.clone();
    malformed_lane[23] = 4;
    assert!(
        super::draft_feature_leading_index_lane(super::OperationRecord {
            bytes: &malformed_lane,
            payload: &malformed_lane,
            ..record
        })
        .is_none()
    );
    let ambiguous = [prefix.as_slice(), graph.as_slice(), graph.as_slice()].concat();
    assert!(
        super::draft_feature_payload_references(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
    assert!(
        super::draft_feature_payload_references(super::OperationRecord {
            bytes: &payload[..prefix.len() + graph.len() - 2],
            payload: &payload[..prefix.len() + graph.len() - 2],
            ..record
        })
        .is_none()
    );
    assert!(super::draft_feature_terminal_lane(super::OperationRecord {
        bytes: &payload[..payload.len() - 1],
        payload: &payload[..payload.len() - 1],
        ..record
    })
    .is_none());
}

#[test]
fn om_surface_feature_references_require_the_complete_common_envelope() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::surface_feature_payload_references(record).expect("complete envelope");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 598, 599, 600,]
    );

    let studio_payload = [&[0x14], &payload[1..]].concat();
    let studio = super::OperationRecord {
        label: super::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(super::surface_feature_payload_references(studio).is_some());

    let mut malformed = payload.to_vec();
    let last = malformed.len() - 1;
    malformed[last] = 0x00;
    assert!(
        super::surface_feature_payload_references(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), &payload[51..]].concat();
    assert!(
        super::surface_feature_payload_references(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_surface_feature_branches_require_one_complete_counted_group() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\xa0\x5a\x14\x13\x01\x02\x40\x01\x04\xf1\x1b\xf4\xf1\x1b\xf5\xf1\x1b\xf6\x01\x04\x00\x00\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xf7\x00\x81\x58\x01\x02\x40\x01\x05\xf1\x1b\xf8\xf1\x1b\xf9\xf1\x1b\xfa\xf1\x1b\xfb\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xfc\x00\x81\x1c\x00\x00\x00\x01\x03\x00\x00\x00\xff\xff\x01";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let group = super::surface_feature_payload_branches(record).expect("complete group");
    assert_eq!(group.family, 0x14);
    assert_eq!(group.header_code, 0x13);
    assert_eq!(group.branches.len(), 2);
    assert_eq!(group.branches[0].mode, 0x40);
    assert_eq!(group.branches[0].declared_count, 4);
    assert!(group.branches[0].witnessed);
    assert_eq!(group.branches[0].members.len(), 3);
    assert_eq!(group.branches[0].terminal.object_index, 7159);
    assert_eq!(group.branches[0].suffix, [0x81, 0x58, 0x01, 0x02]);
    assert_eq!(group.branches[1].declared_count, 5);
    assert!(!group.branches[1].witnessed);
    assert_eq!(group.branches[1].members.len(), 4);
    assert_eq!(group.branches[1].terminal.object_index, 7164);
    assert_eq!(group.branches[1].suffix, [0x81, 0x1c]);

    let studio_payload = [
        &payload[..payload.len() - 11],
        &[0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01],
    ]
    .concat();
    let studio = super::OperationRecord {
        label: super::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(super::surface_feature_payload_branches(studio).is_some());

    let mut malformed = payload.to_vec();
    malformed[19] = 0x03;
    assert!(
        super::surface_feature_payload_branches(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::surface_feature_payload_branches(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_sketch_payload_reference_field_is_counted_ordered_and_canonical() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKETCH",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x00\x01\x05\xf0\xff\xf1\x01\x00\xf1\x01\x01\xf1\x01\x02\x00\x00\xf1\x01\x03\x01\x00\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::sketch_payload_references(record).unwrap();
    assert_eq!(field.declared_count, 5);
    let references: [super::PayloadObjectReference; 5] =
        field.references.clone().try_into().unwrap();
    assert_eq!(
        references.clone().map(|reference| reference.object_index),
        [255, 256, 257, 258, 259]
    );
    assert_eq!(
        references.map(|reference| reference.offset),
        [204, 206, 209, 212, 217]
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.raw_object_index.as_slice())
            .collect::<Vec<_>>(),
        [
            &[0xf0, 0xff][..],
            &[0xf1, 0x01, 0x00][..],
            &[0xf1, 0x01, 0x01][..],
            &[0xf1, 0x01, 0x02][..],
            &[0xf1, 0x01, 0x03][..],
        ]
    );
    let zero = b"\x01\x00\x00\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = super::sketch_payload_references(super::OperationRecord {
        payload: zero,
        bytes: zero,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 0);
    assert_eq!(field.references.len(), 1);
    assert_eq!(field.references[0].object_index, 0x42);
    let two = b"\x01\x00\x01\x02\xf0\x41\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = super::sketch_payload_references(super::OperationRecord {
        payload: two,
        bytes: two,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 2);
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [0x41, 0x42]
    );

    let mut noncanonical = payload.to_vec();
    noncanonical[7] = 0;
    assert!(super::sketch_payload_references(super::OperationRecord {
        payload: &noncanonical,
        bytes: &noncanonical,
        ..record
    })
    .is_none());
    assert!(super::sketch_payload_references(super::OperationRecord {
        label: super::OperationLabel {
            value: "BLOCK",
            ..label
        },
        ..record
    })
    .is_none());
}

#[test]
fn om_extrude_profile_references_require_matching_witness_field() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::extrude_profile_references(record).unwrap();
    assert!(field.witnessed);
    let references = field.references;
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].object_index, 255);
    assert_eq!(references[0].raw_object_index, [0xf0, 0xff]);
    assert_eq!(references[0].offset, 205);
    assert_eq!(references[1].object_index, 256);
    assert_eq!(references[1].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(references[1].offset, 207);

    let without_witness = &payload[..14];
    let field = super::extrude_profile_references(super::OperationRecord {
        payload: without_witness,
        bytes: without_witness,
        ..record
    })
    .unwrap();
    assert!(!field.witnessed);
    assert_eq!(field.references.len(), 2);
    assert!(super::extrude_profile_references(super::OperationRecord {
        label: super::OperationLabel {
            value: "SKETCH",
            ..label
        },
        ..record
    })
    .is_none());
}

#[test]
fn om_extrude_header_decodes_shifted_ieee_scalars() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = super::extrude_payload_header(record).unwrap();
    assert_eq!(header.offset, 205);
    assert_eq!(header.scalars, [0.04, 0.038]);
    assert_eq!(header.raw_scalars.concat(), payload[5..21]);

    let mut invalid = payload.to_vec();
    invalid[5] = 0xf0;
    assert!(super::extrude_payload_header(super::OperationRecord {
        payload: &invalid,
        bytes: &invalid,
        ..record
    })
    .is_none());
}

#[test]
fn om_operation_terminal_discriminator_requires_one_complete_lane() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let lane = super::operation_terminal_discriminator(record).unwrap();
    assert_eq!(lane.offset, 200);
    assert_eq!(lane.type_indices, [351, 171]);
    assert_eq!(lane.raw_type_indices, [vec![0x81, 0x5f], vec![0x80, 0xab]]);
    assert_eq!(lane.type_index_offsets, [203, 205]);
    assert_eq!(lane.flags, [1, 2, 1, 1]);
    assert_eq!(lane.trailing_indices, [5, 255]);
    assert_eq!(lane.raw_trailing_indices, [vec![0x05], vec![0x80, 0xff]]);
    assert_eq!(lane.trailing_index_offsets, [220, 221]);

    let subtract = super::OperationRecord {
        label: super::OperationLabel {
            value: "SUBTRACT",
            ..label
        },
        ..record
    };
    assert_eq!(
        super::operation_terminal_discriminator(subtract),
        Some(lane.clone())
    );

    let truncated = &payload[..payload.len() - 1];
    assert!(
        super::operation_terminal_discriminator(super::OperationRecord {
            payload: truncated,
            bytes: truncated,
            ..record
        })
        .is_none()
    );

    let mut ambiguous = payload[..payload.len() - 1].to_vec();
    ambiguous.extend_from_slice(payload);
    assert!(
        super::operation_terminal_discriminator(super::OperationRecord {
            payload: &ambiguous,
            bytes: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_operation_body_scalar_clauses_preserve_body_order_and_branch() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x1c\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\xaa\x01\x02\x10\x43\xff\x11\x30\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let triples = super::operation_body_scalar_triples(record);
    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].body_reference_ordinal, 0);
    assert_eq!(triples[0].body_object_index, 66);
    assert_eq!(triples[0].branch, 0x1c);
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.value),
        [0.0, 3.0, -170.0]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.encoding),
        [
            super::PayloadScalarEncoding::Zero,
            super::PayloadScalarEncoding::Binary32,
            super::PayloadScalarEncoding::Binary64,
        ]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.offset),
        [106, 107, 111]
    );
    assert_eq!(
        triples[0]
            .scalars
            .each_ref()
            .map(|scalar| scalar.raw_value.as_slice()),
        [&bytes[6..7], &bytes[7..11], &bytes[11..19]]
    );
    assert_eq!(triples[1].body_reference_ordinal, 1);
    assert_eq!(triples[1].body_object_index, 67);
    assert_eq!(triples[1].branch, 0x11);
    assert_eq!(
        triples[1].scalars.each_ref().map(|scalar| scalar.value),
        [2.0, 0.0, 0.0]
    );
    let truncated = &bytes[..bytes.len() - 1];
    let truncated_triples = super::operation_body_scalar_triples(super::OperationRecord {
        bytes: truncated,
        payload: truncated,
        ..record
    });
    assert_eq!(truncated_triples.len(), 1);
    assert_eq!(truncated_triples[0], triples[0]);
}

#[test]
fn om_operation_body_branch_11_decodes_wrapped_member_lane_atomically() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SEW",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let members = super::operation_body_members(record);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].body_reference_ordinal, 0);
    assert_eq!(members[0].body_object_index, 66);
    assert_eq!(members[0].member_index, 127);
    assert_eq!(members[0].raw_member_index, [0x7f]);
    assert_eq!(members[0].offset, 122);
    assert_eq!(members[1].member_index, 1);
    assert_eq!(members[1].raw_member_index, [0x80, 0x01]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(super::operation_body_members(super::OperationRecord {
        bytes: truncated,
        payload: truncated,
        ..record
    })
    .is_empty());
}

#[test]
fn om_trim_body_branch_11_decodes_terminal_continuation_atomically() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let continuations = super::operation_body_11_continuations(record);
    assert_eq!(continuations.len(), 1);
    let continuation = &continuations[0];
    assert_eq!(continuation.body_reference_ordinal, 0);
    assert_eq!(continuation.body_object_index, 114);
    assert_eq!(continuation.continuation_index, 67);
    assert_eq!(continuation.raw_continuation_index, [0x80, 0x43]);
    assert_eq!(continuation.continuation_offset, 126);
    assert_eq!(continuation.terminal_object_index, 114);
    assert_eq!(continuation.raw_terminal_object_index, [0x72]);
    assert_eq!(continuation.terminal_offset, 131);

    let mut distinct_terminal = bytes.to_vec();
    distinct_terminal[31] = 0x71;
    assert_eq!(
        super::operation_body_11_continuations(super::OperationRecord {
            bytes: &distinct_terminal,
            payload: &distinct_terminal,
            ..record
        })[0]
            .terminal_object_index,
        113
    );

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        super::operation_body_11_continuations(super::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_operation_body_decodes_homogeneous_unwrapped_reference_lanes() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "OFFSET",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let compact = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: compact,
        payload_offset: 100,
        payload: compact,
        label,
    };
    let lanes = super::operation_body_reference_lanes(record);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].body_object_index, 110);
    assert_eq!(
        lanes[0].encoding,
        super::OperationBodyReferenceLaneEncoding::CompactIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| (value.object_index, value.offset))
            .collect::<Vec<_>>(),
        [(13, 111), (105, 113)]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\x80\x0d".as_slice(), b"\x69".as_slice()]
    );

    let objects =
        b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let object_record = super::OperationRecord {
        bytes: objects,
        payload: objects,
        ..record
    };
    let lanes = super::operation_body_reference_lanes(object_record);
    assert_eq!(
        lanes[0].encoding,
        super::OperationBodyReferenceLaneEncoding::PayloadObjectIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\xf1\x02\x9e".as_slice(), b"\xf0\x44".as_slice()]
    );

    let truncated = &objects[..objects.len() - 1];
    assert!(
        super::operation_body_reference_lanes(super::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..object_record
        })
        .is_empty()
    );

    let branch_11 =
        b"\x01\x02\x10\x70\xff\x11\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let lanes = super::operation_body_reference_lanes(super::OperationRecord {
        bytes: branch_11,
        payload: branch_11,
        ..record
    });
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].branch, 0x11);
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
}

#[test]
fn om_extrude_body_32_branch_decodes_counted_lanes() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let branch = super::extrude_payload_32_branch(record).unwrap();
    assert_eq!(branch.offset, 105);
    assert_eq!(branch.body_object_index, 115);
    assert!(branch.scalar.is_finite());
    assert_eq!(branch.raw_scalar, bytes[8..16]);
    assert_eq!(branch.atoms_be, [0x3d82_5600, 0x3d82_5700]);
    assert_eq!(branch.atom_offsets, [118, 122]);
    assert_eq!(branch.atom_indices, [598, 599]);
    assert_eq!(branch.first_indices, [43, 45, 44]);
    assert_eq!(
        branch.raw_first_indices,
        [vec![0x80, 0x2b], vec![0x80, 0x2d], vec![0x80, 0x2c]]
    );
    assert_eq!(branch.first_index_offsets, [128, 130, 132]);
    assert_eq!(branch.second_indices, [46, 119]);
    assert_eq!(
        branch.raw_second_indices,
        [vec![0x80, 0x2e], vec![0x80, 0x77]]
    );
    assert_eq!(branch.second_index_offsets, [136, 138]);
    assert_eq!(branch.terminal_object_index, 115);
    assert_eq!(branch.raw_terminal_object_index, [0x73]);
    assert_eq!(branch.terminal_offset, 142);

    let mut invalid = bytes.to_vec();
    invalid[36] = 0xff;
    assert!(super::extrude_payload_32_branch(super::OperationRecord {
        bytes: &invalid,
        payload: &invalid,
        ..record
    })
    .is_none());

    let mut invalid_atom = bytes.to_vec();
    invalid_atom[18] = 0x3c;
    assert!(super::extrude_payload_32_branch(super::OperationRecord {
        bytes: &invalid_atom,
        payload: &invalid_atom,
        ..record
    })
    .is_none());

    let mut wrong_terminal_body = bytes.to_vec();
    wrong_terminal_body[43] = 0x72;
    assert!(super::extrude_payload_32_branch(super::OperationRecord {
        bytes: &wrong_terminal_body,
        payload: &wrong_terminal_body,
        ..record
    })
    .is_none());
}

#[test]
fn om_block_construction_field_decodes_ordered_canonical_references() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "BLOCK",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let mut payload = vec![0x26, 0, 0, 1, 0, 0];
    for value in 1..=18u8 {
        payload.extend([0xf0, value]);
    }
    payload.extend([0x01, 0xf1, 0x01, 0x00]);
    payload.extend([0xff; 11]);
    payload.extend([0; 4]);
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = super::block_construction_references(record).unwrap();
    assert_eq!(field.control, 0x26);
    assert_eq!(field.references.len(), 19);
    assert_eq!(field.references[0].object_index, 1);
    assert_eq!(field.references[0].raw_object_index, [0xf0, 0x01]);
    assert_eq!(field.references[18].object_index, 256);
    assert_eq!(field.references[18].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(field.references[0].offset, 206);

    let mut invalid = payload.clone();
    invalid[42] = 0xf0;
    assert!(
        super::block_construction_references(super::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_boolean_operations_decode_counted_target_and_tools() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let operations = super::boolean_operations(bytes, 100);
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].kind, super::BooleanOperationKind::Subtract);
    assert_eq!(operations[0].target, 6494);
    assert_eq!(operations[0].raw_target, [0x90, 0x19, 0x5e]);
    assert_eq!(
        operations[0].target_offset,
        100 + bytes
            .windows(3)
            .position(|window| window == [0x90, 0x19, 0x5e])
            .unwrap()
    );
    assert_eq!(operations[0].tools, [6495, 6468, 6467, 6496]);
    assert_eq!(
        operations[0].raw_tools,
        [
            vec![0x90, 0x19, 0x5f],
            vec![0x90, 0x19, 0x44],
            vec![0x90, 0x19, 0x43],
            vec![0x90, 0x19, 0x60],
        ]
    );
    assert_eq!(
        operations[0].tool_offsets,
        [0x5f, 0x44, 0x43, 0x60].map(|low| {
            100 + bytes
                .windows(3)
                .position(|window| window == [0x90, 0x19, low])
                .unwrap()
        })
    );

    let mut invalid = bytes.to_vec();
    *invalid.last_mut().unwrap() = 1;
    assert!(super::boolean_operations(&invalid, 0).is_empty());
}

#[test]
fn om_index_accepts_length_framed_root_version_text() {
    let mut bytes = indexed_om_section();
    let marker = bytes
        .windows(b"\x04\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x04\x01\x0eNX 2027.3102\0")
        .expect("root record");
    bytes[marker + 2] = 0x0f;
    bytes.insert(marker + 3 + 12, b' ');
    let index = bytes
        .windows(4)
        .position(|window| window == 0u32.to_le_bytes())
        .expect("index");
    for ordinal in 2..4 {
        let at = index + ordinal * 4;
        let value = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) + 1;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert!(sections[0].records[0]
        .bytes
        .starts_with(b"\x04\x01\x0fNX 2027.3102 \0"));
}

#[test]
fn om_store_version_can_follow_control_prefix() {
    let bytes = b"\xff\x00prefix\x04\x01\x0eNX 2027.3102\0tail";
    let version = super::store_version(bytes, 100).expect("store version");
    assert_eq!(version.offset, 108);
    assert_eq!(version.value, "NX 2027.3102");
}

#[test]
fn om_offset_only_index_bounds_storage_blocks() {
    let bytes = offset_only_indexed_om_section();
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 0);
    assert_eq!(
        sections[0].control.as_ref().unwrap().bytes,
        &[0, 0, 0, 0, 0, 1, 0, 0]
    );
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(
        sections[0].column_storage.unwrap(),
        [sections[0].records[0].bytes, sections[0].records[1].bytes].concat()
    );
    assert_eq!(sections[0].records[0].object_id, None);
    assert!(sections[0].records[0].bytes.starts_with(b"\x04\x01\x0eNX "));
    assert_eq!(sections[0].records[1].object_id, None);
    assert!(sections[0].records[1].bytes.ends_with(b"\0"));
    let expressions = sections[0].numeric_expressions();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name, "length");
    assert_eq!(expressions[0].value, Some(25.0));
}

#[test]
fn om_indexed_layout_materializes_both_store_forms_without_semantic_drift() {
    for bytes in [indexed_om_section(), offset_only_indexed_om_section()] {
        let section = super::indexed_sections(&bytes)
            .into_iter()
            .next()
            .expect("indexed fixture has one section");
        let layout = super::IndexedSectionLayout::from_section(&section);
        assert_eq!(layout.materialize(&bytes), section);
    }
}

#[test]
fn om_offset_only_index_accepts_one_root_record_inside_control_block() {
    let bytes = control_root_offset_only_indexed_om_section();
    let sections = super::indexed_sections(&bytes);

    assert_eq!(sections.len(), 1);
    assert!(sections[0]
        .control
        .as_ref()
        .unwrap()
        .bytes
        .windows(b"NX 2027.3102".len())
        .any(|window| window == b"NX 2027.3102"));
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].bytes, &[0; 32]);
    assert_eq!(sections[0].numeric_expressions()[0].name, "length");
}

#[test]
fn om_offset_only_index_ignores_product_marker_crossing_record_boundary() {
    use cadmpeg_core::decode::View;

    let mut bytes = control_root_offset_only_indexed_om_section();
    let class_name = b"UGS::ModlFeature";
    let class_start = bytes
        .windows(class_name.len())
        .position(|window| window == class_name)
        .expect("class declaration");
    let index_start = class_start + class_name.len() + 1;
    let first = usize::try_from(View::u32_le_at(&bytes, index_start + 4).unwrap()).unwrap();
    let product = b"\x04\x01\x0eNX 2027.3102\0";
    let split = 3;
    bytes[first - split..first].copy_from_slice(&product[..split]);
    bytes[first..first + product.len() - split].copy_from_slice(&product[split..]);

    assert_eq!(super::indexed_sections(&bytes).len(), 1);
}

#[test]
fn om_product_record_count_respects_containment_boundaries() {
    let ranges = [
        super::ProductRecordRange { start: 10, end: 20 },
        super::ProductRecordRange { start: 30, end: 40 },
        super::ProductRecordRange { start: 50, end: 60 },
    ];

    assert_eq!(super::product_record_count_within(&ranges, 10, 20), 1);
    assert_eq!(super::product_record_count_within(&ranges, 11, 20), 0);
    assert_eq!(super::product_record_count_within(&ranges, 10, 19), 0);
    assert_eq!(super::product_record_count_within(&ranges, 20, 60), 2);
    assert_eq!(super::product_record_count_within(&ranges, 20, 50), 1);
}

#[test]
fn om_offset_only_index_requires_one_supported_product_record() {
    let mut duplicate = control_root_offset_only_indexed_om_section();
    let first_column = duplicate
        .windows(32)
        .position(|window| window == [0; 32])
        .expect("zero first column");
    let duplicate_product = b"\x04\x01\x0eNX 2027.3102\0";
    duplicate[first_column..first_column + duplicate_product.len()]
        .copy_from_slice(duplicate_product);
    assert!(super::indexed_sections(&duplicate).is_empty());

    let mut unsupported = control_root_offset_only_indexed_om_section();
    let product = unsupported
        .windows(b"\x05\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x05\x01\x0eNX 2027.3102\0")
        .expect("product record");
    unsupported[product] = 0x03;
    assert!(super::indexed_sections(&unsupported).is_empty());
}

#[test]
fn om_offset_store_control_values_require_complete_zero_prefixed_words() {
    assert_eq!(
        super::offset_store_control_values(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]),
        Some(vec![0x1234, 0x00ff_ffff])
    );
    assert!(super::offset_store_control_values(&[]).is_none());
    assert!(super::offset_store_control_values(&[0, 1, 2]).is_none());
    assert!(super::offset_store_control_values(&[1, 1, 2, 3]).is_none());
}

#[test]
fn om_offset_store_control_form_requires_one_complete_grammar() {
    assert_eq!(
        super::offset_store_control_form(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]),
        Some(super::OffsetStoreControlForm::ZeroPrefixed {
            values: vec![0x1234, 0x00ff_ffff],
        })
    );

    let mut product = vec![0, 0];
    product.extend_from_slice(&7u32.to_le_bytes());
    product.extend_from_slice(&0x1020u32.to_le_bytes());
    product.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert_eq!(
        super::offset_store_control_form(&product),
        Some(super::OffsetStoreControlForm::ProductAnchored {
            leading_value: Some((2, 0)),
            values: vec![7, 0x1020],
        })
    );

    product.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(super::offset_store_control_form(&product).is_none());
    assert!(super::offset_store_control_form(&[1, 2, 3, 4]).is_none());
}

#[test]
fn om_offset_store_index_rows_require_complete_exact_frames() {
    let first =
        b"\x2d\x02\x0b\x2a\x93\x8a\x03\x80\x18\x20\x20\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let second = b"\x2d\x02\x0b\x83\xb6\x93\x8a\x07\x80\x18\x20\x80\x4d\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let mut bytes = b"prefix".to_vec();
    bytes.extend_from_slice(first);
    bytes.extend_from_slice(b"gap");
    bytes.extend_from_slice(second);

    let rows = super::offset_store_index_rows(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].offset, 6);
    assert_eq!(rows[0].first_index, 42);
    assert_eq!(rows[0].raw_first_index, [0x2a]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].indices, [(24, 13), (32, 15), (32, 16), (65, 17)]);
    assert_eq!(
        rows[0].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x20], vec![0x41]]
    );
    assert_eq!(rows[1].first_index, 950);
    assert_eq!(rows[1].raw_first_index, [0x83, 0xb6]);
    assert_eq!(rows[1].flag, 7);
    assert_eq!(rows[1].indices, [(24, 38), (32, 40), (77, 41), (65, 43)]);
    assert_eq!(
        rows[1].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x80, 0x4d], vec![0x41]]
    );

    let mut null = first.to_vec();
    null[3] = 0xff;
    assert!(super::offset_store_index_rows(&null).is_empty());
    let mut other_flag = first.to_vec();
    other_flag[6] = 0x04;
    assert!(super::offset_store_index_rows(&other_flag).is_empty());
    let mut overlong = first.to_vec();
    overlong.insert(12, 0x01);
    assert!(super::offset_store_index_rows(&overlong).is_empty());
    assert!(super::offset_store_index_rows(&first[..first.len() - 1]).is_empty());
}

#[test]
fn om_color_table_requires_complete_names_indices_and_rgb_atoms() {
    let mut bytes = vec![0x02, 0x80, 0xd9, 0x01];
    for ordinal in 0..=216 {
        let name = if ordinal == 0 {
            "Background".to_string()
        } else {
            format!("Color {ordinal}")
        };
        bytes.push(u8::try_from(name.len() + 2).unwrap());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    bytes.extend_from_slice(&[
        0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a,
        0xc6, 0x01, 0x80, 0xd9, 0x80, 0xc8, 0x01, 0x01, 0x01,
    ]);
    for color_index in 1u16..=216 {
        bytes.push(0x05);
        if color_index < 128 {
            bytes.push(color_index as u8);
        } else {
            bytes.extend_from_slice(&[0x80, (color_index - 1) as u8]);
        }
        bytes.extend_from_slice(&[0x01, 0x80, 0xc8]);
        if color_index == 2 {
            bytes.extend_from_slice(&shifted_f64_bytes(2.0));
            let mut binary32 = 1.0_f32.to_be_bytes();
            binary32[0] += 0x10;
            bytes.extend_from_slice(&binary32);
            bytes.push(0x00);
        } else {
            bytes.extend_from_slice(&[0x01, 0x01, 0x01]);
        }
    }

    let tables = super::color_tables(&bytes);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].background_name, "Background");
    assert_eq!(tables[0].background_rgb, [1.0, 1.0, 1.0]);
    assert_eq!(tables[0].definitions.len(), 216);
    assert_eq!(tables[0].definitions[0].name, "Color 1");
    assert_eq!(tables[0].definitions[0].rgb, [1.0, 1.0, 1.0]);
    assert_eq!(tables[0].definitions[1].rgb, [0.5, 0.25, 0.0]);
    assert_eq!(tables[0].definitions[127].raw_color_index, [0x80, 0x7f]);

    let mut malformed = bytes.clone();
    *malformed.last_mut().unwrap() = 0x02;
    assert!(super::color_tables(&malformed).is_empty());
    let truncated = &bytes[..bytes.len() - 1];
    assert!(super::color_tables(truncated).is_empty());
}

#[test]
fn om_offset_store_linked_index_rows_require_complete_exact_frames() {
    let row = b"\x02\x0b\x83\x93\x93\x8c\x16\x24\xff\xff\x90\xfe\x20\x20\x41\x00\x47\x03\x04\x01\xc0\x44\x04\x00";
    let rows = super::offset_store_linked_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].first_index, (915, 2));
    assert_eq!(rows[0].raw_first_index, [0x83, 0x93]);
    assert_eq!(rows[0].discriminator, 0x16);
    assert_eq!(rows[0].target_index, (36, 7));
    assert_eq!(rows[0].raw_target_index, [0x24]);
    assert_eq!(rows[0].indices, [(32, 12), (32, 13), (65, 14)]);
    assert_eq!(rows[0].raw_indices, [vec![0x20], vec![0x20], vec![0x41]]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].mode, 4);

    let mut null = row.to_vec();
    null[7] = 0xff;
    assert!(super::offset_store_linked_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[6] = 0x15;
    assert!(super::offset_store_linked_index_rows(&discriminator).is_empty());
    let mut flag = row.to_vec();
    flag[17] = 0x04;
    assert!(super::offset_store_linked_index_rows(&flag).is_empty());
    let mut mode = row.to_vec();
    mode[18] = 0x06;
    assert!(super::offset_store_linked_index_rows(&mode).is_empty());
    let mut mode_seven = row.to_vec();
    mode_seven[18] = 0x07;
    assert_eq!(
        super::offset_store_linked_index_rows(&mode_seven)[0].mode,
        7
    );
    assert!(super::offset_store_linked_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_target_index_rows_require_complete_exact_frames() {
    let row =
        b"\x02\x01\x01\x01\x16\x3e\xff\xff\x90\xfe\x1e\x20\x58\x00\x47\x03\x07\x01\xc0\x44\x04\x00";
    let rows = super::offset_store_target_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target_index, (62, 5));
    assert_eq!(rows[0].raw_target_index, [0x3e]);
    assert_eq!(rows[0].indices, [(30, 10), (32, 11), (88, 12)]);
    assert_eq!(rows[0].raw_indices, [vec![0x1e], vec![0x20], vec![0x58]]);
    assert_eq!(rows[0].mode, 7);

    let mut null = row.to_vec();
    null[5] = 0xff;
    assert!(super::offset_store_target_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[4] = 0x17;
    assert!(super::offset_store_target_index_rows(&discriminator).is_empty());
    let mut suffix = row.to_vec();
    suffix[16] = 0x03;
    assert!(super::offset_store_target_index_rows(&suffix).is_empty());
    let mut mode_four = row.to_vec();
    mode_four[16] = 0x04;
    assert_eq!(super::offset_store_target_index_rows(&mode_four)[0].mode, 4);
    assert!(super::offset_store_target_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_control_class_lane_is_a_distinct_in_range_prefix() {
    let encode = |values: &[u32]| {
        values
            .iter()
            .flat_map(|value| {
                let bytes = value.to_le_bytes();
                [0, bytes[0], bytes[1], bytes[2]]
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        super::offset_store_control_class_ordinals(&encode(&[2, 0, 4, 8, 3])),
        Some(vec![2, 0])
    );
    assert!(super::offset_store_control_class_ordinals(&encode(&[2, 2, 4])).is_none());
    assert!(super::offset_store_control_class_ordinals(&encode(&[2, 4, 1])).is_none());
    assert_eq!(
        super::offset_store_control_class_ordinals(&encode(&[4, 8])),
        Some(vec![4])
    );
}

#[test]
fn om_registry_uses_length_framing_and_stays_outside_entity_payloads() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(b"\x10UGS::PayloadText");
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].types.len(), 1);
    assert_eq!(sections[0].types[0].name, "UGS::EXP_expression");
    assert_eq!(sections[0].types[0].trailing_code, 0x81);
    assert_eq!(sections[0].types[0].offset, 8);
}

#[test]
fn om_numeric_expression_retains_identity_name_unit_and_value() {
    let bytes = indexed_om_section();
    let section = super::indexed_sections(&bytes).remove(0);
    let expression_records = section.numeric_expression_records();
    assert_eq!(expression_records[0].0, 1);
    let expressions = expression_records
        .iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].unit, super::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    let declaration = super::expression_declaration_name(section.records[1].bytes).unwrap();
    assert_eq!(
        declaration.value,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(declaration.parameter_index, 8);
    assert_eq!(
        declaration.qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(declaration.literal, Some("120"));
    let declaration =
        super::expression_declaration_name(b"\x04\x04p1\0\x04\x0a-5.1 * 2\0").unwrap();
    assert_eq!(declaration.value, "p1");
    assert_eq!(declaration.literal, Some("-5.1 * 2"));
    let declaration =
        super::expression_declaration_name(b"\x04\x04p1\0\x04\x055.1\0\x04\x05120\0").unwrap();
    assert_eq!(declaration.literal, None);
    assert!(super::expression_declaration_name(b"\x04\x04p1\0\x04\x04p2\0").is_none());
    assert!(super::expression_declaration_name(b"\x04\x05p1-\0").is_none());
}

#[test]
fn om_numeric_expression_types_only_canonical_parameter_names() {
    for name in ["p12foo", "p12_", "p4294967296_radius"] {
        let text = format!("(Number [mm]) {name}: 5; ");
        let mut bytes = b"hostglobalvariables".to_vec();
        bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);

        let expressions = super::numeric_expressions(&bytes);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].name, name);
        assert_eq!(expressions[0].parameter_index, None);
        assert_eq!(expressions[0].qualifier, None);
    }
    assert!(super::expression_declaration_name(b"\x04\x08p12foo\0").is_none());
    assert!(super::expression_declaration_name(b"\x04\x06p12_\0").is_none());
}

#[test]
fn om_numeric_expression_evaluates_constant_arithmetic_formula() {
    let text = b"(Number [mm]) p9: (193.94 - 6) / 2 + 1.5e1; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = super::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].expression, "(193.94 - 6) / 2 + 1.5e1");
    assert_eq!(expressions[0].value, Some(108.97));
}

#[test]
fn om_numeric_expression_applies_power_before_unary_sign() {
    for (formula, expected) in [
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-2", 0.25),
        ("2^3^2", 512.0),
    ] {
        assert_eq!(
            super::evaluate_constant_expression(formula),
            Some(expected),
            "{formula}"
        );
    }
}

#[test]
fn om_numeric_expression_parser_handles_deep_nesting_without_recursion() {
    const DEPTH: usize = 16 * 1024;

    let nested = format!("{}1{}", "(".repeat(DEPTH), ")".repeat(DEPTH));
    assert_eq!(super::evaluate_constant_expression(&nested), Some(1.0));

    let unary = format!("{}1", "+".repeat(DEPTH));
    assert_eq!(super::evaluate_constant_expression(&unary), Some(1.0));

    let malformed = format!("{}1", "(".repeat(DEPTH));
    assert_eq!(super::evaluate_constant_expression(&malformed), None);
}

#[test]
fn om_string_value_requires_marker_length_printability_and_terminator() {
    let bytes = b"\x66\x32\x03\x0cSKETCH_001\0\x66\x32\x03\x03A\0\x66\x32\x03\x03A\x01";
    let values = super::string_values(bytes, 100);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].offset, 100);
    assert_eq!(values[0].value, "SKETCH_001");
    assert_eq!(values[1].value, "A");
}

#[test]
fn om_tagged_references_preserve_family_value_order_and_bounds() {
    let bytes = b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\xe0\x01";
    let references = super::references(bytes, 20);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 20);
    assert_eq!(references[0].kind, super::ReferenceKind::PersistentHandle);
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[1].offset, 25);
    assert_eq!(references[1].kind, super::ReferenceKind::Tagged28);
    assert_eq!(references[1].value, 0x0abc_def0);
}

#[test]
fn om_counted_record_references_require_a_complete_in_bounds_run() {
    let bytes = b"\xff\x01\x03\x90\x00\x02\x90\x00\x04\x01\x02\x90\x00\x05";
    let references = super::counted_record_references(bytes, 100, 5);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 103);
    assert_eq!(references[0].kind, super::ReferenceKind::RecordOrdinal16);
    assert_eq!(references[0].value, 2);
    assert_eq!(references[1].value, 4);
}

#[test]
fn om_record_references_require_adjacent_persistent_tagged_pairs() {
    let mut dense = b"ordinary-prefix".to_vec();
    for value in 1..=8u32 {
        dense.push(0xe0);
        dense.extend_from_slice(&value.to_be_bytes());
        dense.extend_from_slice(&(0xc000_0000 | value).to_be_bytes());
    }
    let references = super::record_references(&dense, 100);
    assert_eq!(references.len(), 16);
    assert_eq!(references[0].offset, 115);

    let mut unpaired = dense;
    unpaired.extend_from_slice(&[0xc0, 0, 0, 2]);
    assert_eq!(
        super::record_references(&unpaired, 0)
            .into_iter()
            .filter(|reference| reference.kind == super::ReferenceKind::Tagged28)
            .count(),
        8
    );

    let lone_tagged = [0xc0, 0, 0, 1];
    assert!(super::record_references(&lone_tagged, 0)
        .into_iter()
        .all(|reference| reference.kind != super::ReferenceKind::Tagged28));
}

#[test]
fn om_numeric_expression_table_is_independent_of_entity_indexing() {
    let bytes = b"hostglobalvariables\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00";
    let expressions = super::numeric_expressions(bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].value, Some(120.0));
}
