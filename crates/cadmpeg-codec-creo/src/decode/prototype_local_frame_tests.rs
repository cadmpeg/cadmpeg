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

fn tabulated_cylinder_record(values: Vec<Option<f64>>) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord {
        declared_family: "tab_cyl".to_string(),
        family: SurfacePrototypeFamily::Extrusion,
        parameters: vec![SurfaceNamedParameter {
            name: "local_sys".to_string(),
            value: SurfaceNamedValue::ScalarArray {
                dimensions: 4,
                count: 3,
                values,
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

#[test]
fn tabulated_cylinder_uses_complete_local_system_origin() {
    let mut values = vec![Some(0.0); 12];
    values[9..12].copy_from_slice(&[Some(-4.0), Some(2.5), Some(8.0)]);

    assert_eq!(
        tabulated_cylinder_record(values).tabulated_cylinder_chart_origin(),
        Some([-4.0, 2.5, 8.0])
    );
}

#[test]
fn tabulated_cylinder_uses_compact_local_system_chart_origin() {
    let mut values = vec![Some(0.0); 7];
    values.extend([Some(-12.25), Some(-7.5), Some(0.0), None, None]);

    assert_eq!(
        tabulated_cylinder_record(values).tabulated_cylinder_chart_origin(),
        Some([-12.25, -7.5, 0.0])
    );
}

#[test]
fn tabulated_cylinder_chart_origin_rejects_other_local_system_shapes() {
    let mut values = vec![Some(0.0); 7];
    values.extend([Some(-12.25), Some(-7.5), None, None, None]);

    assert_eq!(
        tabulated_cylinder_record(values).tabulated_cylinder_chart_origin(),
        None
    );
}
