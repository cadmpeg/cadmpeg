// SPDX-License-Identifier: Apache-2.0
//! Structural signature matching with consistent type and const substitutions.
use super::key;
use rustc_middle::ty::{self, Ty, TyCtxt, TypeVisitableExt};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Pattern {
    Variable(String),
    Rigid(String, Vec<Pattern>),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Call,
    Candidate,
}

impl Pattern {
    pub(super) fn wire(&self) -> String {
        match self {
            Self::Variable(name) => format!("v{}:{name}", name.chars().count()),
            Self::Rigid(name, children) => {
                let mut output = format!("r{}:{name}{}:", name.chars().count(), children.len());
                for child in children {
                    output.push_str(&child.wire());
                }
                output
            }
        }
    }

    pub(super) fn compatible(&self, candidate: &Self) -> bool {
        let mut bindings = BTreeMap::new();
        let mut pending = vec![((Side::Call, self), (Side::Candidate, candidate))];
        while let Some((left, right)) = pending.pop() {
            let left = resolve(left, &bindings);
            let right = resolve(right, &bindings);
            match (left.1, right.1) {
                (Self::Variable(a), Self::Variable(b)) if left.0 == right.0 && a == b => (),
                (Self::Variable(name), _) => {
                    if occurs((left.0, name), right, &bindings) {
                        return false;
                    }
                    bindings.insert((left.0, name.as_str()), right);
                }
                (_, Self::Variable(name)) => {
                    if occurs((right.0, name), left, &bindings) {
                        return false;
                    }
                    bindings.insert((right.0, name.as_str()), left);
                }
                (Self::Rigid(a, x), Self::Rigid(b, y)) => {
                    if a != b || x.len() != y.len() {
                        return false;
                    }
                    pending.extend(x.iter().zip(y).map(|(x, y)| ((left.0, x), (right.0, y))));
                }
            }
        }
        true
    }
}

fn resolve<'a>(
    mut term: (Side, &'a Pattern),
    bindings: &BTreeMap<(Side, &'a str), (Side, &'a Pattern)>,
) -> (Side, &'a Pattern) {
    while let Pattern::Variable(name) = term.1 {
        let Some(target) = bindings.get(&(term.0, name.as_str())) else {
            break;
        };
        term = *target;
    }
    term
}

fn occurs<'a>(
    variable: (Side, &str),
    term: (Side, &'a Pattern),
    bindings: &BTreeMap<(Side, &'a str), (Side, &'a Pattern)>,
) -> bool {
    let term = resolve(term, bindings);
    match term.1 {
        Pattern::Variable(name) => variable.0 == term.0 && variable.1 == name,
        Pattern::Rigid(_, children) => children
            .iter()
            .any(|child| occurs(variable, (term.0, child), bindings)),
    }
}

pub(super) fn value<'tcx>(tcx: TyCtxt<'tcx>, value: Ty<'tcx>) -> Pattern {
    let rigid = |name: String, children| Pattern::Rigid(name, children);
    match value.kind() {
        ty::Param(parameter) => Pattern::Variable(format!("type:{}", parameter.index)),
        ty::Ref(_, child, mutable) => {
            rigid(format!("ref:{mutable:?}"), vec![self::value(tcx, *child)])
        }
        ty::RawPtr(child, mutable) => {
            rigid(format!("ptr:{mutable:?}"), vec![self::value(tcx, *child)])
        }
        ty::Slice(child) => rigid("slice".into(), vec![self::value(tcx, *child)]),
        ty::Array(child, length) => rigid(
            "array".into(),
            vec![self::value(tcx, *child), constant(*length)],
        ),
        ty::Tuple(children) => rigid(
            "tuple".into(),
            children
                .iter()
                .map(|child| self::value(tcx, child))
                .collect(),
        ),
        ty::Adt(definition, args) => rigid(
            format!("adt:{}", key(tcx, definition.did())),
            arguments(tcx, args),
        ),
        ty::FnPtr(..) => {
            let signature = tcx.instantiate_bound_regions_with_erased(value.fn_sig(tcx));
            function(tcx, signature, 0)
        }
        ty::Dynamic(predicates, _) => {
            let mut children = Vec::new();
            for predicate in predicates.iter() {
                match tcx.instantiate_bound_regions_with_erased(predicate) {
                    ty::ExistentialPredicate::Trait(trait_ref) => children.push(rigid(
                        format!("trait:{}", key(tcx, trait_ref.def_id)),
                        arguments(tcx, trait_ref.args),
                    )),
                    ty::ExistentialPredicate::Projection(projection) => {
                        let mut args = arguments(tcx, projection.args);
                        args.push(match projection.term.kind() {
                            ty::TermKind::Ty(value) => self::value(tcx, value),
                            ty::TermKind::Const(value) => constant(value),
                        });
                        children.push(rigid(
                            format!("projection:{}", key(tcx, projection.def_id)),
                            args,
                        ));
                    }
                    ty::ExistentialPredicate::AutoTrait(id) => {
                        children.push(rigid(format!("auto:{}", key(tcx, id)), Vec::new()))
                    }
                }
            }
            rigid("dyn".into(), children)
        }
        ty::Alias(..) if value.has_non_region_param() => {
            Pattern::Variable(format!("projection:{value:?}"))
        }
        _ => rigid(
            ty::print::with_crate_prefix!(ty::print::with_no_trimmed_paths!(format!(
                "type:{value:?}"
            ))),
            Vec::new(),
        ),
    }
}

fn constant(value: ty::Const<'_>) -> Pattern {
    match value.kind() {
        ty::ConstKind::Param(parameter) => Pattern::Variable(format!("const:{}", parameter.index)),
        _ if value.has_non_region_param() => Pattern::Variable(format!("const:{value:?}")),
        _ => Pattern::Rigid(format!("const:{value:?}"), Vec::new()),
    }
}

pub(super) fn arguments<'tcx>(tcx: TyCtxt<'tcx>, args: ty::GenericArgsRef<'tcx>) -> Vec<Pattern> {
    args.iter()
        .filter_map(|argument| match argument.kind() {
            ty::GenericArgKind::Type(value) => Some(self::value(tcx, value)),
            ty::GenericArgKind::Const(value) => Some(constant(value)),
            ty::GenericArgKind::Lifetime(_) => None,
        })
        .collect()
}

pub(super) fn function<'tcx>(
    tcx: TyCtxt<'tcx>,
    signature: ty::FnSig<'tcx>,
    skip: usize,
) -> Pattern {
    let children = signature
        .inputs()
        .iter()
        .skip(skip)
        .copied()
        .chain([signature.output()])
        .map(|input| value(tcx, input))
        .collect();
    Pattern::Rigid(format!("fn:{:?}", signature.fn_sig_kind), children)
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    fn variable(name: &str) -> Pattern {
        Pattern::Variable(name.into())
    }
    fn rigid(name: &str, children: Vec<Pattern>) -> Pattern {
        Pattern::Rigid(name.into(), children)
    }

    #[test]
    fn consistent_signature_substitutions() {
        let repeated = rigid("fn", vec![variable("T"), variable("T")]);
        assert!(repeated.compatible(&rigid("fn", vec![rigid("u8", vec![]), rigid("u8", vec![])])));
        assert!(!repeated.compatible(&rigid(
            "fn",
            vec![rigid("u8", vec![]), rigid("u16", vec![])]
        )));
        assert!(repeated.compatible(&rigid("fn", vec![variable("T"), variable("U")])));
        assert!(!repeated.compatible(&rigid(
            "fn",
            vec![variable("U"), rigid("vec", vec![variable("U")])]
        )));
        assert!(!repeated.compatible(&rigid("fn", vec![rigid("u8", vec![])])));
    }

    #[test]
    fn signature_wire_lengths() {
        assert_eq!(
            rigid("λ", vec![variable("T:0"), rigid("u8", vec![])]).wire(),
            "r1:λ2:v3:T:0r2:u80:"
        );
    }
}
