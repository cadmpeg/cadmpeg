// SPDX-License-Identifier: Apache-2.0
//! Temporary evaluator storage governed by its work slice's decode session.

use super::budget::{DecodeBudget, ScopedReservation};
use super::error::ResourceLimit;

/// A temporary reservation whose lifetime covers evaluator-owned scratch.
#[derive(Debug)]
pub struct WorkScratch<'a> {
    reservation: Option<ScopedReservation<'a>>,
}

impl<'a> WorkScratch<'a> {
    pub(super) fn new(
        session: Option<&'a DecodeBudget>,
        bytes: u64,
        operation: &'static str,
    ) -> Result<Self, ResourceLimit> {
        let reservation = session
            .map(|session| session.reserve_scoped_limit(bytes, operation))
            .transpose()?;
        Ok(Self { reservation })
    }

    /// Admit additional scratch bytes before growing evaluator storage.
    ///
    /// # Errors
    /// Returns the original resource refusal from an attached decode session.
    pub fn grow(&mut self, bytes: u64) -> Result<(), ResourceLimit> {
        if let Some(reservation) = &mut self.reservation {
            reservation.grow_limit(bytes)?;
        }
        Ok(())
    }
}
