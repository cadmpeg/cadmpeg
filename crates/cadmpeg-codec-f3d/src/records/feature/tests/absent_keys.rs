// SPDX-License-Identifier: Apache-2.0
//! Absence spellings of the flattened feature-record readers.

#[test]
fn coil_secondary_identity_wire_refuses_a_null_identity() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "crate::records::feature::deserialize_coil_secondary_identity"
        )]
        secondary: Option<crate::records::feature::DesignSecondaryIdentity<u64>>,
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
            deserialize_with = "crate::records::feature::deserialize_coil_recipe_design"
        )]
        design: Option<crate::records::feature::ConstructionRecipeDesign<String>>,
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
            deserialize_with = "crate::records::feature::deserialize_work_plane_frame"
        )]
        frame: Option<crate::records::feature::DesignWorkPlaneTransform>,
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
            deserialize_with = "crate::records::feature::deserialize_joint_origin_frame"
        )]
        frame: Option<crate::records::feature::DesignJointOriginTransform>,
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
            deserialize_with = "crate::records::feature::deserialize_sketch_entity"
        )]
        entity: Option<crate::records::feature::DesignSketchEntityBinding>,
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
