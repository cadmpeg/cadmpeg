// SPDX-License-Identifier: Apache-2.0
//! Surface, curve, sketch-segment, and design-constraint transfer coverage.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::sketches::{SketchConstraint, SketchConstraintDefinition};

use crate::container::ContainerScan;

use super::feature_history::surface_kind_for_geometry;
use super::records::CreoSurfaceNamedParameterRecord;

pub(crate) fn source_section(scan: &ContainerScan, offset: usize) -> String {
    scan.framing
        .sections
        .iter()
        .find(|section| offset >= section.offset && offset < section.offset + section.length)
        .map_or_else(
            || {
                if scan.framing.layout == crate::container::Layout::LegacyAscii {
                    "legacy_ascii"
                } else {
                    "unknown"
                }
            },
            |section| section.name.as_str(),
        )
        .to_string()
}

pub(crate) fn surface_family(kind: crate::surface::SurfaceKind) -> &'static str {
    match kind {
        crate::surface::SurfaceKind::Plane => "plane",
        crate::surface::SurfaceKind::Cylinder => "cylinder",
        crate::surface::SurfaceKind::Cone => "cone",
        crate::surface::SurfaceKind::TorusOrSphere => "torus_or_sphere",
        crate::surface::SurfaceKind::Spline => "spline",
        crate::surface::SurfaceKind::Fillet => "fillet",
        crate::surface::SurfaceKind::Extrusion => "extrusion",
    }
}

pub(crate) const SURFACE_KINDS: [crate::surface::SurfaceKind; 7] = [
    crate::surface::SurfaceKind::Plane,
    crate::surface::SurfaceKind::Cylinder,
    crate::surface::SurfaceKind::Cone,
    crate::surface::SurfaceKind::TorusOrSphere,
    crate::surface::SurfaceKind::Spline,
    crate::surface::SurfaceKind::Fillet,
    crate::surface::SurfaceKind::Extrusion,
];

const fn surface_family_index(kind: crate::surface::SurfaceKind) -> usize {
    match kind {
        crate::surface::SurfaceKind::Plane => 0,
        crate::surface::SurfaceKind::Cylinder => 1,
        crate::surface::SurfaceKind::Cone => 2,
        crate::surface::SurfaceKind::TorusOrSphere => 3,
        crate::surface::SurfaceKind::Spline => 4,
        crate::surface::SurfaceKind::Fillet => 5,
        crate::surface::SurfaceKind::Extrusion => 6,
    }
}

#[derive(Default)]
pub(crate) struct SurfaceTransferCoverage {
    pub(crate) unique_rows: usize,
    pub(crate) transferred_rows: usize,
    pub(crate) retained_unknown_rows: usize,
    pub(crate) ambiguous_rows: usize,
    by_family: [(usize, usize); 7],
    unknown_by_family: [usize; 7],
}

impl SurfaceTransferCoverage {
    pub(crate) fn family(&self, kind: crate::surface::SurfaceKind) -> (usize, usize) {
        self.by_family[surface_family_index(kind)]
    }

    fn family_mut(&mut self, kind: crate::surface::SurfaceKind) -> &mut (usize, usize) {
        &mut self.by_family[surface_family_index(kind)]
    }

    pub(crate) fn unknown_family(&self, kind: crate::surface::SurfaceKind) -> usize {
        self.unknown_by_family[surface_family_index(kind)]
    }

    fn unknown_family_mut(&mut self, kind: crate::surface::SurfaceKind) -> &mut usize {
        &mut self.unknown_by_family[surface_family_index(kind)]
    }
}

#[derive(Default)]
pub(crate) struct CurveTransferCoverage {
    pub(crate) unique_rows: usize,
    pub(crate) transferred_rows: usize,
    pub(crate) retained_unknown_rows: usize,
    pub(crate) ambiguous_rows: usize,
    pub(crate) by_type: BTreeMap<u8, (usize, usize)>,
    pub(crate) unknown_by_type: BTreeMap<u8, usize>,
}

#[derive(Default)]
pub(crate) struct SketchSegmentTransferCoverage {
    pub(crate) decoded_rows: usize,
    pub(crate) resolved_geometry: usize,
    pub(crate) missing_rows: usize,
    by_family: [(usize, usize); 9],
}

impl SketchSegmentTransferCoverage {
    pub(crate) fn family(&self, family: crate::coverage::SketchSegmentFamily) -> (usize, usize) {
        self.by_family[family.index()]
    }

    pub(crate) fn family_mut(
        &mut self,
        family: crate::coverage::SketchSegmentFamily,
    ) -> &mut (usize, usize) {
        &mut self.by_family[family.index()]
    }
}

#[derive(Default)]
pub(crate) struct DesignConstraintTransferCoverage {
    pub(crate) transferred: usize,
    pub(crate) native: usize,
    pub(crate) active: usize,
    pub(crate) active_native: usize,
    pub(crate) native_by_kind: BTreeMap<u32, usize>,
    pub(crate) active_native_by_kind: BTreeMap<u32, usize>,
}

impl DesignConstraintTransferCoverage {
    pub(crate) fn typed(&self) -> usize {
        self.transferred.saturating_sub(self.native)
    }

    pub(crate) fn active_typed(&self) -> usize {
        self.active.saturating_sub(self.active_native)
    }
}

pub(crate) fn design_constraint_transfer_coverage(
    constraints: &[SketchConstraint],
    id_marker: &str,
    native_kind_prefix: &str,
) -> DesignConstraintTransferCoverage {
    constraints
        .iter()
        .filter(|constraint| constraint.id.0.contains(id_marker))
        .fold(
            DesignConstraintTransferCoverage::default(),
            |mut coverage, constraint| {
                coverage.transferred += 1;
                let native_kind_text = match &constraint.definition {
                    SketchConstraintDefinition::Native { native_kind, .. }
                        if native_kind.starts_with(native_kind_prefix) =>
                    {
                        Some(native_kind.as_str())
                    }
                    _ => None,
                };
                let native_kind = native_kind_text
                    .and_then(|kind| kind.strip_prefix(native_kind_prefix))
                    .and_then(|kind| kind.parse().ok());
                if native_kind_text.is_some() {
                    coverage.native += 1;
                }
                if let Some(native_kind) = native_kind {
                    *coverage.native_by_kind.entry(native_kind).or_default() += 1;
                    if constraint.active == Some(true) {
                        *coverage
                            .active_native_by_kind
                            .entry(native_kind)
                            .or_default() += 1;
                    }
                }
                if constraint.active == Some(true) {
                    coverage.active += 1;
                    if native_kind_text.is_some() {
                        coverage.active_native += 1;
                    }
                }
                coverage
            },
        )
}

pub(crate) fn constraint_kind_breakdown(coverage: &cadmpeg_ir::Coverage, prefix: &str) -> String {
    coverage
        .iter()
        .filter_map(|(key, count)| {
            let kind = key
                .strip_prefix(prefix)?
                .strip_suffix("_constraint_count")?;
            (*count != 0).then_some(format!("type {kind}={count}"))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn curve_transfer_coverage(
    rows: &[crate::curve::CurveTopologyRow],
    curves: &[Curve],
) -> CurveTransferCoverage {
    let unique_rows = crate::topology::uniquely_identified_rows(rows);
    let transferred_ids = curves
        .iter()
        .filter(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. }))
        .filter_map(|curve| {
            curve
                .source_object
                .as_ref()
                .filter(|source| source.format == "creo")?
                .object_id
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    let unknown_ids = curves
        .iter()
        .filter(|curve| matches!(curve.geometry, CurveGeometry::Unknown { .. }))
        .filter_map(|curve| {
            curve
                .source_object
                .as_ref()
                .filter(|source| source.format == "creo")?
                .object_id
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    let mut coverage = CurveTransferCoverage {
        unique_rows: unique_rows.len(),
        ambiguous_rows: rows.len().saturating_sub(unique_rows.len()),
        ..CurveTransferCoverage::default()
    };
    for row in unique_rows {
        let transferred = usize::from(transferred_ids.contains(&row.id));
        let retained_unknown = usize::from(unknown_ids.contains(&row.id));
        coverage.transferred_rows += transferred;
        coverage.retained_unknown_rows += retained_unknown;
        let type_coverage = coverage.by_type.entry(row.type_byte).or_default();
        type_coverage.0 += 1;
        type_coverage.1 += transferred;
        *coverage.unknown_by_type.entry(row.type_byte).or_default() += retained_unknown;
    }
    coverage
}

pub(crate) fn surface_transfer_coverage(
    rows: &[crate::surface::SurfaceRow],
    surfaces: &[Surface],
    procedural_surfaces: &[ProceduralSurface],
) -> SurfaceTransferCoverage {
    let unique_rows = crate::surface::uniquely_identified_rows(rows);
    let extrusion_surfaces = procedural_surfaces
        .iter()
        .filter(|procedural| {
            matches!(
                procedural.definition,
                ProceduralSurfaceDefinition::Extrusion { .. }
            )
        })
        .map(|procedural| &procedural.surface)
        .collect::<BTreeSet<_>>();
    let transferred = surfaces
        .iter()
        .filter_map(|surface| {
            let id = surface
                .source_object
                .as_ref()
                .filter(|source| source.format == "creo")?
                .object_id
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()?;
            let mut kinds = vec![surface_kind_for_geometry(&surface.geometry)?];
            if extrusion_surfaces.contains(&surface.id) {
                kinds.push(crate::surface::SurfaceKind::Extrusion);
            }
            Some((id, kinds))
        })
        .collect::<Vec<_>>();
    let unknown_ids = surfaces
        .iter()
        .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. }))
        .filter_map(|surface| {
            surface
                .source_object
                .as_ref()
                .filter(|source| source.format == "creo")?
                .object_id
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    let mut coverage = SurfaceTransferCoverage {
        unique_rows: unique_rows.len(),
        ambiguous_rows: rows.len().saturating_sub(unique_rows.len()),
        ..SurfaceTransferCoverage::default()
    };
    for row in unique_rows {
        let is_transferred = transferred
            .iter()
            .any(|(id, kinds)| *id == row.id && kinds.contains(&row.kind));
        let retained_unknown = unknown_ids.contains(&row.id);
        coverage.transferred_rows += usize::from(is_transferred);
        coverage.retained_unknown_rows += usize::from(retained_unknown);
        let family_coverage = coverage.family_mut(row.kind);
        family_coverage.0 += 1;
        family_coverage.1 += usize::from(is_transferred);
        *coverage.unknown_family_mut(row.kind) += usize::from(retained_unknown);
    }
    coverage
}

pub(crate) fn surface_variant(type_byte: u8) -> Option<&'static str> {
    match type_byte {
        0x2a => Some("ruled_surface"),
        0x2c => Some("tabulated_cylinder"),
        _ => None,
    }
}

pub(crate) fn surface_prototype_family_name(
    family: &crate::surface::SurfacePrototypeFamily,
) -> String {
    match family {
        crate::surface::SurfacePrototypeFamily::Plane => "plane".to_string(),
        crate::surface::SurfacePrototypeFamily::Cylinder => "cylinder".to_string(),
        crate::surface::SurfacePrototypeFamily::Cone => "cone".to_string(),
        crate::surface::SurfacePrototypeFamily::Torus => "torus_or_sphere".to_string(),
        crate::surface::SurfacePrototypeFamily::Spline => "spline".to_string(),
        crate::surface::SurfacePrototypeFamily::Fillet => "fillet".to_string(),
        crate::surface::SurfacePrototypeFamily::Extrusion => "extrusion".to_string(),
        crate::surface::SurfacePrototypeFamily::Other(name) => format!("other:{name}"),
    }
}

pub(super) fn surface_named_parameter_record(
    parameter: &crate::surface::SurfaceNamedParameter,
) -> CreoSurfaceNamedParameterRecord {
    let (
        value_kind,
        compact_values,
        scalar_dimensions,
        scalar_count,
        scalar_values,
        scalar_tokens,
        opaque,
    ) = match &parameter.value {
        crate::surface::SurfaceNamedValue::Empty => (
            "empty",
            Vec::new(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::CompactInt(value) => (
            "compact_int",
            vec![*value],
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::CompactIntArray(values) => (
            "compact_int_array",
            values.clone(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::ContiguousEntityReferences { entity_ids, .. } => (
            "contiguous_entity_references",
            entity_ids.clone(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::ScalarArray {
            dimensions,
            count,
            values,
            tokens,
        } => (
            "scalar_array",
            Vec::new(),
            Some(*dimensions),
            Some(*count),
            values.clone(),
            tokens.clone(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::CountedScalarArray {
            count,
            values,
            tokens,
        } => (
            "counted_scalar_array",
            Vec::new(),
            None,
            Some(*count),
            values.clone(),
            tokens.clone(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::ScalarSequence(values) => (
            "scalar_sequence",
            Vec::new(),
            None,
            None,
            values.iter().copied().map(Some).collect(),
            Vec::new(),
            Vec::new(),
        ),
        crate::surface::SurfaceNamedValue::Opaque(value) => (
            "opaque",
            Vec::new(),
            None,
            None,
            Vec::new(),
            Vec::new(),
            value.clone(),
        ),
    };
    CreoSurfaceNamedParameterRecord {
        name: parameter.name.clone(),
        value_kind,
        compact_values,
        scalar_dimensions,
        scalar_count,
        scalar_values,
        scalar_tokens,
        opaque,
        body: parameter.body.clone(),
        offset: parameter.offset,
        value_offset: parameter.value_offset,
    }
}
