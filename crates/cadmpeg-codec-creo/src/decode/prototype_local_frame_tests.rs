use super::*;
use crate::surface::{
    SurfaceNamedParameter, SurfaceNamedValue, SurfacePrototypeFamily, SurfacePrototypeRecord,
};

fn record(values: [f64; 12]) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord {
        declared_family: "torus".to_string(),
        family: SurfacePrototypeFamily::Torus,
        parameters: vec![SurfaceNamedParameter {
            name: "local_sys".to_string(),
            value: SurfaceNamedValue::ScalarArray {
                dimensions: 4,
                count: 3,
                values: values.into_iter().map(Some).collect(),
                tokens: Vec::new(),
            },
            body: Vec::new(),
            offset: 0,
            value_offset: 0,
        }],
        offset: 0,
    }
}

#[test]
fn selects_the_unique_orthogonal_equal_scale_support_candidate() {
    let record = record([
        0.8, 0.6, 0.0, 1.0, 0.0, 0.0, -0.6, 0.8, 0.0, -180.0, -3.0, 40.0,
    ]);

    assert_eq!(
        prototype_local_frame(&record),
        Some(([-180.0, -3.0, 40.0], [0.0, -0.0, 1.0], [0.8, 0.6, 0.0]))
    );
}

#[test]
fn rejects_ambiguous_support_candidates() {
    let record = record([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]);

    assert_eq!(prototype_local_frame(&record), None);
}
