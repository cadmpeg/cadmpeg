use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::test_support::{indexed_header, lp_utf16};

pub(crate) enum Prefix {
    Counted(u8),
    Cornered(bool),
}

pub(crate) fn fixture(
    prefix: Prefix,
    inside: Option<(u32, f64)>,
    outside: Option<(u32, f64)>,
    groups: &[u32],
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"412", 90);
    match prefix {
        Prefix::Counted(control) => {
            bytes.extend_from_slice(&[0; 9]);
            bytes.extend_from_slice(&[1, 1, 0, 0, 0, 1, control]);
        }
        Prefix::Cornered(rounded) => {
            bytes.extend_from_slice(&[0; 10]);
            bytes.extend_from_slice(&[1, u8::from(rounded)]);
        }
    }
    for value in [inside, outside] {
        match value {
            Some((record, _)) => marked(&mut bytes, record),
            None => bytes.push(0),
        }
    }
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&u32::try_from(groups.len()).unwrap().to_le_bytes());
    for record in groups {
        marked(&mut bytes, *record)
    }
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    let table_at = u64::try_from(bytes.len()).unwrap();
    let references = groups
        .iter()
        .copied()
        .chain(inside.map(|(record, _)| record))
        .chain(outside.map(|(record, _)| record))
        .collect();
    bytes.resize(500, 0);
    for (record, value) in [inside, outside].into_iter().flatten() {
        scalar(&mut bytes, record, 90, value)
    }
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#90",
        DesignFeatureKind::Shell,
        90,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.reference_count_offset = table_at;
            draft.reference_members = ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = 500;
            draft.frame_length = 500;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

pub(crate) fn scalar(bytes: &mut Vec<u8>, record: u32, owner: u32, value: f64) {
    let mut frame = vec![0; 104];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"277");
    frame[7..11].copy_from_slice(&record.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&owner.to_le_bytes());
    frame[40..48].copy_from_slice(&value.to_le_bytes());
    bytes.extend_from_slice(&frame);
    indexed_header(bytes, *b"261", record);
}

fn marked(bytes: &mut Vec<u8>, record: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}
