// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::topology::IncreasingParameterInterval;

fn sampled(target: [f64; 3], source: [f64; 3]) -> Class25SourceRelation {
    Class25SourceRelation::Sampled {
        range: IncreasingParameterInterval::new([
            source[0].min(source[2]),
            source[0].max(source[2]),
        ])
        .expect("finite fixture interval"),
        samples: Box::new(
            json::from_value(serde_json::json!({
                "target": target,
                "source": source,
            }))
            .expect("monotone paired samples"),
        ),
    }
}

#[test]
fn sampled_identity_preserves_interior_stations_across_parameter_scales() {
    for scale in [1.0_f64, 1.0e3, 1.0e7, 1.0e12] {
        let target = [-scale, 0.0, scale];
        let source = target.map(|t| t.next_up().next_up().next_up());
        let relation = sampled(target, source);
        for axis in [0, 1] {
            for t in [-scale / 2.0, scale / 2.0] {
                assert_eq!(map_parameter(&relation, axis, t), Some(t));
            }
        }
    }
}

#[test]
fn sampled_identity_requires_agreement_at_every_site() {
    let target = [-1.0e7, 0.0, 1.0e7];
    let source = [-1.0e7, 0.001, 1.0e7];
    let relation = sampled(target, source);
    for axis in [0, 1] {
        assert_eq!(map_parameter(&relation, axis, 5.0e6), None);
    }
    assert_eq!(map_parameter(&relation, 0, 0.0), Some(0.001));
    assert_eq!(map_parameter(&relation, 1, 0.001), Some(0.0));
}

#[test]
fn sampled_parameter_direction_and_paired_values_survive_roundoff_handling() {
    let relation = sampled([-1.0e7, 0.0, 1.0e7], [1.0e7, 0.0, -1.0e7]);
    assert_eq!(map_parameter(&relation, 0, -1.0e7), Some(1.0e7));
    assert_eq!(map_parameter(&relation, 1, -1.0e7), Some(1.0e7));
    assert_eq!(map_parameter(&relation, 0, 5.0e6), None);
    assert_eq!(map_parameter(&relation, 1, 5.0e6), None);
}
