// SPDX-License-Identifier: Apache-2.0

use super::super::prototype_mixed_derivatives;
use crate::surface::{
    SurfaceNamedParameter, SurfaceNamedValue, SurfacePrototypeFamily, SurfacePrototypeRecord,
};

const FIELD: &str = "end_uv_deriv";

fn record(value: SurfaceNamedValue) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord::new_for_test(
        SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Splsrf),
        vec![SurfaceNamedParameter {
            name: FIELD.to_owned(),
            value,
            body: Vec::new(),
            offset: 0,
            value_offset: 0,
        }],
        0,
    )
}

fn array(dimensions: u32, count: u32, values: Vec<Option<f64>>) -> SurfaceNamedValue {
    let mut array = crate::surface::arrays::DimensionedScalars::empty(dimensions, count)
        .expect("fixture extent");
    array.fill_values(values).expect("finite slots match shape");
    SurfaceNamedValue::ScalarArray(array)
}

fn assert_projection(record: &SurfacePrototypeRecord, expected: Option<[[f64; 3]; 4]>) {
    assert_eq!(prototype_mixed_derivatives(record), expected);

}

#[test]
fn mixed_derivatives_project_four_triples_without_temporary_storage() {
    let present = record(array(
        4,
        3,
        (0..12).map(|value| Some(f64::from(value))).collect(),
    ));
    assert_projection(
        &present,
        Some([
            [0.0, 1.0, 2.0],
            [3.0, 4.0, 5.0],
            [6.0, 7.0, 8.0],
            [9.0, 10.0, 11.0],
        ]),
    );
}

#[test]
fn mixed_derivatives_preserve_absent_partial_shape_and_ambiguity_routes() {
    for dimensions in [0, 1, 3, 5] {
        assert_projection(
            &record(array(
                dimensions,
                3,
                vec![
                    Some(1.0);
                    usize::try_from(dimensions).expect("fixture dimension fits usize") * 3
                ],
            )),
            None,
        );
    }
    assert_projection(&record(array(6, 2, vec![Some(1.0); 12])), None);
    for absent in [0, 11] {
        let mut values = vec![Some(1.0); 12];
        values[absent] = None;
        assert_projection(&record(array(4, 3, values)), None);
    }
    assert_projection(&record(SurfaceNamedValue::ScalarSequence(vec![1.0])), None);
    let mut duplicate = record(array(4, 3, vec![Some(1.0); 12]));
    let mut parameters = duplicate.parameters().to_vec();
    parameters.push(parameters[0].clone());
    duplicate =
        SurfacePrototypeRecord::new_for_test(duplicate.family, parameters, duplicate.offset);
    assert_projection(&duplicate, None);
    duplicate =
        SurfacePrototypeRecord::new_for_test(duplicate.family, Vec::new(), duplicate.offset);
    assert_projection(&duplicate, None);
}
