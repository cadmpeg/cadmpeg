// SPDX-License-Identifier: Apache-2.0

use crate::design::assembly::placement_ordered_references_hold;

#[test]
fn placement_ordered_owner_count_and_references_match_the_frame() {
    use crate::records::feature::{
        assembly::{DesignAssemblyAlignment, DesignAssemblyOwnerRun},
        scope::{DesignFeatureKind, DesignParameterScope},
    };
    use crate::records::identity::{Located, ReferenceRun};
    let owners = [(0, 50), (1, 51), (2, 52), (3, 53), (4, 54), (5, 55)];
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::AsBuilt,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 422;
            draft.paired_byte_offset = draft.byte_offset + 422;
            draft.reference_members =
                ReferenceRun::unlocated(vec![80, 50, 51, 52, 53, 81, 82, 83, 84, 54, 55]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut alignment = DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        (50..54).map(|value| Located { value, offset: 0 }).collect(),
        None,
    )
    .unwrap();
    alignment.additional_owner_run = Some(DesignAssemblyOwnerRun {
        count: Located {
            value: 2,
            offset: scope.byte_offset() + 43,
        },
        references: vec![
            Located {
                value: 54,
                offset: scope.byte_offset() + 47,
            },
            Located {
                value: 55,
                offset: scope.byte_offset() + 58,
            },
        ],
    });
    let holds = |alignment: &DesignAssemblyAlignment| {
        placement_ordered_references_hold(&scope, alignment, &owners)
    };
    assert!(holds(&alignment));
    let mut absent = alignment.clone();
    absent.additional_owner_run = None;
    assert!(!holds(&absent));
    for value in [0, 1, 3] {
        let mut changed = alignment.clone();
        changed.additional_owner_run.as_mut().unwrap().count.value = value;
        assert!(!holds(&changed));
    }
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().references[1].value = 54;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().references[0].offset += 1;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().count.offset += 1;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed
        .additional_owner_run
        .as_mut()
        .unwrap()
        .references
        .pop();
    assert!(!holds(&changed));
    assert!(!placement_ordered_references_hold(
        &scope,
        &alignment,
        &owners[..5]
    ));
}
