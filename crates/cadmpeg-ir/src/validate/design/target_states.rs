// SPDX-License-Identifier: Apache-2.0
//! The one spelling of an instance target's `state` (§8, part 10): a target
//! whose `state` names the state it reaches with `state: None` is an Error.
//! The decode wrapper's normalizer rewrites the same targets, so a decoded
//! document holds none.

use crate::design::normalize::states::{each_target, Reach, States};
use crate::document::CadIr;
use crate::report::check::Finding;

pub(super) fn check(ir: &CadIr, findings: &mut Vec<Finding>) {
    let states = States::new(&ir.design);
    let reach = Reach::new(&states, ir);
    each_target(&ir.model, |owner, path, state| {
        let Some(state) = state else {
            return;
        };
        let mut spelled = state.clone();
        states.state_ref(&mut spelled);
        if reach.reached(path, &spelled) {
            super::error(
                findings,
                owner,
                crate::rule!(
                    "design.instance-target-state-none-reaches",
                    "a target states the state it reaches with state None, whose one spelling is state None"
                ),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::DecodePolicy;

    use crate::design::refs::{Evaluated, Member, Position, StateRef};
    use crate::document::CadIr;
    use crate::pmi::{PmiAnnotation, PmiDefinition, PmiDimension, PmiDimensionKind, PmiTarget};
    use crate::references::InstanceTarget;
    use crate::validate::design::fixture::{after, body, design_findings, document, id};

    const RULE: &str = "design.instance-target-state-none-reaches";

    fn state(configuration: &str, at: Position) -> StateRef {
        StateRef {
            configuration: Some(id("configuration", configuration)),
            of: Evaluated::State(at),
        }
    }

    /// The fixture document with one PMI annotation per state, each on the
    /// cube's body with no path.
    fn annotated(states: Vec<StateRef>) -> CadIr {
        let mut ir = document();
        let body = body(&ir);
        for (at, state) in states.into_iter().enumerate() {
            ir.model.pmi.push(PmiAnnotation::new(
                id("pmi", &at.to_string()),
                vec![InstanceTarget {
                    state: Some(state),
                    ..InstanceTarget::new(PmiTarget::Body { body: body.clone() })
                }],
                PmiDefinition::Dimension(PmiDimension::new(PmiDimensionKind::Size)),
            ));
        }
        ir
    }

    /// The annotations the rule names, by key.
    fn flagged(ir: &CadIr) -> Vec<String> {
        design_findings(ir)
            .into_iter()
            .filter(|finding| finding.rule.as_str() == RULE)
            .filter_map(|finding| finding.entity)
            .map(|entity| entity.rsplit('#').next().unwrap_or_default().to_owned())
            .collect()
    }

    #[test]
    fn a_state_that_state_none_reaches_is_a_second_spelling() {
        let timeline = Position::After(Member::Container(id("container", "timeline")));
        let ir = annotated(vec![
            state("base", after("fillet")),
            state("base", timeline),
            state("large", after("fillet")),
            state("base", after("extrude")),
        ]);
        assert_eq!(flagged(&ir), ["0", "1"]);
    }

    #[test]
    fn a_normalized_document_holds_the_one_spelling() {
        let mut ir = annotated(vec![
            state("base", after("fillet")),
            state("large", after("fillet")),
        ]);
        crate::design::normalize::normalize(&mut ir, &DecodePolicy::default()).unwrap();
        assert_eq!(ir.model.pmi[0].targets[0].state, None);
        assert!(flagged(&ir).is_empty());
    }
}
