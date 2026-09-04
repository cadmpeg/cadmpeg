// SPDX-License-Identifier: Apache-2.0
//! Object-graph parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use crate::test_support::*;

#[test]
fn outer_object_graph_parser_reads_nested_heads_and_payload_fields() {
    use crate::object_graph::{PayloadField, PayloadSubtype};

    let graph = crate::object_graph::parse(&object_graph_stream()).unwrap();
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, Some(3));
    assert_eq!(graph.records[0].storage_ref, Some(4));
    assert_eq!(graph.records[0].subtype, PayloadSubtype::Mixed);
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Reference { value: 5, .. },
            PayloadField::Scalar {
                tag: 0x3a,
                value: 7,
                ..
            },
            PayloadField::Terminator
        ]
    ));
    assert_eq!(graph.records[1].subtype, PayloadSubtype::Blob);
}

#[test]
fn outer_object_graph_uses_the_unique_length_closing_child_frame() {
    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x7c, 0x0a, 0xff, 0xff, 0xff, 0xff, 0x82, 0x83],
            &[0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x84], &[0xfe]),
    ];
    let graph = crate::object_graph::parse(&object_graph_from_records(&records))
        .expect("length-closing object payload");
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(
        &graph.records[0].head[graph.records[0].head.len() - 2..],
        [
            crate::object_graph::HeadToken::Reference(2),
            crate::object_graph::HeadToken::Reference(3),
        ]
    );
}

#[test]
fn outer_object_graph_rejects_ambiguous_length_closing_child_frames() {
    let mut first = object_graph_record(&[0x04, 0x01, 0x82, 0x83], &[0xfe]);
    let fake = 8;
    first.splice(fake..fake, [0x7c, 0x0a, 0, 0, 0, 0]);
    let closing_len = u32::try_from(first.len() - fake).expect("fixture child length");
    first[fake + 2..fake + 6].copy_from_slice(&closing_len.to_le_bytes());
    let record_len = u32::try_from(first.len()).expect("fixture record length");
    first[2..6].copy_from_slice(&record_len.to_le_bytes());

    let second = object_graph_record(&[0x04, 0x01, 0x82, 0x84], &[0xfe]);
    assert!(crate::object_graph::parse(&object_graph_from_records(&[first, second])).is_none());
}

#[test]
fn outer_object_graph_requires_records_to_cover_the_root_extent() {
    let mut bytes = object_graph_stream();
    bytes.extend_from_slice(&[0xaa, 0xbb]);
    let declared_len = u32::try_from(bytes.len()).expect("fixture graph length");
    bytes[2..6].copy_from_slice(&declared_len.to_le_bytes());

    assert!(crate::object_graph::parse(&bytes).is_none());
}

#[test]
fn outer_object_graph_requires_a_final_payload_terminator() {
    for payload in [&[0xfe, 0xaa][..], &[0xe5, 1, 0, 0, 0, 0xfe][..]] {
        let bytes =
            object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], payload)]);
        assert!(crate::object_graph::parse(&bytes).is_none());
    }
}

#[test]
fn object_graph_payload_assigns_blobs_only_inside_the_terminator_boundary() {
    use crate::object_graph::PayloadField;

    let valid = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[0xe5, 1, 0, 0, 0, 0xaa, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&valid).expect("bounded blob");
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Blob {
                declared_len: 1,
                bytes,
                ..
            },
            PayloadField::Terminator
        ] if bytes.as_slice() == [0xaa]
    ));

    let unbounded = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[0xe5, 0xfd, 0xd8, 0xc1, 0x74, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&unbounded).expect("literal E5 atom");
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Atom {
                value: 0xe5,
                offset: 0
            },
            ..,
            PayloadField::Terminator
        ]
    ));
}

#[test]
fn object_graph_payload_preserves_the_complete_terminator_run() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04], &[0x83, 0xfe, 0xfe, 0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("multi-terminator payload");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Atom { value: 3, .. },
            crate::object_graph::PayloadField::Terminator,
            crate::object_graph::PayloadField::Terminator,
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_payload_reads_tagged_fixed_width_references() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[
            0x81, 0x80, 0xfe, 0x1e, 0, 0, 0x81, 0x32, 0xeb, 0, 0, 0, 0xfe,
        ],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("tagged fixed-width references");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Reference { value: 7934, .. },
            crate::object_graph::PayloadField::Reference { value: 235, .. },
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_lists_retain_direct_fixed_width_references() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[0x3b, 0x81, 0x32, 2, 0, 0, 0, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(matches!(
        native.object_graphs[0].records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 1,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
    ));
    assert_eq!(
        native.object_graphs[0].records[0].references[0].entity_id(),
        2
    );
}

#[test]
fn outer_object_graph_requires_a_stored_head_lead() {
    let bytes = object_graph_from_records(&[object_graph_record(&[], &[0xfe])]);
    assert!(crate::object_graph::parse(&bytes).is_none());
}

#[test]
fn outer_object_graph_accepts_one_length_closed_record() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("one-record object graph");

    assert_eq!(graph.records.len(), 1);
    assert_eq!(graph.records[0].owner_ref, Some(1));
    assert_eq!(graph.records[0].class_ref, Some(1));
    assert_eq!(
        graph.records[0].subtype,
        crate::object_graph::PayloadSubtype::Empty
    );
}

#[test]
fn outer_object_graph_preserves_inline_records() {
    let nested = object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]);
    let inline = inline_object_graph_record(&[
        0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
    ]);
    let graph = crate::object_graph::parse(&object_graph_from_records(&[nested, inline]))
        .expect("inline control record");

    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[1].lead, 0x10);
    assert!(graph.records[1].head.is_empty());
    assert_eq!(
        graph.records[1].inline_body.as_deref(),
        Some(&[0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,][..])
    );
    assert!(graph.records[1].payload.fields.is_empty());
}

#[test]
fn outer_object_graph_accepts_each_inline_layout() {
    let bodies = [
        vec![
            0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x82, 0xd3, 0x79, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd4, 0x33, 0x82, 0x32, 0xe6, 0x00, 0x00, 0x00, 0x32, 0xe4, 0x00, 0x00,
            0x00, 0x82, 0xb1, 0x81, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd4, 0x32, 0x82, 0x32, 0xe6, 0x00, 0x00, 0x00, 0x32, 0xe4, 0x00, 0x00,
            0x00, 0x82, 0xd1, 0xfd, 0x82, 0xd4, 0x34, 0x06,
        ],
    ];

    for body in bodies {
        let graph =
            crate::object_graph::parse(&object_graph_from_records(&[inline_object_graph_record(
                &body,
            )]))
            .expect("assigned inline control layout");
        assert_eq!(
            graph.records[0].inline_body.as_deref(),
            Some(body.as_slice())
        );
    }
}

#[test]
fn outer_object_graph_rejects_unassigned_childless_records() {
    let valid = [
        0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
    ];
    for index in [0, 1, 4, 10, 11] {
        let mut body = valid;
        body[index] ^= 1;
        assert!(crate::object_graph::parse(&object_graph_from_records(&[
            inline_object_graph_record(&body)
        ]))
        .is_none());
    }
    assert!(
        crate::object_graph::parse(&object_graph_from_records(&[inline_object_graph_record(
            &[0x10, 0xfe, 0x81, 0x06]
        )]))
        .is_none()
    );
}

#[test]
fn paired_entity_table_admits_an_opaque_childless_object_record() {
    let body = [0x00, 0x90, 0x32, 0x01, 0x00, 0x00, 0x00, 0x81, 0x81, 0x00];
    let bytes = object_graph_from_records(&[inline_object_graph_record(&body)]);
    let paired_roots = std::collections::HashMap::from([(0, 1)]);

    let [graph] = crate::object_graph::parse_all_with_paired_roots(&bytes, &paired_roots)
        .try_into()
        .expect("one entity-paired object graph");
    assert_eq!(graph.records[0].lead, 0x00);
    assert_eq!(
        graph.records[0].inline_body.as_deref(),
        Some(body.as_slice())
    );
    assert!(graph.records[0].head.is_empty());

    assert!(crate::object_graph::parse_all_with_paired_roots(
        &bytes,
        &std::collections::HashMap::from([(0, 2)]),
    )
    .is_empty());
}

#[test]
fn object_graph_payload_lists_keep_direct_fixed_width_atoms() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x3b, 0x81, 0x80, 0x78, 0x56, 0x34, 0x12, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("fixed-width list atom");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 1,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Atom {
            value: 0x1234_5678,
            offset: 2,
        }]
    ));
}

#[test]
fn object_graph_payload_preserves_nonterminal_fe_atoms() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x85, 0x81, 0xfe, 0x81, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("interior FE atom");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Atom { value: 5, .. },
            crate::object_graph::PayloadField::Reference {
                value: 0xfe,
                offset: 1,
            },
            crate::object_graph::PayloadField::Atom { value: 0x81, .. },
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_payload_lists_preserve_nonterminal_fe_atoms() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x3b, 0x82, 0xfe, 0x85, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("interior FE list atom");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 2,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[
            crate::object_graph::ListItem::Atom {
                value: 0xfe,
                offset: 2,
            },
            crate::object_graph::ListItem::Atom {
                value: 5,
                offset: 3,
            },
        ]
    ));
}

#[test]
fn outer_object_graph_keeps_adjacent_compact_head_references_separate() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83, 0x84],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("compact object head");
    let record = &graph.records[0];

    assert_eq!(record.owner_ref, Some(1));
    assert_eq!(record.class_ref, Some(3));
    assert_eq!(record.storage_ref, Some(4));
    assert_eq!(
        &record.head[2..],
        [
            crate::object_graph::HeadToken::Reference(1),
            crate::object_graph::HeadToken::Reference(3),
            crate::object_graph::HeadToken::Reference(4),
        ]
    );
}

#[test]
fn outer_object_graph_does_not_slide_head_roles_across_null_handles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x82, 0xff, 0xff, 0xff, 0xff, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("null-interrupted object head");
    let record = &graph.records[0];

    assert_eq!(record.owner_ref, Some(2));
    assert_eq!(record.class_ref, None);
    assert_eq!(record.storage_ref, None);
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_does_not_promote_unassigned_head_bytes() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0xe5, 0xff, 0xff, 0xff, 0xe4],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal head bytes");

    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[0].storage_ref, None);
    assert_eq!(
        &graph.records[0].head[2..],
        [
            crate::object_graph::HeadToken::Literal(0xe5),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xe4),
        ]
    );
}

#[test]
fn outer_object_graph_requires_the_head_separator_for_relations() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x82, 0x83, 0x84], &[0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("retained malformed head");

    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[0].storage_ref, None);
    assert!(graph.records[0]
        .head
        .iter()
        .any(|token| matches!(token, crate::object_graph::HeadToken::Reference(2))));
}

#[test]
fn outer_object_graph_reads_compact_owner_and_field_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x02, 0x82], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x82, 0x83, 0x84], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("compact heads");

    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[1].owner_ref, Some(2));
    assert_eq!(graph.records[1].class_ref, Some(3));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[2].owner_ref, Some(2));
    assert_eq!(graph.records[2].class_ref, Some(3));
    assert_eq!(graph.records[2].storage_ref, Some(4));
}

#[test]
fn outer_object_graph_reads_extended_compact_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x80, 0x83, 0, 0], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x80, 0xe8, 0x16, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.owner_ref, Some(2));
        assert_eq!(record.class_ref, None);
        assert_eq!(record.storage_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_incomplete_extended_compact_owner_framing() {
    for head in [
        &[0x12, 0x82, 0x80, 0x83, 0][..],
        &[0x12, 0x82, 0x80, 0x83, 0, 1][..],
        &[0x12, 0x80, 0x80, 0x83, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x16, 0x94, 0x80, 0x95, 22, 0, 0, 0x80, 0x96, 0, 0],
            &[0xfe],
        ),
        object_graph_record(&[0x16, 0x94, 0x80, 0x95, 0, 0, 0x80, 17, 28, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
        assert_eq!(record.owner_ref, Some(21));
    }
}

#[test]
fn outer_object_graph_reads_short_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0, 0], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x95, 0x80, 17, 21, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("short extended compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(21));
        assert_eq!(record.owner_ref, Some(0));
    }
}

#[test]
fn outer_object_graph_reads_reference_terminated_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x96, 0x80, 0x97, 0, 0], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x80, 0x96, 23, 0, 0, 0xd2, 0x2b], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x80, 123, 21, 0, 0, 0xd2, 0x2b], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("reference-terminated compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
    }
    assert_eq!(graph.records[0].storage_ref, Some(22));
    assert_eq!(graph.records[0].owner_ref, Some(0));
    for record in &graph.records[1..] {
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[1].owner_ref, Some(22));
    assert_eq!(graph.records[2].owner_ref, None);
    for record in &graph.records[1..] {
        assert!(matches!(
            record.head.last(),
            Some(crate::object_graph::HeadToken::Reference(300))
        ));
    }
}

#[test]
fn outer_object_graph_rejects_partial_reference_terminated_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x96, 23, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x96, 22, 0, 0, 0x97][..],
        &[0x16, 0x94, 0x80, 0x96, 23, 0, 0, 97][..],
        &[0x16, 0x94, 0x80, 0x80, 0x97, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_partial_short_extended_class_storage_owner_roles() {
    for head in [
        &[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0][..],
        &[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0, 1][..],
        &[0x16, 0x94, 0x80, 0x80, 0x96, 20, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_two_block_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[0x16, 0x94, 0x80, 95, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[0x16, 0x94, 0x80, 1, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("two-block extended compact heads");

    assert_eq!(graph.records[0].owner_ref, Some(21));
    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[1].owner_ref, None);
    assert_eq!(graph.records[2].owner_ref, None);
}

#[test]
fn outer_object_graph_retains_roles_before_a_literal_short_extended_owner() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x16, 0x94, 0x80, 66, 23, 0, 0, 0x80, 0x97, 0, 0],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal-owner extended head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(0));
    assert_eq!(record.owner_ref, None);
    assert_eq!(record.owner_literal, Some(66));
}

#[test]
fn outer_object_graph_rejects_partial_two_block_extended_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 25, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 24, 0, 0, 0x80, 0x96, 25, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 26, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_partial_extended_class_storage_owner_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x95, 22, 0, 1, 0x80, 0x96, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 0, 0, 0x80, 17, 29, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x80, 22, 0, 0, 0x80, 0x96, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_class_storage_owner_compact_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x16, 0x92, 0xd2, 0x2b, 0xd2, 0x39],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("class-storage-owner compact head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(18));
    assert_eq!(record.storage_ref, Some(300));
    assert_eq!(record.owner_ref, Some(314));
}

#[test]
fn outer_object_graph_retains_class_first_roles_before_an_unassigned_slot() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x95, 95], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 95, 0x96], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("class-first compact heads");

    assert_eq!(graph.records[0].class_ref, Some(20));
    assert_eq!(graph.records[0].storage_ref, Some(21));
    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[1].class_ref, Some(20));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[1].owner_ref, None);
}

#[test]
fn outer_object_graph_reads_null_lane_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b],
            &[0xfe],
        ),
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[
                0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 23, 0, 0,
            ],
            &[0xfe],
        ),
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 95, 23, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("null-lane compact head");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[0].owner_ref, Some(300));
    for record in &graph.records[1..] {
        assert_eq!(record.owner_ref, Some(0));
    }
}

#[test]
fn outer_object_graph_reads_terminal_null_lane_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminal null-lane head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(0));
    assert_eq!(record.owner_ref, Some(300));
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_rejects_incomplete_terminal_null_lane_roles() {
    for head in [
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b][..],
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b, 0x84][..],
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x83][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained terminal null-lane head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_terminal_lane_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x56, 0x94, 0x95, 0x96, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminal-lane compact head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(21));
    assert_eq!(record.owner_ref, Some(22));
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_reads_extended_terminal_lane_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x56, 0x94, 0x80, 0x96, 22, 0, 0, 0x80, 0x97, 0, 0, 0x83],
            &[0xfe],
        ),
        object_graph_record(
            &[0x56, 0x94, 0x80, 96, 23, 0, 0, 0x80, 97, 25, 0, 0, 0x83],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended terminal-lane heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[0].owner_ref, Some(22));
    assert_eq!(graph.records[1].owner_ref, None);
}

#[test]
fn outer_object_graph_rejects_incomplete_terminal_lane_roles() {
    for head in [
        &[0x56, 0x94, 0x95, 0x96][..],
        &[0x56, 0x94, 0x95, 0x96, 0x84][..],
        &[0x56, 0x94, 0x95, 0x80, 0x83][..],
        &[0x56, 0x94, 0x80, 0x96, 22, 0, 0, 0x80, 0x97, 0, 0, 0x84][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_incomplete_null_lane_roles() {
    for head in [
        &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff][..],
        &[0x1a, 0x94, 0x80, 0, 0, 0, 0, 0xd2, 0x2b][..],
        &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80][..],
        &[
            0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 24, 0, 0,
        ][..],
        &[
            0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 23, 0, 1,
        ][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_extended_owner_class_storage_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 22, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 0, 0, 0x80, 0x95, 0, 0], &[0xfe]),
        object_graph_record(
            &[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x80, 95, 22, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.owner_ref, Some(18));
        assert_eq!(record.class_ref, Some(0));
    }
    assert_eq!(graph.records[0].storage_ref, Some(21));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[2].storage_ref, Some(21));
    assert_eq!(graph.records[3].storage_ref, Some(21));
    assert_eq!(graph.records[4].storage_ref, None);
}

#[test]
fn outer_object_graph_rejects_incomplete_extended_owner_class_storage_roles() {
    for head in [
        &[0x52, 0x92, 0x80, 0x95, 22, 0, 0, 0x84][..],
        &[0x52, 0x92, 0x80, 0x95, 0, 0, 0x80, 0x96, 0, 0][..],
        &[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x80, 95, 23, 0, 0][..],
        &[0x52, 0x80, 0x80, 0x95, 22, 0, 0, 0x83][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn object_graph_payload_reads_fixed_width_escaped_values() {
    use crate::object_graph::PayloadField;

    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x83],
            &[
                0x80, 0x78, 0x56, 0x34, 0x12, 0x32, 2, 0, 0, 0, 0x32, 0xef, 0xcd, 0xab, 0x89, 0xfe,
            ],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
    ];
    let bytes = object_graph_from_records(&records);
    let graph = crate::object_graph::parse(&bytes).expect("fixed-width object payload");
    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 0x1234_5678,
                offset: 0,
            },
            PayloadField::Reference {
                value: 2,
                offset: 5,
            },
            PayloadField::Reference {
                value: 0x89ab_cdef,
                offset: 10,
            },
            PayloadField::Terminator,
        ]
    );
    let native =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&records));
    assert_eq!(
        native.object_graphs[0].records[0].references,
        [
            crate::native::CatiaObjectRecordReference::from_parts(
                2,
                5,
                crate::native::CatiaObjectRecordReferenceSource::Field,
                false,
                Some(native.object_graphs[0].records[1].id.clone()),
                native.object_graphs[0].records[1].design_object.clone(),
            ),
            crate::native::CatiaObjectRecordReference::from_parts(
                0x89ab_cdef,
                10,
                crate::native::CatiaObjectRecordReferenceSource::Field,
                false,
                None,
                None,
            ),
        ]
    );
}

#[test]
fn incomplete_object_payload_tags_do_not_consume_the_terminator() {
    for tag in [0x81, 0x3a, 0x39, 0x7a] {
        let bytes = object_graph_from_records(&[object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[tag, 0xfe],
        )]);
        let graph = crate::object_graph::parse(&bytes).expect("terminated tagged payload");
        let record = &graph.records[0];

        assert_eq!(
            record.payload.fields,
            [
                crate::object_graph::PayloadField::Atom {
                    value: u32::from(tag),
                    offset: 0,
                },
                crate::object_graph::PayloadField::Terminator,
            ]
        );
        assert!(
            crate::native::CatiaNative::decode(&bytes).object_graphs[0].records[0]
                .references
                .is_empty()
        );
    }
}

#[test]
fn incomplete_object_lists_do_not_assert_reference_links() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0x3b, 0x83, 0x81, 0x82, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.object_graphs[0].records[0].references.is_empty());
    assert!(native.design_objects[0].relations.is_empty());
    assert!(matches!(
        &native.object_graphs[0].records[0].payload.fields[0],
        crate::object_graph::PayloadField::List {
            declared_count: 3,
            items,
            ..
        } if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
    ));
}

#[test]
fn incomplete_object_list_tags_do_not_consume_the_payload_terminator() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[0x3b, 0x82, 0x81, 0x82, 0x81, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let record = &native.object_graphs[0].records[0];

    assert!(record.references.is_empty());
    assert!(native.design_objects[0].relations.is_empty());
    assert!(matches!(
        record.payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 2,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
    ));
}

#[test]
fn incomplete_object_list_headers_do_not_consume_the_payload_terminator() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x3b, 0xfe],
    )]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let record = &native.object_graphs[0].records[0];

    assert_eq!(
        record.payload.fields,
        [
            crate::object_graph::PayloadField::Atom {
                value: 0x3b,
                offset: 0,
            },
            crate::object_graph::PayloadField::Terminator,
        ]
    );
    assert!(record.references.is_empty());
    assert!(native.design_objects[0].relations.is_empty());
}

#[test]
fn outer_object_graph_resolves_class_names_from_following_schema() {
    let mut bytes = object_graph_stream();
    let graph_len = bytes.len();
    bytes.extend(value_block_stream(&[0x81]));
    let catalog_pos = bytes.len();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let graph = crate::object_graph::parse(&bytes).expect("object graph with schema");
    assert_eq!(graph.total_len, graph_len);
    assert_eq!(graph.catalog_pos, Some(catalog_pos));
    assert_eq!(graph.records[0].class_name.as_deref(), Some(""));
    assert_eq!(graph.records[1].class_name.as_deref(), Some("Sketch"));
    let mut native_bytes = entity_table_record(1);
    native_bytes.extend(entity_table_record(2));
    native_bytes.push(0xde);
    native_bytes.extend_from_slice(&bytes);
    let native = crate::native::CatiaNative::decode(&native_bytes);
    assert_eq!(
        native.object_graphs[0].catalog,
        Some(native.catalogs[0].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[0].class_entry,
        Some(native.catalogs[0].entries[3].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[1].class_entry,
        Some(native.catalogs[0].entries[4].id.clone())
    );
    assert_eq!(
        native.design_objects[0].field_classes,
        [
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[3].id.clone(),
                name: String::new(),
            },
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[4].id.clone(),
                name: "Sketch".to_string(),
            },
        ]
    );
    assert_eq!(
        native.design_objects[0].owner_class,
        Some(crate::native::CatiaDesignClass {
            entry: native.catalogs[0].entries[4].id.clone(),
            name: "Sketch".to_string(),
        })
    );
    assert_eq!(native.design_objects[0].owner_storage_ref, None);
}

#[test]
fn outer_object_graph_parser_preserves_every_root() {
    let first = object_graph_stream();
    let mut bytes = first.clone();
    bytes.extend(object_graph_vm_stream());
    let graphs = crate::object_graph::parse_all(&bytes);
    assert_eq!(graphs.len(), 2);
    assert_eq!(graphs[0].pos, 0);
    assert_eq!(graphs[1].pos, first.len());
}

#[test]
fn outer_object_graph_suppresses_roots_inside_framed_payloads() {
    let nested =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut payload = vec![0xe5];
    payload.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("fixture nested graph length")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&nested);
    payload.push(0xfe);
    let outer =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)]);

    let graphs = crate::object_graph::parse_all(&outer);
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].pos, 0);
}

#[test]
fn outer_object_graph_resolves_paged_class_ordinals() {
    let records = [
        object_graph_record(&[0x14, 0x01, 0x82, 0xd1, 0x88], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82], &[0xfe]),
    ];
    let mut bytes = object_graph_from_records(&records);
    let mut names = vec!["field"; 138];
    names[0] = "CATCatalogManager";
    names[1] = "catalogManager";
    names[2] = "catalogLinks";
    names[3] = "";
    names[137] = "Pad";
    let mut schema = vec![0x7c, 0x02, 0, 0, 0, 0, 0xd1, 0x8a];
    for name in names {
        schema.push(u8::try_from(name.len() + 1).expect("fixture schema name length"));
        schema.extend_from_slice(name.as_bytes());
    }
    let schema_len = u32::try_from(schema.len()).expect("fixture schema length");
    schema[2..6].copy_from_slice(&schema_len.to_le_bytes());
    bytes.extend(schema);
    let graph = crate::object_graph::parse(&bytes).expect("paged class graph");
    assert_eq!(graph.records[0].class_ref, Some(137));
    assert_eq!(graph.records[0].class_name.as_deref(), Some("Pad"));
}

#[test]
fn object_graph_payload_does_not_consume_terminator_as_fixed_width_atom_data() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x8d, 0x80, 0x8f, 0x81, 0x8b, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminator-bounded object payload");

    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 13,
                offset: 0,
            },
            PayloadField::Atom {
                value: 0,
                offset: 1,
            },
            PayloadField::Atom {
                value: 15,
                offset: 2,
            },
            PayloadField::Reference {
                value: 11,
                offset: 3,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn object_graph_payload_does_not_consume_terminator_as_paged_atom_data() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x8d, 0xd2, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminator-bounded paged atom");

    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 13,
                offset: 0,
            },
            PayloadField::Atom {
                value: 0xd2,
                offset: 1,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn outer_object_graph_vm_reads_lists_paged_atoms_and_null_handles() {
    use crate::object_graph::{HeadToken, ListItem, PayloadField, PayloadSubtype};

    let graph = crate::object_graph::parse(&object_graph_vm_stream()).unwrap();
    assert!(graph.records[0].head.contains(&HeadToken::NullHandle));
    assert_eq!(graph.records[0].subtype, PayloadSubtype::ListAggregator);
    assert!(matches!(
        &graph.records[0].payload.fields[0],
        PayloadField::List { items, .. }
            if items == &vec![
                ListItem::Reference {
                    value: 5,
                    offset: 2,
                },
                ListItem::Atom {
                    value: 6,
                    offset: 4,
                },
                ListItem::Atom {
                    value: 10,
                    offset: 6,
                },
            ]
    ));
}

#[test]
fn outer_object_graph_rejects_an_ambiguous_3c_bulk_row_id() {
    assert!(crate::object_graph::parse(&object_graph_ambiguous_3c_stream()).is_none());
}

#[test]
fn object_graph_payload_decodes_3c_bulk_table_rows() {
    use crate::object_graph::{BulkTableRow, PayloadField};

    let graph = crate::object_graph::parse(&object_graph_bulk_table_stream()).expect("bulk table");
    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::BulkTable {
                count: 0,
                table_count: 3,
                rows: vec![
                    BulkTableRow {
                        row_id: 17,
                        handle: 0x692f,
                        offset: 6,
                    },
                    BulkTableRow {
                        row_id: 257,
                        handle: 0x6931,
                        offset: 13,
                    },
                    BulkTableRow {
                        row_id: 5121,
                        handle: 0x6933,
                        offset: 21,
                    },
                ],
                offset: 0,
            },
            PayloadField::Scalar {
                tag: 0x3a,
                value: 5,
                offset: 32,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn object_graph_payload_keeps_3c_as_literal_when_no_bulk_extent_is_possible() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x3c, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal 3c payload");
    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 0x3c,
                offset: 0,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn outer_surface_alias_parser_reads_fixed_core() {
    use crate::object_graph::AliasLead;

    let rows = crate::object_graph::surface_aliases(&surface_alias_stream());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].lead, AliasLead::SurfaceSupportStorage);
    assert_eq!(rows[0].tag, 0x0012_3456);
    assert_eq!(rows[0].tag_raw, 0xab12_3456);
    assert_eq!(rows[0].entity_record_ordinal, 7);
    assert_eq!((rows[0].f2, rows[0].f3), (0x1122_3344, 0x5566_7788));
}

#[test]
fn outer_alias_parser_classifies_both_ordinal_linked_storage_leads() {
    use crate::object_graph::AliasLead;

    for (lead, expected) in [
        (0x8eu32, AliasLead::E5LinkedSurfaceStorage),
        (0x8fu32, AliasLead::OrdinalLinkedStorage8f),
    ] {
        let mut bytes = surface_alias_stream();
        bytes[..4].copy_from_slice(&lead.to_le_bytes());
        let [row] = crate::object_graph::surface_aliases(&bytes)
            .try_into()
            .expect("one ordinal-linked alias row");
        assert_eq!(row.lead, expected);
        assert_eq!(row.entity_record_ordinal, 7);
    }
}

#[test]
fn outer_alias_parser_retains_exact_unclassified_0133_lead() {
    use crate::object_graph::AliasLead;

    let mut bytes = surface_alias_stream();
    bytes[..4].copy_from_slice(&0x0000_0133u32.to_le_bytes());
    let [row] = crate::object_graph::surface_aliases(&bytes)
        .try_into()
        .expect("one unclassified alias row");
    assert_eq!(row.lead, AliasLead::Unclassified(0x0000_0133));
    assert_eq!(row.entity_record_ordinal, 7);
}

#[test]
fn outer_alias_parser_rejects_marker_literals_without_an_alias_lead() {
    for lead in [0u32, 0x15] {
        let mut bytes = surface_alias_stream();
        bytes[..4].copy_from_slice(&lead.to_le_bytes());
        assert!(crate::object_graph::surface_aliases(&bytes).is_empty());
    }
}

#[test]
fn outer_alias_parser_closes_group_header_and_overlapping_target_slot() {
    let mut bytes = vec![0x02, 0x00];
    bytes.extend_from_slice(&0xafu32.to_le_bytes());
    bytes.extend_from_slice(&0x148u32.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
    let mut alias = surface_alias_stream();
    alias[15..19].copy_from_slice(&0x0000_017bu32.to_le_bytes());
    bytes.extend(alias);

    let [row] = crate::object_graph::surface_aliases(&bytes)
        .try_into()
        .expect("one grouped alias row");
    let group = row.group.expect("exact group header");
    assert_eq!(group.prototype, 0xaf);
    assert_eq!(group.group_id, 0x148);
    assert_eq!(group.target_slot, 0x17b);
    assert_eq!(group.storage_prefix, [0x01, 0x00, 0x00, 0x00]);
    assert_eq!(row.entity_record_ordinal, 0x7b);

    bytes[10] = 1;
    let [row] = crate::object_graph::surface_aliases(&bytes)
        .try_into()
        .expect("one ungrouped alias row");
    assert!(row.group.is_none());
}

#[test]
fn outer_alias_group_parser_accepts_each_bounded_storage_prefix() {
    for storage in [
        &[0x00, 0x00, 0x00][..],
        &[0x01, 0x00, 0x00, 0x00],
        &[0x01, 0x01, 0x00, 0x7c, 0x02, 0x00, 0x00],
        &[0x01, 0x00, 0x01, 0x00, 0x7c, 0x02, 0x00, 0x00],
    ] {
        let mut bytes = vec![0x02, 0x00];
        bytes.extend_from_slice(&0xafu32.to_le_bytes());
        bytes.extend_from_slice(&0x147u32.to_le_bytes());
        bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
        bytes.extend_from_slice(storage);
        let mut alias = surface_alias_stream();
        alias.drain(..4);
        alias[11..15].copy_from_slice(&0x0000_017du32.to_le_bytes());
        bytes.extend(alias);

        let [row] = crate::object_graph::surface_aliases(&bytes)
            .try_into()
            .expect("one grouped alias row");
        let group = row.group.expect("bounded group storage");
        assert_eq!(group.storage_prefix, storage);
        assert_eq!(group.target_slot, 0x17d);
    }
}

#[test]
fn outer_surface_alias_parser_retains_zero_low_tag_bits() {
    let mut bytes = surface_alias_stream();
    bytes[8..12].copy_from_slice(&0xab00_0000u32.to_le_bytes());

    let rows = crate::object_graph::surface_aliases(&bytes);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tag, 0);
    assert_eq!(rows[0].tag_raw, 0xab00_0000);
    assert_eq!(rows[0].entity_record_ordinal, 7);
    assert_eq!((rows[0].f2, rows[0].f3), (0x1122_3344, 0x5566_7788));
}

#[test]
fn outer_surface_alias_parser_requires_the_lead_word() {
    let bytes = surface_alias_stream();
    assert!(crate::object_graph::surface_aliases(&bytes[4..]).is_empty());
}

#[test]
fn unresolved_7cd9_scanner_preserves_bounded_context_and_spacing() {
    let markers = crate::object_graph::markers_7cd9(&marker_7cd9_stream(), 5);
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].pos, 1);
    assert_eq!(markers[0].context, [0x7c, 0xd9, 1, 2, 3]);
    assert_eq!(markers[0].next_delta, Some(5));
    assert_eq!(markers[1].next_delta, None);
}
