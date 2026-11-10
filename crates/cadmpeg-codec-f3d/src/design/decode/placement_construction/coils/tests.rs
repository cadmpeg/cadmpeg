// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::{indexed_header, lp_utf16};

fn fixture() -> (
    Vec<u8>,
    DesignParameterScope,
    DesignRecordHeader,
    DesignPlacementConstruction,
) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"333", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-1111-4111-8111-111111111111");
    lp_utf16(&mut bytes, "22222222-2222-4222-8222-222222222222");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"265", 100);
    indexed_header(&mut bytes, *b"301", 101);
    indexed_header(&mut bytes, *b"446", 102);
    indexed_header(&mut bytes, *b"429", 103);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&1331u64.to_le_bytes());
    bytes.extend_from_slice(&183u64.to_le_bytes());
    indexed_header(&mut bytes, *b"311", 104);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::CoilPrimitive,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 431;
            draft.paired_byte_offset = draft.byte_offset + 431;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                100, 41, 300, 301, 302, 303, 304, 305,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:design-record-header#0".into(),
        record_index: 100,
        byte_offset: 0,
        class_tag: "333".to_owned().try_into().unwrap(),
    };
    let placement = crate::test_support::placement_construction::grammar_placement(8, 0);
    (bytes, scope, header, placement)
}

#[test]
fn compact_coil_binds_registered_version_eight_frame() {
    let (bytes, scope, header, placement) = fixture();
    for identity in [false, true] {
        let mut placement = placement.clone();
        if identity {
            placement.fields.matrix = None;
            placement.fields.identity.as_mut().unwrap().value = true;
        }
        let resolved =
            resolved_placement(&bytes, &scope, std::slice::from_ref(&header), &[placement])
                .unwrap();
        assert_eq!(resolved.explicit_transform.is_none(), identity);
        assert_eq!(resolved.selection_record_index, 100);
        assert_eq!(resolved.transform_record_index, 41);
        assert!(matches!(
            resolved.selection,
            DesignCoilSelection::Persistent {
                identity_record_index: 103,
                primary_identity: 1331,
                ..
            }
        ));
        assert_eq!(
            resolved.transform().rows(),
            if identity {
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
            } else {
                crate::test_support::placement_construction::GRAMMAR_FRAME_ROWS
            }
        );
    }
}

#[test]
fn compact_coil_requires_unique_checked_local_carriers() {
    let (bytes, scope, header, placement) = fixture();
    let mut foreign = placement.clone();
    foreign.id = "f3d:Other/BulkStream.dat:design-placement-construction#0".into();
    let mut flipped = placement.clone();
    flipped.fields.z_flip.value = 1;
    let mut inferred = placement.clone();
    inferred.fields.inferred_z_axis.value = 1;
    let mut inferred_x = placement.clone();
    inferred_x.fields.inferred_x_axis.value = 1;
    let mut axis_flipped = placement.clone();
    axis_flipped.fields.axis_flipped.as_mut().unwrap().value = 1;
    let mut version = placement.clone();
    version.type_version = 7;
    let mut reflected = placement.clone();
    let mut matrix = reflected.fields.matrix.unwrap();
    let mut rows = matrix.value.rows();
    for row in &mut rows {
        row[0] = -row[0];
    }
    matrix.value = rows.try_into().unwrap();
    reflected.fields.matrix = Some(matrix);
    for placements in [
        vec![],
        vec![foreign],
        vec![flipped],
        vec![inferred],
        vec![inferred_x],
        vec![axis_flipped],
        vec![version],
        vec![reflected],
        vec![placement.clone(), placement.clone()],
    ] {
        assert!(
            resolved_placement(&bytes, &scope, std::slice::from_ref(&header), &placements)
                .is_none()
        );
    }
    assert!(resolved_placement(&bytes, &scope, &[header.clone(), header], &[placement]).is_none());
    let (bytes, scope, header, placement) = fixture();
    assert!(resolved_placement(
        &[],
        &scope,
        std::slice::from_ref(&header),
        std::slice::from_ref(&placement)
    )
    .is_none());
    let mut foreign = header.clone();
    foreign.id = "f3d:Other/BulkStream.dat:design-record-header#0".into();
    assert!(resolved_placement(&bytes, &scope, &[foreign], &[placement]).is_none());
}
