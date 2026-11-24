// SPDX-License-Identifier: Apache-2.0
//! The selections and constructions a design record holds.
//!
//! [`Walk`] visits a record and every value inside it that can hold a
//! selection, a construction, or an expression, and collects each selection
//! and construction it reaches. Every impl destructures its type
//! exhaustively, so a field or variant added to a design type is walked or
//! skipped by name, never silently. The family modules hold one impl per
//! design type that reaches a selection; this module holds the impls of the
//! selection, construction, and expression types themselves.

mod assembly;
mod body;
mod datum;
mod dressup;
mod root;
mod shared;
mod sheet_metal;
mod sketch;
mod surface;
mod swept;

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, ConstructionKind, InputReference,
    PostStep,
};
use crate::design::expr::{Expression, MeasureTarget, Neutral, Node};
use crate::design::selection::{
    Anchor, MemberOrder, Members, PickDomain, Picked, Picks, Propagation, RuleTerm, Selection,
    SelectionRule,
};
use crate::features::ConfigurationId;

/// A value that can hold selections or constructions.
pub(super) trait Walk {
    /// Collects every selection and construction this value holds into
    /// `into`, outermost first.
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError>;
}

/// Where an expression occurs inside a walked feature operation or sheet-metal
/// rule.
#[derive(Clone, Copy)]
pub(super) enum ExpressionPlace<'a> {
    /// A `Law::Expression`.
    Law,
    /// An `AngleRange.value`.
    BendAllowance,
    /// A pattern instance's expression for the seed parameter it names.
    Seed(&'a str),
}

/// The selections, constructions, and expressions one walk reaches.
pub(super) struct Collect<'a, 'c> {
    ctx: &'c DecodeContext<'c>,
    /// The selections, construction references included, in walk order.
    pub(super) selections: Vec<SelectionView<'a>>,
    /// The constructions, nested ones included, in walk order.
    pub(super) constructions: Vec<ConstructionView<'a>>,
    /// The expressions held inside feature operations and sheet-metal rules.
    pub(super) expressions: Vec<(ExpressionPlace<'a>, &'a Expression)>,
}

impl<'a, 'c> Collect<'a, 'c> {
    pub(super) const fn new(ctx: &'c DecodeContext<'c>) -> Self {
        Self {
            ctx,
            selections: Vec::new(),
            constructions: Vec::new(),
            expressions: Vec::new(),
        }
    }

    pub(super) fn push_expression(
        &mut self,
        place: ExpressionPlace<'a>,
        expression: &'a Expression,
    ) {
        self.expressions.push((place, expression));
    }

    /// Enters one level of a recursive design type.
    fn enter(&self, operation: &'static str) -> Result<DepthGuard<'c>, CodecError> {
        self.ctx.enter_nested(operation)
    }
}

/// One selection, with its order marker erased.
pub(super) struct SelectionView<'a> {
    /// The state the picks resolve in.
    pub(super) at: &'a Anchor,
    /// The picks, or the rule that gives them.
    pub(super) picks: PicksView<'a>,
}

/// The members of a selection, with the order marker erased.
pub(super) enum PicksView<'a> {
    /// The decoder did not read the members.
    Unread,
    /// A list of picks.
    Named(&'a [Picked]),
    /// A rule.
    Rule {
        /// The united terms.
        terms: Vec<TermView<'a>>,
        /// The removed terms.
        excluded: Vec<TermView<'a>>,
        /// The rule's cached result.
        resolved: Option<&'a [Picked]>,
    },
}

/// One rule term, with the order marker erased.
pub(super) struct TermView<'a> {
    /// The rule.
    pub(super) rule: &'a SelectionRule,
    /// The seeds.
    pub(super) seeds: &'a [Picked],
    /// The propagation.
    pub(super) propagation: Option<&'a Propagation>,
}

impl<'a> SelectionView<'a> {
    /// Every pick of the selection: listed picks, seeds, the picks a rule
    /// names, its cached result, and the limiting picks of each domain.
    pub(super) fn picks(&self) -> Vec<&'a Picked> {
        let mut stack: Vec<&'a Picked> = Vec::new();
        match &self.picks {
            PicksView::Unread => {}
            PicksView::Named(picks) => stack.extend(picks.iter()),
            PicksView::Rule {
                terms,
                excluded,
                resolved,
            } => {
                for term in terms.iter().chain(excluded) {
                    stack.extend(term.seeds);
                    stack.extend(rule_picks(term.rule));
                }
                stack.extend(resolved.iter().flat_map(|picks| picks.iter()));
            }
        }
        let mut all = Vec::with_capacity(stack.len());
        while let Some(pick) = stack.pop() {
            all.push(pick);
            if let Some(PickDomain::Limits { start, end }) = &pick.domain {
                stack.extend(start.as_deref());
                stack.extend(end.as_deref());
            }
        }
        all.reverse();
        all
    }
}

/// The picks a rule names beside its seeds.
pub(super) fn rule_picks(rule: &SelectionRule) -> impl Iterator<Item = &Picked> {
    let (one, many): (Option<&Picked>, &[Picked]) = match rule {
        SelectionRule::Chain { end } => (end.as_deref(), &[]),
        SelectionRule::Region { boundary } => (None, boundary.as_slice()),
        SelectionRule::Intersection { with } => (None, with.as_slice()),
        SelectionRule::Explicit
        | SelectionRule::All { .. }
        | SelectionRule::Of { .. }
        | SelectionRule::LoopEdges
        | SelectionRule::Boundary
        | SelectionRule::Native(_) => (None, &[]),
    };
    one.into_iter().chain(many)
}

fn term_view<O: MemberOrder>(term: &RuleTerm<O>) -> TermView<'_> {
    TermView {
        rule: &term.rule,
        seeds: term.seeds.as_slice(),
        propagation: term.propagation.as_ref(),
    }
}

/// The kind of element a construction builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Point,
    Direction,
    Axis,
    Plane,
    Frame,
    Curve,
}

impl Kind {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Direction => "direction",
            Self::Axis => "axis",
            Self::Plane => "plane",
            Self::Frame => "frame",
            Self::Curve => "curve",
        }
    }
}

/// One construction, by its kind.
#[derive(Clone, Copy)]
pub(super) enum ConstructionView<'a> {
    Point(&'a Construction<kinds::Point>),
    Direction(&'a Construction<kinds::Direction>),
    Axis(&'a Construction<kinds::Axis>),
    Plane(&'a Construction<kinds::Plane>),
    Frame(&'a Construction<kinds::Frame>),
    Curve(&'a Construction<kinds::Curve>),
}

macro_rules! each_kind {
    ($view:expr, $construction:ident => $body:expr) => {
        match $view {
            ConstructionView::Point($construction) => $body,
            ConstructionView::Direction($construction) => $body,
            ConstructionView::Axis($construction) => $body,
            ConstructionView::Plane($construction) => $body,
            ConstructionView::Frame($construction) => $body,
            ConstructionView::Curve($construction) => $body,
        }
    };
}

impl<'a> ConstructionView<'a> {
    pub(super) const fn kind(self) -> Kind {
        match self {
            Self::Point(_) => Kind::Point,
            Self::Direction(_) => Kind::Direction,
            Self::Axis(_) => Kind::Axis,
            Self::Plane(_) => Kind::Plane,
            Self::Frame(_) => Kind::Frame,
            Self::Curve(_) => Kind::Curve,
        }
    }

    pub(super) fn inputs(self) -> &'a [ConstructionInput] {
        each_kind!(self, construction => &construction.inputs)
    }

    pub(super) fn after(self) -> &'a [PostStep] {
        each_kind!(self, construction => &construction.after)
    }

    pub(super) fn solution(self) -> Option<u32> {
        each_kind!(self, construction => construction.solution)
    }

    pub(super) fn has_resolved(self) -> bool {
        each_kind!(self, construction => construction.resolved.is_some())
    }

    pub(super) fn resolved_in(self) -> Vec<&'a ConfigurationId> {
        each_kind!(self, construction => construction.resolved_in.keys().collect())
    }
}

/// A construction kind whose constructions the walk can view.
pub(super) trait Viewed: ConstructionKind + Sized {
    fn view(construction: &Construction<Self>) -> ConstructionView<'_>;
}

macro_rules! viewed {
    ($($kind:ident),*) => {$(
        impl Viewed for kinds::$kind {
            fn view(construction: &Construction<Self>) -> ConstructionView<'_> {
                ConstructionView::$kind(construction)
            }
        }
    )*};
}

viewed!(Point, Direction, Axis, Plane, Frame, Curve);

impl<K: Viewed> Walk for Construction<K> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let _depth = into.enter("construction")?;
        let Self {
            inputs,
            after: _,
            solution: _,
            mode,
            resolved: _,
            resolved_in: _,
        } = self;
        into.constructions.push(K::view(self));
        for input in inputs {
            input.walk(into)?;
        }
        mode.walk(into)
    }
}

impl Walk for ConstructionInput {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            role: _,
            reference,
            value: _,
            reversed: _,
        } = self;
        reference.walk(into)
    }
}

impl Walk for InputReference {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Picks(selection) => selection.walk(into),
            Self::Built(construction) => construction.walk(into),
        }
    }
}

impl Walk for AnyConstruction {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Point(construction) => construction.walk(into),
            Self::Direction(construction) => construction.walk(into),
            Self::Axis(construction) => construction.walk(into),
            Self::Plane(construction) => construction.walk(into),
            Self::Frame(construction) => construction.walk(into),
            Self::Curve(construction) => construction.walk(into),
        }
    }
}

impl<O: MemberOrder> Walk for Selection<O> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            at,
            picks,
            native: _,
        } = self;
        let picks = match picks {
            Picks::Unread => PicksView::Unread,
            Picks::Named(picks) => PicksView::Named(picks.as_slice()),
            Picks::Rule {
                terms,
                excluded,
                resolved,
            } => PicksView::Rule {
                terms: terms.iter().map(term_view).collect(),
                excluded: excluded.iter().map(term_view).collect(),
                resolved: resolved.as_ref().map(Members::as_slice),
            },
        };
        into.selections.push(SelectionView { at, picks });
        Ok(())
    }
}

/// A pick outside a selection is checked by the record that holds it.
impl Walk for Picked {
    fn walk<'a>(&'a self, _: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        Ok(())
    }
}

impl<R> Walk for Expression<R> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Neutral(neutral) => neutral.walk(into),
            Self::Source { .. } => Ok(()),
        }
    }
}

impl<R> Walk for Neutral<R> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let mut stack: Vec<&'a Node<R>> = vec![&self.0];
        while let Some(node) = stack.pop() {
            if let Node::Measure(measure) = node {
                if let MeasureTarget::Selection(selection) = &measure.of {
                    selection.walk(into)?;
                }
                continue;
            }
            let operands: Vec<_> = node.operands().collect();
            stack.extend(operands.into_iter().rev());
        }
        Ok(())
    }
}

impl<T: Walk> Walk for Option<T> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        self.as_ref().map_or(Ok(()), |value| value.walk(into))
    }
}

impl<T: Walk + ?Sized> Walk for Box<T> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        (**self).walk(into)
    }
}

impl<T: Walk> Walk for [T] {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        self.iter().try_for_each(|value| value.walk(into))
    }
}

impl<T: Walk> Walk for Vec<T> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        self.as_slice().walk(into)
    }
}

impl<T: Walk, const N: usize> Walk for [T; N] {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        self.as_slice().walk(into)
    }
}

impl<K, V: Walk> Walk for BTreeMap<K, V> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        self.values().try_for_each(|value| value.walk(into))
    }
}

impl<T: Walk, O> Walk for Members<T, O> {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        self.as_slice().walk(into)
    }
}
