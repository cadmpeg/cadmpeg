// SPDX-License-Identifier: Apache-2.0

use super::{owned_test_file, OwnedTestEntity};

pub(super) const LINEAR_INTERVAL_CONTROLS: usize = 65;

// Unequal rail degrees make the first interval vector exceed the trimming peak.
pub(super) fn linear_bezier_ruled_file() -> Vec<u8> {
    let rail = |y: usize, control_count: usize| {
        let degree = control_count - 1;
        let mut parameters = vec!["126".to_owned(), degree.to_string(), degree.to_string(),
            "0".into(), "0".into(), "1".into(), "0".into()];
        parameters.extend((0..control_count).map(|_| "0".to_owned()));
        parameters.extend((0..control_count).map(|_| "1".to_owned()));
        parameters.extend((0..control_count).map(|_| "1".to_owned()));
        for x in 0..control_count {
            parameters.extend([x.to_string(), y.to_string(), "0".into()]);
        }
        parameters.extend(["0".into(), "1".into()]);
        parameters.join(",") + ";"
    };
    owned_test_file(&[
        OwnedTestEntity { entity_type: 126, form: 0, label: "RAIL1".into(),
            status: "00000000", parameters: rail(0, LINEAR_INTERVAL_CONTROLS) },
        OwnedTestEntity { entity_type: 126, form: 0, label: "RAIL2".into(),
            status: "00000000", parameters: rail(1, 4) },
        OwnedTestEntity { entity_type: 118, form: 0, label: "RULED".into(),
            status: "00000000", parameters: "118,1,3,0,0;".into() },
    ])
}
