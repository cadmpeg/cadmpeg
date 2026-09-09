// SPDX-License-Identifier: Apache-2.0
//! Surface, curve, sketch-segment, and design-constraint transfer coverage.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::sketches::{SketchConstraint, SketchConstraintDefinitionInput};

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
                if matches!(
                    scan.framing.layout,
                    crate::container::Layout::LegacyAscii(_)
                ) {
                    "legacy_ascii"
                } else {
                    "unknown"
                }
            },
            |section| section.name(),
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
        crate::surface::SurfaceKind::Extrusion(_) => "extrusion",
    }
}

pub(crate) const SURFACE_KINDS: [crate::surface::SurfaceKind; 7] = [
    crate::surface::SurfaceKind::Plane,
    crate::surface::SurfaceKind::Cylinder,
    crate::surface::SurfaceKind::Cone,
    crate::surface::SurfaceKind::TorusOrSphere,
    crate::surface::SurfaceKind::Spline,
    crate::surface::SurfaceKind::Fillet,
    crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
];

const fn surface_family_index(kind: crate::surface::SurfaceKind) -> usize {
    match kind {
        crate::surface::SurfaceKind::Plane => 0,
        crate::surface::SurfaceKind::Cylinder => 1,
        crate::surface::SurfaceKind::Cone => 2,
        crate::surface::SurfaceKind::TorusOrSphere => 3,
        crate::surface::SurfaceKind::Spline => 4,
        crate::surface::SurfaceKind::Fillet => 5,
        crate::surface::SurfaceKind::Extrusion(_) => 6,
    }
}

#[derive(Default)]
pub(crate) struct SurfaceTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    retained_unknown_rows: usize,
    ambiguous_rows: usize,
    by_family: [(usize, usize); 7],
    unknown_by_family: [usize; 7],
}

impl SurfaceTransferCoverage {
    /// Recorded unique rows.
    pub(crate) fn unique_rows(&self) -> usize {
        self.unique_rows
    }

    /// Recorded transferred rows.
    pub(crate) fn transferred_rows(&self) -> usize {
        self.transferred_rows
    }

    /// Recorded retained unknown rows.
    pub(crate) fn retained_unknown_rows(&self) -> usize {
        self.retained_unknown_rows
    }

    /// Recorded ambiguous rows.
    pub(crate) fn ambiguous_rows(&self) -> usize {
        self.ambiguous_rows
    }

    fn record_ambiguous_rows(&mut self, count: usize) {
        self.ambiguous_rows += count;
    }

    pub(crate) fn family(&self, kind: crate::surface::SurfaceKind) -> (usize, usize) {
        self.by_family[surface_family_index(kind)]
    }

    pub(crate) fn unknown_family(&self, kind: crate::surface::SurfaceKind) -> usize {
        self.unknown_by_family[surface_family_index(kind)]
    }

    fn record_source_row(&mut self, kind: crate::surface::SurfaceKind) {
        self.unique_rows += 1;
        self.by_family[surface_family_index(kind)].0 += 1;
    }

    fn record_transferred_row(&mut self, kind: crate::surface::SurfaceKind) {
        self.transferred_rows += 1;
        self.by_family[surface_family_index(kind)].1 += 1;
    }

    fn record_retained_unknown_row(&mut self, kind: crate::surface::SurfaceKind) {
        self.retained_unknown_rows += 1;
        self.unknown_by_family[surface_family_index(kind)] += 1;
    }
}

#[derive(Default)]
pub(crate) struct CurveTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    retained_unknown_rows: usize,
    ambiguous_rows: usize,
    by_type: BTreeMap<u8, (usize, usize)>,
    unknown_by_type: BTreeMap<u8, usize>,
}

impl CurveTransferCoverage {
    /// Recorded unique rows.
    pub(crate) fn unique_rows(&self) -> usize {
        self.unique_rows
    }

    /// Recorded transferred rows.
    pub(crate) fn transferred_rows(&self) -> usize {
        self.transferred_rows
    }

    /// Recorded retained unknown rows.
    pub(crate) fn retained_unknown_rows(&self) -> usize {
        self.retained_unknown_rows
    }

    /// Recorded ambiguous rows.
    pub(crate) fn ambiguous_rows(&self) -> usize {
        self.ambiguous_rows
    }

    fn record_ambiguous_rows(&mut self, count: usize) {
        self.ambiguous_rows += count;
    }

    fn record_source_row(&mut self, type_byte: u8) {
        self.unique_rows += 1;
        self.by_type.entry(type_byte).or_default().0 += 1;
        self.unknown_by_type.entry(type_byte).or_default();
    }

    fn record_transferred_row(&mut self, type_byte: u8) {
        self.transferred_rows += 1;
        self.by_type.entry(type_byte).or_default().1 += 1;
    }

    fn record_retained_unknown_row(&mut self, type_byte: u8) {
        self.retained_unknown_rows += 1;
        *self.unknown_by_type.entry(type_byte).or_default() += 1;
    }

    /// Source and transferred counts by native curve type.
    pub(crate) fn by_type(&self) -> &BTreeMap<u8, (usize, usize)> {
        &self.by_type
    }

    /// Retained unknown counts by native curve type.
    pub(crate) fn unknown_by_type(&self) -> &BTreeMap<u8, usize> {
        &self.unknown_by_type
    }
}

#[derive(Default)]
pub(crate) struct SketchSegmentTransferCoverage {
    decoded_rows: usize,
    resolved_geometry: usize,
    missing_rows: usize,
    by_family: [Option<(usize, usize)>; 9],
}

impl SketchSegmentTransferCoverage {
    /// Records decoded and missing rows for a segment table.
    pub(crate) fn record_table_rows(&mut self, decoded: usize, expected: usize) {
        self.decoded_rows += decoded;
        self.missing_rows += expected.saturating_sub(decoded);
    }

    /// Records decoded rows in one segment family.
    pub(crate) fn record_family_rows(
        &mut self,
        family: crate::coverage::SketchSegmentFamily,
        count: usize,
    ) {
        self.family_mut(family).0 += count;
    }

    /// Records resolved geometry instances.
    pub(crate) fn record_resolved_geometry(&mut self, count: usize) {
        self.resolved_geometry += count;
    }

    /// Records resolved rows in one segment family.
    pub(crate) fn record_family_resolution(
        &mut self,
        family: crate::coverage::SketchSegmentFamily,
        count: usize,
    ) {
        self.family_mut(family).1 += count;
    }

    /// Recorded decoded rows.
    pub(crate) fn decoded_rows(&self) -> usize {
        self.decoded_rows
    }

    /// Recorded resolved geometry.
    pub(crate) fn resolved_geometry(&self) -> usize {
        self.resolved_geometry
    }

    /// Recorded missing rows.
    pub(crate) fn missing_rows(&self) -> usize {
        self.missing_rows
    }

    pub(crate) fn families(
        &self,
    ) -> impl Iterator<Item = (crate::coverage::SketchSegmentFamily, (usize, usize))> + '_ {
        crate::coverage::SketchSegmentFamily::ALL
            .into_iter()
            .zip(self.by_family)
            .filter_map(|(family, counts)| counts.map(|counts| (family, counts)))
    }

    fn family_mut(&mut self, family: crate::coverage::SketchSegmentFamily) -> &mut (usize, usize) {
        self.by_family[family.index()].get_or_insert((0, 0))
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
        .filter(|constraint| constraint.id.as_str().contains(id_marker))
        .fold(
            DesignConstraintTransferCoverage::default(),
            |mut coverage, constraint| {
                coverage.transferred += 1;
                let native_kind_text = match constraint.definition.kind() {
                    SketchConstraintDefinitionInput::Native { native_kind, .. }
                        if native_kind.as_str().starts_with(native_kind_prefix) =>
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
                .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)?
                .object_id
                .as_str()
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
                .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)?
                .object_id
                .as_str()
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    let mut coverage = CurveTransferCoverage::default();
    coverage.record_ambiguous_rows(rows.len().saturating_sub(unique_rows.len()));
    for row in unique_rows {
        coverage.record_source_row(row.type_byte);
        if transferred_ids.contains(&row.id) {
            coverage.record_transferred_row(row.type_byte);
        }
        if unknown_ids.contains(&row.id) {
            coverage.record_retained_unknown_row(row.type_byte);
        }
    }
    coverage
}

pub(crate) fn surface_transfer_coverage(
    rows: &[crate::surface::SurfaceRow],
    surfaces: &[Surface],
    procedural_surfaces: &[ProceduralSurface],
) -> SurfaceTransferCoverage {
    let unique_rows = crate::surface::uniquely_identified_rows(rows);
    let extrusion_constructions = procedural_surfaces
        .iter()
        .filter(|procedural| {
            matches!(
                procedural.definition(),
                ProceduralSurfaceDefinition::Extrusion { .. }
            )
        })
        .map(|procedural| &procedural.id)
        .collect::<BTreeSet<_>>();
    let extrusion_surfaces = surfaces
        .iter()
        .filter(|surface| {
            surface
                .geometry
                .procedural_construction()
                .is_some_and(|id| extrusion_constructions.contains(id))
        })
        .map(|surface| &surface.id)
        .collect::<BTreeSet<_>>();
    let transferred = surfaces
        .iter()
        .filter_map(|surface| {
            let id = surface
                .source_object
                .as_ref()
                .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)?
                .object_id
                .as_str()
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()?;
            let mut kinds = vec![surface_kind_for_geometry(&surface.geometry)?];
            if extrusion_surfaces.contains(&surface.id) {
                kinds.push(crate::surface::SurfaceKind::Extrusion(
                    crate::surface::ExtrusionVariant::Linear,
                ));
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
                .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)?
                .object_id
                .as_str()
                .strip_prefix("VisibGeom:")?
                .parse::<u32>()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    let mut coverage = SurfaceTransferCoverage::default();
    coverage.record_ambiguous_rows(rows.len().saturating_sub(unique_rows.len()));
    for row in unique_rows {
        coverage.record_source_row(row.kind);
        if transferred
            .iter()
            .any(|(id, kinds)| *id == row.id && kinds.iter().any(|kind| kind.same_family(row.kind)))
        {
            coverage.record_transferred_row(row.kind);
        }
        if unknown_ids.contains(&row.id) {
            coverage.record_retained_unknown_row(row.kind);
        }
    }
    coverage
}

pub(crate) fn surface_variant(kind: crate::surface::SurfaceKind) -> Option<&'static str> {
    match kind {
        crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear) => {
            Some("ruled_surface")
        }
        crate::surface::SurfaceKind::Extrusion(
            crate::surface::ExtrusionVariant::TabulatedCylinder,
        ) => Some("tabulated_cylinder"),
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
        crate::surface::SurfacePrototypeFamily::Torus(_) => "torus_or_sphere".to_string(),
        crate::surface::SurfacePrototypeFamily::Spline(_) => "spline".to_string(),
        crate::surface::SurfacePrototypeFamily::Fillet(_) => "fillet".to_string(),
        crate::surface::SurfacePrototypeFamily::Extrusion(_) => "extrusion".to_string(),
        crate::surface::SurfacePrototypeFamily::Other(name) => format!("other:{name}"),
    }
}

pub(super) fn surface_named_parameter_record(
    parameter: &crate::surface::SurfaceNamedParameter,
) -> CreoSurfaceNamedParameterRecord {
    CreoSurfaceNamedParameterRecord {
        name: parameter.name.clone(),
        value: parameter.value.clone(),
        body: parameter.body.clone(),
        offset: parameter.offset,
        value_offset: parameter.value_offset,
    }
}
