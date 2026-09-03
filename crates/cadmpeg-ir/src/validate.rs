// SPDX-License-Identifier: Apache-2.0
//! Structural and numeric validation for [`CadIr`].
//!
//! Validation checks identity and arena order, references, topology rings,
//! carrier reachability, annotations, native links, parameter
//! domains, payload integrity, tessellation, numeric bounds, and geometric
//! consistency (edge-curve endpoints and pcurve surface images against vertex
//! positions). It does not evaluate interior surface membership or solid
//! closure.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::document::CadIr;
use crate::features::Feature;
use crate::geometry::{
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use crate::math::Vector3;
use crate::report::{Check, Finding, LossNote, Severity, ValidationReport};
use crate::source_fidelity::SourceFidelity;
use crate::topology::Coedge;
use crate::units::LengthUnit;

/// Frozen accept/reject IR builders for Phase 5 gate swaps.
pub mod admissibility_freeze;
/// Narrow admissibility predicates as documented `Check` subsets.
pub mod admit;
mod annotations_native;
mod assets;
mod carriers_parameterization;
mod drawings;
mod geometry_consistency;
mod geometry_payloads;
mod identity_order;
mod pmi;
mod presentation;
mod products;
mod referential_integrity;
mod semantic_annotations;
mod sketches;
mod spreadsheets;
mod subd;
mod topology;

use annotations_native::{check_annotations, check_native_links};
use assets::check_assets;
use carriers_parameterization::{check_carrier_reachability, check_parameter_domains};
use drawings::check_drawings;
use geometry_consistency::{
    check_edge_endpoint_consistency, check_pcurve_surface_consistency,
    check_procedural_support_consistency,
};
use geometry_payloads::{check_bounds, check_tessellations};
use identity_order::{check_identity_and_order, collect_native_ids};
use pmi::check_pmi;
use presentation::check_presentation;
use products::check_products;
use referential_integrity::check_typed_references;
use semantic_annotations::check_semantic_annotations;
use sketches::check_sketches;
use spreadsheets::check_spreadsheets;
use subd::{check_procedural_surfaces, check_source_associations, check_subds};
use topology::{
    check_coedge_pairing, check_loops, check_references, check_shell_connectivity, check_units,
    check_wire_topology,
};

/// A radius/length that is not a finite positive number is invalid geometry.
fn nonpositive(x: f64) -> bool {
    !(x.is_finite() && x > 0.0)
}

/// Count the records represented by the IR arenas without running validation.
///
/// Prefer [`CadIr::census`](crate::CadIr::census); this alias remains for
/// existing `cadmpeg_ir::entity_census` call sites.
pub fn entity_census(ir: &CadIr) -> BTreeMap<String, usize> {
    crate::document::entity_census(ir)
}

/// Validate `ir` and copy `losses` into the returned report unchanged.
fn validate_model(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    let index = crate::index::ModelIndex::new(ir);
    validate_model_with_index(ir, losses, &index)
}

fn validate_model_with_index(
    ir: &CadIr,
    losses: Vec<LossNote>,
    ids: &crate::index::ModelIndex<'_>,
) -> ValidationReport {
    let mut findings = Vec::new();

    check_assets(ir, &mut findings);
    // The identity walk enumerates every entity id in the product document;
    // native links resolve against that set.
    check_identity_and_order(ir, &mut findings);
    check_units(ir, &mut findings);
    check_references(ir, ids, &mut findings);
    check_pmi(ir, &mut findings);
    check_loops(ir, ids, &mut findings);
    check_coedge_pairing(ir, &mut findings);
    check_shell_connectivity(ir, &mut findings);
    check_wire_topology(ir, &mut findings);
    check_carrier_reachability(ir, &mut findings);
    check_native_links(ir, ids, &mut findings);
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
    check_products(ir, &mut findings);
    check_presentation(ir, ids, &mut findings);
    check_drawings(ir, ids, &mut findings);
    check_semantic_annotations(ir, ids, &mut findings);
    check_typed_references(ir, ids, &mut findings);

    ValidationReport {
        entity_counts: entity_census(ir),
        findings,
        losses,
    }
}

/// Validates a model while treating staged retained-record identities as native entities.
pub fn validate_neutral_with_additional_native_identities<'a>(
    ir: &'a CadIr,
    additional: impl IntoIterator<Item = &'a str>,
    losses: Vec<LossNote>,
) -> ValidationReport {
    let index = crate::index::ModelIndex::with_additional_native_identities(ir, additional);
    validate_model_with_index(ir, losses, &index)
}

/// Validate one neutral product model.
pub fn validate_neutral(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    validate_model(ir, losses)
}

/// Validate one neutral product model together with borrowed annotations.
pub fn validate_neutral_with_annotations(
    ir: &CadIr,
    annotations: &crate::annotations::Annotations,
    losses: Vec<LossNote>,
) -> ValidationReport {
    let mut report = validate_model(ir, losses);
    let all_ids = crate::index::ModelIndex::new(ir)
        .identities()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    check_annotations(ir, annotations, &all_ids, &mut report.findings);
    report
}

/// Validate a neutral product model together with its decode-time source sidecar.
pub fn validate_neutral_with_source_fidelity(
    ir: &CadIr,
    source_fidelity: &SourceFidelity,
    losses: Vec<LossNote>,
) -> ValidationReport {
    let mut report = validate_model(ir, losses);
    if let Err(error) = source_fidelity.validate() {
        report.findings.push(Finding {
            check: Check::PayloadIntegrity,
            severity: Severity::Error,
            message: format!("invalid source fidelity: {error}"),
            entity: None,
        });
    }
    let index = crate::index::ModelIndex::new(ir);
    let mut all_ids = index
        .identities()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    all_ids.extend(
        source_fidelity
            .retained_records
            .iter()
            .map(|record| record.id().to_owned()),
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
    use super::validate_neutral;
    use crate::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, FaceSelection, Feature,
        FeatureDefinition, FeatureId, PrincipalPlane, SplitFaceTool,
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
            definition: FeatureDefinition::Sketch { sketch: None },
            native_ref: None,
        });
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
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
            active: true.into(),
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
                        sketch: Some(sketch_id),
                    },
                },
            )]),
            native_ref: None,
        });

        let report = validate_neutral(&ir, Vec::new());

        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn split_face_plane_sets_require_two_unique_plane_dependencies() {
        let feature =
            |id: FeatureId, ordinal, dependencies, definition: FeatureDefinition| Feature {
                id,
                ordinal,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies,
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition,
                native_ref: None,
            };
        let first = FeatureId("test:model:feature#plane-a".into());
        let second = FeatureId("test:model:feature#plane-b".into());
        let split = FeatureId("test:model:feature#split".into());
        let mut ir = CadIr::empty(Units::default());
        ir.model.features = vec![
            feature(
                first.clone(),
                0,
                Vec::new(),
                FeatureDefinition::DatumPrincipalPlane {
                    plane: PrincipalPlane::Front,
                },
            ),
            feature(
                second.clone(),
                1,
                Vec::new(),
                FeatureDefinition::DatumPrincipalPlane {
                    plane: PrincipalPlane::Right,
                },
            ),
            feature(
                split,
                2,
                vec![first.clone(), second.clone()],
                FeatureDefinition::SplitFace {
                    targets: FaceSelection::Unresolved,
                    tool: SplitFaceTool::Planes {
                        planes: vec![first.clone(), second.clone()],
                    },
                },
            ),
        ];

        let report = validate_neutral(&ir, Vec::new());
        assert!(report.findings.is_empty(), "{:?}", report.findings);

        if let FeatureDefinition::SplitFace { tool, .. } = &mut ir.model.features[2].definition {
            *tool = SplitFaceTool::Planes {
                planes: vec![first.clone()],
            };
        } else {
            unreachable!();
        }
        let report = validate_neutral(&ir, Vec::new());
        assert!(report.findings.iter().any(|finding| {
            finding.message == "split-face plane set has fewer than two planes"
        }));

        if let FeatureDefinition::SplitFace { tool, .. } = &mut ir.model.features[2].definition {
            *tool = SplitFaceTool::Planes {
                planes: vec![first.clone(), first],
            };
        } else {
            unreachable!();
        }
        let report = validate_neutral(&ir, Vec::new());
        assert!(report
            .findings
            .iter()
            .any(|finding| { finding.message == "split-face plane set contains repeated planes" }));
    }
}
