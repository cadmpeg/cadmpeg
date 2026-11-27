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

/// Sites whose derived bodies take a stated kind in one and the kind of
/// their faces in another say both.
#[test]
fn merged_derived_hierarchies_name_both_kind_sources() {
    let mut target = Brep::default();
    target.stats.synthetic_body_grouping = true;
    target.stats.derived_kind.unlinked_records = true;
    target.stats.derived_kind.stated = true;
    let mut source = Brep::default();
    source.stats.synthetic_body_grouping = true;
    source.stats.derived_kind.from_faces = true;
    merge_brep(&mut target, source).unwrap();
    let losses = super::super::brep_losses(&mut target).unwrap();
    let derived: Vec<_> = losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::SldprtLossCode::TopologyBodyHierarchyDerived.kind()
        })
        .map(|loss| loss.message.as_str())
        .collect();
    let [message] = derived.as_slice() else {
        panic!("one derived-hierarchy note: {losses:#?}");
    };
    assert!(
        message.contains("with the kind the body records state where they agree")
            && message.contains("classified from their faces elsewhere"),
        "{message}"
    );
}
