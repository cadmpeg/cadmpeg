// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn surface_class_selects_only_the_surface_extrusion_family() {
    let class = ClassId::try_from("f0a497e5d411d8841000faa5bba647b5".to_owned())
        .expect("surface extrusion class");
    assert!(matches!(
        crate::feature::FeatureFamily::from_class_id(class),
        Some(crate::feature::FeatureFamily::SurfaceExtrusion)
    ));
    assert!(!crate::feature::FeatureFamily::Extrusion
        .class_ids()
        .contains(&class));
    assert_eq!(
        crate::feature::native_kind(Some(class)).as_str(),
        "ExtrudeSurface"
    );
}
