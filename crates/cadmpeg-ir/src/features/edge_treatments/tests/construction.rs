// SPDX-License-Identifier: Apache-2.0
use super::point;
use crate::features::edge_treatments::{VariableRadii, VariableRadius};
use crate::scalar::{Fraction, NonNegativeLength};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn raw() -> Vec<VariableRadius> {
    vec![point(0., 0.), point(0.5, 0.), point(1., 2.)]
}
fn typed() -> Vec<VariableRadius<Fraction, NonNegativeLength>> {
    raw()
        .into_iter()
        .map(|point| VariableRadius {
            parameter: Fraction::new(point.parameter).expect("fraction"),
            radius: NonNegativeLength::try_from(point.radius).expect("nonnegative"),
        })
        .collect()
}

#[test]
fn radius_construction_moves_typed_storage_and_shares_serde_admission() {
    let bytes =
        u64::try_from(3 * std::mem::size_of::<VariableRadius<Fraction, NonNegativeLength>>())
            .expect("bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let law = VariableRadii::new(raw()).expect("valid");
    ctx.finish_session().expect("no scratch or duplicate scan");
    assert_eq!(
        serde_json::from_str::<VariableRadii>(&serde_json::to_string(&law).expect("wire"))
            .expect("context-free serde"),
        law
    );
    let arena = DecodeArena::new();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let points = typed();
    let address = points.as_ptr();
    let checked = VariableRadii::from_parts(points).expect("valid");
    assert_eq!(checked.as_slice().as_ptr(), address);
    assert_eq!(checked, law);
    ctx.finish_session().expect("source rows moved");
}

#[test]
fn radius_construction_keeps_geometry_absence_distinct_from_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        VariableRadii::new(Vec::new()),
        Err(super::super::INVALID_VARIABLE_RADII)
    );
    assert_eq!(
        VariableRadii::from_parts(typed()[..1].to_vec()),
        Err(super::super::INVALID_VARIABLE_RADII)
    );
    ctx.finish_session()
        .expect("static diagnostic needs no allocation");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut points = typed();
    for point in &mut points {
        point.radius = NonNegativeLength::ZERO;
    }
    assert_eq!(
        VariableRadii::from_parts(points),
        Err(super::super::INVALID_VARIABLE_RADII)
    );
    ctx.finish_session()
        .expect("parameter order is not scanned after zero radii");
}
