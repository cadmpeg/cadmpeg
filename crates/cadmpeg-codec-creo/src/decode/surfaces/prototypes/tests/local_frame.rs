use crate::decode::surfaces::prototypes::prototype_local_frame;
use crate::surface::{
    SurfaceNamedParameter, SurfaceNamedValue, SurfacePrototypeFamily, SurfacePrototypeRecord,
};

fn record(values: [f64; 12]) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord::new_for_test(
        SurfacePrototypeFamily::Torus(crate::surface::TorusLabel::Torus),
        vec![SurfaceNamedParameter {
            name: "local_sys".to_string(),
            value: SurfaceNamedValue::ScalarArray({
                let mut array = crate::surface::arrays::DimensionedScalars::empty(4, 3)
                    .expect("valid scalar array");
                array
                    .fill_values(values.into_iter().map(Some).collect())
                    .expect("matching scalar extent");
                array
            }),
            body: Vec::new(),
            offset: 0,
            value_offset: 0,
        }],
        0,
    )
}

#[test]
fn selects_the_unique_orthogonal_equal_scale_support_candidate() {
    let record = record([
        0.8, 0.6, 0.0, 1.0, 0.0, 0.0, -0.6, 0.8, 0.0, -180.0, -3.0, 40.0,
    ]);

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| prototype_local_frame(ctx, &record))
            .expect("frame selection admission"),
        Some(([-180.0, -3.0, 40.0], [0.0, -0.0, 1.0], [0.8, 0.6, 0.0]))
    );
}

#[test]
fn rejects_ambiguous_support_candidates() {
    let record = record([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]);

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| prototype_local_frame(ctx, &record))
            .expect("frame selection admission"),
        None
    );
}
