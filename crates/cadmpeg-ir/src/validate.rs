// SPDX-License-Identifier: Apache-2.0
//! Validation of an IR document using only in-IR arithmetic.
//!
//! These checks need no geometry kernel: referential integrity of the topology
//! graph, loop-ring closure, coedge pairing, unit presence, and cheap geometric
//! sanity (non-degenerate directions, positive radii, well-formed NURBS pole
//! counts). Anything requiring true geometric evaluation (does a pcurve lie on
//! its surface, do faces actually bound a closed solid) is out of scope and is
//! deliberately *not* faked here.

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
mod geometry_payloads;
mod identity_order;
mod topology;

use annotations_native::{
    build_entity_index, check_annotations, check_design_records, check_feature_input_lanes,
    check_native_ids, check_native_links,
};
use carriers_parameterization::{check_carrier_reachability, check_parameter_domains};
use geometry_payloads::{check_bounds, check_tessellations, check_unknown_payloads};
use identity_order::{check_identity_and_order, check_version, collect_native_ids, entity_counts};
use topology::{
    check_coedge_pairing, check_loops, check_references, check_units, check_wire_topology, IdSets,
};

/// A radius/length that is not a finite positive number is invalid geometry.
/// Written without a negated comparison operator so it stays clippy-clean while
/// still rejecting NaN and non-positive values.
fn nonpositive(x: f64) -> bool {
    !(x.is_finite() && x > 0.0)
}

/// Validate `ir`, returning a report. `losses` are propagated into the report
/// unchanged (e.g. loss notes from the decode that produced `ir`).
pub fn validate(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    let mut findings = Vec::new();

    // Serialize the IR once and index every id-bearing object; the annotation
    // and native-link checks resolve ids against this index instead of
    // re-walking the serialized tree per lookup.
    let json_ir = serde_json::to_value(ir).ok();
    let entity_index = json_ir.as_ref().map(build_entity_index);

    let ids = IdSets::build(ir);
    check_version(ir, &mut findings);
    check_identity_and_order(ir, &mut findings);
    check_units(ir, &mut findings);
    check_references(ir, &ids, &mut findings);
    check_loops(ir, &mut findings);
    check_coedge_pairing(ir, &mut findings);
    check_wire_topology(ir, &mut findings);
    check_carrier_reachability(ir, &mut findings);
    check_annotations(ir, entity_index.as_ref(), &mut findings);
    check_native_links(ir, entity_index.as_ref(), &mut findings);
    check_parameter_domains(ir, &mut findings);
    check_bounds(ir, &mut findings);
    check_tessellations(ir, &mut findings);
    check_feature_input_lanes(ir, &mut findings);
    check_design_records(ir, &mut findings);
    check_native_ids(ir, &mut findings);
    check_unknown_payloads(ir, &mut findings);

    ValidationReport {
        entity_counts: entity_counts(ir),
        findings,
        losses,
    }
}
