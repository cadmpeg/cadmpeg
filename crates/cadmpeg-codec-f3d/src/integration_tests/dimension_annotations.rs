// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchLocus};

use super::{assert_valid, decode};
use crate::loss::F3dLossCode;
use crate::test_support::native_test::f3d_native;
use crate::test_support::zip_test::{f3d_with_generated_sketch_annotation, SketchAnnotationCase};
use crate::F3dCodec;

#[test]
fn dimension_annotation_empty_metadata_projects_the_governed_parameter_and_entities() {
    let result = decode(f3d_with_generated_sketch_annotation(
        SketchAnnotationCase::Valid,
    ));
    assert_valid(&result);
    let native = f3d_native(result.ir());
    assert_eq!(native.design_dimension_annotation_frames.len(), 1);
    let frame = &native.design_dimension_annotation_frames[0];
    assert_eq!(frame.entity_genesis, None);
    assert_eq!(frame.companion_record_index, Some(1202));
    assert_eq!(frame.governing_companion_record_index, 1205);
    let source_parameter = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.record_index == 1204)
        .unwrap();
    let expected_parameter = crate::ids::neutral_parameter_id(source_parameter);
    let constraints = result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.native_ref.as_deref() == Some(frame.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(constraints.len(), 1);
    let SketchConstraintDefinitionInput::HorizontalDistance {
        first: SketchLocus::Entity(first),
        second: SketchLocus::Entity(second),
        parameter,
    } = constraints[0].definition.kind()
    else {
        panic!("horizontal dimension")
    };
    assert_eq!(
        parameter,
        &cadmpeg_ir::Dimension::Parameter(expected_parameter)
    );
    for (index, entity) in [(1300, first), (1302, second)] {
        let source = native
            .sketch_points
            .iter()
            .find(|point| point.record_index == index)
            .unwrap();
        let expected = result
            .ir()
            .model
            .sketch_entities
            .iter()
            .find(|entity| entity.native_ref.as_deref() == Some(source.id.as_str()))
            .unwrap();
        assert_eq!(entity, expected.id());
    }
    assert!(<F3dCodec as CodecBackend>::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        result.ir(),
        None
    )
    .unwrap()
    .iter()
    .all(|finding| finding.entity.as_deref() != Some(frame.id.as_str())));
    assert!(!result.report().losses.iter().any(|loss| loss.code
        == F3dLossCode::SketchDimensionNativeRetained
            .note(String::new())
            .code));
}

#[test]
fn dimension_annotation_empty_metadata_keeps_ambiguous_and_refused_dimensions_with_losses() {
    for case in [
        SketchAnnotationCase::Ambiguous,
        SketchAnnotationCase::UnknownOperand,
        SketchAnnotationCase::MetadataCount,
    ] {
        let result = decode(f3d_with_generated_sketch_annotation(case));
        let native = f3d_native(result.ir());
        assert_eq!(
            native.design_dimension_annotation_frames.len(),
            usize::from(case == SketchAnnotationCase::Ambiguous)
        );
        let source = native
            .design_parameters
            .iter()
            .find(|parameter| parameter.record_index == 1204)
            .unwrap();
        let expected_parameter = crate::ids::neutral_parameter_id(source);
        assert!(result.ir().model.sketch_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Native { parameter: Some(parameter), .. } if parameter == &expected_parameter)));
        assert!(result.report().losses.iter().any(|loss| loss.code
            == F3dLossCode::SketchDimensionNativeRetained
                .note(String::new())
                .code));
    }
}
