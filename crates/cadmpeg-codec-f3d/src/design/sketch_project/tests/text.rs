// SPDX-License-Identifier: Apache-2.0

use super::super::sketch_text_horizontal_alignment;
use super::super::sketch_text_vertical_alignment;
use super::*;

#[test]
fn fixture_text_styles_project_only_representable_states() {
    for case in crate::test_support::sketch_text::text_fixture_records() {
        let text = case.decode();
        let placement = DesignSketchPlacement {
            frame: crate::records::sketch_placement::DesignSketchFrame::new(
                0,
                crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
            )
            .unwrap(),
            id: "f3d:Design/BulkStream.dat:placement#0".into(),
            scope_record_index: None,
            entity_id: format!("Sketch_{}", text.owner_reference)
                .try_into()
                .unwrap(),
            visibility: None,
            class_tag: "256".to_owned().try_into().unwrap(),
            record_index: 1,
            paired_class_tag: "257".to_owned().try_into().unwrap(),
        };
        let (_, entities) = crate::test_support::with_decode_context(|ctx| {
            project_sketch_design(
                ctx,
                &SketchProjectInputs::new(&[placement], &[], &[], &[], &[text]),
                EPS_POINT_PROJECTION,
            )
        })
        .unwrap()
        .into_tuple();
        assert_eq!(
            entities.len(),
            usize::from(case.neutral_state),
            "{}",
            case.label
        );
    }
}

#[test]
fn text_of_a_model_space_sketch_has_no_planar_sketch_to_reference() {
    for (depth, planar) in [(5.0, false), (0.0, true)] {
        let text = crate::test_support::sketch_text::neutral_text_record();
        let owner = text.owner_reference;
        let stream = crate::ids::native_stream(&text.id).unwrap();
        let placement = DesignSketchPlacement {
            frame: crate::records::sketch_placement::DesignSketchFrame::new(
                0,
                crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
            )
            .unwrap(),
            id: format!("{stream}:placement#0"),
            scope_record_index: None,
            entity_id: format!("Sketch_{owner}").try_into().unwrap(),
            visibility: None,
            class_tag: "256".to_owned().try_into().unwrap(),
            record_index: 1,
            paired_class_tag: "257".to_owned().try_into().unwrap(),
        };
        let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: format!("{stream}:point#14"),
            record_index: 14,
            owner_reference: Some(owner),
            class_tag: crate::records::references::DesignClassTag::try_from("413".to_owned())
                .unwrap(),
            byte_offset: Some(14),
            coordinate_offset: Some(0),
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                14,
                crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
                None,
                depth,
            ),
            paired_reference: 15,
            coordinates: Point2::new(0.0, 0.0),
        })
        .unwrap();
        let design = crate::test_support::with_decode_context(|ctx| {
            project_sketch_design(
                ctx,
                &SketchProjectInputs::new(&[placement], &[point], &[], &[], &[text]),
                EPS_POINT_PROJECTION,
            )
        })
        .unwrap();
        let (sketches, entities) = design.into_tuple();
        assert_eq!(!sketches.is_empty(), planar);
        for entity in &entities {
            assert!(sketches.iter().any(|sketch| sketch.id == entity.sketch));
        }
        if !planar {
            assert!(entities.is_empty());
        }
    }
}

#[test]
fn sketch_text_alignment_ordinals_project_to_named_positions() {
    assert_eq!(
        sketch_text_horizontal_alignment(Some(1)),
        Some(Horizontal::Left)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(2)),
        Some(Horizontal::Right)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(3)),
        Some(Horizontal::Center)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(99)),
        Some(Horizontal::Native(99))
    );
    assert_eq!(sketch_text_vertical_alignment(Some(1)), Some(Vertical::Top));
    assert_eq!(
        sketch_text_vertical_alignment(Some(2)),
        Some(Vertical::Bottom)
    );
    assert_eq!(
        sketch_text_vertical_alignment(Some(3)),
        Some(Vertical::Middle)
    );
    assert_eq!(
        sketch_text_vertical_alignment(Some(99)),
        Some(Vertical::Native(99))
    );
    assert_eq!(sketch_text_horizontal_alignment(None), None);
    assert_eq!(sketch_text_vertical_alignment(None), None);
}
