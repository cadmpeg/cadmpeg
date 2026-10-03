// SPDX-License-Identifier: Apache-2.0
//! Borrowed identity index over a complete CAD model.

pub(crate) mod identities;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, DepthGuard, ResourceLimit, ScopedReservation};

use crate::appearance::Appearance;
use crate::document::CadIr;
use crate::geometry::{pcurve::Pcurve, Curve, ProceduralCurve, ProceduralSurface, Surface};
use crate::schema::EntitySchema;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};

/// Borrowed text keys with explicit hashing and collision comparisons.
struct BorrowedIdentityIndex<'ir, V> {
    slots: HashMap<u64, Vec<(&'ir str, V)>>,
}

impl<'ir, V> BorrowedIdentityIndex<'ir, V> {
    fn new<S: IndexStorage>(storage: &S, operation: &'static str) -> Result<Self, S::Error> {
        Ok(Self { slots: storage.map(0, operation)? })
    }

    fn entry<S: IndexStorage>(
        &mut self, identity: &'ir str, value: impl FnOnce() -> V,
        storage: &S, operation: &'static str,
    ) -> Result<&mut V, S::Error> {
        let (hash, found) = self.position(identity,
            |count| storage.work(count, operation),
            |first, second| storage.equal(first, second, operation))?;
        storage.work(1, operation)?;
        storage.entry(&mut self.slots, &hash, operation)?;
        let entries = self.slots.entry(hash).or_default();
        let position = match found {
            Some(position) => position,
            None => {
                let position = entries.len();
                storage.work(std::mem::size_of::<(&str, V)>(), operation)?;
                storage.push(entries, (identity, value()), operation)?;
                position
            }
        };
        Ok(&mut entries[position].1)
    }

    fn get<P: IndexQuery>(
        &self, identity: &str, query: &P, operation: &'static str,
    ) -> Result<Option<&V>, P::Error> {
        let (hash, found) = self.position(identity,
            |count| query.work(count, operation),
            |first, second| query.equal(first, second, operation))?;
        Ok(found.map(|position| &self.slots[&hash][position].1))
    }

    fn position<E>(
        &self, identity: &str,
        work: impl Fn(usize) -> Result<(), E>,
        equal: impl Fn(&str, &str) -> Result<bool, E>,
    ) -> Result<(u64, Option<usize>), E> {
        work(identity.len())?;
        let hash = identity_hash(identity);
        work(1)?;
        let Some(entries) = self.slots.get(&hash) else { return Ok((hash, None)); };
        for (position, (candidate, _)) in entries.iter().enumerate() {
            work(1)?;
            if equal(candidate, identity)? { return Ok((hash, Some(position))); }
        }
        Ok((hash, None))
    }

    fn identities(&self) -> impl Iterator<Item = &'ir str> + '_ {
        self.slots.values().flat_map(|entries| entries.iter().map(|(identity, _)| *identity))
    }
}

/// Collision-safe, allocation-free identity slots for one typed arena.
///
/// The index stores arena positions rather than borrowed keys. This keeps a
/// lazy index covariant over the document lifetime and avoids copying every
/// identity into each phase-local lookup map. Hash collisions and duplicate
/// identities are retained in insertion order and checked against the source
/// entity before a result is returned.
#[derive(Debug)]
enum IdentityEntry {
    One(usize),
    Many(Vec<usize>),
}

type IdentityIndex = HashMap<u64, IdentityEntry>;

/// Allocation policy for infallible public indexes and fallible decode indexes.
pub(crate) trait IndexStorage {
    type Error;
    const EAGER_LOOKUPS: bool;
    fn enter_nested(&self, operation: &'static str) -> Result<Option<DepthGuard<'_>>, Self::Error>;
    fn map<K: Eq + Hash, V>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<HashMap<K, V>, Self::Error>;
    fn temporary<T>(
        &self, operation: &'static str,
        run: impl FnOnce() -> Result<T, Self::Error>,
    ) -> Result<TemporaryIndex<'_, T>, Self::Error>;
    fn entry<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), Self::Error>;
    fn push<T>(
        &self,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), Self::Error>;
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error>;
    fn equal(&self, first: &str, second: &str, operation: &'static str) -> Result<bool, Self::Error>;
}

/// Temporary index data is dropped before its storage reservation.
pub(crate) struct TemporaryIndex<'ctx, T> {
    value: T,
    _storage: Option<ScopedReservation<'ctx>>,
}

impl<T> std::ops::Deref for TemporaryIndex<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { &self.value }
}

pub(crate) struct PublicStorage;

impl IndexStorage for PublicStorage {
    type Error = std::convert::Infallible;
    const EAGER_LOOKUPS: bool = false;
    fn enter_nested(&self, _operation: &'static str) -> Result<Option<DepthGuard<'_>>, Self::Error> { Ok(None) }
    fn map<K: Eq + Hash, V>(
        &self,
        count: usize,
        _operation: &'static str,
    ) -> Result<HashMap<K, V>, Self::Error> {
        Ok(HashMap::with_capacity(count))
    }
    fn temporary<T>(
        &self, _operation: &'static str,
        run: impl FnOnce() -> Result<T, Self::Error>,
    ) -> Result<TemporaryIndex<'_, T>, Self::Error> {
        Ok(TemporaryIndex { value: run()?, _storage: None })
    }
    fn entry<K: Eq + Hash, V>(
        &self,
        _values: &mut HashMap<K, V>,
        _key: &K,
        _operation: &'static str,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn push<T>(
        &self,
        values: &mut Vec<T>,
        value: T,
        _operation: &'static str,
    ) -> Result<(), Self::Error> {
        values.push(value);
        Ok(())
    }
    fn work(&self, _count: usize, _operation: &'static str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn equal(&self, first: &str, second: &str, _operation: &'static str) -> Result<bool, Self::Error> {
        Ok(first == second)
    }
}

pub(crate) struct DecodeStorage<'ctx, 'arena>(pub(crate) &'ctx DecodeContext<'arena>);

impl IndexStorage for DecodeStorage<'_, '_> {
    type Error = ResourceLimit;
    const EAGER_LOOKUPS: bool = true;
    fn enter_nested(&self, operation: &'static str) -> Result<Option<DepthGuard<'_>>, Self::Error> {
        self.0.enter_nested_limit(operation).map(Some)
    }
    fn map<K: Eq + Hash, V>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<HashMap<K, V>, Self::Error> {
        self.0
            .charge_collection_items_limit(u64_from_index(count), operation)?;
        let capacity = match count {
            0 => 0,
            1..=3 => 3,
            4..=7 => 7,
            count => count
                .checked_mul(8)
                .and_then(|value| value.checked_div(7))
                .and_then(usize::checked_next_power_of_two)
                .and_then(|buckets| (buckets / 8).checked_mul(7))
                .ok_or_else(|| {
                    ResourceLimit::allocation_failed(
                        cadmpeg_core::decode::ResourceDimension::Codec(operation),
                        0,
                        u64::MAX,
                        operation,
                    )
                })?,
        };
        let bytes = capacity
            .checked_mul(std::mem::size_of::<(K, V)>())
            .ok_or_else(|| {
                ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::Codec(operation),
                    0,
                    u64::MAX,
                    operation,
                )
            })?;
        self.0
            .charge_retained_limit(u64_from_index(bytes), operation)?;
        let mut values = HashMap::new();
        values.try_reserve(count).map_err(|_| {
            ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::Codec(operation),
                u64_from_index(count),
                u64_from_index(count),
                operation,
            )
        })?;
        Ok(values)
    }
    fn temporary<T>(
        &self, operation: &'static str,
        run: impl FnOnce() -> Result<T, Self::Error>,
    ) -> Result<TemporaryIndex<'_, T>, Self::Error> {
        let mut reservation = self.0.reserve_scoped_limit(0, operation)?;
        let value = reservation.with_storage_limit(run)?;
        Ok(TemporaryIndex { value, _storage: Some(reservation) })
    }
    fn entry<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), Self::Error> {
        if values.contains_key(key) {
            return Ok(());
        }
        if values.len() == values.capacity() {
            let capacity = match values.capacity() {
                0 => 3,
                3 => 7,
                capacity => capacity.checked_mul(2).ok_or_else(|| {
                    ResourceLimit::allocation_failed(
                        cadmpeg_core::decode::ResourceDimension::Codec(operation),
                        0,
                        u64::MAX,
                        operation,
                    )
                })?,
            };
            let bytes = (capacity - values.capacity())
                .checked_mul(std::mem::size_of::<(K, V)>())
                .ok_or_else(|| {
                    ResourceLimit::allocation_failed(
                        cadmpeg_core::decode::ResourceDimension::Codec(operation),
                        0,
                        u64::MAX,
                        operation,
                    )
                })?;
            self.0
                .charge_retained_limit(u64_from_index(bytes), operation)?;
        }
        self.0.charge_collection_items_limit(1, operation)?;
        values.try_reserve(1).map_err(|_| {
            ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::Codec(operation),
                1,
                1,
                operation,
            )
        })
    }
    fn push<T>(
        &self,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), Self::Error> {
        self.0.reserve_retained_vec_limit(values, 1, operation)?;
        values.push(value);
        Ok(())
    }
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error> {
        self.0.charge_work_limit(u64_from_index(count), operation)
    }
    fn equal(&self, first: &str, second: &str, operation: &'static str) -> Result<bool, Self::Error> {
        crate::ids::comparison::equal(self.0, first, second, operation)
    }
}

pub(crate) fn public_result<T>(value: Result<T, std::convert::Infallible>) -> T {
    match value {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

pub(crate) fn identity_hash(identity: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish()
}

fn build_identity_index<T: EntitySchema, S: IndexStorage>(
    entities: &[T],
    storage: &S,
) -> Result<IdentityIndex, S::Error> {
    let mut index = storage.map(entities.len(), "model identity index slots")?;
    for (slot, entity) in entities.iter().enumerate() {
        storage.work(entity.identity().len(), "model identity hash")?;
        match index.entry(identity_hash(entity.identity())) {
            Entry::Vacant(entry) => {
                entry.insert(IdentityEntry::One(slot));
            }
            Entry::Occupied(entry) => {
                let value = entry.into_mut();
                match value {
                    IdentityEntry::One(previous) => {
                        let mut slots = Vec::new();
                        storage.push(&mut slots, *previous, "model identity collision slots")?;
                        storage.push(&mut slots, slot, "model identity collision slots")?;
                        *value = IdentityEntry::Many(slots);
                    }
                    IdentityEntry::Many(slots) => {
                        storage.push(slots, slot, "model identity collision slots")?;
                    }
                }
            }
        }
    }
    Ok(index)
}

fn lookup_identity<'a, T: EntitySchema, S: IndexQuery>(
    entities: &'a [T],
    index: &OnceLock<IdentityIndex>,
    identity: &str,
    storage: &S,
) -> Result<Option<&'a T>, S::Error> {
    let index = if !storage.lazy() {
        index.get()
    } else {
        Some(index.get_or_init(|| public_result(build_identity_index(entities, &PublicStorage))))
    };
    let Some(index) = index else {
        for entity in entities.iter().rev() {
            storage.work(1, "model identity lookup scan")?;
            if storage.equal(entity.identity(), identity, "model identity lookup comparison")? {
                return Ok(Some(entity));
            }
        }
        return Ok(None);
    };
    storage.work(identity.len(), "model identity lookup hash")?;
    let hash = identity_hash(identity);
    storage.work(1, "model identity lookup slot")?;
    let Some(entry) = index.get(&hash) else { return Ok(None); };
    let slots = match entry {
        IdentityEntry::One(slot) => std::slice::from_ref(slot),
        IdentityEntry::Many(slots) => slots.as_slice(),
    };
    for slot in slots.iter().rev() {
        storage.work(1, "model identity lookup collision")?;
        if let Some(entity) = entities.get(*slot) {
            if storage.equal(entity.identity(), identity, "model identity lookup comparison")? {
                return Ok(Some(entity));
            }
        }
    }
    Ok(None)
}

/// Borrowed model lookups and the reservation for their temporary storage.
pub struct DecodeModelIndex<'ctx, 'ir> {
    index: ModelIndex<'ir>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ir> std::ops::Deref for DecodeModelIndex<'_, 'ir> {
    type Target = ModelIndex<'ir>;
    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

pub(crate) mod sealed {
    pub trait IndexQuery {}
    impl IndexQuery for super::StandardIndex {}
    impl IndexQuery for &cadmpeg_core::decode::DecodeContext<'_> {}
    pub trait IndexAdmission {}
    impl IndexAdmission for super::StandardIndex {}
    impl IndexAdmission for &cadmpeg_core::decode::DecodeContext<'_> {}
}

/// Explicit standard allocation policy for context-free model indexes.
#[derive(Debug, Clone, Copy)]
pub struct StandardIndex;

/// Work policy for one borrowed model identity query.
pub trait IndexQuery: sealed::IndexQuery {
    /// The refusal type selected by this policy.
    type Error;
    /// An infallible value or a value carrying the resource refusal.
    type Output<T>;
    #[doc(hidden)]
    fn finish<T>(&self, result: Result<T, Self::Error>) -> Self::Output<T>;
    #[doc(hidden)]
    fn lazy(&self) -> bool;
    #[doc(hidden)]
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error>;
    #[doc(hidden)]
    fn equal(&self, first: &str, second: &str, operation: &'static str) -> Result<bool, Self::Error>;
}

impl IndexQuery for StandardIndex {
    type Error = std::convert::Infallible;
    type Output<T> = T;
    fn finish<T>(&self, result: Result<T, Self::Error>) -> T { public_result(result) }
    fn lazy(&self) -> bool { true }
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error> {
        IndexStorage::work(&PublicStorage, count, operation)
    }
    fn equal(&self, first: &str, second: &str, operation: &'static str) -> Result<bool, Self::Error> {
        IndexStorage::equal(&PublicStorage, first, second, operation)
    }
}

impl IndexQuery for &DecodeContext<'_> {
    type Error = ResourceLimit;
    type Output<T> = Result<T, ResourceLimit>;
    fn finish<T>(&self, result: Result<T, ResourceLimit>) -> Self::Output<T> { result }
    fn lazy(&self) -> bool { false }
    fn work(&self, count: usize, operation: &'static str) -> Result<(), ResourceLimit> {
        IndexStorage::work(&DecodeStorage(self), count, operation)
    }
    fn equal(&self, first: &str, second: &str, operation: &'static str) -> Result<bool, ResourceLimit> {
        IndexStorage::equal(&DecodeStorage(self), first, second, operation)
    }
}

/// Select standard lazy lookups or live-session scoped decode lookups.
pub trait IndexAdmission<'ir>: sealed::IndexAdmission {
    /// The standard index or the fallible index with scoped decode storage.
    type Index;

    #[doc(hidden)]
    fn construct(
        self,
        ir: &'ir CadIr,
        include_native: bool,
        additional: impl IntoIterator<Item = &'ir str>,
        native_unknowns: Option<(&'ir str, &'ir [crate::unknown::UnknownRecord], &'ir [usize])>,
    ) -> Self::Index;
}

impl<'ir> IndexAdmission<'ir> for StandardIndex {
    type Index = ModelIndex<'ir>;

    fn construct(
        self,
        ir: &'ir CadIr,
        include_native: bool,
        additional: impl IntoIterator<Item = &'ir str>,
        native_unknowns: Option<(&'ir str, &'ir [crate::unknown::UnknownRecord], &'ir [usize])>,
    ) -> Self::Index {
        public_result(ModelIndex::with_identity_sources(ir, include_native, additional, native_unknowns, &PublicStorage))
    }
}

impl<'ctx, 'ir> IndexAdmission<'ir> for &'ctx DecodeContext<'_> {
    type Index = Result<DecodeModelIndex<'ctx, 'ir>, ResourceLimit>;

    fn construct(
        self,
        ir: &'ir CadIr,
        include_native: bool,
        additional: impl IntoIterator<Item = &'ir str>,
        native_unknowns: Option<(&'ir str, &'ir [crate::unknown::UnknownRecord], &'ir [usize])>,
    ) -> Self::Index {
        let mut reservation = self.reserve_scoped_limit(0, "model lookup storage")?;
        let index = reservation.with_storage_limit(|| {
            ModelIndex::with_identity_sources(ir, include_native, additional, native_unknowns, &DecodeStorage(self))
        })?;
        Ok(DecodeModelIndex { index, _storage: reservation })
    }
}

macro_rules! define_model_index {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        define_model_index! {
            @generate [$($field),*];
            [
                bodies: Body;
                regions: Region;
                shells: Shell;
                faces: Face;
                loops: Loop;
                coedges: Coedge;
                edges: Edge;
                vertices: Vertex;
                points: Point;
                surfaces: Surface;
                curves: Curve;
                subds: SubdSurface;
                pcurves: Pcurve;
                procedural_surfaces: ProceduralSurface;
                procedural_curves: ProceduralCurve;
                tessellations: Tessellation;
                appearances: Appearance;
            ]
        }
    };
    (@generate [$($field:ident),*]; [$($lookup:ident: $element:ty;)*]) => {
        /// One-pass borrowed lookup index for neutral and native identities.
        pub struct ModelIndex<'a> {
            ir: &'a CadIr,
            $($lookup: OnceLock<IdentityIndex>,)*
            procedural_surface_by_surface: BorrowedIdentityIndex<'a, &'a ProceduralSurface>,
            procedural_curves_by_curve: BorrowedIdentityIndex<'a, Vec<&'a ProceduralCurve>>,
            identities: BorrowedIdentityIndex<'a, ()>,
            include_native: bool,
            additional_native_identities: Vec<&'a str>,
            native_unknowns: Option<(&'a str, &'a [crate::unknown::UnknownRecord], &'a [usize])>,
        }

        impl<'a> ModelIndex<'a> {
            /// Build borrowed model and native lookups under the explicit policy.
            pub fn new<P: IndexAdmission<'a>>(ir: &'a CadIr, admission: P) -> P::Index {
                admission.construct(ir, true, std::iter::empty(), None)
            }

            /// Build model lookups without indexing native namespaces.
            pub fn new_model_only<P: IndexAdmission<'a>>(ir: &'a CadIr, admission: P) -> P::Index {
                admission.construct(ir, false, std::iter::empty(), None)
            }

            /// Build scoped decode lookups with native identities staged outside the document.
            pub fn with_additional_native_identities<'ctx>(
                ir: &'a CadIr,
                additional: impl IntoIterator<Item = &'a str>,
                ctx: &'ctx DecodeContext<'_>,
            ) -> Result<DecodeModelIndex<'ctx, 'a>, ResourceLimit> {
                ctx.construct(ir, true, additional, None)
            }

            /// Build scoped lookups with one native unknown arena replaced by borrowed source facts.
            pub(crate) fn with_native_unknowns<'ctx>(
                ir: &'a CadIr,
                format: &'a str,
                records: &'a [crate::unknown::UnknownRecord],
                order: &'a [usize],
                ctx: &'ctx DecodeContext<'_>,
            ) -> Result<DecodeModelIndex<'ctx, 'a>, ResourceLimit> {
                ctx.construct(ir, true, std::iter::empty(), Some((format, records, order)))
            }

            pub(crate) fn native_view(&self) -> crate::native::view::NativeView<'a> {
                crate::native::view::NativeView::new(self.ir, self.native_unknowns)
            }

            fn with_identity_sources<S: IndexStorage>(
                ir: &'a CadIr,
                include_native: bool,
                additional: impl IntoIterator<Item = &'a str>,
                native_unknowns: Option<(&'a str, &'a [crate::unknown::UnknownRecord], &'a [usize])>,
                storage: &S,
            ) -> Result<Self, S::Error> {
                let mut procedural_surface_by_surface = BorrowedIdentityIndex::new(storage, "model procedural surface carriers")?;
                let mut procedural_curves_by_curve = BorrowedIdentityIndex::new(storage, "model procedural curve carriers")?;
                let procedural_surfaces_by_id = storage.temporary("model procedural surface IDs", || {
                    let mut index = BorrowedIdentityIndex::new(storage, "model procedural surface IDs")?;
                    for procedural in &ir.model.procedural_surfaces {
                        storage.work(1, "model procedural surface scan")?;
                        index.entry(procedural.id.as_str(), || procedural, storage, "model procedural surface IDs")?;
                    }
                    Ok(index)
                })?;
                let procedural_curves_by_id = storage.temporary("model procedural curve IDs", || {
                    let mut index = BorrowedIdentityIndex::new(storage, "model procedural curve IDs")?;
                    for procedural in &ir.model.procedural_curves {
                        storage.work(1, "model procedural curve scan")?;
                        index.entry(procedural.id.as_str(), || procedural, storage, "model procedural curve IDs")?;
                    }
                    Ok(index)
                })?;
                for carrier in &ir.model.surfaces {
                    storage.work(1, "model surface carrier scan")?;
                    if let Some(construction) = carrier.geometry.procedural_construction() {
                        let (hash, found) = procedural_surfaces_by_id.position(construction.as_str(),
                            |count| storage.work(count, "model procedural surface ID query"),
                            |first, second| storage.equal(first, second, "model procedural surface ID query"))?;
                        if let Some(position) = found {
                            let procedural = procedural_surfaces_by_id.slots[&hash][position].1;
                            storage.work(std::mem::size_of::<&ProceduralSurface>(), "model procedural surface carrier copy")?;
                            *procedural_surface_by_surface.entry(carrier.id.as_str(), || procedural, storage, "model procedural surface carriers")? = procedural;
                        }
                    }
                }
                for carrier in &ir.model.curves {
                    storage.work(1, "model curve carrier scan")?;
                    if let Some(construction) = carrier.geometry.procedural_construction() {
                        let (hash, found) = procedural_curves_by_id.position(construction.as_str(),
                            |count| storage.work(count, "model procedural curve ID query"),
                            |first, second| storage.equal(first, second, "model procedural curve ID query"))?;
                        if let Some(position) = found {
                            let procedural = procedural_curves_by_id.slots[&hash][position].1;
                            let members = procedural_curves_by_curve.entry(carrier.id.as_str(), Vec::new, storage, "model procedural curve carriers")?;
                            storage.work(std::mem::size_of::<&ProceduralCurve>(), "model procedural curve carrier copy")?;
                            storage.push(members, procedural, "model procedural curve carrier members")?;
                        }
                    }
                }
                let mut additional_native_identities = Vec::new();
                for identity in additional { storage.push(&mut additional_native_identities, identity, "model additional identities")?; }
                let mut index = Self {
                    ir,
                    $($lookup: OnceLock::new(),)*
                    procedural_surface_by_surface,
                    procedural_curves_by_curve,
                    identities: BorrowedIdentityIndex { slots: HashMap::new() },
                    include_native,
                    additional_native_identities,
                    native_unknowns,
                };
                index.identities = index.build_identity_set(storage)?;
                if S::EAGER_LOOKUPS {
                    $(index.$lookup = OnceLock::from(build_identity_index(&ir.model.$lookup, storage)?);)*
                }
                Ok(index)
            }

            fn identity_set(&self) -> &BorrowedIdentityIndex<'a, ()> {
                &self.identities
            }

            fn build_identity_set<S: IndexStorage>(&self, storage: &S) -> Result<BorrowedIdentityIndex<'a, ()>, S::Error> {
                let mut identities = BorrowedIdentityIndex::new(storage, "model identity universe slots")?;
                $(for entity in &self.ir.model.$field { storage.work(1, "model identity universe scan")?; identities.entry(entity.identity(), || (), storage, "model identity universe slots")?; })*
                if self.include_native {
                    self.native_view().visit(|count| storage.work(count, "model native arena scan"), |_, _, records| {
                        for record in records.records() {
                            storage.work(1, "model native identity scan")?;
                            identities.entry(record.id(), || (), storage, "model identity universe slots")?;
                        }
                        Ok(())
                    })?;
                    for identity in &self.additional_native_identities { storage.work(1, "model additional identity scan")?; identities.entry(identity, || (), storage, "model identity universe slots")?; }
                }
                Ok(identities)
            }

            /// Returns the indexed document.
            pub fn ir(&self) -> &'a CadIr {
                self.ir
            }

            /// Returns whether any neutral or native entity owns `identity`.
            pub fn contains<P: IndexQuery>(&self, identity: &str, query: P) -> P::Output<bool> {
                query.finish(self.identity_set().get(identity, &query, "model identity universe query").map(|value| value.is_some()))
            }

            /// Iterates every neutral and native identity.
            pub fn identities<'index, P: IndexQuery + 'index>(
                &'index self, query: P,
            ) -> impl Iterator<Item = P::Output<&'a str>> + 'index {
                let mut identities = self.identity_set().identities();
                let mut finished = false;
                std::iter::from_fn(move || {
                    if finished { return None; }
                    if let Err(error) = query.work(1, "model identity universe iteration") {
                        finished = true;
                        return Some(query.finish(Err(error)));
                    }
                    match identities.next() {
                        Some(identity) => Some(query.finish(Ok(identity))),
                        None => { finished = true; None }
                    }
                })
            }

            /// Looks up the procedural construction that owns a surface.
            pub fn procedural_surface_for_surface<P: IndexQuery>(
                &self, surface: &str, query: P,
            ) -> P::Output<Option<&'a ProceduralSurface>> {
                query.finish(self.procedural_surface_by_surface.get(surface, &query, "model procedural surface query").map(|value| value.copied()))
            }

            /// Looks up procedural constructions that own a curve in arena order.
            pub fn procedural_curves_for_curve<P: IndexQuery>(
                &self, curve: &str, query: P,
            ) -> P::Output<Option<&[&'a ProceduralCurve]>> {
                query.finish(self.procedural_curves_by_curve.get(curve, &query, "model procedural curve query").map(|value| value.map(Vec::as_slice)))
            }

            $(
                #[doc = concat!("Looks up an entity in the `", stringify!($lookup), "` arena.")]
                pub fn $lookup<P: IndexQuery>(&self, identity: &str, admission: P) -> P::Output<Option<&'a $element>> {
                    admission.finish(lookup_identity(&self.ir.model.$lookup, &self.$lookup, identity, &admission))
                }
            )*
        }
    };
}

crate::document::arena_registry!(define_model_index);

#[cfg(test)]
mod tests {
    use super::ModelIndex;
    use crate::document::CadIr;
    use crate::geometry::{ProceduralSurface, Surface};
    use crate::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::{NativeNamespace, NativeRecord};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use serde_json::Map;

    #[test]
fn temporary_identity_storage_owns_growth_and_releases_it_after_the_value() {
    use super::IndexStorage;
    for test_hold in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let storage = super::DecodeStorage(&ctx);
        let index = storage.temporary("test temporary index", || {
            let mut values = Vec::new();
            storage.push(&mut values, 7u64, "test temporary index growth")?;
            Ok(values)
        }).unwrap();
        assert_eq!(index.as_slice(), [7]);
        if test_hold {
            let allocated = cadmpeg_core::decode::u64_from_index(index.capacity().checked_mul(std::mem::size_of::<u64>()).unwrap());
            let first = ctx.reserve_scoped_limit(64, "test held temporary index").unwrap_err();
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.limit, first.used, first.additional), (64, allocated, 64));
            drop(index);
            assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
        } else {
            drop(index);
            let released = ctx.reserve_scoped_limit(64, "test released temporary index").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn borrowed_carrier_queries_and_identity_iteration_preserve_refusals() {
    let mut ir = CadIr::empty();
    let id = crate::ids::PointId::mint("test:model:point#identity-walk").unwrap();
    ir.model.points.push(crate::topology::Point::new(id.clone(), crate::features::FinitePoint3::ZERO, None));
    let index = ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    for route in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = match route {
            0 => index.procedural_surface_for_surface(id.as_str(), &ctx).unwrap_err(),
            1 => index.procedural_curves_for_curve(id.as_str(), &ctx).unwrap_err(),
            _ => index.identities(&ctx).next().unwrap().unwrap_err(),
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!((first.limit, first.used, first.additional),
            (0, 0, if route == 2 { 1 } else { cadmpeg_core::decode::u64_from_index(id.as_str().len()) }));
        assert!(matches!(index.procedural_surface_for_surface("missing", &ctx), Err(sticky) if sticky == first));
        assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut identities = index.identities(&ctx);
    assert_eq!(identities.next(), Some(Ok(id.as_str())));
    let first = identities.next().unwrap().unwrap_err();
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert!(identities.next().is_none());
    drop(identities);
    assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn identity_universe_lookup_preserves_collision_and_resource_results() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let query = "abcd";
    let hash_work = cadmpeg_core::decode::u64_from_index(query.len());
    let index = super::BorrowedIdentityIndex {
        slots: std::collections::HashMap::from([(super::identity_hash(query), vec![("abce", ())])]),
    };
    assert!(index.get(query, &crate::index::StandardIndex, "test universe query").unwrap().is_none());
    for cap in [0, hash_work, hash_work + 1, hash_work + 2, hash_work + 3, hash_work + 5] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = index.get(query, &&ctx, "test universe query").unwrap_err();
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!((first.limit, first.used, first.additional), (cap, cap, if cap == 0 { hash_work } else { 1 }));
        assert_eq!(index.get("missing", &&ctx, "test universe query"), Err(first));
        assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
    }
    let ir = crate::examples::unit_cube().unwrap();
    let index = ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    assert!(index.contains(ir.model.points[0].id.as_str(), crate::index::StandardIndex));
    assert!(!index.contains("missing", crate::index::StandardIndex));
}

#[test]
    fn typed_model_getters_admit_queries_and_preserve_the_first_refusal() {
        let ir = CadIr::empty();
        let index = ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
        let query = "test:model:entity#missing";
        macro_rules! check {
            ($($lookup:ident),+ $(,)?) => {$({
                assert!(index.$lookup(query, crate::index::StandardIndex).is_none());
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let first = index.$lookup(query, &ctx).unwrap_err();
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, "model identity lookup hash");
                assert_eq!((first.limit, first.used, first.additional), (0, 0, cadmpeg_core::decode::u64_from_index(query.len())));
                assert_eq!(index.$lookup("test:model:entity#other", &ctx).unwrap_err(), first);
                assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
            })+};
        }
        check!(bodies, regions, shells, faces, loops, coedges, edges, vertices,
            points, surfaces, curves, subds, pcurves, procedural_surfaces,
            procedural_curves, tessellations, appearances);
    }

    #[test]
    fn explicit_index_policy_keeps_decode_storage_scoped() {
        let mut ir = CadIr::empty();
        let id = crate::ids::PointId::mint("test:model:point#policy").unwrap();
        ir.model.points.push(crate::topology::Point::new(
            id.clone(), crate::features::FinitePoint3::ZERO, None,
        ));
        let standard = ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
        assert!(standard.points.get().is_none());
        for trigger in 0..4 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 4096;
            let dimension = match trigger {
                0 => { policy.limits.max_materialized_bytes = 0; Some(ResourceDimension::MaterializedBytes) }
                1 => { policy.limits.max_collection_items = 0; Some(ResourceDimension::CollectionItems) }
                2 => { policy.limits.max_work_units = 0; Some(ResourceDimension::WorkUnits) }
                _ => { policy.limits.max_retained_bytes = 0; None }
            };
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = ModelIndex::new_model_only(&ir, &ctx);
            if let Some(dimension) = dimension {
                let first = result.err().expect("decode index must retain its resource refusal");
                assert_eq!(first.dimension, dimension);
                assert_eq!((first.limit, first.used), (0, 0));
                assert!(first.additional > 0);
                assert!(matches!(ModelIndex::new_model_only(&ir, &ctx), Err(sticky) if sticky == first));
                assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
            } else {
                let index = result.unwrap();
                assert!(index.points.get().is_some());
                assert!(index.bodies.get().is_some());
                assert_eq!(index.points(id.as_str(), &ctx).unwrap().map(|point| &point.id), Some(&id));
                drop(index);
                let storage = ctx.reserve_scoped_limit(4096, "test released model index").unwrap();
                drop(storage);
                ctx.finish_session().unwrap();
            }
        }
    }

    #[test]
    fn typed_lookup_preserves_hash_collision_and_comparison_refusals() {
        let mut ir = CadIr::empty();
        let id = crate::ids::PointId::mint("test:model:point#stored").unwrap();
        ir.model.points.push(crate::topology::Point::new(id, crate::features::FinitePoint3::ZERO, None));
        let query = "test:model:point#absent";
        let cache = std::sync::OnceLock::from(std::collections::HashMap::from([
            (super::identity_hash(query), super::IdentityEntry::Many(vec![0, 0])),
        ]));
        let hash_work = cadmpeg_core::decode::u64_from_index(query.len());
        for (cap, operation) in [
            (0, "model identity lookup hash"),
            (hash_work + 1, "model identity lookup collision"),
            (hash_work + 2, "model identity lookup comparison"),
            (hash_work + 3, "model identity lookup comparison"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let first = super::lookup_identity(&ir.model.points, &cache, query, &&ctx).unwrap_err();
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, operation);
            assert_eq!(first.limit, cap);
            assert_eq!(first.used, cap);
            assert_eq!(first.additional, if cap == 0 { hash_work } else { 1 });
            assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
        }
    }

    #[test]
    fn typed_decode_lookup_keeps_missing_cache_borrowed_and_last_duplicate() {
        let mut ir = CadIr::empty();
        let id = crate::ids::PointId::mint("test:model:point#duplicate").unwrap();
        for position in [crate::features::FinitePoint3::ZERO, crate::features::FinitePoint3::new(crate::math::Point3::new(1.0, 0.0, 0.0)).unwrap()] {
            ir.model.points.push(crate::topology::Point::new(id.clone(), position, None));
        }
        let cache = std::sync::OnceLock::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let found = super::lookup_identity(&ir.model.points, &cache, id.as_str(), &&ctx).unwrap().unwrap();
        assert_eq!(found.position().get(), crate::math::Point3::new(1.0, 0.0, 0.0));
        assert!(cache.get().is_none());
        ctx.finish_session().unwrap();

        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = super::lookup_identity(&ir.model.points, &cache, id.as_str(), &&ctx).unwrap_err();
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "model identity lookup scan");
        assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
        assert!(cache.get().is_none());
        assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
    }

    #[test]
    fn decode_index_admits_identity_slots_and_text_before_building() {
        let mut ir = CadIr::empty();
        let point_id = crate::ids::PointId::mint("test:model:point#0").expect("identity grammar");
        ir.model.points.push(crate::topology::Point::new(
            point_id.clone(),
            crate::features::FinitePoint3::ZERO,
            None,
        ));
        for (collection_cap, retained_cap, operation, dimension) in [
            (
                0,
                u64::MAX,
                "model identity universe slots",
                ResourceDimension::CollectionItems,
            ),
            (
                u64::MAX,
                0,
                "model identity universe slots",
                ResourceDimension::MaterializedBytes,
            ),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = collection_cap;
            policy.limits.max_materialized_bytes = retained_cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = ModelIndex::new_model_only(&ir, &ctx);
            assert!(matches!(result, Err(limit)
                if limit.dimension == dimension && limit.operation == operation));
        }
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let index = ModelIndex::new_model_only(&ir, &ctx).unwrap();
        assert_eq!(
            index.points(point_id.as_str(), &ctx).unwrap().map(|point| &point.id),
            Some(&point_id)
        );
    }

    macro_rules! procedural_surface {
        (
            id: $id:expr,
            definition: $definition:expr,
            cache_fit_tolerance: $cache_fit_tolerance:expr,
            record_bounds: $record_bounds:expr $(,)?
        ) => {{
            let mut definition = $definition;
            definition
                .set_legacy_cache($cache_fit_tolerance.map(|value: f64| {
                    crate::geometry::LegacyCache::try_new(value)
                        .expect("admissible fit tolerance fixture")
                }))
                .expect("valid procedural surface cache fixture");
            ProceduralSurface::new($id, definition, $record_bounds)
        }};
    }

    #[test]
    fn model_only_index_excludes_native_identity_universe() {
        let mut ir = CadIr::empty();
        let native_id = "test:fixture:native#0";
        let mut namespace = NativeNamespace::default();
        namespace.arenas_mut().insert(
            "records".into(),
            vec![NativeRecord::new(
                crate::ids::Identity::new(native_id).expect("valid identity"),
                Map::new(),
            )
            .expect("valid native identity")],
        );
        ir.native.0.insert("test".into(), namespace);
        let model_id = "test:fixture:model#0";
        ir.model.parameters.push(crate::features::DesignParameter {
            id: crate::features::ParameterId::mint(model_id).expect("identity grammar"),
            owner: None,
            ordinal: 0,
            name: "p1".into(),
            expression: "1".into(),
            display: None,
            value: None,
            dependencies: crate::features::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });

        let full = ModelIndex::new(&ir, crate::index::StandardIndex);
        let model_only = ModelIndex::new_model_only(&ir, crate::index::StandardIndex);

        assert!(full.contains(native_id, crate::index::StandardIndex));
        assert!(full.contains(model_id, crate::index::StandardIndex));
        assert!(!model_only.contains(native_id, crate::index::StandardIndex));
        assert!(model_only.contains(model_id, crate::index::StandardIndex));
        assert!(!model_only
            .identities(crate::index::StandardIndex)
            .any(|identity| identity == native_id));
    }

    #[test]
    fn typed_lookup_indexes_are_lazy_and_preserve_last_duplicate() {
        let mut ir = CadIr::empty();
        let parameter_id =
            crate::features::ParameterId::mint("test:test:parameter#0").expect("identity grammar");
        for (ordinal, expression) in [(0, "first"), (1, "last")] {
            ir.model.parameters.push(crate::features::DesignParameter {
                id: parameter_id.clone(),
                owner: None,
                ordinal,
                name: format!("p{ordinal}"),
                expression: expression.into(),
                display: None,
                value: None,
                dependencies: crate::features::DistinctMembers::default(),
                properties: std::collections::BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        }

        let index = ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
        let parameters = std::sync::OnceLock::new();
        assert!(parameters.get().is_none());
        assert!(index.bodies.get().is_none());
        assert_eq!(
            super::lookup_identity(&ir.model.parameters, &parameters, parameter_id.as_str(), &crate::index::StandardIndex).unwrap()
                .map(|parameter| parameter.expression.as_str()),
            Some("last")
        );
        assert!(parameters.get().is_some());
        assert!(index.bodies.get().is_none());
    }

    #[test]
    fn procedural_carrier_index_follows_the_owning_geometry() {
        let mut ir = CadIr::empty();
        let exact_surface = crate::ids::SurfaceId::mint("test:model:surface#exact".to_string())
            .expect("valid identity");
        let exact_construction =
            crate::ids::ProceduralSurfaceId::mint("test:model:procedural#exact")
                .expect("valid identity");
        ir.model.surfaces.push(Surface {
            id: exact_surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: exact_construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(procedural_surface! {
            id: exact_construction.clone(),
            definition: ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
            cache_fit_tolerance: None,
            record_bounds: None,
        });

        let cached_surface = crate::ids::SurfaceId::mint("test:model:surface#cached".to_string())
            .expect("valid identity");
        ir.model.surfaces.push(Surface {
            id: cached_surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    crate::math::Point3::new(0.0, 0.0, 0.0),
                    crate::math::Vector3::new(0.0, 0.0, 1.0),
                    crate::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        ir.model
            .add_procedural_surface(None, 
                &cached_surface,
                procedural_surface! {
                    id: crate::ids::ProceduralSurfaceId::mint("test:model:procedural#cached").expect("valid identity"),
                    definition: ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
                    cache_fit_tolerance: Some(0.01),
                    record_bounds: None,
                },
            ).unwrap()
            .unwrap();

        let index = ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
        assert_eq!(
            index
                .procedural_surface_for_surface(exact_surface.as_str(), crate::index::StandardIndex)
                .map(|surface| surface.id.as_str()),
            Some("test:model:procedural#exact")
        );
        assert_eq!(
            index
                .procedural_surface_for_surface(cached_surface.as_str(), crate::index::StandardIndex)
                .map(|surface| surface.id.as_str()),
            Some("test:model:procedural#cached")
        );

        assert!(ir
            .model
            .add_procedural_surface(None, 
                &cached_surface,
                procedural_surface! {
                    id: crate::ids::ProceduralSurfaceId::mint("test:model:procedural#cached-duplicate").expect("valid identity"),
                    definition: ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
                    cache_fit_tolerance: Some(0.02),
                    record_bounds: None,
                },
            ).unwrap()
            .is_err());
    }
}
