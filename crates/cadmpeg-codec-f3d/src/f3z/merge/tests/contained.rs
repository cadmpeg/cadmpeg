// SPDX-License-Identifier: Apache-2.0
//! External occurrences placed by their contained occurrences' component paths.

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::transform::Transform;

use crate::loss::F3dLossCode;
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
};
use crate::records::xref::{XrefContainedPose, XrefOwnedPose, XrefPlacementSource, XrefReference};
use crate::test_support::{local_reference, typed_cross_document_reference};

const ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
const ROOT_COMPONENT: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
const PLACEMENT: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";

fn translation(x: f64, y: f64, z: f64) -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, y],
        [0.0, 0.0, 1.0, z],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// A component-path carrier: the referenced design's root component and its
/// placement `contained`, this document's placement 8506, then `owners`.
fn carrier(
    entity_id: u64,
    first_type: &str,
    second_role: &str,
    contained: u64,
    owners: &[u64],
    transform: [[f64; 4]; 4],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&u32::try_from(3 + owners.len()).unwrap().to_le_bytes());
    bytes.extend(typed_cross_document_reference(250, first_type, ROLE));
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    bytes.extend(typed_cross_document_reference(
        contained,
        PLACEMENT,
        second_role,
    ));
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    for (discriminator, target) in std::iter::once(8506)
        .chain(owners.iter().copied())
        .enumerate()
    {
        bytes.extend(local_reference(target));
        bytes.extend_from_slice(&u32::try_from(discriminator).unwrap().to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(8506));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

/// A component-path carrier of the external occurrence itself: the referenced
/// design's root component, this document's placement 8506, then `owners`.
fn owned_carrier(entity_id: u64, owners: &[u64], transform: [[f64; 4]; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&u32::try_from(2 + owners.len()).unwrap().to_le_bytes());
    bytes.extend(typed_cross_document_reference(250, ROOT_COMPONENT, ROLE));
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    for (discriminator, target) in std::iter::once(8506)
        .chain(owners.iter().copied())
        .enumerate()
    {
        bytes.extend(local_reference(target));
        bytes.extend_from_slice(&u32::try_from(discriminator).unwrap().to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(8506));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn owned(entity_id: u64, owners: &[u64], world: [[f64; 4]; 4]) -> XrefOwnedPose {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = owned_carrier(entity_id, owners, world);
    let pose = crate::xref::owned_component_path_pose(&bytes, magic).unwrap();
    XrefOwnedPose {
        carrier_record_index: pose.carrier_record_index,
        frame_bytes: bytes.into(),
        owner_record_indices: pose.owner_record_indices,
        transform: pose.transform.map(|rows| rows.try_into().unwrap()),
    }
}

fn reference(contained_poses: Vec<XrefContainedPose>) -> XrefReference {
    with_owned(contained_poses, Vec::new())
}

fn with_owned(
    contained_poses: Vec<XrefContainedPose>,
    owned_poses: Vec<XrefOwnedPose>,
) -> XrefReference {
    XrefReference {
        current_pose: None,
        contained_poses,
        owned_poses,
        id: "f3d:xref:reference#0-occurrence-0".to_owned(),
        ordinal: 0,
        occurrence_ordinal: 0,
        placement_source: Some(XrefPlacementSource {
            frame_bytes: Vec::new().into(),
            occurrence_guid: None,
            stream: "Design".to_owned(),
            carrier_record_index: 8506,
            owner: None,
        }),
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: ROLE.to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    }
}

fn retained(
    entity_id: u64,
    contained: u64,
    owners: &[u64],
    world: [[f64; 4]; 4],
) -> XrefContainedPose {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = carrier(entity_id, ROOT_COMPONENT, ROLE, contained, owners, world);
    let pose = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
    XrefContainedPose {
        carrier_record_index: pose.carrier_record_index,
        frame_bytes: bytes.into(),
        contained_record_index: pose.contained_record_index,
        owner_record_indices: pose.owner_record_indices,
        transform: pose.transform.map(|rows| rows.try_into().unwrap()),
    }
}

#[test]
fn a_contained_component_path_carrier_names_its_external_placement_and_owners() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let world = translation(20.0, 0.3, 5.0);
    let bytes = carrier(11, ROOT_COMPONENT, ROLE, 496, &[9222], world);
    let pose = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
    assert_eq!(
        (
            pose.carrier_record_index,
            pose.component_record_index,
            pose.contained_record_index,
            pose.placement_record_index,
            pose.owner_record_indices.as_slice(),
            pose.role.as_str(),
        ),
        (11, 250, 496, 8506, &[9222][..], ROLE)
    );
    assert_eq!(pose.transform, Some(world));
    // The first element names a root component and both carry one role.
    let wrong_type = carrier(11, PLACEMENT, ROLE, 496, &[9222], world);
    assert!(crate::xref::contained_component_path_pose(&wrong_type, magic).is_none());
    let other_role = carrier(11, ROOT_COMPONENT, "other", 496, &[9222], world);
    assert!(crate::xref::contained_component_path_pose(&other_role, magic).is_none());
    // A component-path carrier of an external occurrence itself is not one.
    assert!(crate::xref::component_path_pose(&bytes, magic).is_none());

    let reference = reference(vec![retained(11, 496, &[9222], world)]);
    assert!(crate::xref::contained_poses_match(&reference));
    let mut moved = reference.clone();
    moved.contained_poses[0].transform = Some(translation(21.0, 0.3, 5.0).try_into().unwrap());
    assert!(!crate::xref::contained_poses_match(&moved));
    let mut owners = reference.clone();
    owners.contained_poses[0].owner_record_indices = vec![1];
    assert!(!crate::xref::contained_poses_match(&owners));
    let mut placement = reference;
    placement
        .placement_source
        .as_mut()
        .unwrap()
        .carrier_record_index = 1;
    assert!(!crate::xref::contained_poses_match(&placement));
}

fn occurrence(id: &str, parent: Option<&str>, native_ref: Option<&str>, x: f64) -> Occurrence {
    Occurrence {
        id: OccurrenceId::mint(id).unwrap(),
        prototype: PrototypeReference::Unresolved {},
        parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId::mint(parent).unwrap(),
            }
        }),
        ordinal: 0,
        transform: Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: native_ref.map(str::to_owned),
    }
}

fn local_carrier(id: &str, record_index: u32) -> DesignComponentOccurrence {
    DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
        id: id.to_owned(),
        class_tag: crate::records::references::DesignClassTag::try_from("290".to_owned()).unwrap(),
        record_index,
        byte_offset: u64::from(record_index),
        component_record_index: 1,
        component_guid: "2ad641b3-9ea3-4e5f-a4da-926c3faeccae"
            .to_owned()
            .try_into()
            .unwrap(),
        occurrence_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(format!(
            "00000000-0000-4000-8000-{record_index:012}"
        ))
        .unwrap()
        .into(),
        placement: DesignComponentOccurrencePlacement::Base,
        owner_entity: 1,
    })
    .unwrap()
}

/// An owner at x = 5 mm holding the external instance (stored at 10 mm), whose
/// member root holds parts 496 at x = 30 mm and 497 at x = -30 mm, with these
/// contained poses. Returns the instance's placement and the loss codes.
fn place(poses: Vec<XrefContainedPose>) -> (Transform, Vec<cadmpeg_ir::report::loss::LossKind>) {
    place_reference(reference(poses))
}

fn place_reference(
    reference: XrefReference,
) -> (Transform, Vec<cadmpeg_ir::report::loss::LossKind>) {
    const OWNER: &str = "f3d:Design/BulkStream.dat:design-component-occurrence#9222";
    const PART: &str =
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:design-component-occurrence#";
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.occurrences = vec![
        occurrence("f3d:model:occurrence#root", None, None, 0.0),
        occurrence(
            "f3d:model:occurrence#owner",
            Some("f3d:model:occurrence#root"),
            Some(OWNER),
            5.0,
        ),
        occurrence(
            "f3d:model:occurrence#owner:xref-0-0",
            Some("f3d:model:occurrence#owner"),
            Some("f3d:xref:reference#0-occurrence-0"),
            10.0,
        ),
        occurrence(
            "f3d:xref/role/occurrence-0/model:occurrence#member-root",
            Some("f3d:model:occurrence#owner:xref-0-0"),
            None,
            0.0,
        ),
        occurrence(
            "f3d:xref/role/occurrence-0/model:occurrence#part-496",
            Some("f3d:xref/role/occurrence-0/model:occurrence#member-root"),
            Some(&format!("{PART}496")),
            30.0,
        ),
        occurrence(
            "f3d:xref/role/occurrence-0/model:occurrence#part-497",
            Some("f3d:xref/role/occurrence-0/model:occurrence#member-root"),
            Some(&format!("{PART}497")),
            -30.0,
        ),
    ];
    crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        native.design_component_occurrences = vec![
            local_carrier(OWNER, 9222),
            local_carrier(&format!("{PART}496"), 496),
            local_carrier(&format!("{PART}497"), 497),
        ];
        native.xref_references = vec![reference.clone()];
    });
    let table = crate::xref::XrefTable {
        references: vec![reference],
        ..Default::default()
    };
    let mut losses = Vec::new();
    super::super::place_contained_poses(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
        &table,
        &mut losses,
    )
    .unwrap();
    let instance = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str() == "f3d:model:occurrence#owner:xref-0-0")
        .unwrap()
        .transform;
    (instance, losses.into_iter().map(|loss| loss.code).collect())
}

#[test]
fn an_external_occurrence_takes_the_pose_its_contained_occurrences_give() {
    // The parts' world poses put the instance at x = 200 mm, y = 40 mm: below
    // the owner at 5 mm that is 195 mm, in place of its stored 10 mm.
    let poses = || {
        vec![
            retained(11, 496, &[9222], translation(23.0, 4.0, 0.0)),
            retained(12, 497, &[9222], translation(17.0, 4.0, 0.0)),
        ]
    };
    let (transform, losses) = place(poses());
    let rows = transform.rows();
    assert!(
        (rows[0][3] - 195.0).abs() < 1e-9 && (rows[1][3] - 40.0).abs() < 1e-9,
        "{rows:?}"
    );
    assert_eq!(losses, [F3dLossCode::XrefContainedPose.kind()]);

    // A carrier that lists fewer owners and agrees changes nothing; one that disagrees,
    // as when it keeps a pose from before an owner moved, yields to those that list more.
    for x in [23.0, 25.0] {
        let mut shorter = poses();
        shorter.push(retained(13, 496, &[], translation(x, 4.0, 0.0)));
        let (transform, losses) = place(shorter);
        assert!(
            (transform.rows()[0][3] - 195.0).abs() < 1e-9,
            "{:?}",
            transform.rows()
        );
        assert_eq!(losses, [F3dLossCode::XrefContainedPose.kind()]);
    }

    // Parts that give two poses leave the stored placement.
    let mut disagreeing = poses();
    disagreeing[1] = retained(12, 497, &[9222], translation(18.0, 4.0, 0.0));
    let (transform, losses) = place(disagreeing);
    assert!((transform.rows()[0][3] - 10.0).abs() < 1e-9);
    assert_eq!(losses, [F3dLossCode::XrefContainedPoseDisagrees.kind()]);

    // A carrier whose owner chain is not the instance's binds nothing.
    let (transform, losses) = place(vec![retained(
        11,
        496,
        &[1234],
        translation(23.0, 4.0, 0.0),
    )]);
    assert!((transform.rows()[0][3] - 10.0).abs() < 1e-9);
    assert_eq!(losses, [F3dLossCode::XrefContainedPoseNative.kind()]);
}

#[test]
fn an_owned_external_occurrence_takes_its_own_pose_unless_a_contained_pose_places_it() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = owned_carrier(11, &[9222], translation(20.0, 4.0, 0.0));
    let pose = crate::xref::owned_component_path_pose(&bytes, magic).unwrap();
    assert_eq!(
        (
            pose.placement_record_index,
            pose.owner_record_indices.as_slice(),
            pose.role.as_str()
        ),
        (8506, &[9222][..], ROLE)
    );
    // Its own carrier lists an owner; a two-element carrier is #486's form.
    assert!(crate::xref::owned_component_path_pose(
        &owned_carrier(11, &[], translation(20.0, 4.0, 0.0)),
        magic
    )
    .is_none());
    assert!(crate::xref::contained_component_path_pose(&bytes, magic).is_none());
    let reference = with_owned(
        Vec::new(),
        vec![owned(11, &[9222], translation(20.0, 4.0, 0.0))],
    );
    assert!(crate::xref::owned_poses_match(&reference));
    let mut moved = reference.clone();
    moved.owned_poses[0].transform = Some(translation(21.0, 4.0, 0.0).try_into().unwrap());
    assert!(!crate::xref::owned_poses_match(&moved));

    // The instance's own world pose, 200 mm, is 195 mm below the owner at 5 mm.
    let (transform, losses) = place_reference(reference);
    assert!(
        (transform.rows()[0][3] - 195.0).abs() < 1e-9,
        "{:?}",
        transform.rows()
    );
    assert_eq!(losses, [F3dLossCode::XrefOwnedPose.kind()]);

    // A contained pose wins over the instance's own carrier, which stays native.
    let (transform, losses) = place_reference(with_owned(
        vec![retained(12, 496, &[9222], translation(33.0, 4.0, 0.0))],
        vec![owned(11, &[9222], translation(20.0, 4.0, 0.0))],
    ));
    assert!(
        (transform.rows()[0][3] - 295.0).abs() < 1e-9,
        "{:?}",
        transform.rows()
    );
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefContainedPose.kind(),
            F3dLossCode::XrefContainedPoseNative.kind()
        ]
    );
}
