// SPDX-License-Identifier: Apache-2.0
use super::{base_feature, scope_between, state};
use crate::history::{bind_feature_outputs, binding::transition_geometry::unchanged};
use crate::history_records::{AsmEntityVersion, AsmHistoricalTopology};
use cadmpeg_asm::sab::{Record, Token};
use std::collections::HashMap;

#[test]
fn transition_geometry_property_requires_bound_attribute_only_transition() {
    for (inserted, surface_changed, bound, expected) in [
        (false, false, true, Some("unchanged")),
        (true, false, true, None),
        (false, true, true, None),
        (false, false, false, None),
    ] {
        let scope = scope_between(1, 0);
        let mut feature = base_feature();
        feature.native_ref = Some(scope.id.clone());
        let topology = AsmHistoricalTopology {
            faces: vec![1],
            ..Default::default()
        };
        let mut current = state(1, 1, Some(0), topology.clone());
        current.2.push(AsmEntityVersion {
            entity_ref: 1,
            record_ref: 11,
        });
        let mut previous = state(
            0,
            0,
            None,
            if inserted {
                AsmHistoricalTopology::default()
            } else {
                topology
            },
        );
        if !inserted {
            previous.2.push(AsmEntityVersion {
                entity_ref: 1,
                record_ref: 10,
            });
        }
        let mut history = crate::test_support::history::history(
            "f3d:asm-history#1".into(),
            0,
            None,
            false,
            vec![current, previous],
        );
        let record = |attribute, surface| Record {
            index: 1,
            name: "face".into(),
            offset: 0,
            len: 0,
            tokens: vec![
                Token::Ref(attribute),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(surface),
                Token::False,
                Token::False,
            ]
            .into(),
        };
        let archive = HashMap::from([
            (10, record(20, 30)),
            (11, record(21, if surface_changed { 31 } else { 30 })),
        ]);
        let transition = history.states_mut()[0].transition.as_mut().unwrap();
        transition.geometry_unchanged = unchanged(
            transition,
            &std::collections::BTreeMap::from([(1, 10)]),
            &std::collections::BTreeMap::from([(1, 11)]),
            &archive,
        );
        let owners = HashMap::from([(scope.id.clone(), history.id.clone())]);
        let mut histories = if bound { vec![history] } else { vec![] };
        bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut histories,
            &[],
            &owners,
            None,
        )
        .unwrap();
        assert_eq!(
            feature
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("transition_geometry"))
                .map(String::as_str),
            expected
        );
    }
}
