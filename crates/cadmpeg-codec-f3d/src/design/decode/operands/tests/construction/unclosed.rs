// SPDX-License-Identifier: Apache-2.0

use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::test_support::indexed_header;

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";
const SCOPE: u32 = 12;

fn reference(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

/// A counted Extrude body group whose tail names `closing` where the grammar
/// requires the record index plus two.
fn group(bytes: &mut Vec<u8>, record_index: u32, closing: u32) -> DesignRecordHeader {
    let header = DesignRecordHeader {
        id: format!("{STREAM}:design-record-header#{}", bytes.len()),
        record_index,
        class_tag: "332".to_owned().try_into().unwrap(),
        byte_offset: Some(u64::try_from(bytes.len()).unwrap()),
    };
    indexed_header(bytes, *b"332", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    reference(bytes, 200);
    reference(bytes, 201);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(bytes, 300);
    bytes.extend_from_slice(&0x0000_0008_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&180u32.to_le_bytes());
    bytes.extend_from_slice(&0.125f64.to_le_bytes());
    bytes.extend_from_slice(&180u32.to_le_bytes());
    reference(bytes, closing);
    bytes.extend_from_slice(&[1, 0]);
    reference(bytes, record_index + 1);
    bytes.push(0);
    reference(bytes, SCOPE);
    indexed_header(bytes, *b"259", record_index);
    header
}

#[test]
fn an_unclosed_group_is_given_up_and_its_sibling_decodes() {
    let mut bytes = Vec::new();
    let closed = group(&mut bytes, 100, 102);
    let unclosed = group(&mut bytes, 400, 409);
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{SCOPE}"),
        DesignFeatureKind::Extrude,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 400]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let metadata =
        crate::test_support::zip_test::indexed_fixture_metastream(&bytes, &[100, 400], &[100, 400]);
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                ctx,
            );
            let groups = crate::design::decode::operands::decode_construction_operand_groups(
                ctx,
                &mut parts,
                scan,
                std::slice::from_mut(&mut scope),
                &[closed, unclosed.clone()],
            )
            .unwrap();
            assert_eq!(
                groups
                    .iter()
                    .map(|group| group.record_index)
                    .collect::<Vec<_>>(),
                [100]
            );
            let (_, native, fidelity, losses) = parts.into_parts();
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "record.unreadable");
            assert!(losses[0].message.contains("f3d.bulk_record"));
            let retained = fidelity.retained_records().values().next().unwrap();
            assert_eq!(
                retained.byte_len(),
                u64::try_from(bytes.len()).unwrap() - unclosed.byte_offset.unwrap()
            );
        });
    });
}
