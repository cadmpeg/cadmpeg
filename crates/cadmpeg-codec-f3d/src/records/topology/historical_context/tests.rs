// SPDX-License-Identifier: Apache-2.0
//! Historical face contexts: every binding stage is complete.

#[test]
fn historical_loop_wire_preserves_each_complete_binding_stage() {
    for count in [0_u32, 1, 3] {
        for stage in 0..4 {
            let mut wire = serde_json::json!({
                "loop_slot": 10,
                "coedge_slots": (0..count).map(|index| 20 + index).collect::<Vec<_>>(),
                "edge_slots": (0..count).map(|index| 30 + index).collect::<Vec<_>>()
            });
            if count != 0 {
                if stage >= 1 {
                    wire["vertex_slots"] =
                        serde_json::json!((0..count).map(|index| 40 + index).collect::<Vec<_>>());
                }
                if stage >= 2 {
                    wire["point_slots"] =
                        serde_json::json!((0..count).map(|index| 50 + index).collect::<Vec<_>>());
                }
                if stage >= 3 {
                    wire["positions"] = serde_json::json!((0..count)
                        .map(|index| cadmpeg_ir::math::Point3::new(f64::from(index), 0.0, 0.0))
                        .collect::<Vec<_>>());
                }
            }
            let context: super::DesignHistoricalFaceLoopContext =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(context.boundary.coedges().count(), count as usize);
            assert_eq!(serde_json::to_value(&context).unwrap(), wire);
            for field in ["edge_slots", "vertex_slots", "point_slots", "positions"] {
                let mut invalid = wire.clone();
                let mut values = invalid
                    .get(field)
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for _ in 0..=count {
                    values.push(if field == "positions" {
                        serde_json::to_value(cadmpeg_ir::math::Point3::new(9.0, 0.0, 0.0)).unwrap()
                    } else {
                        serde_json::json!(99)
                    });
                }
                invalid[field] = serde_json::Value::Array(values);
                assert!(
                    serde_json::from_value::<super::DesignHistoricalFaceLoopContext>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains(field)
                );
            }
        }
    }
}
