// SPDX-License-Identifier: Apache-2.0
//! The depth bounds of a document (§2.5): no record, and nothing outside
//! every record, nests past one.

use super::{record_finding, CadIr, Finding};
use crate::report::check::Check;
use crate::report::Severity;

pub(super) fn check_bounds(ir: &CadIr, findings: &mut Vec<Finding>) {
    let Ok((_, measured)) = crate::hash::measured_canonical_json(ir) else {
        return;
    };
    if !measured.records.is_empty() {
        let mut sorted = ir.clone();
        sorted.finalize();
        for (place, bound) in measured.records {
            let seed = sorted.record_at(&place);
            record_finding(
                findings,
                Check::Depth,
                Severity::Error,
                seed.as_ref().map(|seed| seed.id.as_str()),
                crate::rule!(
                    "bounds.record-past-depth-bound",
                    "record of arena {} {}",
                    seed.as_ref()
                        .map_or_else(String::new, |seed| seed.arena.to_string()),
                    bound.describe()
                ),
            );
        }
    }
    if let Some(bound) = measured.outside.values().next() {
        let fields = measured
            .outside
            .keys()
            .map(|outside| outside.field.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        record_finding(
            findings,
            Check::Depth,
            Severity::Error,
            None,
            crate::rule!(
                "bounds.document-past-depth-bound",
                "the document {} outside every record ({})",
                bound.describe(),
                fields.into_iter().collect::<Vec<_>>().join(", ")
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::bounds::MAX_EXPRESSION_DEPTH;
    use crate::design::expr::{Expression, Neutral, Node, UnaryOp};
    use crate::design::parameter::ParameterDefinition;
    use crate::design::units::ValueKind;
    use crate::report::check::Check;
    use crate::report::Severity;
    use crate::validate::design::fixture;
    use crate::validate::validate_neutral;

    /// A Boolean parameter whose expression is `depth - 1` negations of
    /// `true`: a tree `depth` levels deep.
    fn negated(key: &str, depth: usize) -> crate::design::parameter::DesignParameter {
        let mut parameter = fixture::parameter(key, ValueKind::Boolean);
        parameter.definition = ParameterDefinition::Expression(Expression::Neutral(Neutral(
            (1..depth).fold(Node::Boolean(true), |operand, _| Node::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            }),
        )));
        parameter
    }

    #[test]
    fn validation_reports_a_record_past_the_expression_bound() {
        let mut ir = fixture::document();
        ir.design
            .parameters
            .push(negated("within", MAX_EXPRESSION_DEPTH));
        ir.design
            .parameters
            .push(negated("past", MAX_EXPRESSION_DEPTH + 1));
        let report = validate_neutral(&ir, Vec::new()).unwrap();
        let depth = report
            .findings
            .iter()
            .filter(|finding| finding.check == Check::Depth)
            .collect::<Vec<_>>();
        let [finding] = depth.as_slice() else {
            panic!("one finding for the one record: {depth:?}");
        };
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(
            finding.rule,
            crate::rule_name!("bounds.record-past-depth-bound")
        );
        assert_eq!(
            finding.entity.as_deref(),
            Some("test:design:parameter#past")
        );
        assert!(
            finding.message.contains("design.parameters"),
            "{}",
            finding.message
        );
    }
}
