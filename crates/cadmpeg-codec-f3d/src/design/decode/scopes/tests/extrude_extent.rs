// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports, clippy::default_trait_access, clippy::wildcard_imports)]

use super::exact_extrude_extent;
use super::prelude::*;

#[test]
fn extrude_extent_tuple_is_one_admission_key() {
    let accepted = [
        (1, [1, 0], DesignExtrudeExtent::OneSidedDistance),
        (1, [2, 0], DesignExtrudeExtent::OneSidedToFace),
        (1, [3, 0], DesignExtrudeExtent::OneSidedThroughNext),
        (1, [4, 0], DesignExtrudeExtent::OneSidedThroughAll),
        (2, [2, 0], DesignExtrudeExtent::TwoSidedToFaces),
        (2, [1, 1], DesignExtrudeExtent::TwoSidedDistance),
        (3, [1, 0], DesignExtrudeExtent::SymmetricDistance),
        (3, [4, 4], DesignExtrudeExtent::SymmetricThroughAll),
    ];
    for (direction, side_extent_discriminators, expected) in accepted {
        assert_eq!(
            exact_extrude_extent(direction, side_extent_discriminators),
            Some(expected)
        );
    }

    for (direction, side_extent_discriminators) in [
        (0, [1, 0]),
        (1, [1, 1]),
        (1, [4, 4]),
        (2, [1, 0]),
        (2, [4, 4]),
        (3, [1, 1]),
    ] {
        assert_eq!(
            exact_extrude_extent(direction, side_extent_discriminators),
            None
        );
    }
}
