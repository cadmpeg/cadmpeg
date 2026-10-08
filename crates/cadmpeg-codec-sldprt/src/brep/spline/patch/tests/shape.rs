// SPDX-License-Identifier: Apache-2.0
//! Homogeneous write-back shape and missing-lane checks.

use super::super::{unique_knots, unique_surface_knot_span};
use super::super::super::Arrays;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsPoles3;

    #[test]
    fn patch_shape_counts_do_not_narrow_to_the_native_multiplicity_width() {
        let count = usize::from(u16::MAX) + 1;
        let knots = std::iter::repeat_n(0.0, count)
            .chain(std::iter::once(1.0))
            .collect::<Vec<_>>();
        let (values, multiplicities) =
            unique_knots(&cadmpeg_test_support::service_decode_context(), &knots).unwrap();
        assert_eq!(values, [0.0, 1.0]);
        assert_eq!(multiplicities, [count, 1]);
    }

    /// A rational pole whose coordinate times its weight overflows declines
    /// the patch, where its homogeneous coordinate was written as an
    /// infinity.
    #[test]
    fn a_pole_whose_weighted_coordinate_overflows_declines_the_patch() {
        let point = FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0e300, 0.0, 0.0)).unwrap();
        assert_eq!(
            super::super::homogeneous_poles(
                &cadmpeg_test_support::service_decode_context(),
                &NurbsPoles3::Rational {
                    points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                        point,
                        weight: cadmpeg_ir::scalar::NonZeroReal::new(1.0e300).unwrap()
                    }]
                },
                0.001
            )
            .unwrap(),
            None
        );
        assert_eq!(
            super::super::homogeneous_poles(
                &cadmpeg_test_support::service_decode_context(),
                &NurbsPoles3::Rational {
                    points: vec![cadmpeg_ir::geometry::nurbs::WeightedPole3 {
                        point,
                        weight: cadmpeg_ir::scalar::NonZeroReal::new(2.0).unwrap()
                    }]
                },
                0.001
            )
            .unwrap(),
            Some(vec![1.0e300 * 0.001 * 2.0, 0.0, 0.0, 2.0])
        );
    }

    #[test]
    fn missing_surface_knot_arrays_decline_the_patch() {
        assert_eq!(
            unique_surface_knot_span(
                &cadmpeg_test_support::service_decode_context(),
                &[],
                &Arrays::default(),
                (2, 3),
                1,
                &[0.0],
                &[1]
            )
            .unwrap(),
            None
        );
    }
