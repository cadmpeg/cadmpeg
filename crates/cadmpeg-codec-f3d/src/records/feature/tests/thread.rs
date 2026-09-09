// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::{
    DesignThreadConstruction, DesignThreadConstructionWire, DesignThreadDiameters,
};

fn thread_wire() -> serde_json::Value {
    serde_json::json!({"form":"standard", "designation_offset":38, "designation":"M1", "nominal_size_text":"1.0", "nominal_size":1.0, "profile":"ISO Metric profile", "major_diameter":1.0, "minor_diameter":0.5, "pitch":0.1, "pitch_diameter":0.75, "face_group_record_indices":[10]})
}

#[test]
fn thread_diameters_require_strict_order_and_positive_finite_values() {
    assert!(DesignThreadDiameters::new(1.0, 0.5, 0.75).is_some());
    for (major, minor, pitch) in [(1.0, 1.0, 1.0), (1.0, 0.75, 0.5), (0.75, 0.5, 1.0)] {
        assert!(DesignThreadDiameters::new(major, minor, pitch).is_none());
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for lane in 0..4 {
            let mut wire: DesignThreadConstructionWire =
                serde_json::from_value(thread_wire()).unwrap();
            *[
                &mut wire.major_diameter,
                &mut wire.minor_diameter,
                &mut wire.pitch_diameter,
                &mut wire.pitch,
            ][lane] = invalid;
            assert!(DesignThreadConstruction::try_from(wire).is_err());
        }
    }
}

#[test]
fn thread_wire_rejects_empty_names_and_invalid_dimensions() {
    for field in ["designation", "profile"] {
        let mut wire = thread_wire();
        wire[field] = serde_json::json!("");
        assert!(serde_json::from_value::<DesignThreadConstruction>(wire).is_err());
        let mut wire = thread_wire();
        wire[field] = serde_json::json!(" ");
        let admitted: DesignThreadConstruction = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    }
    for (field, invalid) in [
        ("major_diameter", 0.75),
        ("minor_diameter", 0.75),
        ("pitch_diameter", 1.0),
        ("pitch", 0.0),
    ] {
        let mut wire = thread_wire();
        wire[field] = serde_json::json!(invalid);
        assert!(serde_json::from_value::<DesignThreadConstruction>(wire).is_err());
    }
}
