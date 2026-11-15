use cadmpeg_ir::features::form::{FormCageBinding, FormResourceAvailability};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn form_cage_program_failures_retain_the_active_resource_and_native_parser_loss() {
    for (valid, program) in [
        (true, crate::test_support::zip_test::generated_form_tsm()),
        (false, b"invalid synthetic cage program".to_vec()),
    ] {
        let bytes = crate::test_support::zip_test::f3d_with_form_body_resource(&program);
        let decoded = super::decode(bytes);
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some("Form"))
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Form(construction)) =
            feature.evaluation.definition()
        else {
            panic!("Form");
        };
        assert_eq!(construction.membership_is_complete(), valid);
        assert_eq!(decoded.ir().model.subds.len(), usize::from(valid));
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == crate::loss::F3dLossCode::TsplineCageUndecoded.kind()),
            !valid
        );
        let observation = &construction.observations()[0];
        let resource = match &observation.binding {
            FormCageBinding::Decoded {
                resource: Some(resource),
                ..
            } if valid => resource,
            FormCageBinding::Undecoded {
                resource,
                availability: FormResourceAvailability::ActiveUndecoded,
            } if !valid => resource,
            _ => panic!("active Form resource"),
        };
        assert_eq!(resource.entry.as_str(), "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm");
    }
}
