// SPDX-License-Identifier: Apache-2.0
//! Unrepresented-content accounting for the STEP writer.

use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};
use cadmpeg_ir::report::{LossCategory, LossCode, Severity};

use super::Builder;

impl Builder<'_> {
    pub(super) fn note_unrepresented(&mut self) {
        let nonstandard_analytic_surfaces = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| match &surface.geometry {
                SurfaceGeometry::Sphere { radius, .. } => *radius < 0.0,
                SurfaceGeometry::Torus {
                    major_radius,
                    minor_radius,
                    ..
                } => {
                    *major_radius < 0.0
                        || *minor_radius < 0.0
                        || (minor_radius.abs() > major_radius.abs()
                            && !self.ir.model.procedural_surfaces.iter().any(|procedural| {
                                procedural.surface == surface.id
                                    && self.written_procedural_surfaces.contains(&procedural.id.0)
                                    && matches!(
                                        procedural.definition,
                                        ProceduralSurfaceDefinition::DegenerateTorus { .. }
                                    )
                            }))
                }
                _ => false,
            })
            .count();
        if nonstandard_analytic_surfaces > 0 {
            self.loss(
                LossCode::AnalyticSurfaceNormalized,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{nonstandard_analytic_surfaces} signed or self-intersecting analytic \
                     surface(s) were normalized to positive STEP radii"
                ),
            );
        }
        let elliptical_cones = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(
                    surface.geometry,
                    SurfaceGeometry::Cone { ratio, .. } if ratio != 1.0
                )
            })
            .count();
        if elliptical_cones > 0 {
            self.loss(
                LossCode::EllipticalConeReduced,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{elliptical_cones} elliptical cone surface(s) were reduced to circular STEP CONICAL_SURFACE carriers"
                ),
            );
        }
        if !self.curveless_edges.is_empty() {
            self.loss(
                LossCode::CurvelessEdgeOmitted,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} edge(s) have no typed 3D curve or carry a STEP-unsupported transform and were omitted from \
                     their edge loops (STEP EDGE_CURVE requires a 3D curve)",
                    self.curveless_edges.len()
                ),
            );
        }
        if !self.unknown_surface_faces.is_empty() {
            self.loss(
                LossCode::UnknownSurfaceFaceOmitted,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} face(s) rest on an unknown or STEP-unsupported surface and were omitted \
                     from the STEP shell (an ADVANCED_FACE requires a surface); their \
                     topology remains in the IR",
                    self.unknown_surface_faces.len()
                ),
            );
        }
        if self.unsupported_standalone_geometry > 0 {
            self.loss(
                LossCode::GeometryNotTransferred,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} standalone unknown geometry carrier(s) were not written",
                    self.unsupported_standalone_geometry
                ),
            );
        }
        let missing_pcurve_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .filter(|use_| !self.index.pcurves.contains_key(use_.pcurve.as_str()))
            .count();
        if missing_pcurve_count > 0 {
            self.loss(
                LossCode::PcurveOmitted,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{missing_pcurve_count} coedge pcurve reference(s) have no geometry and were not written"
                ),
            );
        }
        let reduced_pcurve_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .filter_map(|use_| self.index.pcurves.get(use_.pcurve.as_str()))
            .filter(|pcurve| {
                pcurve.wrapper_reversed.is_some()
                    || pcurve.native_tail_flags.is_some()
                    || pcurve.parameter_range.is_some()
                    || pcurve.fit_tolerance.is_some()
            })
            .count();
        if reduced_pcurve_count > 0 {
            self.loss(
                LossCode::PcurveOmitted,
                LossCategory::Geometry,
                Severity::Info,
                format!(
                    "{reduced_pcurve_count} emitted coedge pcurve(s) carry native-only metadata not represented in STEP"
                ),
            );
        }
        if !self.ir.model.subds.is_empty() {
            self.loss(
                LossCode::SubdOmitted,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} subdivision surface(s) were omitted because this STEP writer \
                     does not encode SubD control cages",
                    self.ir.model.subds.len()
                ),
            );
        }
        let unwritten_pmi = self.ir.model.pmi.len().saturating_sub(self.written_pmi);
        if unwritten_pmi > 0 {
            self.loss(
                LossCode::PmiOmitted,
                LossCategory::Attribute,
                Severity::Warning,
                format!("{unwritten_pmi} PMI annotation(s) were not written to STEP"),
            );
        }
        let source_object_count = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.source_object.is_some())
            .count()
            + self
                .ir
                .model
                .curves
                .iter()
                .filter(|curve| curve.source_object.is_some())
                .count()
            + self
                .ir
                .model
                .subds
                .iter()
                .filter(|subd| subd.source_object.is_some())
                .count()
            + self
                .ir
                .model
                .tessellations
                .iter()
                .filter(|tessellation| tessellation.source_object.is_some())
                .count();
        if source_object_count > 0 {
            self.loss(
                LossCode::SourceAssociationOmitted,
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{source_object_count} source-object association(s) were not represented in STEP"
                ),
            );
        }
        let unknown_count = self
            .ir
            .native
            .loss_counts()
            .into_iter()
            .filter(|count| count.kind == "unknowns")
            .map(|count| count.count)
            .sum::<usize>();
        if unknown_count > 0 {
            self.loss(
                LossCode::PassthroughRecordOmitted,
                LossCategory::Metadata,
                Severity::Info,
                format!("{unknown_count} uninterpreted passthrough record(s) were not represented in STEP"),
            );
        }
        if self.unstyled_colors > 0 {
            self.loss(
                LossCode::AttributesNotTransferred,
                LossCategory::Attribute,
                Severity::Info,
                format!(
                    "{} display color(s) had no emitted STEP item and were not written \
                     to STEP presentation",
                    self.unstyled_colors
                ),
            );
        }
        let lossy_appearances = self
            .ir
            .model
            .appearances
            .iter()
            .filter(|appearance| {
                let bindings = self
                    .ir
                    .model
                    .appearance_bindings
                    .iter()
                    .filter(|binding| binding.appearance == appearance.id)
                    .collect::<Vec<_>>();
                appearance.asset_guid.is_some()
                    || appearance.visual_guid.is_some()
                    || appearance.physical_token.is_some()
                    || appearance
                        .schema
                        .as_deref()
                        .is_some_and(|schema| schema != "step_surface_style")
                    || appearance.category.is_some()
                    || !appearance.properties.is_empty()
                    || appearance.base_color.is_none_or(|color| color.a != 1.0)
                    || bindings.is_empty()
                    || bindings
                        .iter()
                        .any(|binding| !self.written_appearance_bindings.contains(&binding.id))
            })
            .count();
        if lossy_appearances > 0 {
            self.loss(
                LossCode::AppearanceReduced,
                LossCategory::Material,
                Severity::Info,
                format!(
                    "{lossy_appearances} appearance asset(s) were reduced to STYLED_ITEM base colors; \
                     schemas, textures, and shader properties were not written to STEP"
                ),
            );
        }
        let lossy_binding_metadata = self
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.object_type.is_some() || !binding.channels.is_empty())
            .count();
        if lossy_binding_metadata > 0 {
            self.loss(
                LossCode::AppearanceReduced,
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{lossy_binding_metadata} appearance binding(s) carry source object or channel metadata not represented in STEP"
                ),
            );
        }
        if !self.ir.model.attributes.is_empty() {
            self.loss(
                LossCode::AttributesNotTransferred,
                LossCategory::Attribute,
                Severity::Info,
                format!(
                    "{} source attribute record(s) were not written to STEP",
                    self.ir.model.attributes.len()
                ),
            );
        }
        let procedural_surface_count = self
            .ir
            .model
            .procedural_surfaces
            .iter()
            .filter(|procedural| !self.written_procedural_surfaces.contains(&procedural.id.0))
            .count();
        let procedural_curve_count = self
            .ir
            .model
            .procedural_curves
            .iter()
            .filter(|procedural| !self.written_procedural_curves.contains(&procedural.id.0))
            .count();
        if procedural_surface_count > 0 || procedural_curve_count > 0 {
            self.loss(
                LossCode::ProceduralReduced,
                LossCategory::Geometry,
                Severity::Info,
                format!(
                    "{procedural_surface_count} procedural surface definition(s) and {procedural_curve_count} procedural curve definition(s) were reduced to their solved STEP carriers"
                ),
            );
        }
        let source_native_records: usize = self
            .ir
            .native
            .loss_counts()
            .iter()
            .filter(|loss| loss.kind != "unknowns")
            .map(|loss| loss.count)
            .sum();
        if source_native_records > 0 {
            self.loss(
                LossCode::ParametricRecordOmitted,
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{source_native_records} source-native record(s) were not represented in STEP"
                ),
            );
        }
    }
}
