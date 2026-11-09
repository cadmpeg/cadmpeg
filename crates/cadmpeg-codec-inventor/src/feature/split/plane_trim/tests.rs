// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::{
    BodySelection, BodyTrimSide, FeatureDefinition, FeatureId, FeatureOperation,
};

use crate::{
    feature::{
        split, tests::reference, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind,
        PmDcFeatureReferenceFamily,
    },
    pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList},
    record_identity::RecordTypeId,
    test_support::split::{
        boolean, plane_trim_fixture, set_kind, set_marker, set_slot, set_version,
    },
};

const PLANE: u32 = 42;

fn plane_id() -> FeatureId {
    FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        cadmpeg_ir::identity_key!("plane"),
    )
}

fn admit(
    source: &PmDcFeature,
    properties: &[PmDcFeatureProperty],
) -> Result<FeatureDefinition, split::NativeReason> {
    split::definition(
        source,
        |index| {
            properties
                .iter()
                .find(|p| p.identity.record_ordinal + 1 == index)
        },
        |ordinal| (ordinal == PLANE).then(plane_id),
    )
}

#[test]
fn plane_trim_keeps_the_side_the_slot_6_control_selects() {
    for (side, version, keep) in [
        (false, 19, BodyTrimSide::Reverse),
        (false, 23, BodyTrimSide::Reverse),
        (true, 19, BodyTrimSide::Forward),
    ] {
        let (mut source, properties) = plane_trim_fixture(side);
        set_version(&mut source, version);
        let definition = admit(&source, &properties).expect("plane trim");
        assert_eq!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::TrimBodiesAtPlane {
                targets: BodySelection::Native(properties[7].id()),
                plane: plane_id(),
                keep,
            }),
            "side {side} version {version}"
        );
        assert_eq!(split::output_slot(&definition), 7);
    }
}

#[test]
fn plane_trim_refuses_the_unmeasured_side_and_version_pairs() {
    for version in [18, 20, 21, 25, 31] {
        for side in [false, true] {
            let (mut source, properties) = plane_trim_fixture(side);
            set_version(&mut source, version);
            assert!(admit(&source, &properties).is_err(), "{version} {side}");
        }
    }
    let (mut source, properties) = plane_trim_fixture(true);
    set_version(&mut source, 23);
    assert!(admit(&source, &properties).is_err());
}

#[test]
fn plane_trim_checks_each_slot_type_and_qualifier() {
    for slot in 0..10 {
        let (mut source, properties) = plane_trim_fixture(false);
        let old = source.properties.references()[slot];
        let replacement = if old.index() == 0 {
            reference(62)
        } else {
            reference(0)
        };
        set_slot(&mut source, slot, replacement);
        assert!(admit(&source, &properties).is_err(), "slot {slot}");
        let (mut source, properties) = plane_trim_fixture(false);
        let old = source.properties.references()[slot];
        set_slot(
            &mut source,
            slot,
            PmDcReference::new(old.index(), !old.qualified()).expect("reference"),
        );
        assert!(admit(&source, &properties).is_err(), "qualifier {slot}");
    }
    for position in [0, 1, 2, 3, 4, 5, 7] {
        let (source, mut properties) = plane_trim_fixture(false);
        properties[position].identity.type_id = RecordTypeId::from_bytes([0; 16]);
        assert!(admit(&source, &properties).is_err(), "type {position}");
    }
    let (mut source, properties) = plane_trim_fixture(false);
    source.identity.type_id = RecordTypeId::from_bytes([0; 16]);
    assert!(admit(&source, &properties).is_err());
}

#[test]
fn plane_trim_refuses_unresolved_planes_controls_and_bodies() {
    let (source, properties) = plane_trim_fixture(false);
    let unresolved = split::definition(
        &source,
        |index| {
            properties
                .iter()
                .find(|p| p.identity.record_ordinal + 1 == index)
        },
        |_| None,
    );
    assert!(unresolved.is_err());
    for kind in [
        boolean(true),
        PmDcFeaturePropertyKind::Boolean {
            name: "control".into(),
            name_value: 0,
            value: false,
        },
    ] {
        let (source, mut properties) = plane_trim_fixture(false);
        set_kind(&mut properties, 1, kind);
        assert!(admit(&source, &properties).is_err());
    }
    for body in [reference(0), reference(1)] {
        let (source, mut properties) = plane_trim_fixture(false);
        set_kind(
            &mut properties,
            7,
            PmDcFeaturePropertyKind::SurfaceBody { body },
        );
        assert!(admit(&source, &properties).is_err());
    }
    for tool in [3, 1] {
        let (source, mut properties) = plane_trim_fixture(false);
        set_kind(
            &mut properties,
            2,
            PmDcFeaturePropertyKind::Enumeration {
                family: crate::feature::PmDcFeatureEnumFamily::Auxiliary,
                type_value: 4,
                value: tool,
            },
        );
        assert!(admit(&source, &properties).is_err());
    }
    let (source, mut properties) = plane_trim_fixture(false);
    set_kind(
        &mut properties,
        3,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BoundaryPatch,
            items: crate::feature::tests::reference_list(&[69]),
        },
    );
    assert!(admit(&source, &properties).is_err());
    for position in [3, 5] {
        let (source, mut properties) = plane_trim_fixture(false);
        set_marker(&mut properties, position);
        assert!(admit(&source, &properties).is_err());
    }
}

#[test]
fn plane_trim_requires_unqualified_nonnull_references_in_null_slots() {
    for slot in [2, 8, 9] {
        let (mut source, properties) = plane_trim_fixture(false);
        assert_eq!(source.properties.references()[slot].index(), 0);
        set_slot(
            &mut source,
            slot,
            PmDcReference::new(62, false).expect("reference"),
        );
        assert!(admit(&source, &properties).is_err(), "slot {slot}");
    }
}

#[test]
fn plane_trim_requires_a_zero_name_value_in_the_control_boolean() {
    let (source, mut properties) = plane_trim_fixture(false);
    set_kind(
        &mut properties,
        1,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 1,
            value: false,
        },
    );
    assert!(admit(&source, &properties).is_err());
}

#[test]
fn plane_trim_requires_one_qualified_body_selection_member() {
    let (_, properties) = plane_trim_fixture(false);
    let member = match &properties[5].kind {
        PmDcFeaturePropertyKind::References { items, .. } => items.references()[0],
        _ => panic!("body selection"),
    };
    let unqualified = PmDcReference::new(member.index(), false).expect("reference");
    for members in [vec![], vec![member, member], vec![unqualified]] {
        let (source, mut properties) = plane_trim_fixture(false);
        set_kind(
            &mut properties,
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: PmDcReferenceList::new(
                    2,
                    (!members.is_empty()).then_some(PmDcListMetadata::U32([
                        u32::try_from(members.len()).expect("small list"),
                        0,
                    ])),
                    members.clone(),
                )
                .expect("list"),
            },
        );
        assert!(
            admit(&source, &properties).is_err(),
            "{} members",
            members.len()
        );
    }
}

#[test]
fn plane_trim_requires_an_unnamed_zero_name_value_side_boolean() {
    for kind in [
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 1,
            value: false,
        },
        PmDcFeaturePropertyKind::Boolean {
            name: "side".into(),
            name_value: 0,
            value: false,
        },
    ] {
        let (source, mut properties) = plane_trim_fixture(false);
        set_kind(&mut properties, 4, kind);
        assert!(admit(&source, &properties).is_err());
    }
}
