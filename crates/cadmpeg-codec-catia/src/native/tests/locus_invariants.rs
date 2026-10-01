// SPDX-License-Identifier: Apache-2.0
use crate::native::{CatiaZeroEntityEndpointLocusCandidate, CatiaZeroEntityEndpointPairEndpoint};

#[test]
fn endpoint_locus_wire_rejects_empty_incidence_and_negative_deviation() {
    let value = CatiaZeroEntityEndpointLocusCandidate {
        id: "locus".to_owned(),
        incident_endpoint_pair_endpoints: cadmpeg_ir::features::NonEmptyMembers::one(CatiaZeroEntityEndpointPairEndpoint {
            endpoint_pair: "pair".to_owned(), endpoint_index: crate::families::zero_entity::topology::EdgeEnd::Start,
        }),
        representative_point: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)).expect("finite point"),
        maximum_deviation: cadmpeg_ir::scalar::NonNegativeReal::new(0.0).expect("zero deviation"),
    };
    let wire = serde_json::to_value(&value).expect("locus wire");
    assert_eq!(serde_json::from_value::<CatiaZeroEntityEndpointLocusCandidate>(wire.clone()).expect("valid locus"), value);
    let mut empty = wire.clone();
    empty["incident_endpoint_pair_endpoints"] = serde_json::json!([]);
    assert!(serde_json::from_value::<CatiaZeroEntityEndpointLocusCandidate>(empty).is_err());
    let mut negative = wire;
    negative["maximum_deviation"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<CatiaZeroEntityEndpointLocusCandidate>(negative).is_err());
    assert!(cadmpeg_ir::scalar::NonNegativeReal::new(f64::INFINITY).is_none());
    assert!(cadmpeg_ir::scalar::NonNegativeReal::new(f64::NAN).is_none());
}
