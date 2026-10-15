// SPDX-License-Identifier: Apache-2.0
use crate::features::FinitePoint3;
use crate::geometry::sampled::{PolylineSamples, PolylineVertex};
use crate::math::Point3;
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn samples(parameterized: bool) -> PolylineSamples<FiniteReal, FinitePoint3> {
    let points = [
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(1., 1., 0.),
    ]
    .map(|point| FinitePoint3::new(point).expect("finite"));
    if parameterized {
        PolylineSamples::Parameterized {
            vertices: points
                .into_iter()
                .zip([2., 1., 0.])
                .map(|(point, parameter)| PolylineVertex {
                    point,
                    parameter: FiniteReal::new(parameter).expect("finite"),
                })
                .collect::<Vec<_>>()
                .try_into()
                .expect("three rows"),
        }
    } else {
        PolylineSamples::Unparameterized {
            points: points.to_vec().try_into().expect("three points"),
        }
    }
}

#[test]
fn sampled_point_edit_keeps_storage_parameters_and_releases_scratch() {
    for parameterized in [false, true] {
        let original = samples(parameterized);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut edited = original.clone();
        let mut calls = 0;
        assert_eq!(
            edited.edit_admitted_points(|point| {
                calls += 1;
                if calls == 3 {
                    Err("caller refused third point")
                } else {
                    Ok(point.negated())
                }
            }),
            Err("caller refused third point")
        );
        assert_eq!(edited, original);
        let address = match &edited {
            PolylineSamples::Unparameterized { points } => points.as_ptr().cast::<()>(),
            PolylineSamples::Parameterized { vertices } => vertices.as_ptr().cast::<()>(),
        };
        for _ in 0..2 {
            edited
                .edit_admitted_points(|point| Ok::<_, ()>(point.negated()))
                .expect("valid edit");
        }
        assert_eq!(edited, original);
        assert_eq!(
            match &edited {
                PolylineSamples::Unparameterized { points } => points.as_ptr().cast::<()>(),
                PolylineSamples::Parameterized { vertices } => vertices.as_ptr().cast::<()>(),
            },
            address
        );
        ctx.finish_session()
            .expect("scoped bytes are released on refusal and success");
    }
}
