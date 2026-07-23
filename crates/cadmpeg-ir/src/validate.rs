// SPDX-License-Identifier: Apache-2.0
//! Structural and numeric validation for [`CadIr`].
//!
//! Validation checks schema version, identity and arena order, references,
//! topology rings, carrier reachability, annotations, native links, parameter
//! domains, payload integrity, tessellation, numeric bounds, and geometric
//! consistency (edge-curve endpoints and pcurve surface images against vertex
//! positions). It does not evaluate interior surface membership or solid
//! closure.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::document::{CadIr, IR_VERSION};
use crate::features::Feature;
use crate::geometry::{
    Curve, CurveGeometry, Pcurve, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use crate::math::Vector3;
use crate::report::{LossNote, Severity};
use crate::source_fidelity::SourceFidelity;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use crate::units::LengthUnit;

mod annotations_native;
mod carriers_parameterization;
mod drawings;
mod geometry_consistency;
mod geometry_payloads;
mod identity_order;
mod index;
mod pmi;
mod presentation;
mod product;
mod products;
mod report;
mod semantic_annotations;
mod sketches;
mod spreadsheets;
mod subd;
mod topology;

pub use report::{Check, Finding, ValidationReport};

use index::ModelIndex;

use annotations_native::{check_annotations, check_native_links};
use carriers_parameterization::{check_carrier_reachability, check_parameter_domains};
use drawings::check_drawings;
use geometry_consistency::{
    check_edge_endpoint_consistency, check_pcurve_surface_consistency,
    check_procedural_support_consistency,
};
use geometry_payloads::{check_bounds, check_tessellations};
use identity_order::{check_identity_and_order, check_version, collect_native_ids, entity_counts};
use pmi::check_pmi;
use presentation::check_presentation;
use product::check_products as check_step_products;
use products::check_products as check_component_products;
use semantic_annotations::check_semantic_annotations;
use sketches::check_sketches;
use spreadsheets::check_spreadsheets;
use subd::{check_procedural_surfaces, check_source_associations, check_subds};
use topology::{
    check_coedge_pairing, check_loops, check_references, check_shell_connectivity, check_units,
    check_wire_topology,
};

/// A radius/length that is not a finite positive number is invalid geometry.
/// Written without a negated comparison operator so it stays clippy-clean while
/// still rejecting NaN and non-positive values.
fn nonpositive(x: f64) -> bool {
    !(x.is_finite() && x > 0.0)
}

/// Validate `ir` and copy `losses` into the returned report unchanged.
fn validate_with_ids(ir: &CadIr, losses: Vec<LossNote>) -> (ValidationReport, HashSet<String>) {
    let mut findings = Vec::new();

    // One shared id index for the whole run: per-arena `id -> entity` maps, the
    // native unknown-record presence set, and `all_ids`, the set of every entity
    // id in the document that reference targets resolve against.
    let index = ModelIndex::build(ir);
    check_version(ir, &mut findings);
    check_identity_and_order(ir, &mut findings);
    check_units(ir, &mut findings);
    check_references(ir, &index, &mut findings);
    check_step_products(ir, &mut findings);
    check_pmi(ir, &mut findings);
    check_loops(ir, &index, &mut findings);
    check_coedge_pairing(ir, &index, &mut findings);
    check_shell_connectivity(ir, &index, &mut findings);
    check_wire_topology(ir, &index, &mut findings);
    check_carrier_reachability(ir, &mut findings);
    check_native_links(ir, &index, &mut findings);
    check_parameter_domains(ir, &mut findings);
    check_edge_endpoint_consistency(ir, &mut findings);
    check_pcurve_surface_consistency(ir, &mut findings);
    check_procedural_support_consistency(ir, &mut findings);
    check_bounds(ir, &mut findings);
    check_tessellations(ir, &mut findings);
    check_subds(ir, &mut findings);
    check_procedural_surfaces(ir, &mut findings);
    check_source_associations(ir, &mut findings);
    check_sketches(ir, &mut findings);
    check_spreadsheets(ir, &mut findings);
    check_component_products(ir, &mut findings);
    check_presentation(ir, &index.all_ids, &mut findings);
    check_drawings(ir, &index, &mut findings);
    check_semantic_annotations(ir, &index, &mut findings);

    let ModelIndex { all_ids, .. } = index;
    (
        ValidationReport {
            entity_counts: entity_counts(ir),
            findings,
            losses,
        },
        all_ids,
    )
}

/// Validate one neutral product model.
pub fn validate(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    validate_with_ids(ir, losses).0
}

/// Validate a neutral product model together with its decode-time source sidecar.
pub fn validate_with_source_fidelity(
    ir: &CadIr,
    source_fidelity: &SourceFidelity,
    losses: Vec<LossNote>,
) -> ValidationReport {
    let (mut report, mut all_ids) = validate_with_ids(ir, losses);
    if let Err(error) = source_fidelity.validate() {
        report.findings.push(Finding {
            check: Check::PayloadIntegrity,
            severity: Severity::Error,
            message: format!("invalid source fidelity: {error}"),
            entity: None,
        });
    }
    all_ids.extend(
        source_fidelity
            .retained_records
            .iter()
            .map(|record| record.id.clone()),
    );
    check_annotations(
        ir,
        &source_fidelity.annotations,
        &all_ids,
        &mut report.findings,
    );
    report
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, Feature,
        FeatureDefinition, FeatureId,
    };
    use crate::math::{Point3, Vector3};
    use crate::sketches::{Sketch, SketchId};
    use crate::units::Units;
    use crate::CadIr;
    use std::collections::BTreeMap;

    #[test]
    fn configuration_feature_sketch_resolves_against_model_sketches() {
        let mut ir = CadIr::empty(Units::default());
        let feature_id = FeatureId("test:model:feature#sketch".into());
        let sketch_id = SketchId("test:model:sketch#sketch".into());
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: crate::features::SketchSpace::Planar,
                sketch: None,
            },
            native_ref: None,
        });
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            placement: crate::sketches::SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("test:model:configuration#default".into()),
            ordinal: 0,
            active: true,
            source_index: None,
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            suppressed_features: Vec::new(),
            bodies: crate::features::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id,
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::Sketch {
                        space: crate::features::SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    },
                },
            )]),
            native_ref: None,
        });

        let report = validate(&ir, Vec::new());

        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    /// Determinism net for the `ModelIndex` conversions (P4). Each fixture pins
    /// the complete findings `Vec` — order, check, severity, entity, and message —
    /// so that swapping per-check id rebuilds for the shared index cannot silently
    /// reorder or alter what validation reports. Every line is
    /// `check|severity|entity|message`; an empty expected slice pins zero findings.
    #[test]
    fn model_index_conversions_preserve_finding_order_and_content() {
        use super::Finding;
        use crate::examples::unit_cube;
        use crate::ids::{RegionId, SurfaceId};

        fn render(findings: &[Finding]) -> Vec<String> {
            findings
                .iter()
                .map(|f| {
                    format!(
                        "{:?}|{:?}|{}|{}",
                        f.check,
                        f.severity,
                        f.entity.as_deref().unwrap_or(""),
                        f.message
                    )
                })
                .collect()
        }

        // A valid cube: no findings.
        let baseline = unit_cube();

        // A body references a region id that is not in the region arena.
        let mut dangling_region = unit_cube();
        dangling_region.model.bodies[0].regions =
            vec![RegionId("synthetic:cube:region#missing".into())];

        // Dropping one coedge from a loop leaves its `next` ring unable to close.
        let mut unclosed_ring = unit_cube();
        unclosed_ring.model.loops[0].coedges.pop();

        // A face references a surface id that is not in the surface arena; the
        // surface it abandoned then reads as an orphan carrier.
        let mut missing_surface = unit_cube();
        missing_surface.model.faces[0].surface = SurfaceId("synthetic:cube:surface#missing".into());

        let cases: [(&str, CadIr, &[&str]); 4] = [
            ("baseline", baseline, &[]),
            (
                "dangling_region",
                dangling_region,
                &["ReferentialIntegrity|Error|synthetic:cube:body#0|references missing region `synthetic:cube:region#missing`"],
            ),
            (
                "unclosed_ring",
                unclosed_ring,
                &["LoopClosure|Error|synthetic:cube:loop#back|coedge `next` ring does not close over the loop's 3 coedges"],
            ),
            (
                "missing_surface",
                missing_surface,
                &[
                    "ReferentialIntegrity|Error|synthetic:cube:face#back|references missing surface `synthetic:cube:surface#missing`",
                    "CarrierReachability|Error|synthetic:cube:surface#back|orphan surface carrier",
                ],
            ),
        ];

        for (name, ir, expected) in cases {
            let report = validate(&ir, Vec::new());
            assert_eq!(render(&report.findings), expected, "fixture `{name}`");
        }
    }
}
