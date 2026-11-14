// SPDX-License-Identifier: Apache-2.0
//! Derived base scopes take the timeline position of the owner they name.

use crate::design::feature_project::authored_scope_ordinals;
use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::feature::derived_base::DesignDerivedBaseOwner;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;

const STREAM: &str = "f3d:Design/BulkStream.dat";

/// A derived base scope at `record_index` on history states
/// `previous -> state` that names `owner`.
fn derived(
    record_index: u32,
    previous: i64,
    state: i64,
    owner: Option<u32>,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{record_index}"),
        DesignFeatureKind::native("Derived Base Feature"),
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 100;
            draft.history_state_id = Some(state);
            draft.previous_history_state_id = Some(previous);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.derived_base_owner = owner.map(|owner| DesignDerivedBaseOwner {
        record_index: Located {
            value: owner,
            offset: 0,
        },
        repeat_offset: 0,
    });
    scope
}

fn timeline(items: &[u64]) -> DesignFeatureTimeline {
    DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(STREAM, 10),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            10,
            items
                .iter()
                .map(|&value| Located { value, offset: 0 })
                .collect(),
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(35).unwrap(),
        0,
        std::num::NonZeroU64::new(17).unwrap(),
    )
    .unwrap()
}

fn ordinals(scopes: &[DesignParameterScope], items: &[u64]) -> Vec<(u32, u64)> {
    let mut out = authored_scope_ordinals(scopes, &[timeline(items)])
        .unwrap()
        .into_iter()
        .map(|((_, record_index), ordinal)| (record_index, ordinal))
        .collect::<Vec<_>>();
    out.sort_unstable();
    out
}

#[test]
fn the_derived_base_scopes_of_one_owner_form_one_feature_at_its_position() {
    let scopes = [
        derived(70, 62, 77, Some(50)),
        derived(79, 77, 78, Some(50)),
        derived(88, 78, 79, Some(50)),
    ];
    assert_eq!(ordinals(&scopes, &[40, 50]), [(70, 1)]);
}

#[test]
fn a_derived_base_scope_without_a_vacant_timeline_owner_forms_no_feature() {
    for (scopes, items, reason) in [
        (
            vec![derived(70, 62, 77, Some(50))],
            vec![40],
            "the owner is not a timeline item",
        ),
        (
            vec![derived(70, 62, 77, None)],
            vec![50],
            "the frame names no owner",
        ),
        (
            vec![derived(70, 62, 77, Some(50)), derived(79, 70, 78, Some(50))],
            vec![50],
            "the second scope does not continue the first scope's state",
        ),
    ] {
        assert_eq!(ordinals(&scopes, &items), [], "{reason}");
    }
    let occupied = [
        derived(70, 62, 77, Some(50)),
        DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#50"),
            DesignFeatureKind::Extrude,
            50,
        ),
    ];
    assert_eq!(
        ordinals(&occupied, &[50]),
        [(50, 0)],
        "a scope occupies the owner"
    );
}

#[test]
fn a_derived_base_group_yields_to_a_claimed_owner_or_a_positioned_member() {
    let positioned = [derived(70, 62, 77, Some(50)), derived(79, 77, 78, Some(50))];
    assert_eq!(
        ordinals(&positioned, &[50, 79]),
        [(79, 1)],
        "a member that is a timeline item keeps its position and voids the group"
    );

    let mut base = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#60"),
        DesignFeatureKind::BaseFeature,
        60,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::BaseFeature(slot) =
        base.payload_mut()
    {
        *slot = Some(
            crate::records::feature::base_feature::DesignBaseFeatureConstruction::TimelineOwnedBodies {
                parameter_bodies: Vec::new(),
                bodies: Vec::new(),
                owner_record: 50,
                owner_reference_offset: 0,
                copy_sources: Vec::new(),
            },
        );
    }
    let claimed = [base, derived(70, 62, 77, Some(50))];
    assert_eq!(
        ordinals(&claimed, &[50]),
        [(60, 0)],
        "a Base Feature that the owner already positions keeps it"
    );
}
