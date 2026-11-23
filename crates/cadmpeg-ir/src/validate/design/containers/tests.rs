// SPDX-License-Identifier: Apache-2.0

use crate::design::container::{Container, Folder, OrderedKind, Rollback};
use crate::design::feature::OperationFamily;
use crate::design::refs::Member;
use crate::validate::design::fixture::{
    assert_finding, design_findings, document, feature, id, unresolved,
};

#[test]
fn every_feature_and_container_is_a_member_exactly_once() {
    let mut ir = document();
    ir.design
        .features
        .push(unresolved("orphan", OperationFamily::Hole));
    assert_finding(&ir, "is a member or scope root 0 times");
    let mut ir = document();
    ir.design.scopes[0].roots.push(id("container", "timeline"));
    assert_finding(&ir, "is a member or scope root 2 times");
    let mut ir = document();
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.push(Member::Feature(feature("missing")));
    assert_finding(&ir, "which is no record of the design");
}

#[test]
fn containers_form_a_tree() {
    let mut ir = document();
    for (this, other) in [("a", "b"), ("b", "a")] {
        ir.design.containers.push(Container::Ordered {
            id: id("container", this),
            kind: OrderedKind::Body,
            members: vec![Member::Container(id("container", other))],
            folders: Vec::new(),
            rollback: Some(Rollback::End),
            placement: None,
        });
    }
    assert_finding(&ir, "is its own ancestor through membership");
}

#[test]
fn rollback_and_folders_stay_within_the_container() {
    let mut ir = document();
    let Container::Ordered {
        rollback, folders, ..
    } = &mut ir.design.containers[0]
    else {
        unreachable!()
    };
    *rollback = Some(Rollback::Before(Member::Feature(feature("missing"))));
    folders.push(Folder {
        name: None,
        first: 0,
        last: 2,
    });
    assert_finding(&ir, "which is no member of the container");
    assert_finding(&ir, "is no run of the container's 2 members");
    let mut ir = document();
    let Container::Ordered { folders, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    folders.push(Folder {
        name: None,
        first: 0,
        last: 0,
    });
    folders.push(Folder {
        name: None,
        first: 0,
        last: 1,
    });
    assert!(design_findings(&ir).is_empty());
    let mut ir = document();
    ir.design.containers[0] = Container::Ordered {
        id: id("container", "timeline"),
        kind: OrderedKind::Timeline,
        members: vec![
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
            Member::Feature(feature("hole")),
        ],
        folders: vec![
            Folder {
                name: None,
                first: 0,
                last: 1,
            },
            Folder {
                name: None,
                first: 1,
                last: 2,
            },
        ],
        rollback: Some(Rollback::Before(Member::Feature(feature("hole")))),
        placement: None,
    };
    ir.design
        .features
        .push(unresolved("hole", OperationFamily::Hole));
    assert_finding(&ir, "neither nest nor are disjoint");
}
