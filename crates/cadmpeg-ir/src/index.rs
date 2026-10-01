// SPDX-License-Identifier: Apache-2.0
//! Borrowed identity index over a complete CAD model.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};

use crate::appearance::Appearance;
use crate::document::CadIr;
use crate::geometry::{pcurve::Pcurve, Curve, ProceduralCurve, ProceduralSurface, Surface};
use crate::schema::EntitySchema;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};

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
    fn map<K: Eq + Hash, V>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<HashMap<K, V>, Self::Error>;
    fn temporary_map<K: Eq + Hash, V>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<TemporaryIndexMap<'_, K, V>, Self::Error>;
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
}

pub(crate) struct TemporaryIndexMap<'a, K, V> {
    values: HashMap<K, V>,
    _reservation: Option<ScopedReservation<'a>>,
}

impl<K, V> std::ops::Deref for TemporaryIndexMap<'_, K, V> {
    type Target = HashMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<K, V> std::ops::DerefMut for TemporaryIndexMap<'_, K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

pub(crate) struct PublicStorage;

impl IndexStorage for PublicStorage {
    type Error = std::convert::Infallible;
    fn map<K: Eq + Hash, V>(
        &self,
        count: usize,
        _operation: &'static str,
    ) -> Result<HashMap<K, V>, Self::Error> {
        Ok(HashMap::with_capacity(count))
    }
    fn temporary_map<K: Eq + Hash, V>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<TemporaryIndexMap<'_, K, V>, Self::Error> {
        Ok(TemporaryIndexMap {
            values: public_result(self.map(count, operation)),
            _reservation: None,
        })
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
}

pub(crate) struct DecodeStorage<'ctx, 'arena>(pub(crate) &'ctx DecodeContext<'arena>);

impl IndexStorage for DecodeStorage<'_, '_> {
    type Error = ResourceLimit;
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
    fn temporary_map<K: Eq + Hash, V>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<TemporaryIndexMap<'_, K, V>, Self::Error> {
        let mut reservation = self.0.reserve_scoped_limit(0, operation)?;
        let values = reservation.with_storage_limit(|| self.map(count, operation))?;
        Ok(TemporaryIndexMap {
            values,
            _reservation: Some(reservation),
        })
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
        self.0.reserve_vec_limit(values, 1, operation)?;
        values.push(value);
        Ok(())
    }
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error> {
        self.0.charge_work_limit(u64_from_index(count), operation)
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

fn lookup_identity<'a, T: EntitySchema>(
    entities: &'a [T],
    index: &OnceLock<IdentityIndex>,
    identity: &str,
) -> Option<&'a T> {
    let entry = index
        .get_or_init(|| public_result(build_identity_index(entities, &PublicStorage)))
        .get(&identity_hash(identity))?;
    match entry {
        IdentityEntry::One(slot) => entities
            .get(*slot)
            .filter(|entity| entity.identity() == identity),
        IdentityEntry::Many(slots) => slots.iter().rev().find_map(|slot| {
            entities
                .get(*slot)
                .filter(|entity| entity.identity() == identity)
        }),
    }
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
            procedural_surface_by_surface: HashMap<&'a str, &'a ProceduralSurface>,
            procedural_curves_by_curve: HashMap<&'a str, Vec<&'a ProceduralCurve>>,
            identities: HashMap<&'a str, ()>,
            include_native: bool,
            additional_native_identities: Vec<&'a str>,
        }

        impl<'a> ModelIndex<'a> {
            /// Build all decode lookups under a live temporary reservation.
            pub fn new_for_decode<'ctx>(ir: &'a CadIr, ctx: &'ctx DecodeContext<'_>) -> Result<DecodeModelIndex<'ctx, 'a>, ResourceLimit> {
                Self::new_with_sources_for_decode(ir, true, ctx)
            }

            /// Build model-only decode lookups under a live temporary reservation.
            pub fn new_model_only_for_decode<'ctx>(ir: &'a CadIr, ctx: &'ctx DecodeContext<'_>) -> Result<DecodeModelIndex<'ctx, 'a>, ResourceLimit> {
                Self::new_with_sources_for_decode(ir, false, ctx)
            }

            fn new_with_sources_for_decode<'ctx>(ir: &'a CadIr, include_native: bool, ctx: &'ctx DecodeContext<'_>) -> Result<DecodeModelIndex<'ctx, 'a>, ResourceLimit> {
                let mut reservation = ctx.reserve_scoped_limit(0, "model lookup storage")?;
                let index = reservation.with_storage_limit(|| {
                    let storage = DecodeStorage(ctx);
                    let mut index = Self::with_identity_sources(ir, include_native, std::iter::empty(), &storage)?;
                    $(index.$lookup = OnceLock::from(build_identity_index(&ir.model.$lookup, &storage)?);)*
                    Ok::<_, ResourceLimit>(index)
                })?;
                Ok(DecodeModelIndex { index, _storage: reservation })
            }

            /// Builds lazy typed lookups and a borrowed identity universe.
            pub fn new(ir: &'a CadIr) -> Self {
                public_result(Self::with_identity_sources(ir, true, std::iter::empty(), &PublicStorage))
            }

            /// Builds typed model lookups without indexing the native namespaces.
            ///
            /// Codec decode phases use this constructor when they resolve only
            /// neutral model identities. Indexing native records in those phases
            /// adds work without changing any lookup result and is especially
            /// costly for codecs that retain a large native namespace.
            pub fn new_model_only(ir: &'a CadIr) -> Self {
                public_result(Self::with_identity_sources(ir, false, std::iter::empty(), &PublicStorage))
            }

            /// Builds the index with native identities staged outside the document.
            pub fn with_additional_native_identities(
                ir: &'a CadIr,
                additional: impl IntoIterator<Item = &'a str>,
            ) -> Self {
                public_result(Self::with_identity_sources(ir, true, additional, &PublicStorage))
            }

            fn with_identity_sources<S: IndexStorage>(
                ir: &'a CadIr,
                include_native: bool,
                additional: impl IntoIterator<Item = &'a str>,
                storage: &S,
            ) -> Result<Self, S::Error> {
                let mut procedural_surface_by_surface =
                    storage.map(ir.model.surfaces.len(), "model procedural surface carriers")?;
                let mut procedural_curves_by_curve =
                    storage.map::<&'a str, Vec<&'a ProceduralCurve>>(ir.model.curves.len(), "model procedural curve carriers")?;
                let mut procedural_surfaces_by_id =
                    storage.temporary_map(ir.model.procedural_surfaces.len(), "model procedural surface IDs")?;
                for procedural in &ir.model.procedural_surfaces {
                    storage.work(1, "model procedural surface scan")?;
                    procedural_surfaces_by_id
                        .entry(procedural.id.as_str())
                        .or_insert(procedural);
                }
                let mut procedural_curves_by_id =
                    storage.temporary_map(ir.model.procedural_curves.len(), "model procedural curve IDs")?;
                for procedural in &ir.model.procedural_curves {
                    storage.work(1, "model procedural curve scan")?;
                    procedural_curves_by_id
                        .entry(procedural.id.as_str())
                        .or_insert(procedural);
                }
                for carrier in &ir.model.surfaces {
                    storage.work(1, "model surface carrier scan")?;
                    if let Some(procedural) = carrier
                        .geometry
                        .procedural_construction()
                        .and_then(|construction| {
                            procedural_surfaces_by_id.get(construction.as_str())
                        })
                        .copied()
                    {
                        procedural_surface_by_surface
                            .insert(carrier.id.as_str(), procedural);
                    }
                }
                for carrier in &ir.model.curves {
                    storage.work(1, "model curve carrier scan")?;
                    if let Some(procedural) = carrier
                        .geometry
                        .procedural_construction()
                        .and_then(|construction| {
                            procedural_curves_by_id.get(construction.as_str())
                        })
                        .copied()
                    {
                        storage.push(procedural_curves_by_curve.entry(carrier.id.as_str()).or_default(), procedural, "model procedural curve carrier members")?;
                    }
                }
                let mut additional_native_identities = Vec::new();
                for identity in additional { storage.push(&mut additional_native_identities, identity, "model additional identities")?; }
                let mut index = Self {
                    ir,
                    $($lookup: OnceLock::new(),)*
                    procedural_surface_by_surface,
                    procedural_curves_by_curve,
                    identities: HashMap::new(),
                    include_native,
                    additional_native_identities,
                };
                index.identities = index.build_identity_set(storage)?;
                Ok(index)
            }

            fn identity_set(&self) -> &HashMap<&'a str, ()> {
                &self.identities
            }

            fn build_identity_set<S: IndexStorage>(&self, storage: &S) -> Result<HashMap<&'a str, ()>, S::Error> {
                let native_count = if self.include_native { self.ir.native.0.values().flat_map(|namespace| namespace.arenas().values().flatten()).count() } else { 0 };
                let count = self.ir.model.entity_count() + native_count + self.additional_native_identities.len();
                let mut identities = storage.map(count, "model identity universe slots")?;
                $(for entity in &self.ir.model.$field { storage.work(entity.identity().len(), "model identity universe scan")?; identities.insert(entity.identity(), ()); })*
                if self.include_native {
                    for record in self.ir.native.0.values().flat_map(|namespace| namespace.arenas().values().flatten()) {
                        storage.work(record.id().len(), "model native identity scan")?;
                        identities.insert(record.id(), ());
                    }
                    for identity in &self.additional_native_identities { storage.work(identity.len(), "model additional identity scan")?; identities.insert(*identity, ()); }
                }
                Ok(identities)
            }

            /// Returns the indexed document.
            pub fn ir(&self) -> &'a CadIr {
                self.ir
            }

            /// Returns whether any neutral or native entity owns `identity`.
            pub fn contains(&self, identity: &str) -> bool {
                self.identity_set().contains_key(identity)
            }

            /// Iterates every neutral and native identity.
            pub fn identities(&self) -> impl Iterator<Item = &'a str> + '_ {
                self.identity_set().keys().copied()
            }

            /// Looks up the procedural construction that owns a surface.
            pub fn procedural_surface_for_surface(
                &self,
                surface: &str,
            ) -> Option<&'a ProceduralSurface> {
                self.procedural_surface_by_surface.get(surface).copied()
            }

            /// Looks up procedural constructions that own a curve in arena order.
            pub fn procedural_curves_for_curve(
                &self,
                curve: &str,
            ) -> Option<&[&'a ProceduralCurve]> {
                self.procedural_curves_by_curve.get(curve).map(Vec::as_slice)
            }

            /// Looks up the unique procedural construction for a surface carrier.
            ///
            /// A procedural carrier follows its exact construction identity. A
            /// non-procedural carrier accepts a cached producer only when that
            /// producer is unique for the carrier.
            pub fn procedural_surface_for_carrier(
                &self,
                surface: &str,
            ) -> Option<&'a ProceduralSurface> {
                self.procedural_surface_by_surface.get(surface).copied()
            }

            $(
                #[doc = concat!("Looks up an entity in the `", stringify!($lookup), "` arena.")]
                pub fn $lookup(&self, identity: &str) -> Option<&'a $element> {
                    lookup_identity(&self.ir.model.$lookup, &self.$lookup, identity)
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
            let result = ModelIndex::new_model_only_for_decode(&ir, &ctx);
            assert!(matches!(result, Err(limit)
                if limit.dimension == dimension && limit.operation == operation));
        }
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let index = ModelIndex::new_model_only_for_decode(&ir, &ctx).unwrap();
        assert_eq!(
            index.points(point_id.as_str()).map(|point| &point.id),
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

        let full = ModelIndex::new(&ir);
        let model_only = ModelIndex::new_model_only(&ir);

        assert!(full.contains(native_id));
        assert!(full.contains(model_id));
        assert!(!model_only.contains(native_id));
        assert!(model_only.contains(model_id));
        assert!(!model_only
            .identities()
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

        let index = ModelIndex::new_model_only(&ir);
        let parameters = std::sync::OnceLock::new();
        assert!(parameters.get().is_none());
        assert!(index.bodies.get().is_none());
        assert_eq!(
            super::lookup_identity(&ir.model.parameters, &parameters, parameter_id.as_str())
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
            .add_procedural_surface(
                &cached_surface,
                procedural_surface! {
                    id: crate::ids::ProceduralSurfaceId::mint("test:model:procedural#cached").expect("valid identity"),
                    definition: ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
                    cache_fit_tolerance: Some(0.01),
                    record_bounds: None,
                },
            )
            .unwrap();

        let index = ModelIndex::new_model_only(&ir);
        assert_eq!(
            index
                .procedural_surface_for_carrier(exact_surface.as_str())
                .map(|surface| surface.id.as_str()),
            Some("test:model:procedural#exact")
        );
        assert_eq!(
            index
                .procedural_surface_for_carrier(cached_surface.as_str())
                .map(|surface| surface.id.as_str()),
            Some("test:model:procedural#cached")
        );

        assert!(ir
            .model
            .add_procedural_surface(
                &cached_surface,
                procedural_surface! {
                    id: crate::ids::ProceduralSurfaceId::mint("test:model:procedural#cached-duplicate").expect("valid identity"),
                    definition: ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
                    cache_fit_tolerance: Some(0.02),
                    record_bounds: None,
                },
            )
            .is_err());
    }
}
