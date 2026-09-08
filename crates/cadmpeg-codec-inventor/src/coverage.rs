// SPDX-License-Identifier: Apache-2.0
//! Statically declared decode-coverage measures.

use cadmpeg_ir::CoverageKey;

pub(crate) const RSE_STORAGE_BANDS: CoverageKey = CoverageKey::new("rse_storage_bands");
pub(crate) const RSE_DATABASES: CoverageKey = CoverageKey::new("rse_databases");
pub(crate) const RSE_REGISTRY_ENTRIES: CoverageKey = CoverageKey::new("rse_registry_entries");
pub(crate) const RSE_REVISIONS: CoverageKey = CoverageKey::new("rse_revisions");
pub(crate) const RSE_SEGMENT_PAIRS: CoverageKey = CoverageKey::new("rse_segment_pairs");
pub(crate) const RSE_SEGMENT_META: CoverageKey = CoverageKey::new("rse_segment_meta");
pub(crate) const RSE_META_TYPES: CoverageKey = CoverageKey::new("rse_meta_types");
pub(crate) const RSE_SEGMENT_META_ISSUES: CoverageKey = CoverageKey::new("rse_segment_meta_issues");
pub(crate) const RSE_SEGMENT_BULK: CoverageKey = CoverageKey::new("rse_segment_bulk");
pub(crate) const RSE_RECORDS: CoverageKey = CoverageKey::new("rse_records");
pub(crate) const RSE_SEGMENT_BULK_ISSUES: CoverageKey = CoverageKey::new("rse_segment_bulk_issues");
pub(crate) const PROPERTY_SETS: CoverageKey = CoverageKey::new("property_sets");
pub(crate) const PROPERTIES: CoverageKey = CoverageKey::new("properties");
pub(crate) const PREVIEW_ASSETS: CoverageKey = CoverageKey::new("preview_assets");
pub(crate) const PROTEIN_ENTRIES: CoverageKey = CoverageKey::new("protein_entries");
pub(crate) const PROTEIN_ASSETS: CoverageKey = CoverageKey::new("protein_assets");
pub(crate) const PROTEIN_REJECTIONS: CoverageKey = CoverageKey::new("protein_rejections");
pub(crate) const PROTEIN_APPEARANCES: CoverageKey = CoverageKey::new("protein_appearances");
pub(crate) const APPEARANCE_BINDINGS_TRANSFERRED: CoverageKey =
    CoverageKey::new("appearance_bindings_transferred");
pub(crate) const PM_APP_DEFAULT_STYLES: CoverageKey = CoverageKey::new("pm_app_default_styles");
pub(crate) const PM_APP_RENDERING_STYLES: CoverageKey = CoverageKey::new("pm_app_rendering_styles");
pub(crate) const PM_GRAPHICS_FACES: CoverageKey = CoverageKey::new("pm_graphics_faces");
pub(crate) const PM_GRAPHICS_STYLE_COLLECTIONS: CoverageKey =
    CoverageKey::new("pm_graphics_style_collections");
pub(crate) const PM_GRAPHICS_PRIMARY_COLOR_STYLES: CoverageKey =
    CoverageKey::new("pm_graphics_primary_color_styles");
pub(crate) const FACE_COLOR_APPEARANCES: CoverageKey = CoverageKey::new("face_color_appearances");
pub(crate) const PRESENTATION_RECORD_ISSUES: CoverageKey =
    CoverageKey::new("presentation_record_issues");
pub(crate) const PM_DC_PARAMETERS: CoverageKey = CoverageKey::new("pm_dc_parameters");
pub(crate) const PM_DC_EXPRESSIONS: CoverageKey = CoverageKey::new("pm_dc_expressions");
pub(crate) const PM_DC_UNITS: CoverageKey = CoverageKey::new("pm_dc_units");
pub(crate) const DESIGN_PARAMETERS_TRANSFERRED: CoverageKey =
    CoverageKey::new("design_parameters_transferred");
pub(crate) const DESIGN_RECORD_ISSUES: CoverageKey = CoverageKey::new("design_record_issues");
pub(crate) const PM_DC_SKETCHES: CoverageKey = CoverageKey::new("pm_dc_sketches");
pub(crate) const PM_DC_SKETCH_ENTITIES: CoverageKey = CoverageKey::new("pm_dc_sketch_entities");
pub(crate) const PM_DC_TRANSFORMS: CoverageKey = CoverageKey::new("pm_dc_transforms");
pub(crate) const PM_DC_DIRECTIONS: CoverageKey = CoverageKey::new("pm_dc_directions");
pub(crate) const PM_DC_SKETCH_CONSTRAINTS: CoverageKey =
    CoverageKey::new("pm_dc_sketch_constraints");
pub(crate) const SKETCH_RECORD_ISSUES: CoverageKey = CoverageKey::new("sketch_record_issues");
pub(crate) const PM_DC_FEATURES: CoverageKey = CoverageKey::new("pm_dc_features");
pub(crate) const PM_DC_PATTERN_FEATURES: CoverageKey = CoverageKey::new("pm_dc_pattern_features");
pub(crate) const PM_DC_FEATURE_TERMINATORS: CoverageKey =
    CoverageKey::new("pm_dc_feature_terminators");
pub(crate) const PM_DC_FEATURE_PROPERTIES: CoverageKey =
    CoverageKey::new("pm_dc_feature_properties");
pub(crate) const PM_DC_FEATURE_LABELS: CoverageKey = CoverageKey::new("pm_dc_feature_labels");
pub(crate) const PM_DC_ENTITY_STYLE_LINKS: CoverageKey =
    CoverageKey::new("pm_dc_entity_style_links");
pub(crate) const FEATURE_RECORD_ISSUES: CoverageKey = CoverageKey::new("feature_record_issues");
pub(crate) const FEATURES_TRANSFERRED: CoverageKey = CoverageKey::new("features_transferred");
pub(crate) const FEATURE_RESULT_TOPOLOGIES_TRANSFERRED: CoverageKey =
    CoverageKey::new("feature_result_topologies_transferred");
pub(crate) const SKETCHES_TRANSFERRED: CoverageKey = CoverageKey::new("sketches_transferred");
pub(crate) const SKETCH_ENTITIES_TRANSFERRED: CoverageKey =
    CoverageKey::new("sketch_entities_transferred");
pub(crate) const SKETCH_CONSTRAINTS_TRANSFERRED: CoverageKey =
    CoverageKey::new("sketch_constraints_transferred");
pub(crate) const EXTERNAL_REFERENCES: CoverageKey = CoverageKey::new("external_references");
pub(crate) const EMBEDDED_REFERENCES: CoverageKey = CoverageKey::new("embedded_references");
pub(crate) const UFRX_MODEL_STATES: CoverageKey = CoverageKey::new("ufrx_model_states");
pub(crate) const UFRX_OCCURRENCES: CoverageKey = CoverageKey::new("ufrx_occurrences");
pub(crate) const ASSEMBLY_OCCURRENCES: CoverageKey = CoverageKey::new("assembly_occurrences");
pub(crate) const ASSEMBLY_PLACEMENTS: CoverageKey = CoverageKey::new("assembly_placements");
pub(crate) const ASSEMBLY_OCCURRENCES_TRANSFERRED: CoverageKey =
    CoverageKey::new("assembly_occurrences_transferred");
pub(crate) const ASSEMBLY_RECORD_ISSUES: CoverageKey = CoverageKey::new("assembly_record_issues");
pub(crate) const ACTIVE_KERNEL_CARRIERS: CoverageKey = CoverageKey::new("active_kernel_carriers");
pub(crate) const KERNEL_UNKNOWN_RECORDS: CoverageKey = CoverageKey::new("kernel_unknown_records");
pub(crate) const KERNEL_UNKNOWN_SURFACE_FACES: CoverageKey =
    CoverageKey::new("kernel_unknown_surface_faces");
