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
use crate::report::{Check, Finding, LossNote, Severity, ValidationReport};
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use crate::units::LengthUnit;
use sha2::{Digest, Sha256};

mod annotations_native;
mod carriers_parameterization;
mod geometry_consistency;
mod geometry_payloads;
mod identity_order;
mod sketches;
mod subd;
mod topology;

use annotations_native::{check_annotations, check_native_links};
use carriers_parameterization::{check_carrier_reachability, check_parameter_domains};
use geometry_consistency::{check_edge_endpoint_consistency, check_pcurve_surface_consistency};
use geometry_payloads::{check_bounds, check_tessellations, check_unknown_payloads};
use identity_order::{check_identity_and_order, check_version, collect_native_ids, entity_counts};
use sketches::check_sketches;
use subd::{check_procedural_surfaces, check_source_associations, check_subds};
use topology::{
    check_coedge_pairing, check_loops, check_references, check_units, check_wire_topology, IdSets,
};

/// A radius/length that is not a finite positive number is invalid geometry.
/// Written without a negated comparison operator so it stays clippy-clean while
/// still rejecting NaN and non-positive values.
fn nonpositive(x: f64) -> bool {
    !(x.is_finite() && x > 0.0)
}

/// Validate `ir` and copy `losses` into the returned report unchanged.
pub fn validate(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    let mut findings = Vec::new();

    let ids = IdSets::build(ir);
    check_version(ir, &mut findings);
    // The identity walk enumerates every entity id in the document; annotation
    // and link targets resolve against that set.
    let all_ids = check_identity_and_order(ir, &mut findings);
    check_units(ir, &mut findings);
    check_references(ir, &ids, &mut findings);
    check_loops(ir, &mut findings);
    check_coedge_pairing(ir, &mut findings);
    check_wire_topology(ir, &mut findings);
    check_carrier_reachability(ir, &mut findings);
    check_annotations(ir, &all_ids, &mut findings);
    check_native_links(ir, &all_ids, &mut findings);
    check_parameter_domains(ir, &mut findings);
    check_edge_endpoint_consistency(ir, &mut findings);
    check_pcurve_surface_consistency(ir, &mut findings);
    check_bounds(ir, &mut findings);
    check_tessellations(ir, &mut findings);
    check_subds(ir, &mut findings);
    check_procedural_surfaces(ir, &mut findings);
    check_source_associations(ir, &mut findings);
    check_unknown_payloads(ir, &mut findings);
    check_sketches(ir, &mut findings);

    ValidationReport {
        entity_counts: entity_counts(ir),
        findings,
        losses,
    }
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, Feature,
        FeatureDefinition, FeatureId, SketchSpace,
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
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: None,
            },
            native_ref: None,
        });
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
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
            bodies: Vec::new(),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id,
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::Sketch {
                        space: SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    },
                },
            )]),
            native_ref: None,
        });

        let report = validate(&ir, Vec::new());

        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }
}
