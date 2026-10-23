// SPDX-License-Identifier: Apache-2.0
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use cadmpeg_core::decode::{DecodeContext, ResourceDimension, ResourceLimit, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;

use crate::brep::location_identity::{LocationIdentity, LocationIdentityError};
use crate::brep::{LocationRef, Tables};

const MAX_TOPOLOGY_LOCATION_WORK: usize = 16_777_216;

#[derive(Clone, Copy, Debug)]
pub(super) struct Located {
    pub(super) transform: Transform,
    pub(super) identity: usize,
}

impl Located {
    pub(super) fn identity() -> Self {
        Self {
            transform: Transform::identity(),
            identity: 0,
        }
    }
}

pub(super) struct Locations {
    identities: Vec<(Rc<LocationIdentity>, Transform)>,
    interned: HashMap<Rc<LocationIdentity>, usize>,
    source: Vec<usize>,
    products: HashMap<(usize, usize), usize>,
    work: WorkBudget,
}

impl Locations {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        tables: Tables<'_>,
    ) -> Result<Self, LocationIdentityError> {
        let allowance = tables
            .locations
            .len()
            .checked_add(tables.tshapes.len())
            .and_then(|count| count.checked_mul(1024))
            .ok_or(LocationIdentityError::Bounded)?
            .max(MAX_TOPOLOGY_LOCATION_WORK);
        let mut result = Self {
            identities: Vec::new(),
            interned: HashMap::new(),
            source: Vec::new(),
            products: HashMap::new(),
            work: WorkBudget::new(allowance),
        };
        for identity in tables.location_identities(ctx)? {
            let index = result.intern(ctx, tables, identity)?;
            result
                .source
                .try_reserve(1)
                .map_err(|_| allocation_failed(ctx))?;
            result.source.push(index);
        }
        Ok(result)
    }

    fn charge(&self, count: usize) -> Result<(), LocationIdentityError> {
        if self.work.charge_by(count) {
            Ok(())
        } else {
            Err(LocationIdentityError::Bounded)
        }
    }

    pub(super) fn visit(
        &self,
        ctx: &DecodeContext<'_>,
        visited: &mut HashSet<(usize, usize)>,
        shape: usize,
        location: Located,
    ) -> Result<bool, LocationIdentityError> {
        self.charge(1)?;
        let key = (shape, location.identity);
        if visited.contains(&key) {
            return Ok(false);
        }
        visited.try_reserve(1).map_err(|_| allocation_failed(ctx))?;
        visited.insert(key);
        Ok(true)
    }

    fn intern(
        &mut self,
        ctx: &DecodeContext<'_>,
        tables: Tables<'_>,
        identity: LocationIdentity,
    ) -> Result<usize, LocationIdentityError> {
        if let Some(index) = self.interned.get(&identity) {
            return Ok(*index);
        }
        self.charge(
            identity
                .term_count()
                .checked_add(1)
                .ok_or(LocationIdentityError::Bounded)?,
        )?;
        let transform = identity.transform(tables.locations)?;
        self.interned
            .try_reserve(1)
            .map_err(|_| allocation_failed(ctx))?;
        self.identities
            .try_reserve(1)
            .map_err(|_| allocation_failed(ctx))?;
        let index = self.identities.len();
        let identity = Rc::new(identity);
        self.interned.insert(Rc::clone(&identity), index);
        self.identities.push((identity, transform));
        Ok(index)
    }

    pub(super) fn source(
        &self,
        tables: Tables<'_>,
        location: impl Into<LocationRef>,
    ) -> Result<Located, CodecError> {
        let location = location.into();
        location.resolve(tables.locations)?;
        let identity = self.source[usize::from(location)];
        let transform = self.identities[identity].1;
        Ok(Located {
            transform,
            identity,
        })
    }

    pub(super) fn compose(&self, left: Located, right: Located) -> Result<Located, CodecError> {
        let identity = if left.identity == 0 {
            right.identity
        } else if right.identity == 0 {
            left.identity
        } else {
            *self
                .products
                .get(&(left.identity, right.identity))
                .ok_or_else(|| {
                    CodecError::malformed("topology location composition was not admitted")
                })?
        };
        Ok(Located {
            identity,
            transform: self.identities[identity].1,
        })
    }

    pub(super) fn admit(
        &mut self,
        ctx: &DecodeContext<'_>,
        tables: Tables<'_>,
        left: Located,
        right: Located,
    ) -> Result<Located, LocationIdentityError> {
        self.charge(1)?;
        let identity = if left.identity == 0 {
            right.identity
        } else if right.identity == 0 {
            left.identity
        } else if let Some(identity) = self.products.get(&(left.identity, right.identity)) {
            *identity
        } else {
            self.charge(
                self.identities[left.identity]
                    .0
                    .term_count()
                    .checked_add(self.identities[right.identity].0.term_count())
                    .ok_or(LocationIdentityError::Bounded)?,
            )?;
            let product = self.identities[left.identity]
                .0
                .compose(ctx, &self.identities[right.identity].0)?;
            let identity = self.intern(ctx, tables, product)?;
            self.charge(1)?;
            self.products
                .try_reserve(1)
                .map_err(|_| allocation_failed(ctx))?;
            self.products
                .insert((left.identity, right.identity), identity);
            identity
        };
        let transform = self.identities[identity].1;
        Ok(Located {
            transform,
            identity,
        })
    }
}

pub(super) fn allocation_failed(ctx: &DecodeContext<'_>) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit::allocation_failed(
        ResourceDimension::RetainedBytes,
        ctx.policy().limits.max_retained_bytes,
        1,
        "freecad_topology_location_cache",
    ))
}

#[cfg(test)]
mod tests;
