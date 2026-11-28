// SPDX-License-Identifier: Apache-2.0

use super::{local_reference, redirections_json, repeated_target_occurrence_record};
use crate::xref::OccurrencePlacement;

#[test]
fn exact_component_insert_carriers_precede_structured_placements() {
    let direct = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let structured = OccurrencePlacement {
        frame_bytes: {
            let mut frame = repeated_target_occurrence_record("role", 23, 27, None);
            let owner = local_reference(8);
            let start = frame.len() - owner.len();
            frame[start..].copy_from_slice(&owner);
            frame
        },
        occurrence_guid: Some(
            "bbbbbbbb-cccc-dddd-eeee-ffffffffffff"
                .to_owned()
                .try_into()
                .unwrap(),
        ),
        carrier_record_index: 23,
        owner: Some(crate::records::xref::XrefLocalOwner {
            target: 8,
            type_guid: None,
        }),
        link_names: vec!["role".into()],
        transform: Some([
            [1.0, 0.0, 0.0, -5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
    };

    let other = OccurrencePlacement {
        frame_bytes: Vec::new(),
        carrier_record_index: 24,
        transform: None,
        ..structured.clone()
    };
    let selected = crate::xref::occurrence_transforms_with_precedence(
        vec![(23, direct), (23, direct)],
        &[structured.clone(), other.clone()],
        "role",
    );
    assert_eq!(selected.len(), 2);
    // The carrier stores a placement other than the insertion frame.
    assert_eq!(selected[0], structured);
    assert_eq!(selected[1], other);
    assert_eq!(
        crate::xref::superseded_placement_count(
            &[(23, direct)],
            &[structured.clone(), other.clone()],
            "role"
        ),
        0
    );
    let equal = OccurrencePlacement {
        transform: Some(direct),
        ..structured.clone()
    };
    let selected = crate::xref::occurrence_transforms_with_precedence(
        vec![(23, direct), (23, direct), (24, direct)],
        &[equal.clone(), other.clone()],
        "role",
    );
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0], equal);
    assert_eq!(selected[1], other);
    assert_eq!(
        crate::xref::superseded_placement_count(&[(23, direct)], &[equal, other], "role"),
        1
    );
    assert_eq!(
        crate::xref::occurrence_transforms_with_precedence(vec![(25, direct)], &selected, "role")
            .len(),
        3
    );
}

#[test]
fn component_insert_selection_uses_stream_and_role_not_class_tag() {
    let selected = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let ignored = [
        [1.0, 0.0, 0.0, 9.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let selected_construction =
        crate::records::feature::assembly_features::DesignComponentInsertConstruction {
            relation_record_index: 1,
            carrier_record_index: 2,
            occurrence_identity: None,
            neutron_role: "role".into(),
            neutron_role_offset: 0,
            occurrence_key: None,
            placement: Some(
                crate::records::feature::assembly_features::DesignComponentInsertMatrix {
                    scope: crate::records::identity::Located {
                        value: selected.try_into().unwrap(),
                        offset: 0,
                    },
                    carrier_offset: Some(0),
                },
            ),
        };
    let ignored_construction =
        crate::records::feature::assembly_features::DesignComponentInsertConstruction {
            neutron_role: "other".into(),
            placement: Some(
                crate::records::feature::assembly_features::DesignComponentInsertMatrix {
                    scope: crate::records::identity::Located {
                        value: ignored.try_into().unwrap(),
                        offset: 0,
                    },
                    carrier_offset: Some(0),
                },
            ),
            ..selected_construction.clone()
        };

    assert_eq!(
        crate::xref::select_component_insert_transforms(
            [
                ("stream", &selected_construction),
                ("stream", &ignored_construction),
                ("other-stream", &selected_construction),
            ],
            "stream",
            "role"
        ),
        vec![(2, selected)]
    );
    let mut reference = crate::xref::parse(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        redirections_json("root.f3d", &[("part.f3d", "role")]).as_bytes(),
    )
    .unwrap()
    .references
    .remove(0);
    reference.placement_source = Some(crate::records::xref::XrefPlacementSource {
        frame_bytes: Vec::new().into(),
        occurrence_guid: None,
        stream: "stream".into(),
        carrier_record_index: 2,
        owner: None,
    });
    reference.transform = Some(selected.try_into().unwrap());
    let mut other = reference.clone();
    other.ordinal = 1;
    other
        .placement_source
        .as_mut()
        .unwrap()
        .carrier_record_index = 3;
    let references = [other, reference];
    assert_eq!(
        crate::xref::component_insert_reference(&references, "stream", &selected_construction)
            .unwrap()
            .ordinal,
        0
    );
    assert!(crate::xref::component_insert_reference(
        &references,
        "other-stream",
        &selected_construction
    )
    .is_none());
}
