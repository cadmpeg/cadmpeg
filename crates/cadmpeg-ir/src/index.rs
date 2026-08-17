// SPDX-License-Identifier: Apache-2.0
//! Borrowed identity index over a complete CAD model.

use std::collections::{HashMap, HashSet};

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

macro_rules! define_model_index {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*]; )*) => {
        /// One-pass borrowed lookup index for neutral and native identities.
        pub struct ModelIndex<'a> {
            ir: &'a CadIr,
            $($field: HashMap<&'a str, &'a $element>,)*
            procedural_surface_by_surface: HashMap<&'a str, &'a ProceduralSurface>,
            procedural_surface_for_carrier: HashMap<&'a str, &'a ProceduralSurface>,
            procedural_curves_by_curve: HashMap<&'a str, Vec<&'a ProceduralCurve>>,
            identities: HashSet<&'a str>,
            native_identities: HashSet<&'a str>,
        }

        impl<'a> ModelIndex<'a> {
            /// Builds every typed lookup and the global identity universe once.
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
                let mut identities = HashSet::with_capacity(ir.model.entity_count());
                $(let $field = ir.model.$field.iter().map(|entity| {
                    let identity = entity.identity();
                    identities.insert(identity);
                    (identity, entity)
                }).collect();)*
                let mut native_identities = include_native
                    .then(|| {
                        ir.native
                            .0
                            .values()
                            .flat_map(|namespace| {
                                namespace.arenas.values().flatten().map(|record| record.id())
                            })
                            .collect::<HashSet<_>>()
                    })
                    .unwrap_or_default();
                native_identities.extend(additional);
                identities.extend(native_identities.iter().copied());
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
                    if procedural.cache_fit_tolerance.is_some() {
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
                    $($field,)*
                    procedural_surface_by_surface,
                    procedural_surface_for_carrier,
                    procedural_curves_by_curve,
                    identities,
                    native_identities,
                }
            }

            /// Returns the indexed document.
            pub fn ir(&self) -> &'a CadIr {
                self.ir
            }

            /// Returns whether any neutral or native entity owns `identity`.
            pub fn contains(&self, identity: &str) -> bool {
                self.identities.contains(identity)
            }

            /// Returns whether a native entity owns `identity`.
            pub fn contains_native(&self, identity: &str) -> bool {
                self.native_identities.contains(identity)
            }

            /// Iterates every neutral and native identity.
            pub fn identities(&self) -> impl Iterator<Item = &'a str> + '_ {
                self.identities.iter().copied()
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
                    self.$field.get(identity).copied()
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
    use crate::units::Units;
    use crate::{NativeNamespace, NativeRecord};
    use serde_json::Map;

    #[test]
    fn procedural_surface_owner_index_preserves_arena_precedence() {
        let mut ir = CadIr::empty(Units::default());
        for id in ["first", "second"] {
            ir.model.procedural_surfaces.push(ProceduralSurface {
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
        let mut ir = CadIr::empty(Units::default());
        let curve = CurveId("test:curve#owner".to_string());
        for id in ["first", "second"] {
            ir.model.procedural_curves.push(ProceduralCurve {
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
        let mut ir = CadIr::empty(Units::default());
        let native_id = "test:native#0";
        ir.native.0.insert(
            "test".into(),
            NativeNamespace {
                version: 1,
                arenas: [(
                    "records".into(),
                    vec![NativeRecord::new(native_id, Map::new())],
                )]
                .into_iter()
                .collect(),
            },
        );
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
    fn procedural_carrier_index_preserves_exact_and_unique_producer_rules() {
        let mut ir = CadIr::empty(Units::default());
        let exact_surface = crate::ids::SurfaceId("test:surface#exact".to_string());
        let exact_construction = crate::ids::ProceduralSurfaceId("test:procedural#exact".into());
        ir.model.surfaces.push(Surface {
            id: exact_surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: exact_construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: exact_construction.clone(),
            surface: exact_surface.clone(),
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
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
        ir.model.procedural_surfaces.push(ProceduralSurface {
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

        ir.model.procedural_surfaces.push(ProceduralSurface {
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
