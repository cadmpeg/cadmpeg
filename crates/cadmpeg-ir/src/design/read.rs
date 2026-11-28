// SPDX-License-Identifier: Apache-2.0
//! Reading CADIR design values.
//!
//! [`from_str`] refuses text that nests deeper than a CADIR document's bound
//! (§2.5) before it parses it, then parses it with no recursion limit of its
//! own, so a document at the bound reads. A chain of state bases is not
//! nested JSON; reading a document walks each chain under
//! [`ResourceLimits::max_state_chain_depth`] and refuses a longer one with
//! [`CodecError::ResourceLimit`].
//!
//! [`from_str`] states that ceiling for one read. A read outside it uses the
//! ceiling of [`ResourceLimits::default`].

use std::cell::Cell;

use cadmpeg_core::decode::{
    LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, ResourceLimits,
};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;

#[derive(Clone, Copy)]
struct Session {
    state_chain_limit: Option<u64>,
    refusal: Option<ResourceLimit>,
}

impl Session {
    const IDLE: Self = Self {
        state_chain_limit: None,
        refusal: None,
    };
}

thread_local! {
    static SESSION: Cell<Session> = const { Cell::new(Session::IDLE) };
}

/// Reads a design value from JSON text under the state-chain ceiling of
/// `limits`.
///
/// # Errors
///
/// Returns [`CodecError::ResourceLimit`] for a chain of state bases past the
/// ceiling, and [`CodecError::Malformed`] for text that is not the value's
/// wire form, including text deeper than a CADIR document's bound.
pub fn from_str<T: DeserializeOwned>(text: &str, limits: &ResourceLimits) -> Result<T, CodecError> {
    let (read, refusal) = with_limits(limits, || crate::bounds::read_json::<T>(text));
    read.map_err(|error| match refusal {
        Some(limit) => CodecError::ResourceLimit(limit),
        None => CodecError::malformed(error),
    })
}

/// Runs `read` under the state-chain ceiling of `limits` and returns its
/// result with the refusal of a chain past it.
pub(crate) fn with_limits<R>(
    limits: &ResourceLimits,
    read: impl FnOnce() -> R,
) -> (R, Option<ResourceLimit>) {
    let outer = SESSION.replace(Session {
        state_chain_limit: Some(limits.max_state_chain_depth),
        refusal: None,
    });
    let restore = Restore(outer);
    let result = read();
    let refusal = SESSION.get().refusal;
    drop(restore);
    (result, refusal)
}

struct Restore(Session);

impl Drop for Restore {
    fn drop(&mut self) {
        SESSION.set(self.0);
    }
}

/// Admits a chain of state bases `depth` states long, or refuses it past
/// [`ResourceLimits::max_state_chain_depth`].
pub(super) fn within_state_chain(depth: u64) -> Result<(), String> {
    let mut session = SESSION.get();
    let limit = session
        .state_chain_limit
        .unwrap_or(ResourceLimits::default().max_state_chain_depth);
    if depth <= limit {
        return Ok(());
    }
    let refusal = ResourceLimit {
        scope: LimitScope::Session,
        dimension: ResourceDimension::RecursionDepth,
        reason: ResourceFailure::BudgetExceeded,
        limit,
        used: limit,
        additional: depth - limit,
        operation: "CADIR state base chain depth",
    };
    session.refusal.get_or_insert(refusal);
    SESSION.set(session);
    Err(format!(
        "{} exceeds the ceiling of {limit}",
        refusal.operation
    ))
}
