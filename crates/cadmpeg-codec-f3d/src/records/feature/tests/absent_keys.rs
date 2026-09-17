// SPDX-License-Identifier: Apache-2.0
//! Absence spellings of the flattened feature-record readers.

#[test]
fn coil_secondary_identity_wire_refuses_a_null_identity() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::coil::deserialize_coil_secondary_identity"
        )]
        secondary: Option<crate::records::identity::DesignSecondaryIdentity<u64>>,
    }
    for key in ["secondary_identity", "curve_secondary_identity"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.secondary.is_none());
}

#[test]
fn coil_recipe_design_wire_refuses_a_null_design_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::coil::deserialize_coil_recipe_design"
        )]
        design: Option<crate::records::recipes::ConstructionRecipeDesign<String>>,
    }
    for key in ["design_id", "design_selector"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.design.is_none());
}

#[test]
fn work_plane_frame_wire_refuses_a_null_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::scope::deserialize_work_plane_frame"
        )]
        frame: Option<crate::records::feature::scope::DesignWorkPlaneTransform>,
    }
    for key in [
        "work_plane_transform",
        "work_plane_transform_offset",
        "work_plane_reference",
        "work_plane_reference_offset",
        "work_plane_construction",
    ] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.frame.is_none());
}

#[test]
fn joint_origin_frame_wire_refuses_a_null_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::scope::deserialize_joint_origin_frame"
        )]
        frame: Option<crate::records::feature::scope::DesignJointOriginTransform>,
    }
    for key in [
        "joint_origin_transform",
        "joint_origin_transform_offset",
        "joint_origin_reference",
        "joint_origin_reference_offset",
    ] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.frame.is_none());
}

#[test]
fn sketch_entity_wire_refuses_a_null_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::scope::deserialize_sketch_entity"
        )]
        entity: Option<crate::records::feature::scope::DesignSketchEntityBinding>,
    }
    for key in ["entity_id", "entity_suffix", "entity_reference_offset"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.entity.is_none());
}

/// Every flattened feature reader names the key it refuses.
///
/// Serde buffers a flattened field's keys into its own content map before the
/// reader runs, so no path a surrounding deserializer tracks reaches inside
/// one. The key reaches the refusal because the reading declaration states it.
#[test]
fn a_flattened_feature_reader_names_the_null_key_it_refuses() {
    fn refusal<T: serde::de::DeserializeOwned>(key: &str) -> String {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        let Err(refused) = serde_json::from_value::<T>(wire) else {
            panic!("{key}: null was admitted")
        };
        refused.to_string()
    }

    #[derive(serde::Deserialize)]
    struct Secondary {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::coil::deserialize_coil_secondary_identity"
        )]
        #[allow(dead_code)]
        value: Option<crate::records::identity::DesignSecondaryIdentity<u64>>,
    }
    #[derive(serde::Deserialize)]
    struct Design {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::coil::deserialize_coil_recipe_design"
        )]
        #[allow(dead_code)]
        value: Option<crate::records::recipes::ConstructionRecipeDesign<String>>,
    }
    #[derive(serde::Deserialize)]
    struct WorkPlane {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::scope::deserialize_work_plane_frame"
        )]
        #[allow(dead_code)]
        value: Option<crate::records::feature::scope::DesignWorkPlaneTransform>,
    }
    #[derive(serde::Deserialize)]
    struct JointOrigin {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::scope::deserialize_joint_origin_frame"
        )]
        #[allow(dead_code)]
        value: Option<crate::records::feature::scope::DesignJointOriginTransform>,
    }
    #[derive(serde::Deserialize)]
    struct SketchEntity {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::scope::deserialize_sketch_entity"
        )]
        #[allow(dead_code)]
        value: Option<crate::records::feature::scope::DesignSketchEntityBinding>,
    }

    let refusals = [
        (
            "secondary_identity",
            refusal::<Secondary>("secondary_identity"),
        ),
        (
            "curve_secondary_identity",
            refusal::<Secondary>("curve_secondary_identity"),
        ),
        ("design_id", refusal::<Design>("design_id")),
        ("design_selector", refusal::<Design>("design_selector")),
        (
            "work_plane_transform",
            refusal::<WorkPlane>("work_plane_transform"),
        ),
        (
            "work_plane_transform_offset",
            refusal::<WorkPlane>("work_plane_transform_offset"),
        ),
        (
            "work_plane_reference",
            refusal::<WorkPlane>("work_plane_reference"),
        ),
        (
            "work_plane_reference_offset",
            refusal::<WorkPlane>("work_plane_reference_offset"),
        ),
        (
            "work_plane_construction",
            refusal::<WorkPlane>("work_plane_construction"),
        ),
        (
            "joint_origin_transform",
            refusal::<JointOrigin>("joint_origin_transform"),
        ),
        (
            "joint_origin_transform_offset",
            refusal::<JointOrigin>("joint_origin_transform_offset"),
        ),
        (
            "joint_origin_reference",
            refusal::<JointOrigin>("joint_origin_reference"),
        ),
        (
            "joint_origin_reference_offset",
            refusal::<JointOrigin>("joint_origin_reference_offset"),
        ),
        ("entity_id", refusal::<SketchEntity>("entity_id")),
        ("entity_suffix", refusal::<SketchEntity>("entity_suffix")),
        (
            "entity_reference_offset",
            refusal::<SketchEntity>("entity_reference_offset"),
        ),
    ];
    for (key, message) in refusals {
        assert!(
            message.starts_with(&format!("{key}: ")),
            "the refusal of a null {key} states {message}"
        );
        assert!(
            message.contains("it does not state null"),
            "the refusal of a null {key} states {message}"
        );
    }
}
