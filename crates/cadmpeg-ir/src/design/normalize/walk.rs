// SPDX-License-Identifier: Apache-2.0
//! The selections and constructions a design record holds, to normalize in
//! place.
//!
//! [`Walk`] visits a record and every value inside it that can hold a
//! selection, a construction, or an expression, and puts each selection and
//! construction it reaches into its one spelling, innermost first. Every impl
//! destructures its type exhaustively, so a field or variant added to a design
//! type is walked or skipped by name, never silently. The family modules hold
//! one impl per design or process type that reaches a selection; this module
//! holds the impls of the selection, construction, and expression types
//! themselves.

mod assembly;
mod body;
mod datum;
mod dressup;
mod process;
mod root;
mod shared;
mod sheet_metal;
mod sketch;
mod surface;
mod swept;

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use super::constructions::{self, ConstructionFacts, Kind};
use super::selections;
use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, ConstructionKind, InputReference,
};
use crate::design::expr::{Expression, MeasureTarget, Neutral, Node};
use crate::design::selection::{MemberOrder, Members, Picked, Selection};

/// A value that can hold selections or constructions.
pub(super) trait Walk {
    /// Puts every selection and construction this value holds into its one
    /// spelling.
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError>;
}

/// The state of one normalizing walk.
pub(super) struct Visit<'f, 'c> {
    ctx: &'c DecodeContext<'c>,
    facts: &'f dyn ConstructionFacts,
}

impl<'f, 'c> Visit<'f, 'c> {
    pub(super) fn new(ctx: &'c DecodeContext<'c>, facts: &'f dyn ConstructionFacts) -> Self {
        Self { ctx, facts }
    }

    /// Enters one level of a recursive design type.
    fn enter(&self, operation: &'static str) -> Result<DepthGuard<'c>, CodecError> {
        self.ctx.enter_nested(operation)
    }
}

/// A construction kind whose constructions the walk normalizes.
pub(super) trait Kinded: ConstructionKind {
    const KIND: Kind;
}

impl Kinded for kinds::Point {
    const KIND: Kind = Kind::Point;
}

impl Kinded for kinds::Direction {
    const KIND: Kind = Kind::Direction;
}

impl Kinded for kinds::Axis {
    const KIND: Kind = Kind::Axis;
}

impl Kinded for kinds::Plane {
    const KIND: Kind = Kind::Plane;
}

impl Kinded for kinds::Frame {
    const KIND: Kind = Kind::Frame;
}

impl Kinded for kinds::Curve {
    const KIND: Kind = Kind::Curve;
}

impl<K: Kinded> Walk for Construction<K> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let _depth = into.enter("construction")?;
        let Self {
            inputs,
            after: _,
            solution: _,
            mode,
            resolved: _,
            resolved_in: _,
        } = self;
        for input in inputs.iter_mut() {
            input.walk(into)?;
        }
        mode.walk(into)?;
        constructions::normalize(self, K::KIND, into.facts);
        Ok(())
    }
}

impl Walk for ConstructionInput {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
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
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Picks(selection) => selection.walk(into),
            Self::Built(construction) => construction.walk(into),
        }
    }
}

impl Walk for AnyConstruction {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
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
    fn walk(&mut self, _: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        selections::normalize(&mut self.picks);
        Ok(())
    }
}

/// A pick holds no selection and no construction.
impl Walk for Picked {
    fn walk(&mut self, _: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        Ok(())
    }
}

impl<R> Walk for Expression<R> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        match self {
            Self::Neutral(neutral) => neutral.walk(into),
            Self::Source { .. } => Ok(()),
        }
    }
}

impl<R> Walk for Neutral<R> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        let mut stack: Vec<&mut Node<R>> = vec![&mut self.0];
        while let Some(node) = stack.pop() {
            if let Node::Measure(measure) = &mut *node {
                if let MeasureTarget::Selection(selection) = &mut measure.of {
                    selection.walk(into)?;
                }
                continue;
            }
            let operands: Vec<_> = node.operands_mut().collect();
            stack.extend(operands.into_iter().rev());
        }
        Ok(())
    }
}

impl<T: Walk> Walk for Option<T> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        self.as_mut().map_or(Ok(()), |value| value.walk(into))
    }
}

impl<T: Walk + ?Sized> Walk for Box<T> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        (**self).walk(into)
    }
}

impl<T: Walk> Walk for [T] {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        self.iter_mut().try_for_each(|value| value.walk(into))
    }
}

impl<T: Walk> Walk for Vec<T> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        self.as_mut_slice().walk(into)
    }
}

impl<T: Walk, const N: usize> Walk for [T; N] {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        self.as_mut_slice().walk(into)
    }
}

impl<K, V: Walk> Walk for BTreeMap<K, V> {
    fn walk(&mut self, into: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        self.values_mut().try_for_each(|value| value.walk(into))
    }
}

/// A member list holds picks only, and a pick holds no selection.
impl<O> Walk for Members<Picked, O> {
    fn walk(&mut self, _: &mut Visit<'_, '_>) -> Result<(), CodecError> {
        Ok(())
    }
}
