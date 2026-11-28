// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::topology::Point;

use super::super::{merge_brep, Brep};

#[test]
fn brep_merge_moves_arena_records_without_copying_identities() {
    let mut target = Brep::default();
    let mut source = Brep::default();
    source.points.push(Point::new(
        "sldprt:model:point#one".try_into().unwrap(),
        FinitePoint3::ZERO,
        None,
    ));
    let identity_storage = source.points[0].id.as_str().as_ptr();
    merge_brep(&mut target, source).unwrap();
    assert_eq!(target.points[0].id.as_str(), "sldprt:model:point#one");
    assert_eq!(target.points[0].id.as_str().as_ptr(), identity_storage);
}

/// The derived-hierarchy note of merged sites whose derived bodies take
/// their kind as `kinds` say.
fn merged_derived_note(kinds: [crate::brep::graph::DerivedKind; 2]) -> String {
    let [first, second] = kinds.map(|kind| {
        let mut brep = Brep::default();
        brep.stats.synthetic_body_grouping = true;
        brep.stats.derived_kind = kind;
        brep
    });
    let mut target = first;
    merge_brep(&mut target, second).unwrap();
    let losses = super::super::brep_losses(&mut target).unwrap();
    let derived: Vec<_> = losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::SldprtLossCode::TopologyBodyHierarchyDerived.kind()
        })
        .map(|loss| loss.message.clone())
        .collect();
    let [message] = derived.as_slice() else {
        panic!("one derived-hierarchy note: {losses:#?}");
    };
    message.clone()
}

const AGREED: crate::brep::graph::DerivedKind = crate::brep::graph::DerivedKind {
    agreed: true,
    disagree: false,
    absent: false,
};
const DISAGREE: crate::brep::graph::DerivedKind = crate::brep::graph::DerivedKind {
    agreed: false,
    disagree: true,
    absent: false,
};
const ABSENT: crate::brep::graph::DerivedKind = crate::brep::graph::DerivedKind {
    agreed: false,
    disagree: false,
    absent: true,
};

/// Sites whose derived bodies take a stated kind in one and the kind of
/// their faces in another say both.
#[test]
fn merged_derived_hierarchies_name_both_kind_sources() {
    assert_eq!(
        merged_derived_note([AGREED, ABSENT]),
        "The body records do not link up with the faces; body/region/shell hierarchies were \
         derived, with the kind the body records state where they agree, and classified from \
         their faces where no body record names a region."
    );
}

/// Sites whose body records state more than one kind in one, and name no
/// region in another, say why each derived body is classified from its
/// faces.
#[test]
fn merged_derived_hierarchies_name_each_reason_for_a_kind_from_faces() {
    assert_eq!(
        merged_derived_note([DISAGREE, ABSENT]),
        "The body records do not link up with the faces; body/region/shell hierarchies were \
         derived, classified from their faces where the body records state more than one kind, \
         and classified from their faces where no body record names a region."
    );
}
