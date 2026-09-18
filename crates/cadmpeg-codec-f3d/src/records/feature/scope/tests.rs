// SPDX-License-Identifier: Apache-2.0
//! Absence spellings of the flattened scope-record readers.

#[test]
fn work_plane_frame_wire_refuses_a_null_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(flatten, deserialize_with = "super::deserialize_work_plane_frame")]
        frame: Option<super::DesignWorkPlaneTransform>,
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
        #[serde(flatten, deserialize_with = "super::deserialize_joint_origin_frame")]
        frame: Option<super::DesignJointOriginTransform>,
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
        #[serde(flatten, deserialize_with = "super::deserialize_sketch_entity")]
        entity: Option<super::DesignSketchEntityBinding>,
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

/// Every flattened scope reader names the key it refuses.
///
/// Serde buffers a flattened field's keys into its own content map before the
/// reader runs, so no path a surrounding deserializer tracks reaches inside
/// one. The key reaches the refusal because the reading declaration states it.
#[test]
fn a_flattened_scope_reader_names_the_null_key_it_refuses() {
    use crate::records::test_support::refusal;

    #[derive(serde::Deserialize)]
    struct WorkPlane {
        #[serde(flatten, deserialize_with = "super::deserialize_work_plane_frame")]
        #[allow(dead_code)]
        value: Option<super::DesignWorkPlaneTransform>,
    }
    #[derive(serde::Deserialize)]
    struct JointOrigin {
        #[serde(flatten, deserialize_with = "super::deserialize_joint_origin_frame")]
        #[allow(dead_code)]
        value: Option<super::DesignJointOriginTransform>,
    }
    #[derive(serde::Deserialize)]
    struct SketchEntity {
        #[serde(flatten, deserialize_with = "super::deserialize_sketch_entity")]
        #[allow(dead_code)]
        value: Option<super::DesignSketchEntityBinding>,
    }

    let refusals = [
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
