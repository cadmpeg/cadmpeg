// SPDX-License-Identifier: Apache-2.0

use super::{
    DesignAssemblyFrame, DesignDeleteFaceFrame, DesignExtrudeFrame, DesignRigidGroupTail,
    DesignScopeGeneration, DesignSplitFaceFrame, ScopeFrameFacts,
};
use crate::records::feature::scope::DesignFeatureKind;

fn facts<'a>(
    kind: &'a DesignFeatureKind,
    classes: (&'a str, &'a str),
    frame_length: u64,
    reference_count: usize,
) -> ScopeFrameFacts<'a> {
    ScopeFrameFacts {
        kind,
        class_tag: classes.0,
        paired_class_tag: classes.1,
        frame_length,
        reference_count_delta: 0,
        kind_delta: 0,
        reference_count,
        tail_length: 0,
    }
}

/// f3d.md "parameter scope": the rigid `As-built` scope of an N-member rigid
/// group with the ordinary prologue and 88-byte tail is 269 + 11(N − 2) bytes.
#[test]
fn a_rigid_group_frame_names_its_member_count() {
    for (length, members) in [(269, 2), (280, 3), (302, 5)] {
        let frame = DesignAssemblyFrame::rigid(length, 88).unwrap();
        assert_eq!(frame.rigid_members(), usize::try_from(members).ok());
        assert_eq!(frame.frame_length(), Some(length));
        assert_eq!(frame.rigid_tail_length(), Some(88));
    }
    assert_eq!(DesignAssemblyFrame::rigid(270, 88), None);
}

#[test]
fn every_named_assembly_frame_restates_its_length() {
    for length in [
        378, 399, 421, 604, 627, 633, 637, 671, 692, 705, 732, 744, 748, 772, 806, 850, 861, 945,
        968, 1011,
    ] {
        let frame = DesignAssemblyFrame::from_frame_length(length).unwrap();
        assert_eq!(frame.frame_length(), Some(length));
        assert_eq!(frame.rigid_members(), None);
    }
}

#[test]
fn a_frame_of_a_fixed_family_takes_its_named_form() {
    let assemble = DesignFeatureKind::Assemble;
    assert_eq!(
        DesignScopeGeneration::derive(&facts(&assemble, ("276", "258"), 604, 8)),
        DesignScopeGeneration::Assembly(DesignAssemblyFrame::Frame604)
    );
    let extrude = DesignFeatureKind::Extrude;
    assert_eq!(
        DesignScopeGeneration::derive(&facts(&extrude, ("330", "258"), 476, 8)),
        DesignScopeGeneration::Extrude(DesignExtrudeFrame::ReferenceAware476)
    );
    let split = DesignFeatureKind::SplitFace;
    assert_eq!(
        DesignScopeGeneration::derive(&facts(&split, ("277", "258"), 290 + 33, 4)),
        DesignScopeGeneration::SplitFace(DesignSplitFaceFrame::Base290)
    );
}

/// f3d.md: `DeleteFace` forms are keyed by the frame length and the kind
/// offset, each less the eleven-byte reference entries.
#[test]
fn a_delete_face_frame_is_keyed_by_its_base_length_and_kind_offset() {
    let kind = DesignFeatureKind::DeleteFace;
    let mut frame = facts(&kind, ("264", "262"), 236 + 22, 2);
    frame.kind_delta = 139 + 22;
    assert_eq!(
        DesignScopeGeneration::derive(&frame),
        DesignScopeGeneration::DeleteFace(DesignDeleteFaceFrame::Base236Kind139)
    );
    frame.kind_delta = 140 + 22;
    assert_eq!(
        DesignScopeGeneration::derive(&frame),
        DesignScopeGeneration::Envelope
    );
}

#[test]
fn a_frame_no_fixed_form_names_is_the_general_envelope() {
    let sketch = DesignFeatureKind::Sketch;
    assert_eq!(
        DesignScopeGeneration::derive(&facts(&sketch, ("268", "268"), 604, 2)),
        DesignScopeGeneration::Envelope
    );
    let assemble = DesignFeatureKind::Assemble;
    assert_eq!(
        DesignScopeGeneration::derive(&facts(&assemble, ("283", "264"), 869, 12)),
        DesignScopeGeneration::Envelope
    );
}

#[test]
fn a_generation_admits_only_its_family() {
    let generation = DesignScopeGeneration::Assembly(DesignAssemblyFrame::Frame604);
    assert!(generation.admits_kind(&DesignFeatureKind::AsBuilt));
    assert!(!generation.admits_kind(&DesignFeatureKind::Sketch));
    assert!(DesignScopeGeneration::Envelope.admits_kind(&DesignFeatureKind::Sketch));
}

#[test]
fn the_wire_form_names_the_generation() {
    let generation = DesignScopeGeneration::Assembly(DesignAssemblyFrame::Rigid {
        members: 3,
        alternate_prologue: false,
        tail: DesignRigidGroupTail::Ordinary {
            additional_reference: false,
            wide_field: true,
        },
    });
    let wire = serde_json::to_value(generation).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"form": "assembly", "frame": {"rigid": {
            "members": 3,
            "alternate_prologue": false,
            "tail": {"ordinary": {"additional_reference": false, "wide_field": true}}
        }}})
    );
    assert_eq!(
        serde_json::from_value::<DesignScopeGeneration>(wire).unwrap(),
        generation
    );
    assert_eq!(
        serde_json::to_value(DesignScopeGeneration::Envelope).unwrap(),
        serde_json::json!({"form": "envelope"})
    );
}
