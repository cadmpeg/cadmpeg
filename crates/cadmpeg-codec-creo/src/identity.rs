// SPDX-License-Identifier: Apache-2.0
//! Checked identity namespaces used by the Creo decoder, and the row
//! uniqueness a namespace identity depends on.

use std::collections::BTreeMap;
use std::fmt::Display;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{IdentityError, IdentityNamespace};

/// Build a typed identity after admitting its exact retained text length.
pub(crate) fn compose_checked<I>(
    ctx: &DecodeContext<'_>,
    namespace: &IdentityNamespace,
    key: impl Display,
    operation: &'static str,
) -> Result<I, CodecError>
where
    I: TryFrom<String, Error = IdentityError>,
{
    let text = ctx.format_retained(
        format_args!(
            "{}:{}:{}#{key}",
            namespace.format(),
            namespace.scope(),
            namespace.kind()
        ),
        operation,
    )?;
    I::try_from(text).map_err(|_| CodecError::malformed("generated Creo identity is invalid"))
}

/// Copy an existing typed identity after admitting its retained text.
pub(crate) fn copy_checked_id<I>(
    ctx: &DecodeContext<'_>,
    source: &str,
    operation: &'static str,
) -> Result<I, CodecError>
where
    I: TryFrom<String, Error = IdentityError>,
{
    I::try_from(ctx.copy_retained_text(source, operation)?)
        .map_err(|_| CodecError::malformed("copied Creo identity is invalid"))
}

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

/// Return unique native rows after admitting the count map and selected rows.
pub(crate) fn uniquely_identified_rows_checked<'a, T>(
    ctx: &DecodeContext<'_>,
    rows: &'a [T],
    id: impl Fn(&T) -> u32,
) -> Result<Vec<&'a T>, CodecError> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        match counts.entry(id(row)) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo unique-row count nodes")?;
                entry.insert(1);
            }
        }
    }
    let mut unique = Vec::new();
    for row in rows {
        if counts.get(&id(row)) == Some(&1) {
            ctx.try_reserve_items(&mut unique, 1, "creo unique-row projection")?;
            unique.push(row);
        }
    }
    Ok(unique)
}

/// Compare a numbered identity without constructing a temporary identity string.
pub(crate) fn matches_numbered_identity(actual: &str, prefix: &str, number: u32) -> bool {
    let Some(suffix) = actual.strip_prefix(prefix) else {
        return false;
    };
    let mut digits = 1;
    let mut remaining = number;
    while remaining >= 10 {
        remaining /= 10;
        digits += 1;
    }
    suffix.len() == digits && suffix.parse::<u32>().ok() == Some(number)
}

#[cfg(test)]
mod tests {
    use super::{compose_checked, copy_checked_id, matches_numbered_identity};

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::ids::ShellId;

    #[test]
    fn composed_identity_refuses_retained_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let error = compose_checked::<ShellId>(
            &ctx,
            &crate::identity::VISIBGEOM_SHELL,
            format_args!("1:{}", 2),
            "creo B-rep shell identity",
        )
        .err()
        .expect("identity text refused");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo B-rep shell identity"));
    }

    #[test]
    fn checked_identity_preserves_compose_and_copy() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let composed = compose_checked::<ShellId>(
            &ctx,
            &crate::identity::VISIBGEOM_SHELL,
            format_args!("1:{}", 2),
            "creo B-rep shell identity",
        )
        .expect("service identity admitted");
        let copied = copy_checked_id::<ShellId>(
            &ctx,
            composed.as_str(),
            "creo B-rep shell identity copies",
        )
        .expect("service identity copy admitted");
        assert_eq!(composed, ShellId::compose(
            &crate::identity::VISIBGEOM_SHELL,
            cadmpeg_ir::ids::IdentityKey::from(1).colon(2),
        ));
        assert_eq!(copied, composed);
    }

    #[test]
    fn numbered_identity_match_requires_canonical_decimal_bytes() {
        let prefix = "creo:visibgeom:surface#";
        assert!(matches_numbered_identity("creo:visibgeom:surface#0", prefix, 0));
        assert!(matches_numbered_identity("creo:visibgeom:surface#4294967295", prefix, u32::MAX));
        assert!(!matches_numbered_identity("creo:visibgeom:surface#01", prefix, 1));
        assert!(!matches_numbered_identity("creo:visibgeom:surface#+1", prefix, 1));
        assert!(!matches_numbered_identity("creo:visibgeom:face#1", prefix, 1));
    }
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
