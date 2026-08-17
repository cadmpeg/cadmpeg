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
use crate::geometry::{Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface};
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
            procedural_curves_by_curve: HashMap<&'a str, Vec<&'a ProceduralCurve>>,
            identities: HashSet<&'a str>,
            native_identities: HashSet<&'a str>,
        }

        impl<'a> ModelIndex<'a> {
            /// Builds every typed lookup and the global identity universe once.
            pub fn new(ir: &'a CadIr) -> Self {
                Self::with_additional_native_identities(ir, std::iter::empty())
            }

            /// Builds the index with native identities staged outside the document.
            pub fn with_additional_native_identities(
                ir: &'a CadIr,
                additional: impl IntoIterator<Item = &'a str>,
            ) -> Self {
                let mut identities = HashSet::with_capacity(ir.model.entity_count());
                $(let $field = ir.model.$field.iter().map(|entity| {
                    let identity = entity.identity();
                    identities.insert(identity);
                    (identity, entity)
                }).collect();)*
                let mut native_identities = ir.native.0.values().flat_map(|namespace| {
                    namespace.arenas.values().flatten().map(|record| record.id())
                }).collect::<HashSet<_>>();
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
                Self {
                    ir,
                    $($field,)*
                    procedural_surface_by_surface,
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
}
