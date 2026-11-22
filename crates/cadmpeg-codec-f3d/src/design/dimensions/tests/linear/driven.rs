// SPDX-License-Identifier: Apache-2.0
use super::super::project_dimension_constraints;
use super::super::EPS_DIMENSION_PROJECTION_LINEAR;
use super::{exact_pair, ExactPair};
use crate::records::dimensions::{
    DesignDimensionAnnotationFrame, DesignDimensionPresentationFrame,
};
use cadmpeg_core::decode::u64_from_index;

/// The 129-byte linear measurement, then the text frame's style byte, four
/// style values, its byte and its zero byte.
fn styled(style: u8) -> Vec<u8> {
    let mut bytes = vec![0; 129];
    bytes.push(style);
    bytes.extend([0; 16]);
    bytes.extend([1, 0]);
    bytes
}

/// The annotation frame of the fixture's dimension, with text style `style`.
fn annotation(stream: &str, style: u8) -> DesignDimensionAnnotationFrame {
    let annotation_bytes = styled(style);
    let owner_reference_at = 74 + u64_from_index(annotation_bytes.len()) + 1;
    let operand =
        |index: u32, at: u64| crate::records::dimensions::DesignDimensionAnnotationOperand {
            geometry_record_index: std::num::NonZeroU32::new(index),
            geometry_reference_offset: at + 1,
            role: 1,
            role_offset: at + 11,
        };
    DesignDimensionAnnotationFrame::try_new(
        crate::records::dimensions::DesignDimensionAnnotationFrameDraft {
            id: format!("{stream}:design-dimension-annotation-frame#50"),
            companion_record_index: None,
            governing_companion_record_index: 22,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                .unwrap(),
            record_index: 50,
            frame_length: 300,
            operands: vec![operand(0, 24), operand(40, 39), operand(41, 54)],
            entity_genesis: None,
            metadata_absent: false,
            annotation_bytes,
            annotation_byte_offset: 74,
            governing_owner_record_index: 21,
            governing_owner_reference_offset: owner_reference_at,
            return_members: [40, 41]
                .into_iter()
                .zip([owner_reference_at + 15, owner_reference_at + 26])
                .map(|(value, offset)| crate::records::identity::Located {
                    value: std::num::NonZeroU32::new(value).unwrap(),
                    offset,
                })
                .collect(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "257".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
            owner_reference: 100,
            owner_reference_offset: 320,
        },
    )
    .unwrap()
}

/// The presentation frame of the fixture's dimension, with text style
/// `style` after an empty property block.
fn presentation(stream: &str, style: u8) -> DesignDimensionPresentationFrame {
    let mut presentation_bytes = vec![0];
    presentation_bytes.extend(styled(style));
    DesignDimensionPresentationFrame {
        id: format!("{stream}:design-dimension-presentation-frame#60"),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("289".to_owned()).unwrap(),
        record_index: 60,
        frame_length: 0,
        operands: [40, 41]
            .into_iter()
            .map(
                |record_index| crate::records::dimensions::DesignDimensionPresentationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(record_index).unwrap(),
                    geometry_reference_offset: 0,
                    role: 0,
                    role_offset: 0,
                },
            )
            .collect(),
        presentation_bytes: presentation_bytes.into(),
        presentation_byte_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("291".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
        owner_reference: 100,
        owner_reference_offset: 0,
        governing_owner_record_index: 21,
        governing_parameter_record_index: 20,
        governing_companion_record_index: 22,
    }
}

#[test]
fn a_driven_parameter_makes_its_constraint_driven() {
    let ExactPair {
        placement,
        parameter,
        owner,
        companion,
        pair,
        group,
        points,
        entities,
        ..
    } = exact_pair();
    let driven = parameter
        .clone()
        .with_flags(crate::records::parameters::DesignParameterFlags {
            driven: true,
            favorite: false,
        })
        .unwrap();
    let driven_constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&driven),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(driven_constraints.len(), 1);
    assert_eq!(driven_constraints[0].driving, Some(false));
}

/// A parameter whose trailer states no driven flag is driven when its
/// annotation frame or presentation frame stores the driven text-frame style
/// byte `1` (f3d.md §3.1, "Sketch dimensions"). The constraint join takes
/// that state, so a driven dimension stays driven even if the trailer's
/// driven flag is read at the wrong byte (DR-144).
#[test]
fn a_driven_text_style_makes_the_constraint_driven() {
    let ExactPair {
        stream,
        placement,
        parameter,
        owner,
        companion,
        pair,
        group,
        points,
        entities,
        ..
    } = exact_pair();
    let project = |annotation_frames: &[DesignDimensionAnnotationFrame],
                   presentation_frames: &[DesignDimensionPresentationFrame]| {
        crate::design::dimensions::project_dimension_constraints_with_presentations(
            &crate::design::dimensions::DimensionConstraintInputs {
                placements: std::slice::from_ref(&placement),
                parameters: std::slice::from_ref(&parameter),
                owners: std::slice::from_ref(&owner),
                pairs: std::slice::from_ref(&pair),
                groups: std::slice::from_ref(&group),
                annotation_frames,
                null_pairs: &[],
                companions: std::slice::from_ref(&companion),
                recipe_records: &[],
                points: &points,
                curves: &[],
                entities: &entities,
            },
            presentation_frames,
            &[],
            EPS_DIMENSION_PROJECTION_LINEAR,
        )
    };
    for (style, driving) in [(1, Some(false)), (0, None)] {
        for constraints in [
            project(&[annotation(stream, style)], &[]),
            project(&[], &[presentation(stream, style)]),
        ] {
            assert!(
                !constraints.is_empty()
                    && constraints
                        .iter()
                        .all(|constraint| constraint.driving == driving),
                "style {style}: {constraints:#?}"
            );
        }
    }
}
