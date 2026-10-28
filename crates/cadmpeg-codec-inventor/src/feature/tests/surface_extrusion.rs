// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn surface_class_does_not_select_the_solid_decoder_or_validator_family() {
    let class = ClassId::try_from("f0a497e5d411d8841000faa5bba647b5".to_owned())
        .expect("surface extrusion class");
    assert!(crate::feature::FeatureFamily::from_class_id(class).is_none());
    assert!(!crate::feature::FeatureFamily::Extrusion
        .class_ids()
        .contains(&class));
    assert_eq!(
        crate::feature::native_kind(Some(class)).as_str(),
        "ExtrudeSurface"
    );
}
