// SPDX-License-Identifier: Apache-2.0

#[test]
fn external_reference_record_slots_resolve_atomically_in_the_same_stream() {
    use crate::native::om::{
        external_reference_record_children, external_reference_record_string_uses,
        ExternalReference, ExternalReferenceRecord,
    };

    crate::test_support::with_decode_context(|ctx| {
        let references = (0..4)
            .map(|ordinal| ExternalReference {
                id: format!("reference#{ordinal}"),
                ordinal,
                path: format!("value-{ordinal}"),
                source_entry: "stream".into(),
                source_offset: 100 + u64::from(ordinal),
            })
            .collect::<Vec<_>>();
        let record = ExternalReferenceRecord {
            id: "record#7".into(),
            record_id: 7,
            declared_count: 2,
            id_slots: [0, 3, 1, 2],
            handles: crate::container::extref_handles::ExtrefHandles::new(vec![10, 20, 20])
                .unwrap(),
            tail_byte_len: 5,
            source_entry: "stream".into(),
            source_offset: 20,
        };
        let uses =
            external_reference_record_string_uses(ctx, std::slice::from_ref(&record), &references)
                .expect("complete string use lane");
        assert_eq!(uses.len(), 4);
        assert_eq!(uses[0].id, "nx:external-reference:record-string-use#7-0");
        assert_eq!(
            uses.iter()
                .map(|use_| u8::from(use_.slot))
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            uses.iter()
                .map(|use_| use_.string_index)
                .collect::<Vec<_>>(),
            [0, 3, 1, 2]
        );
        assert_eq!(uses[1].external_reference, "reference#3");
        assert_eq!(uses[1].source_offset, 31);
        let mut child_references = references.clone();
        child_references[0].path = "child.prt".into();
        let child_uses = external_reference_record_string_uses(
            ctx,
            std::slice::from_ref(&record),
            &child_references,
        )
        .expect("complete child string use lane");
        let children = external_reference_record_children(
            ctx,
            std::slice::from_ref(&record),
            &child_references,
            &child_uses,
        )
        .expect("complete child record");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].external_record, record.id);
        assert_eq!(children[0].name_reference, "reference#0");
        assert_eq!(children[0].directory_reference, "reference#1");
        assert!(external_reference_record_children(
            ctx,
            std::slice::from_ref(&record),
            &references,
            &uses
        )
        .expect("non-child record")
        .is_empty());

        let mut out_of_range = record.clone();
        out_of_range.id_slots[2] = 4;
        assert!(
            external_reference_record_string_uses(ctx, &[out_of_range], &references)
                .expect("unresolved slot")
                .is_empty()
        );
        let mut duplicate = references.clone();
        duplicate.push(references[0].clone());
        assert!(
            external_reference_record_string_uses(ctx, &[record], &duplicate)
                .expect("duplicate slot")
                .is_empty()
        );
    });
}
