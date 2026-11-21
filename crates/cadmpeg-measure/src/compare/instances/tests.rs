// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::geometry::FiniteVector3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::transform::Transform;

use super::deviation;

fn shifted(x: f64) -> Transform {
    Transform::identity().with_translation(FiniteVector3::new(Vector3::new(x, 0.0, 0.0)).unwrap())
}

#[test]
fn a_translation_deviates_by_its_length() {
    assert!((deviation(shifted(1.0), shifted(4.0), 10.0) - 3.0).abs() < 1e-12);
}
