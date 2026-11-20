// SPDX-License-Identifier: Apache-2.0
//! Reading CADIR design values under the nesting ceilings of
//! [`ResourceLimits`].
//!
//! Canonical expression text, a `TopoName`, a `Picked`, and a construction's
//! nested constructions nest below the JSON depth limit. Reading CADIR
//! enforces a ceiling of `ResourceLimits` for each of them and refuses a
//! document past one with [`CodecError::ResourceLimit`]. A chain of state
//! bases is not nested JSON; reading a document walks each chain under its
//! own ceiling and refuses it the same way.
//!
//! [`from_str`] states the ceilings for one read. A read outside it uses the
//! ceilings of [`ResourceLimits::default`].

use std::cell::Cell;

use cadmpeg_core::decode::{
    LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, ResourceLimits,
};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;

/// A nested structure whose depth a ceiling of [`ResourceLimits`] bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ceiling {
    /// The nesting of canonical expression text,
    /// [`ResourceLimits::max_expression_depth`].
    Expression,
    /// The nesting of a `TopoName`, [`ResourceLimits::max_name_depth`].
    Name,
    /// The nesting of a `Picked` inside a pick's domain or rule,
    /// [`ResourceLimits::max_pick_depth`].
    Pick,
    /// The nesting of a construction's nested constructions,
    /// [`ResourceLimits::max_construction_depth`].
    Construction,
    /// The depth of a chain of state bases,
    /// [`ResourceLimits::max_state_chain_depth`].
    StateChain,
}

impl Ceiling {
    const ALL: [Self; 5] = [
        Self::Expression,
        Self::Name,
        Self::Pick,
        Self::Construction,
        Self::StateChain,
    ];

    /// The ceiling's value in `limits`.
    #[must_use]
    pub const fn of(self, limits: &ResourceLimits) -> u64 {
        match self {
            Self::Expression => limits.max_expression_depth,
            Self::Name => limits.max_name_depth,
            Self::Pick => limits.max_pick_depth,
            Self::Construction => limits.max_construction_depth,
            Self::StateChain => limits.max_state_chain_depth,
        }
    }

    const fn operation(self) -> &'static str {
        match self {
            Self::Expression => "CADIR expression text nesting",
            Self::Name => "CADIR TopoName nesting",
            Self::Pick => "CADIR Picked nesting",
            Self::Construction => "CADIR construction nesting",
            Self::StateChain => "CADIR state base chain depth",
        }
    }

    const fn slot(self) -> usize {
        match self {
            Self::Expression => 0,
            Self::Name => 1,
            Self::Pick => 2,
            Self::Construction => 3,
            Self::StateChain => 4,
        }
    }
}

#[derive(Clone, Copy)]
struct Session {
    limits: Option<ResourceLimits>,
    depths: [u64; Ceiling::ALL.len()],
    refusal: Option<ResourceLimit>,
}

impl Session {
    const IDLE: Self = Self {
        limits: None,
        depths: [0; Ceiling::ALL.len()],
        refusal: None,
    };
}

thread_local! {
    static SESSION: Cell<Session> = const { Cell::new(Session::IDLE) };
}

/// Reads a design value from JSON text under the nesting ceilings of
/// `limits`.
///
/// # Errors
///
/// Returns [`CodecError::ResourceLimit`] for a value nested past a ceiling,
/// and [`CodecError::Malformed`] for text that is not the value's wire form.
pub fn from_str<T: DeserializeOwned>(text: &str, limits: &ResourceLimits) -> Result<T, CodecError> {
    let outer = SESSION.replace(Session {
        limits: Some(*limits),
        ..Session::IDLE
    });
    let restore = Restore(outer);
    let read = serde_json::from_str::<T>(text);
    let refusal = SESSION.get().refusal;
    drop(restore);
    read.map_err(|error| match refusal {
        Some(limit) => CodecError::ResourceLimit(limit),
        None => CodecError::malformed(error),
    })
}

struct Restore(Session);

impl Drop for Restore {
    fn drop(&mut self) {
        SESSION.set(self.0);
    }
}

/// One level of a nested structure, held until the guard is dropped.
pub(super) struct Depth(Ceiling);

impl Drop for Depth {
    fn drop(&mut self) {
        let mut session = SESSION.get();
        let slot = &mut session.depths[self.0.slot()];
        *slot = slot.checked_sub(1).unwrap_or_default();
        SESSION.set(session);
    }
}

/// Enters one level of `ceiling`, or refuses the level past its value.
pub(super) fn enter(ceiling: Ceiling) -> Result<Depth, ResourceLimit> {
    let mut session = SESSION.get();
    let limit = ceiling.of(&session.limits.unwrap_or_default());
    let used = session.depths[ceiling.slot()];
    let Some(next) = used.checked_add(1).filter(|next| *next <= limit) else {
        let refusal = ResourceLimit {
            scope: LimitScope::for_dimension(ResourceDimension::RecursionDepth),
            dimension: ResourceDimension::RecursionDepth,
            reason: ResourceFailure::BudgetExceeded,
            limit,
            used,
            additional: 1,
            operation: ceiling.operation(),
        };
        session.refusal.get_or_insert(refusal);
        SESSION.set(session);
        return Err(refusal);
    };
    session.depths[ceiling.slot()] = next;
    SESSION.set(session);
    Ok(Depth(ceiling))
}

/// Admits a value whose nesting under `ceiling` is `depth` levels, counting
/// the value itself, or refuses it past the ceiling's value.
pub(super) fn within(ceiling: Ceiling, depth: u64) -> Result<(), String> {
    let mut session = SESSION.get();
    let limit = ceiling.of(&session.limits.unwrap_or_default());
    if depth <= limit {
        return Ok(());
    }
    let refusal = ResourceLimit {
        scope: LimitScope::for_dimension(ResourceDimension::RecursionDepth),
        dimension: ResourceDimension::RecursionDepth,
        reason: ResourceFailure::BudgetExceeded,
        limit,
        used: limit,
        additional: depth - limit,
        operation: ceiling.operation(),
    };
    session.refusal.get_or_insert(refusal);
    SESSION.set(session);
    Err(format!(
        "{} exceeds the ceiling of {limit}",
        refusal.operation
    ))
}
