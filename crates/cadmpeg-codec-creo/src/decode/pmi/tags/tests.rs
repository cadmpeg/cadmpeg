// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::pmi::PmiTarget;

use crate::feature::pmi_datums::{DatumAttachment, DatumTagIdentity};

use super::{datum_join, datum_target, DatumIndex, DatumJoin, DatumTarget, TargetCounts};

#[test]
fn incomplete_label_roster_does_not_merge_a_unique_existing_datum() {
    let datum_indices = BTreeMap::from([("A".to_owned(), DatumIndex::Unique(0))]);
    assert_eq!(
        datum_join(false, 1, &datum_indices, "A"),
        DatumJoin::Separate
    );
}

#[test]
fn complete_unique_label_roster_can_join_one_existing_datum() {
    let datum_indices = BTreeMap::from([("A".to_owned(), DatumIndex::Unique(0))]);
    assert_eq!(
        datum_join(true, 1, &datum_indices, "A"),
        DatumJoin::Existing(0)
    );
}

#[test]
fn repeated_tag_labels_stay_separate_from_a_unique_existing_datum() {
    let datum_indices = BTreeMap::from([("A".to_owned(), DatumIndex::Unique(0))]);
    assert_eq!(
        datum_join(true, 2, &datum_indices, "A"),
        DatumJoin::Separate
    );
}

fn face(id: &str) -> PmiTarget {
    PmiTarget::Face {
        face: FaceId::try_from(format!("creo:visibgeom:face#{id}")).expect("face id"),
    }
}

fn tag_with_leader(reference_type: u32, id: u32) -> DatumTagIdentity {
    DatumTagIdentity {
        source_offset: 0,
        definition_id: Some(14),
        definition_slot: Some(14),
        identification: "B".to_owned(),
        attachment: Some(DatumAttachment { reference_type, id }),
    }
}

#[test]
fn a_type_5_leader_uses_the_targets_of_its_dimension_slot() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let dimension_targets = BTreeMap::from([(417, vec![face("506"), face("507")])]);
    let target = datum_target(&tag_with_leader(5, 417), &ir, &dimension_targets);
    assert_eq!(
        target,
        DatumTarget::Dimension(vec![face("506"), face("507")])
    );

    let mut counts = TargetCounts::default();
    counts.record(&target);
    let notes = counts.notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].code,
        crate::loss::CreoLossCode::PmiDatumTargetInferred.kind()
    );
}

#[test]
fn a_type_5_leader_without_dimension_targets_stays_unresolved() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let dimension_targets = BTreeMap::from([(418, vec![face("506")])]);
    let target = datum_target(&tag_with_leader(5, 417), &ir, &dimension_targets);
    assert_eq!(target, DatumTarget::Unresolved);

    let mut counts = TargetCounts::default();
    counts.record(&target);
    assert_eq!(
        counts.notes()[0].code,
        crate::loss::CreoLossCode::PmiDatumTargetUnresolved.kind()
    );
}
