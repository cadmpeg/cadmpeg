// SPDX-License-Identifier: Apache-2.0
//! Borrowed identity index over a complete CAD model.

pub(crate) mod identities;

use std::collections::HashMap;
use std::sync::OnceLock;

use cadmpeg_core::decode::{DecodeContext, DepthGuard, ResourceLimit};

use crate::appearance::Appearance;
use crate::document::CadIr;
use crate::geometry::{pcurve::Pcurve, Curve, ProceduralCurve, ProceduralSurface, Surface};
use crate::schema::EntitySchema;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};

/// Borrowed identity text mapped to its first inserted value.
struct BorrowedIdentityIndex<'ir, V> {
    slots: HashMap<&'ir str, V>,
}

impl<'ir, V> BorrowedIdentityIndex<'ir, V> {
    fn new() -> Self {
        Self {
            slots: HashMap::new(),
        }
    }

    fn entry(&mut self, identity: &'ir str, value: impl FnOnce() -> V) -> &mut V {
        self.slots.entry(identity).or_insert_with(value)
    }

    fn get<P: IndexQuery>(
        &self,
        identity: &str,
        query: &P,
        operation: &'static str,
    ) -> Result<Option<&V>, P::Error> {
        query.work(identity.len(), operation)?;
        query.work(1, operation)?;
        let Some(value) = self.slots.get(identity) else {
            return Ok(None);
        };
        query.work(1, operation)?;
        query.work(1, operation)?;
        query.work(identity.len(), operation)?;
        Ok(Some(value))
    }

    fn identities(&self) -> impl Iterator<Item = &'ir str> + '_ {
        self.slots.keys().copied()
    }
}

type IdentityIndex<'a> = HashMap<&'a str, usize>;

/// Nesting policy for infallible public indexes and fallible decode indexes.
pub(crate) trait IndexStorage {
    type Error;
    fn enter_nested(&self, operation: &'static str) -> Result<Option<DepthGuard<'_>>, Self::Error>;
}

pub(crate) struct PublicStorage;

impl IndexStorage for PublicStorage {
    type Error = std::convert::Infallible;
    fn enter_nested(
        &self,
        _operation: &'static str,
    ) -> Result<Option<DepthGuard<'_>>, Self::Error> {
        Ok(None)
    }
}

pub(crate) struct DecodeStorage<'ctx, 'arena>(pub(crate) &'ctx DecodeContext<'arena>);

impl IndexStorage for DecodeStorage<'_, '_> {
    type Error = ResourceLimit;
    fn enter_nested(&self, operation: &'static str) -> Result<Option<DepthGuard<'_>>, Self::Error> {
        self.0.enter_nested_limit(operation).map(Some)
    }
}

pub(crate) fn public_result<T>(value: Result<T, std::convert::Infallible>) -> T {
    match value {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

fn build_identity_index<T: EntitySchema>(entities: &[T]) -> IdentityIndex<'_> {
    let mut index = HashMap::new();
    for (slot, entity) in entities.iter().enumerate() {
        index.insert(entity.identity(), slot);
    }
    index
}

fn lookup_identity<'a, T: EntitySchema, S: IndexQuery>(
    entities: &'a [T],
    index: &OnceLock<IdentityIndex<'a>>,
    identity: &str,
    storage: &S,
) -> Result<Option<&'a T>, S::Error> {
    let index = index.get_or_init(|| build_identity_index(entities));
    storage.work(identity.len(), "model identity lookup hash")?;
    storage.work(1, "model identity lookup slot")?;
    let Some(entity) = index.get(identity).and_then(|slot| entities.get(*slot)) else {
        return Ok(None);
    };
    storage.work(1, "model identity lookup collision")?;
    storage.work(1, "model identity lookup comparison")?;
    storage.work(identity.len(), "model identity lookup comparison")?;
    Ok(Some(entity))
}

pub(crate) mod sealed {
    pub trait IndexQuery {}
    impl IndexQuery for super::StandardIndex {}
}

/// Explicit standard allocation policy for context-free model indexes.
#[derive(Debug, Clone, Copy)]
pub struct StandardIndex;

impl crate::ids::comparison::sealed::TextWork for StandardIndex {}

impl crate::ids::comparison::TextWork for StandardIndex {
    type Error = std::convert::Infallible;
    fn comparison_work(&self, _count: u64, _operation: &'static str) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Work policy for one borrowed model identity query.
pub trait IndexQuery: sealed::IndexQuery {
    /// The refusal type selected by this policy.
    type Error;
    /// An infallible value or a value carrying the resource refusal.
    type Output<T>;
    #[doc(hidden)]
    fn finish<T>(&self, result: Result<T, Self::Error>) -> Self::Output<T>;
    #[doc(hidden)]
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error>;
}

impl IndexQuery for StandardIndex {
    type Error = std::convert::Infallible;
    type Output<T> = T;
    fn finish<T>(&self, result: Result<T, Self::Error>) -> T {
        public_result(result)
    }
    fn work(&self, _count: usize, _operation: &'static str) -> Result<(), Self::Error> {
        Ok(())
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
            $($lookup: OnceLock<IdentityIndex<'a>>,)*
            procedural_surface_by_surface: BorrowedIdentityIndex<'a, &'a ProceduralSurface>,
            procedural_curves_by_curve: BorrowedIdentityIndex<'a, Vec<&'a ProceduralCurve>>,
            identities: BorrowedIdentityIndex<'a, ()>,
            include_native: bool,
            additional_native_identities: Vec<&'a str>,
            native_unknowns: Option<(&'a str, &'a [crate::unknown::UnknownRecord], &'a [usize])>,
        }

        impl<'a> ModelIndex<'a> {
            /// Build borrowed model and native lookups.
            pub fn build(ir: &'a CadIr) -> Self {
                Self::with_identity_sources(ir, true, std::iter::empty(), None)
            }

            /// Build model lookups without indexing native namespaces.
            pub fn new_model_only(ir: &'a CadIr) -> Self {
                Self::with_identity_sources(ir, false, std::iter::empty(), None)
            }

            /// Build lookups with native identities staged outside the document.
            pub fn with_additional_native_identities(
                ir: &'a CadIr,
                additional: impl IntoIterator<Item = &'a str>,
            ) -> Self {
                Self::with_identity_sources(ir, true, additional, None)
            }

            /// Build lookups with one native unknown arena replaced by borrowed source facts.
            pub(crate) fn with_native_unknowns(
                ir: &'a CadIr,
                format: &'a str,
                records: &'a [crate::unknown::UnknownRecord],
                order: &'a [usize],
            ) -> Self {
                Self::with_identity_sources(ir, true, std::iter::empty(), Some((format, records, order)))
            }

            pub(crate) fn native_view(&self) -> crate::native::view::NativeView<'a> {
                crate::native::view::NativeView::new(self.ir, self.native_unknowns)
            }

            fn with_identity_sources(
                ir: &'a CadIr,
                include_native: bool,
                additional: impl IntoIterator<Item = &'a str>,
                native_unknowns: Option<(&'a str, &'a [crate::unknown::UnknownRecord], &'a [usize])>,
            ) -> Self {
                let mut procedural_surface_by_surface = BorrowedIdentityIndex::new();
                let mut procedural_curves_by_curve = BorrowedIdentityIndex::new();
                let mut procedural_surfaces_by_id = BorrowedIdentityIndex::new();
                for procedural in &ir.model.procedural_surfaces {
                    procedural_surfaces_by_id.entry(procedural.id.as_str(), || procedural);
                }
                let mut procedural_curves_by_id = BorrowedIdentityIndex::new();
                for procedural in &ir.model.procedural_curves {
                    procedural_curves_by_id.entry(procedural.id.as_str(), || procedural);
                }
                for carrier in &ir.model.surfaces {
                    if let Some(construction) = carrier.geometry.procedural_construction() {
                        if let Some(&procedural) = procedural_surfaces_by_id.slots.get(construction.as_str()) {
                            *procedural_surface_by_surface.entry(carrier.id.as_str(), || procedural) = procedural;
                        }
                    }
                }
                for carrier in &ir.model.curves {
                    if let Some(construction) = carrier.geometry.procedural_construction() {
                        if let Some(&procedural) = procedural_curves_by_id.slots.get(construction.as_str()) {
                            let members = procedural_curves_by_curve.entry(carrier.id.as_str(), Vec::new);
                            members.push(procedural);
                        }
                    }
                }
                let additional_native_identities = additional.into_iter().collect();
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
                index.identities = index.build_identity_set();
                index
            }

            fn identity_set(&self) -> &BorrowedIdentityIndex<'a, ()> {
                &self.identities
            }

            fn build_identity_set(&self) -> BorrowedIdentityIndex<'a, ()> {
                let mut identities = BorrowedIdentityIndex::new();
                $(for entity in &self.ir.model.$field { identities.entry(entity.identity(), || ()); })*
                if self.include_native {
                    public_result(self.native_view().visit(|_, _, records| {
                        for record in records.records() {
                            identities.entry(record.id(), || ());
                        }
                        Ok(())
                    }));
                    for identity in &self.additional_native_identities { identities.entry(identity, || ()); }
                }
                identities
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

    use serde_json::Map;

    #[test]
    fn repeated_borrowed_identity_keeps_the_first_value() {
        let first = String::from("test:native:record#same");
        let repeated = first.clone();
        let calls = std::cell::Cell::new(0);
        let mut index = super::BorrowedIdentityIndex::new();
        for identity in [&first, &repeated] {
            assert_eq!(
                *index.entry(identity, || {
                    calls.set(calls.get() + 1);
                    calls.get()
                }),
                1
            );
        }
        assert_eq!(calls.get(), 1);
        assert_eq!(index.identities().count(), 1);
        assert_eq!(
            index
                .get(&repeated, &super::StandardIndex, "test identity lookup")
                .unwrap(),
            Some(&1)
        );
        assert!(index
            .get(
                "test:native:record#missing",
                &super::StandardIndex,
                "test identity lookup"
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn distinct_borrowed_identities_are_each_indexed() {
        let identities = (0..32_000)
            .map(|index| format!("test:native:record#{index}"))
            .collect::<Vec<_>>();
        let mut index = super::BorrowedIdentityIndex::new();
        for identity in &identities {
            index.entry(identity, || ());
        }
        assert_eq!(index.identities().count(), identities.len());
        for identity in [&identities[0], &identities[31_999]] {
            assert!(index
                .get(identity, &super::StandardIndex, "test identity lookup")
                .unwrap()
                .is_some());
        }
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

        let full = ModelIndex::build(&ir);
        let model_only = ModelIndex::new_model_only(&ir);

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

        let index = ModelIndex::new_model_only(&ir);
        let parameters = std::sync::OnceLock::new();
        assert!(parameters.get().is_none());
        assert!(index.bodies.get().is_none());
        assert_eq!(
            super::lookup_identity(
                &ir.model.parameters,
                &parameters,
                parameter_id.as_str(),
                &crate::index::StandardIndex
            )
            .unwrap()
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
            .add_procedural_surface(&cached_surface, procedural_surface! {
                    id: crate::ids::ProceduralSurfaceId::mint("test:model:procedural#cached").expect("valid identity"),
                    definition: ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
                    cache_fit_tolerance: Some(0.01),
                    record_bounds: None,
                }).unwrap()
            ;

        let index = ModelIndex::new_model_only(&ir);
        assert_eq!(
            index
                .procedural_surface_for_surface(exact_surface.as_str(), crate::index::StandardIndex)
                .map(|surface| surface.id.as_str()),
            Some("test:model:procedural#exact")
        );
        assert_eq!(
            index
                .procedural_surface_for_surface(
                    cached_surface.as_str(),
                    crate::index::StandardIndex
                )
                .map(|surface| surface.id.as_str()),
            Some("test:model:procedural#cached")
        );

        assert!(ir
            .model
            .add_procedural_surface(&cached_surface, procedural_surface! {
                    id: crate::ids::ProceduralSurfaceId::mint("test:model:procedural#cached-duplicate").expect("valid identity"),
                    definition: ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
                    cache_fit_tolerance: Some(0.02),
                    record_bounds: None,
                })
            .is_err());
    }

    #[test]
    fn duplicate_identity_lookup_returns_the_last_entity() {
        let mut ir = CadIr::empty();
        let id = crate::ids::PointId::mint("test:model:point#duplicate").unwrap();
        for position in [
            crate::features::FinitePoint3::ZERO,
            crate::features::FinitePoint3::new(crate::math::Point3::new(1.0, 0.0, 0.0)).unwrap(),
        ] {
            ir.model
                .points
                .push(crate::topology::Point::new(id.clone(), position, None));
        }
        let index = ModelIndex::new_model_only(&ir);
        let found = index
            .points(id.as_str(), crate::index::StandardIndex)
            .unwrap();
        assert_eq!(found.id, id);
        assert_eq!(
            found.position().get(),
            crate::math::Point3::new(1.0, 0.0, 0.0)
        );
    }
}
