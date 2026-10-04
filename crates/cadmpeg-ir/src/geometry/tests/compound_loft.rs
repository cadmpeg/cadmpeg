// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{CompoundLoftDirection, CompoundLoftTail};

#[test]
fn the_direction_selector_is_stored_once_on_the_curve_form() {
    for selector in [0, 1, 4, -3] {
        let direction = if selector == 0 {
            CompoundLoftDirection::Vector {
                value: crate::math::Vector3::new(1.0, 0.0, 0.0),
            }
        } else {
            CompoundLoftDirection::Curve {
                curve: "test:model:curve#0".try_into().expect("valid identity"),
                selector: std::num::NonZeroI64::new(selector).unwrap(),
            }
        };
        let tail = CompoundLoftTail::Zero {
            flags: [true, false],
            direction,
            trailing_flags: [false, true],
        };
        let wire = serde_json::to_value(&tail).unwrap();
        assert!(wire.get("selector").is_none());
        if selector == 0 {
            assert_eq!(wire["direction"]["kind"], "vector");
            assert!(wire["direction"].get("selector").is_none());
        } else {
            assert_eq!(wire["direction"]["selector"], selector);
        }
        assert_eq!(
            serde_json::from_value::<CompoundLoftTail>(wire.clone()).unwrap(),
            tail
        );

        let mut zero = wire;
        zero["direction"]["selector"] = serde_json::json!(0);
        assert!(serde_json::from_value::<CompoundLoftTail>(zero).is_err());
    }
}

#[test]
fn a_scaled_direction_keeps_its_exact_nonzero_selector() {
    use crate::geometry::ScaledCompoundLoftBranch;
    let branch = ScaledCompoundLoftBranch::Direct {
        flag: true,
        direction: CompoundLoftDirection::Curve {
            curve: "test:model:curve#0".try_into().expect("valid identity"),
            selector: std::num::NonZeroI64::new(-4).unwrap(),
        },
    };
    let mut wire = serde_json::to_value(&branch).unwrap();
    assert_eq!(wire["direction"]["selector"], -4);
    assert_eq!(
        serde_json::from_value::<ScaledCompoundLoftBranch>(wire.clone()).unwrap(),
        branch
    );
    wire["direction"]["selector"] = serde_json::json!(0);
    assert!(serde_json::from_value::<ScaledCompoundLoftBranch>(wire).is_err());
}
