use super::*;
use crate::decode::MeshBodyTessellation;

const STREAM: &str = "f3d:Design1";

/// A projection in which mesh-body records 10 and 11 join the tessellations
/// `mesh#10` and `mesh#11` of scope 3, and record 12 joins two.
fn projection() -> MeshProjection {
    let joined = |body: u32| {
        (
            (STREAM.to_owned(), body),
            Some(MeshBodyTessellation {
                tessellation: format!("mesh#{body}"),
                scope_record_index: Some(3),
            }),
        )
    };
    MeshProjection {
        count: 2,
        unjoined: 0,
        tessellations_by_scope: Default::default(),
        tessellations_by_body: [joined(10), joined(11), ((STREAM.to_owned(), 12), None)]
            .into_iter()
            .collect(),
    }
}

/// The mesh body that each mesh operation member 1, 2, 3, 4, and 5 names;
/// member 9 is no mesh operation member.
fn member_body(member: u32) -> Option<u32> {
    match member {
        1 => Some(10),
        2 => Some(11),
        3 => Some(12),
        4 => Some(13),
        5 => Some(10),
        _ => None,
    }
}

fn select(members: &[u32]) -> Option<Vec<String>> {
    group_selection(members.iter().copied(), member_body, STREAM, &projection())
        .map(|selected| selected.tessellations)
}

#[test]
fn a_group_of_joined_mesh_members_selects_their_bodies_in_member_order() {
    let selected =
        group_selection([2, 1], member_body, STREAM, &projection()).expect("a selection");
    assert_eq!(selected.tessellations, ["mesh#11", "mesh#10"]);
    assert_eq!(selected.scopes, [3]);
}

#[test]
fn a_group_with_a_member_that_is_no_mesh_operation_member_selects_nothing() {
    assert_eq!(select(&[1, 9]), None);
}

#[test]
fn a_group_with_a_member_whose_body_joins_no_single_tessellation_selects_nothing() {
    assert_eq!(select(&[1, 4]), None, "body 13 joins no tessellation");
    assert_eq!(select(&[1, 3]), None, "body 12 joins two tessellations");
}

#[test]
fn members_that_name_one_body_select_it_once() {
    assert_eq!(
        select(&[1, 5, 2]),
        Some(vec!["mesh#10".into(), "mesh#11".into()])
    );
}

#[test]
fn a_group_without_members_selects_nothing() {
    assert_eq!(select(&[]), None);
}

#[test]
fn a_stream_that_holds_no_joined_body_selects_nothing() {
    assert_eq!(
        group_selection([1], member_body, "f3d:Design2", &projection())
            .map(|selected| selected.tessellations),
        None
    );
}
