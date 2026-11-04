// SPDX-License-Identifier: Apache-2.0

use super::{property, scalar, Object};

#[test]
fn unknown_emission_is_not_an_empty_witness_and_generic_mutation_invalidates_it() {
    let mut object = Object::new("Group".into(), "App::DocumentObjectGroup", "Group");
    assert!(!object.geometry_empty());
    object.mark_grouping_only();
    assert!(object.geometry_empty());
    object.set_text("Label", "Renamed");
    object.set_boolean("Suppressed", false);
    object.set_metadata(
        "CadmpegFeatureOrdinal",
        scalar(
            "CadmpegFeatureOrdinal",
            "App::PropertyString",
            "String",
            "0",
        ),
    );
    object.set_metadata(
        "CadmpegHasFeatureSourceTag",
        scalar(
            "CadmpegHasFeatureSourceTag",
            "App::PropertyBool",
            "Bool",
            u8::from(false),
        ),
    );
    object.set_metadata(
        "CadmpegParameterOrdinals",
        property(
            "CadmpegParameterOrdinals",
            "App::PropertyStringList",
            "<StringList count=\"1\"><String value=\"length=0\"/></StringList>",
        ),
    );
    assert!(object.geometry_empty());
    object.set("Shape", "independently emitted property".into());
    assert!(!object.geometry_empty());
    object.mark_geometry_written();
    assert!(!object.geometry_empty());
}

#[test]
fn changed_runtime_or_extension_and_mutable_property_access_invalidate_grouping_proof() {
    let group = Object::new("Group".into(), "App::DocumentObjectGroup", "Group");
    let mut changed = group.clone();
    changed.mark_grouping_only();
    changed.kind = "Part::Box".into();
    assert!(!changed.geometry_empty());
    let mut extended = group.clone();
    extended.mark_grouping_only();
    extended.extensions.push("independent extension".into());
    assert!(!extended.geometry_empty());
    let mut edited = group;
    edited.mark_grouping_only();
    edited
        .properties
        .get_mut("Label")
        .unwrap()
        .push_str(" edit");
    assert!(!edited.geometry_empty());
    let mut bypassed = edited;
    bypassed.mark_grouping_only();
    bypassed
        .properties
        .insert("Shape".into(), "new geometry".into());
    assert!(!bypassed.geometry_empty());
}
