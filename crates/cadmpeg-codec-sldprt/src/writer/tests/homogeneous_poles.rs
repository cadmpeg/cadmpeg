// SPDX-License-Identifier: Apache-2.0
//! Homogeneous NURBS poles the writer computes from admitted geometry.

/// A rational pole whose coordinate in metres times its weight overflows is
/// refused, where its homogeneous coordinate was written as an infinity.
#[test]
fn a_pole_whose_weighted_coordinate_overflows_is_refused() {
    let point =
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0e300, 0.0, 0.0))
            .unwrap();
    let poles = cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational {
        points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
            point,
            weight: cadmpeg_ir::scalar::NonZeroReal::new(1.0e300).unwrap(),
        }],
    };
    let written = crate::writer::homogeneous_poles(&poles, 0.001);
    let Err(cadmpeg_core::CodecError::NotImplemented(message)) = written else {
        panic!("{written:?}");
    };
    assert!(message.contains("pole 0"), "{message}");
    assert_eq!(
        crate::writer::homogeneous_poles(
            &cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational {
                points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                    point,
                    weight: cadmpeg_ir::scalar::NonZeroReal::new(2.0).unwrap()
                }]
            },
            0.001
        )
        .expect("finite pole"),
        [1.0e300 * 0.001 * 2.0, 0.0, 0.0, 2.0]
    );
}
