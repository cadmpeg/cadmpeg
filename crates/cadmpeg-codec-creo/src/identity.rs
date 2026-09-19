// SPDX-License-Identifier: Apache-2.0
//! Checked identity namespaces used by the Creo decoder, and the row
//! uniqueness a namespace identity depends on.

use std::collections::BTreeMap;

use cadmpeg_ir::ids::IdentityNamespace;

/// Return the rows whose native identifier, read by `id`, occurs exactly once.
///
/// A repeated identifier names no single row, so the namespace identity it
/// would carry is not established and the row is left out.
pub(crate) fn uniquely_identified_rows<T>(rows: &[T], id: impl Fn(&T) -> u32) -> Vec<&T> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        *counts.entry(id(row)).or_default() += 1;
    }
    rows.iter()
        .filter(|row| counts.get(&id(row)) == Some(&1))
        .collect()
}

pub(crate) const VISIBGEOM_BODY: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "body");
pub(crate) const VISIBGEOM_COEDGE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "coedge");
pub(crate) const VISIBGEOM_CURVE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "curve");
pub(crate) const VISIBGEOM_EDGE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "edge");
pub(crate) const VISIBGEOM_FACE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "face");
pub(crate) const VISIBGEOM_LOOP: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "loop");
pub(crate) const VISIBGEOM_POINT: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "point");
pub(crate) const VISIBGEOM_PCURVE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "pcurve");
pub(crate) const VISIBGEOM_REGION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "region");
pub(crate) const VISIBGEOM_SHELL: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "shell");
pub(crate) const VISIBGEOM_SURFACE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "surface");
pub(crate) const VISIBGEOM_VERTEX: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "vertex");

pub(crate) const NOVISGEOM_SURFACE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "novisgeom", "surface");
pub(crate) const ACTDATUM_SURFACE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "actdatums", "surface");

pub(crate) const MODEL_FEATURE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "feature");
pub(crate) const MODEL_FEATURE_RESULT_TOPOLOGY: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "feature-result-topology");
pub(crate) const MODEL_OCCURRENCE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "occurrence");
pub(crate) const MODEL_PRODUCT_DEFINITION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "product_definition");
pub(crate) const MODEL_SKETCH: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "sketch");
pub(crate) const MODEL_SKETCH_FEATURE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "sketch_feature");

pub(crate) const FEATURE_EXTRUSION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "extrusion");
pub(crate) const FEATURE_REVOLUTION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "revolution");
pub(crate) const FEATURE_EXTRUSION_CONSTRUCTION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "extrusion_construction");
pub(crate) const FEATURE_EXTRUSION_DIRECTRIX: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "extrusion_directrix");
pub(crate) const FEATURE_EXTRUSION_VERTEX_ORBIT: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "extrusion_vertex_orbit");
pub(crate) const FEATURE_REVOLUTION_CONSTRUCTION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "revolution_construction");
pub(crate) const FEATURE_REVOLUTION_SURFACE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "revolution_surface");
pub(crate) const FEATURE_REVOLUTION_VERTEX_ORBIT: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "feature", "revolution_vertex_orbit");

pub(crate) const DEPDB_CURVE_EXPRESSION_FEATURE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "depdb", "curve_expression_feature");
pub(crate) const DEPDB_CURVE_EXPRESSION_PARAMETER: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "depdb", "curve_expression_parameter");
pub(crate) const DEPDB_CURVE_EXPRESSION_CURVE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "depdb", "curve_expression_curve");
pub(crate) const DEPDB_CURVE_EXPRESSION_HELIX: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "depdb", "curve_expression_helix");

pub(crate) const FEATDEFS_SAVED_SPLINE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "featdefs", "saved_spline");
pub(crate) const FEATDEFS_SAVED_SPLINE_CURVE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "featdefs", "saved_spline_curve");
pub(crate) const FEATDEFS_SAVED_DUMMY: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "featdefs", "saved_dummy");
pub(crate) const FEATDEFS_SKETCH_ENTITY: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "featdefs", "sketch_entity");
pub(crate) const FEATDEFS_SKETCH_CONSTRAINT: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "featdefs", "sketch_constraint");
pub(crate) const FEATDEFS_SECTION_CURVE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "featdefs", "section_curve");
pub(crate) const FEATDEFS_PARAMETER: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "featdefs", "parameter");

pub(crate) const MDL_REF_INFO_ARC_Z: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "mdl_ref_info", "arc_z");
pub(crate) const MDL_REF_INFO_CONIC: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "mdl_ref_info", "conic");
pub(crate) const MDL_REF_INFO_LINE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "mdl_ref_info", "line");
pub(crate) const MDL_REF_INFO_LINE3D: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "mdl_ref_info", "line3d");

pub(crate) const CROSS_SECTION_GEOMETRY_SURFACE: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "cross_section_geometry", "surface");

pub(crate) const VISIBGEOM_SURFACE_DIRECTRIX: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "surface_directrix");
pub(crate) const VISIBGEOM_SURFACE_EXTRUSION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "surface_extrusion");
pub(crate) const VISIBGEOM_TABULATED_DIRECTRIX: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "tabulated_directrix");
pub(crate) const VISIBGEOM_TABULATED_EXTRUSION: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "visibgeom", "tabulated_extrusion");
