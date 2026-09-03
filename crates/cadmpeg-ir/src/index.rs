// SPDX-License-Identifier: Apache-2.0
//! Borrowed identity index over a complete CAD model.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::document::CadIr;
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{
    Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface, SurfaceGeometry,
};
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Occurrence, ProductDefinition};
use crate::schema::EntitySchema;
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{
    Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use crate::spreadsheets::Spreadsheet;
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

fn identity_hash(identity: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish()
}

fn build_identity_index<T: EntitySchema>(entities: &[T]) -> IdentityIndex {
    let mut index = HashMap::with_capacity(entities.len());
    for (slot, entity) in entities.iter().enumerate() {
        match index.entry(identity_hash(entity.identity())) {
            Entry::Vacant(entry) => {
                entry.insert(IdentityEntry::One(slot));
            }
            Entry::Occupied(entry) => {
                let value = entry.into_mut();
                match value {
                    IdentityEntry::One(previous) => {
                        let previous = *previous;
                        *value = IdentityEntry::Many(vec![previous, slot]);
                    }
                    IdentityEntry::Many(slots) => slots.push(slot),
                }
            }
        }
    }
    index
}

fn lookup_identity<'a, T: EntitySchema>(
    entities: &'a [T],
    index: &OnceLock<IdentityIndex>,
    identity: &str,
) -> Option<&'a T> {
    let entry = index
        .get_or_init(|| build_identity_index(entities))
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

macro_rules! define_model_index {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*]; )*) => {
        /// One-pass borrowed lookup index for neutral and native identities.
        pub struct ModelIndex<'a> {
            ir: &'a CadIr,
            $($field: OnceLock<IdentityIndex>,)*
            procedural_surface_by_surface: HashMap<&'a str, &'a ProceduralSurface>,
            procedural_surface_for_carrier: HashMap<&'a str, &'a ProceduralSurface>,
            procedural_curves_by_curve: HashMap<&'a str, Vec<&'a ProceduralCurve>>,
            identities: OnceLock<HashSet<String>>,
            native_identities: OnceLock<HashSet<String>>,
            include_native: bool,
            additional_native_identities: Vec<&'a str>,
        }

        impl<'a> ModelIndex<'a> {
            /// Builds lazy typed lookups and lazily materializes the identity universe.
            pub fn new(ir: &'a CadIr) -> Self {
                Self::with_identity_sources(ir, true, std::iter::empty())
            }

            /// Builds typed model lookups without indexing the native namespaces.
            ///
            /// Codec decode phases use this constructor when they resolve only
            /// neutral model identities. Indexing native records in those phases
            /// adds work without changing any lookup result and is especially
            /// costly for codecs that retain a large native namespace.
            pub fn new_model_only(ir: &'a CadIr) -> Self {
                Self::with_identity_sources(ir, false, std::iter::empty())
            }

            /// Builds the index with native identities staged outside the document.
            pub fn with_additional_native_identities(
                ir: &'a CadIr,
                additional: impl IntoIterator<Item = &'a str>,
            ) -> Self {
                Self::with_identity_sources(ir, true, additional)
            }

            fn with_identity_sources(
                ir: &'a CadIr,
                include_native: bool,
                additional: impl IntoIterator<Item = &'a str>,
            ) -> Self {
                let mut procedural_surface_by_surface =
                    HashMap::with_capacity(ir.model.procedural_surfaces.len());
                for procedural in &ir.model.procedural_surfaces {
                    procedural_surface_by_surface
                        .entry(procedural.surface.0.as_str())
                        .or_insert(procedural);
                }
                let mut procedural_curves_by_curve =
                    HashMap::<&'a str, Vec<&'a ProceduralCurve>>::with_capacity(
                        ir.model.procedural_curves.len(),
                    );
                for procedural in &ir.model.procedural_curves {
                    procedural_curves_by_curve
                        .entry(procedural.curve.0.as_str())
                        .or_default()
                        .push(procedural);
                }
                let mut unique_cached_producers = HashMap::<
                    &'a str,
                    Option<&'a ProceduralSurface>,
                >::new();
                let mut procedural_surfaces_by_id =
                    HashMap::with_capacity(ir.model.procedural_surfaces.len());
                for procedural in &ir.model.procedural_surfaces {
                    procedural_surfaces_by_id
                        .entry(procedural.id.0.as_str())
                        .or_insert(procedural);
                }
                for procedural in &ir.model.procedural_surfaces {
            if procedural.cache_fit_tolerance().is_some() {
                        if let Some(producer) =
                            unique_cached_producers.get_mut(procedural.surface.0.as_str())
                        {
                            *producer = None;
                        } else {
                            unique_cached_producers
                                .insert(procedural.surface.0.as_str(), Some(procedural));
                        }
                    }
                }
                let mut procedural_surface_for_carrier =
                    HashMap::with_capacity(ir.model.surfaces.len());
                for carrier in &ir.model.surfaces {
                    let procedural = match &carrier.geometry {
                        SurfaceGeometry::Procedural { construction } => procedural_surfaces_by_id
                            .get(construction.0.as_str())
                            .copied()
                            .filter(|procedural| procedural.surface == carrier.id),
                        _ => unique_cached_producers
                            .get(carrier.id.0.as_str())
                            .copied()
                            .flatten(),
                    };
                    if let Some(procedural) = procedural {
                        procedural_surface_for_carrier.insert(carrier.id.0.as_str(), procedural);
                    }
                }
                Self {
                    ir,
                    $($field: OnceLock::new(),)*
                    procedural_surface_by_surface,
                    procedural_surface_for_carrier,
                    procedural_curves_by_curve,
                    identities: OnceLock::new(),
                    native_identities: OnceLock::new(),
                    include_native,
                    additional_native_identities: additional.into_iter().collect(),
                }
            }

            fn model_identity_set(&self) -> HashSet<String> {
                let mut identities = HashSet::with_capacity(self.ir.model.entity_count());
                $(identities.extend(self.ir.model.$field.iter().map(|entity| entity.identity().to_owned()));)*
                identities
            }

            fn native_identity_set(&self) -> &HashSet<String> {
                self.native_identities.get_or_init(|| {
                    if !self.include_native {
                        return HashSet::new();
                    }
                    let mut identities = self
                        .ir
                        .native
                        .0
                        .values()
                        .flat_map(|namespace| {
                            namespace
                                .arenas
                                .values()
                                .flatten()
                                .map(|record| record.id().to_owned())
                        })
                        .collect::<HashSet<_>>();
                    identities.extend(
                        self.additional_native_identities
                            .iter()
                            .map(|identity| (*identity).to_owned()),
                    );
                    identities
                })
            }

            fn identity_set(&self) -> &HashSet<String> {
                self.identities.get_or_init(|| {
                    let mut identities = self.model_identity_set();
                    if self.include_native {
                        identities.extend(self.native_identity_set().iter().cloned());
                    }
                    identities
                })
            }

            fn borrowed_identity_set(&self) -> HashSet<&'a str> {
                let mut identities = HashSet::with_capacity(self.ir.model.entity_count());
                $(identities.extend(self.ir.model.$field.iter().map(EntitySchema::identity));)*
                if self.include_native {
                    identities.extend(
                        self.ir
                            .native
                            .0
                            .values()
                            .flat_map(|namespace| {
                                namespace.arenas.values().flatten().map(|record| record.id())
                            }),
                    );
                    identities.extend(self.additional_native_identities.iter().copied());
                }
                identities
            }

            /// Returns the indexed document.
            pub fn ir(&self) -> &'a CadIr {
                self.ir
            }

            /// Returns whether any neutral or native entity owns `identity`.
            pub fn contains(&self, identity: &str) -> bool {
                self.identity_set().contains(identity)
            }

            /// Returns whether a native entity owns `identity`.
            pub fn contains_native(&self, identity: &str) -> bool {
                self.native_identity_set().contains(identity)
            }

            /// Iterates every neutral and native identity.
            pub fn identities(&self) -> impl Iterator<Item = &'a str> + '_ {
                self.borrowed_identity_set().into_iter()
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
                self.procedural_surface_for_carrier.get(surface).copied()
            }

            $(
                #[doc = concat!("Looks up an entity in the `", stringify!($field), "` arena.")]
                pub fn $field(&self, identity: &str) -> Option<&'a $element> {
                    lookup_identity(&self.ir.model.$field, &self.$field, identity)
                }
            )*
        }
    };
}

crate::document::arena_registry!(define_model_index);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ProceduralCurveDefinition, ProceduralSurfaceDefinition};
    use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use crate::{NativeNamespace, NativeRecord};
    use serde_json::Map;

    macro_rules! procedural_surface {
        (
            id: $id:expr,
            surface: $surface:expr,
            definition: $definition:expr,
            cache_fit_tolerance: $cache_fit_tolerance:expr,
            record_bounds: $record_bounds:expr $(,)?
        ) => {
            ProceduralSurface::try_new(
                $id,
                $surface,
                $definition,
                $cache_fit_tolerance,
                $record_bounds,
            )
            .expect("valid procedural surface fixture")
        };
    }

    macro_rules! procedural_curve {
        (
            id: $id:expr,
            curve: $curve:expr,
            definition: $definition:expr,
            cache_fit_tolerance: $cache_fit_tolerance:expr $(,)?
        ) => {
            ProceduralCurve::try_new($id, $curve, $definition, $cache_fit_tolerance)
                .expect("valid procedural curve fixture")
        };
    }

    #[test]
    fn procedural_surface_owner_index_preserves_arena_precedence() {
        let mut ir = CadIr::empty();
        for id in ["first", "second"] {
            ir.model.procedural_surfaces.push(procedural_surface! {
                id: ProceduralSurfaceId(format!("test:procedural-surface#{id}")),
                surface: SurfaceId("test:surface#owner".to_string()),
                definition: ProceduralSurfaceDefinition::Unknown { record: None },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
        }

        let index = ModelIndex::new(&ir);

        assert_eq!(
            index
                .procedural_surface_for_surface("test:surface#owner")
                .map(|procedural| procedural.id.0.as_str()),
            Some("test:procedural-surface#first")
        );
    }

    #[test]
    fn procedural_curve_owner_index_preserves_arena_precedence() {
        let mut ir = CadIr::empty();
        let curve = CurveId("test:curve#owner".to_string());
        for id in ["first", "second"] {
            ir.model.procedural_curves.push(procedural_curve! {
                id: ProceduralCurveId(format!("test:procedural-curve#{id}")),
                curve: curve.clone(),
                definition: ProceduralCurveDefinition::Unknown {
                    native_kind: None,
                    record: None,
                },
                cache_fit_tolerance: None,
            });
        }

        let index = ModelIndex::new(&ir);

        assert_eq!(
            index
                .procedural_curves_for_curve(curve.0.as_str())
                .and_then(|procedurals| procedurals.first().copied())
                .map(|procedural| procedural.id.0.as_str()),
            Some("test:procedural-curve#first")
        );
    }

    #[test]
    fn model_only_index_excludes_native_identity_universe() {
        let mut ir = CadIr::empty();
        let native_id = "test:native#0";
        let mut namespace = NativeNamespace::new(std::num::NonZeroU32::MIN);
        namespace.arenas.insert(
            "records".into(),
            vec![NativeRecord::new(native_id, Map::new())],
        );
        ir.native.0.insert("test".into(), namespace);
        let model_id = "test:model#0";
        ir.model.parameters.push(crate::features::DesignParameter {
            id: crate::features::ParameterId(model_id.into()),
            owner: None,
            ordinal: 0,
            name: "p1".into(),
            expression: "1".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });

        let full = ModelIndex::new(&ir);
        let model_only = ModelIndex::new_model_only(&ir);

        assert!(full.contains_native(native_id));
        assert!(full.contains(model_id));
        assert!(!model_only.contains_native(native_id));
        assert!(model_only.contains(model_id));
        assert!(!model_only
            .identities()
            .any(|identity| identity == native_id));
    }

    #[test]
    fn typed_lookup_indexes_are_lazy_and_preserve_last_duplicate() {
        let mut ir = CadIr::empty();
        let parameter_id = crate::features::ParameterId("test:parameter#0".into());
        for (ordinal, expression) in [(0, "first"), (1, "last")] {
            ir.model.parameters.push(crate::features::DesignParameter {
                id: parameter_id.clone(),
                owner: None,
                ordinal,
                name: format!("p{ordinal}"),
                expression: expression.into(),
                display: None,
                value: None,
                dependencies: Vec::new(),
                properties: std::collections::BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        }

        let index = ModelIndex::new_model_only(&ir);
        assert!(index.parameters.get().is_none());
        assert!(index.bodies.get().is_none());
        assert_eq!(
            index
                .parameters(parameter_id.0.as_str())
                .map(|parameter| parameter.expression.as_str()),
            Some("last")
        );
        assert!(index.parameters.get().is_some());
        assert!(index.bodies.get().is_none());
    }

    #[test]
    fn procedural_carrier_index_preserves_exact_and_unique_producer_rules() {
        let mut ir = CadIr::empty();
        let exact_surface = crate::ids::SurfaceId("test:surface#exact".to_string());
        let exact_construction = crate::ids::ProceduralSurfaceId("test:procedural#exact".into());
        ir.model.surfaces.push(Surface {
            id: exact_surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: exact_construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(procedural_surface! {
            id: exact_construction.clone(),
            surface: exact_surface.clone(),
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        ir.model.procedural_surfaces.push(procedural_surface! {
            id: exact_construction,
            surface: crate::ids::SurfaceId("test:surface#different".into()),
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
            cache_fit_tolerance: None,
            record_bounds: None,
        });

        let cached_surface = crate::ids::SurfaceId("test:surface#cached".to_string());
        ir.model.surfaces.push(Surface {
            id: cached_surface.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: crate::math::Point3::new(0.0, 0.0, 0.0),
                normal: crate::math::Vector3::new(0.0, 0.0, 1.0),
                u_axis: crate::math::Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(procedural_surface! {
            id: crate::ids::ProceduralSurfaceId("test:procedural#cached".into()),
            surface: cached_surface.clone(),
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
            cache_fit_tolerance: Some(0.01),
            record_bounds: None,
        });

        let index = ModelIndex::new_model_only(&ir);
        assert_eq!(
            index
                .procedural_surface_for_carrier(exact_surface.0.as_str())
                .map(|surface| surface.id.0.as_str()),
            Some("test:procedural#exact")
        );
        assert_eq!(
            index
                .procedural_surface_for_carrier(cached_surface.0.as_str())
                .map(|surface| surface.id.0.as_str()),
            Some("test:procedural#cached")
        );

        ir.model.procedural_surfaces.push(procedural_surface! {
            id: crate::ids::ProceduralSurfaceId("test:procedural#cached-duplicate".into()),
            surface: cached_surface.clone(),
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
            cache_fit_tolerance: Some(0.02),
            record_bounds: None,
        });
        let ambiguous = ModelIndex::new_model_only(&ir);
        assert!(ambiguous
            .procedural_surface_for_carrier(cached_surface.0.as_str())
            .is_none());
    }
}
