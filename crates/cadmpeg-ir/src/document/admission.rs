// SPDX-License-Identifier: Apache-2.0
//! Typed storage and work admission for model construction.

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};

/// Admission selected by model construction or reconstruction.
pub trait ModelAdmission {
    /// Refusal produced by the selected storage policy.
    type Error;

    /// Admit traversal work.
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error>;

    /// Compare identity text after admitting the comparison.
    fn equal(&self, left: &str, right: &str, operation: &'static str) -> Result<bool, Self::Error> {
        self.work(1, operation)?;
        self.work(left.len().min(right.len()), operation)?;
        Ok(left == right)
    }

    /// Format a retained rejection message.
    fn text(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, Self::Error>;

    /// Reserve construction slots.
    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), Self::Error>;

    /// Copy the identity stored by a surface carrier.
    fn surface_id(
        &self,
        id: &ProceduralSurfaceId,
        operation: &'static str,
    ) -> Result<ProceduralSurfaceId, Self::Error>;

    /// Copy the identity stored by a curve carrier.
    fn curve_id(
        &self,
        id: &ProceduralCurveId,
        operation: &'static str,
    ) -> Result<ProceduralCurveId, Self::Error>;

    /// An empty temporary index of arena positions by identity.
    fn positions(&self, operation: &'static str) -> Result<IdentityPositions<'_>, Self::Error>;

    /// Hash identity text for a positions index.
    fn hash_identity(&self, identity: &str, operation: &'static str) -> Result<u64, Self::Error>;

    /// Record an arena position under an identity hash.
    fn push_position(
        &self,
        positions: &mut IdentityPositions<'_>,
        hash: u64,
        position: usize,
        operation: &'static str,
    ) -> Result<(), Self::Error>;

    /// The positions recorded under an identity hash, in recording order.
    /// Distinct identities can share a hash, so callers compare each
    /// candidate's identity.
    fn positions_of<'p>(
        &self,
        positions: &'p IdentityPositions<'_>,
        hash: u64,
        operation: &'static str,
    ) -> Result<&'p [usize], Self::Error>;
}

/// Arena positions grouped by identity hash, held in temporary storage.
pub struct IdentityPositions<'s> {
    slots: HashMap<u64, Vec<usize>>,
    storage: Option<ScopedReservation<'s>>,
}

/// Standard allocation for reconstruction without decode admission.
pub struct StandardAdmission;

impl ModelAdmission for StandardAdmission {
    type Error = Infallible;

    fn work(&self, _count: usize, _operation: &'static str) -> Result<(), Infallible> {
        Ok(())
    }
    fn text(
        &self,
        args: fmt::Arguments<'_>,
        _operation: &'static str,
    ) -> Result<String, Infallible> {
        Ok(args.to_string())
    }
    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        _operation: &'static str,
    ) -> Result<(), Infallible> {
        values.reserve(count);
        Ok(())
    }
    fn surface_id(
        &self,
        id: &ProceduralSurfaceId,
        _operation: &'static str,
    ) -> Result<ProceduralSurfaceId, Infallible> {
        Ok(id.clone())
    }
    fn curve_id(
        &self,
        id: &ProceduralCurveId,
        _operation: &'static str,
    ) -> Result<ProceduralCurveId, Infallible> {
        Ok(id.clone())
    }
    fn positions(&self, _operation: &'static str) -> Result<IdentityPositions<'_>, Infallible> {
        Ok(IdentityPositions {
            slots: HashMap::new(),
            storage: None,
        })
    }
    fn hash_identity(&self, identity: &str, _operation: &'static str) -> Result<u64, Infallible> {
        Ok(crate::index::identity_hash(identity))
    }
    fn push_position(
        &self,
        positions: &mut IdentityPositions<'_>,
        hash: u64,
        position: usize,
        _operation: &'static str,
    ) -> Result<(), Infallible> {
        positions.slots.entry(hash).or_default().push(position);
        Ok(())
    }
    fn positions_of<'p>(
        &self,
        positions: &'p IdentityPositions<'_>,
        hash: u64,
        _operation: &'static str,
    ) -> Result<&'p [usize], Infallible> {
        Ok(positions.slots.get(&hash).map_or(&[][..], Vec::as_slice))
    }
}

impl ModelAdmission for DecodeContext<'_> {
    type Error = CodecError;

    fn work(&self, count: usize, operation: &'static str) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(count), operation)
    }
    fn text(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.format_retained(args, operation)
    }
    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(values, count, operation)
    }
    fn surface_id(
        &self,
        id: &ProceduralSurfaceId,
        operation: &'static str,
    ) -> Result<ProceduralSurfaceId, CodecError> {
        id.try_clone_for_decode(self, operation)
    }
    fn curve_id(
        &self,
        id: &ProceduralCurveId,
        operation: &'static str,
    ) -> Result<ProceduralCurveId, CodecError> {
        id.try_clone_for_decode(self, operation)
    }
    fn positions(&self, operation: &'static str) -> Result<IdentityPositions<'_>, CodecError> {
        Ok(IdentityPositions {
            slots: HashMap::new(),
            storage: Some(self.reserve_scoped(0, operation)?),
        })
    }
    fn hash_identity(&self, identity: &str, operation: &'static str) -> Result<u64, CodecError> {
        self.charge_work(u64_from_index(identity.len()), operation)?;
        Ok(crate::index::identity_hash(identity))
    }
    fn push_position(
        &self,
        positions: &mut IdentityPositions<'_>,
        hash: u64,
        position: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let IdentityPositions { slots, storage } = positions;
        let mut push = || match self.entry_hash_map(slots, hash, operation)? {
            std::collections::hash_map::Entry::Occupied(mut slot) => {
                self.push_vec(slot.get_mut(), position, operation)
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                let mut list = Vec::new();
                self.push_vec(&mut list, position, operation)?;
                slot.insert(list);
                Ok(())
            }
        };
        match storage {
            Some(storage) => storage.with_storage(push),
            None => push(),
        }
    }
    fn positions_of<'p>(
        &self,
        positions: &'p IdentityPositions<'_>,
        hash: u64,
        operation: &'static str,
    ) -> Result<&'p [usize], CodecError> {
        Ok(self
            .get_hash_map(&positions.slots, &hash, operation)?
            .map_or(&[][..], Vec::as_slice))
    }
}
