// SPDX-License-Identifier: Apache-2.0

use crate::pmdc::PmDcReference;
use crate::sketch::PmDcSketchConstraint;

pub(crate) fn entity_operand_cases() -> Vec<(PmDcSketchConstraint, Vec<u32>)> {
    let [r1, r2, r3, r4, r5] = [1, 2, 3, 4, 5].map(PmDcReference::from_packed);
    let cases = [
        (
            "944d8790d011f8d10008cabc0663dc09",
            serde_json::json!({"form": "coincident", "first": r1, "second": r2}),
            vec![1, 2],
        ),
        (
            "954d8790d011f8d10008cabc0663dc09",
            serde_json::json!({"form": "parallel", "first": r1, "second": r2, "orientation": 0}),
            vec![1, 2],
        ),
        (
            "964d8790d011f8d10008cabc0663dc09",
            serde_json::json!({"form": "perpendicular", "first": r1, "second": r2, "orientation": 0}),
            vec![1, 2],
        ),
        (
            "974d8790d011f8d10008cabc0663dc09",
            serde_json::json!({"form": "tangent", "first": r1, "second": r2}),
            vec![1, 2],
        ),
        (
            "984d8790d011f8d10008cabc0663dc09",
            serde_json::json!({"form": "horizontal", "entity": r1, "state": 1}),
            vec![1],
        ),
        (
            "994d8790d011f8d10008cabc0663dc09",
            serde_json::json!({"form": "vertical", "entity": r1, "state": 1}),
            vec![1],
        ),
        (
            "00c0ac00d1115fe0800066b1e13554c7",
            serde_json::json!({"form": "horizontal_distance", "first": r1, "second": r2, "parameter": r5, "values": [0, 0, 0, 0]}),
            vec![1, 2],
        ),
        (
            "40ff8336d1115fe0800066b1e13554c7",
            serde_json::json!({"form": "vertical_distance", "first": r1, "second": r2, "parameter": r5, "values": [0, 0, 0, 0]}),
            vec![1, 2],
        ),
        (
            "00b71b67d11168e0800066b1e13554c7",
            serde_json::json!({"form": "radius", "state": 0, "entity": r1, "values": [0, 0, 0, 0]}),
            vec![1],
        ),
        (
            "e096df74d11169e080006fb1e13554c7",
            serde_json::json!({"form": "diameter", "reference": r5, "entity": r1, "values": [0, 0, 0, 0]}),
            vec![1],
        ),
        (
            "008c10e1d11102e680006db1e13554c7",
            serde_json::json!({"form": "circle_center", "entity": r1, "center": r2}),
            vec![1, 2],
        ),
        (
            "d07d2c44d11189e680006fb1e13554c7",
            serde_json::json!({"form": "equal_radius", "first": r1, "second": r2}),
            vec![1, 2],
        ),
        (
            "e0c281c6d11110e680006eb1e13554c7",
            serde_json::json!({"form": "equal_length", "first": r1, "second": r2}),
            vec![1, 2],
        ),
        (
            "c0a3558fd1115ee0800066b1e13554c7",
            serde_json::json!({"form": "horizontal_alignment", "first": r1, "second": r2}),
            vec![1, 2],
        ),
        (
            "5052da64d1115ee0800066b1e13554c7",
            serde_json::json!({"form": "vertical_alignment", "first": r1, "second": r2}),
            vec![1, 2],
        ),
        (
            "58850511d211e39560000cb38932edb0",
            serde_json::json!({"form": "distance", "value": 0, "first": r1, "second": r2, "values": [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]}),
            vec![1, 2],
        ),
        (
            "100a0d59d111cae680006fb1e13554c7",
            serde_json::json!({"form": "angle", "value": 0, "first": r1, "second": r2, "values": [0.0, 0.0], "flags": 0}),
            vec![1, 2],
        ),
        (
            "bf70e821d011d0d20008ccbc0663dc09",
            serde_json::json!({"form": "midpoint", "entity": r1, "point": r2, "value": 0}),
            vec![1, 2],
        ),
        (
            "b07b170fd11196e680006fb1e13554c7",
            serde_json::json!({"form": "fix", "entity": r1}),
            vec![1],
        ),
        (
            "e07b9a5ad1115de0800066b1e13554c7",
            serde_json::json!({"form": "collinear", "first": r1, "second": r2, "state": 0}),
            vec![1, 2],
        ),
        (
            "107cdd0dd1119ce680006fb1e13554c7",
            serde_json::json!({"form": "symmetric", "first": r1, "second": r2, "axis": r3}),
            vec![1, 2, 3],
        ),
        (
            "bc144b4ed211fea4600010b38932edb0",
            serde_json::json!({"form": "equal_distance", "first": [r1, r2], "second": [r3, r4]}),
            vec![1, 2, 3, 4],
        ),
        (
            "9e146d7cd1115bd5000847b00524dc09",
            serde_json::json!({"form": "spline_point", "spline": r1, "point": r2}),
            vec![1, 2],
        ),
    ];
    cases
        .into_iter()
        .map(|(record_type, kind, operands)| {
            let mut wire = serde_json::to_value(super::sketch_curves::spline_midpoint())
                .expect("relation wire");
            wire["header"]["content"]["context"] = serde_json::json!(PmDcReference::from_packed(0));
            wire["header"]["group"] = serde_json::json!(PmDcReference::from_packed(0));
            wire["type_id"] = serde_json::json!(record_type);
            wire["kind"] = kind;
            (
                serde_json::from_value(wire).expect("synthetic relation"),
                operands,
            )
        })
        .collect()
}
