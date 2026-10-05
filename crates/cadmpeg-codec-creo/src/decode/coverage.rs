// SPDX-License-Identifier: Apache-2.0
//! Surface, curve, sketch-segment, and design-constraint transfer coverage.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::sketches::{SketchConstraint, SketchConstraintDefinitionInput};

use crate::container::ContainerScan;

use super::feature_history::link::surface_kind_for_geometry;

fn charged_map_entry<'a, K: Ord + cadmpeg_core::decode::cost::DecodeCost, V: Default>(
    ctx: &DecodeContext<'_>,
    map: &'a mut BTreeMap<K, V>,
    key: K,
    operation: &'static str,
) -> Result<&'a mut V, CodecError> {
    match ctx.entry_btree_map(map, key, operation)? {
        std::collections::btree_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::btree_map::Entry::Vacant(entry) => Ok(entry.insert(V::default())),
    }
}

pub(super) fn source_section(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    offset: usize,
) -> Result<String, CodecError> {
    ctx.copy_retained_text(
        source_section_ref(ctx, scan, offset)?,
        "creo expression source section",
    )
}

pub(super) fn source_section_ref<'a>(ctx: &DecodeContext<'_>, scan: &'a ContainerScan<'_>, offset: usize) -> Result<&'a str, CodecError> {
    match ctx.find_by(
        &scan.framing.sections,
        |section| Ok(section.contains(offset)),
        "creo source section search",
    )? {
        Some(section) => Ok(section.name()),
        None => Ok(if matches!(scan.framing.layout, crate::container::Layout::LegacyAscii(_)) {
            "legacy_ascii"
        } else {
            "unknown"
        }),
    }
}

pub(super) fn surface_family(kind: crate::surface::SurfaceKind) -> &'static str {
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

pub(super) const SURFACE_KINDS: [crate::surface::SurfaceKind; 7] = [
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
pub(in crate::decode) struct SurfaceTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    retained_unknown_rows: usize,
    ambiguous_rows: usize,
    by_family: [(usize, usize); 7],
    unknown_by_family: [usize; 7],
}

impl SurfaceTransferCoverage {
    /// Recorded unique rows.
    pub(super) fn unique_rows(&self) -> usize {
        self.unique_rows
    }

    /// Recorded transferred rows.
    pub(super) fn transferred_rows(&self) -> usize {
        self.transferred_rows
    }

    /// Recorded retained unknown rows.
    pub(super) fn retained_unknown_rows(&self) -> usize {
        self.retained_unknown_rows
    }

    /// Recorded ambiguous rows.
    pub(super) fn ambiguous_rows(&self) -> usize {
        self.ambiguous_rows
    }

    fn record_ambiguous_rows(&mut self, count: usize) {
        self.ambiguous_rows += count;
    }

    pub(super) fn family(&self, kind: crate::surface::SurfaceKind) -> (usize, usize) {
        self.by_family[surface_family_index(kind)]
    }

    pub(super) fn unknown_family(&self, kind: crate::surface::SurfaceKind) -> usize {
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
pub(in crate::decode) struct CurveTransferCoverage {
    unique_rows: usize,
    transferred_rows: usize,
    retained_unknown_rows: usize,
    ambiguous_rows: usize,
    by_type: BTreeMap<u8, (usize, usize)>,
    unknown_by_type: BTreeMap<u8, usize>,
}

impl CurveTransferCoverage {
    /// Recorded unique rows.
    pub(super) fn unique_rows(&self) -> usize {
        self.unique_rows
    }

    /// Recorded transferred rows.
    pub(super) fn transferred_rows(&self) -> usize {
        self.transferred_rows
    }

    /// Recorded retained unknown rows.
    pub(super) fn retained_unknown_rows(&self) -> usize {
        self.retained_unknown_rows
    }

    /// Recorded ambiguous rows.
    pub(super) fn ambiguous_rows(&self) -> usize {
        self.ambiguous_rows
    }

    fn record_ambiguous_rows(&mut self, count: usize) {
        self.ambiguous_rows += count;
    }

    fn record_source_row(
        &mut self,
        ctx: &DecodeContext<'_>,
        type_byte: u8,
    ) -> Result<(), CodecError> {
        self.unique_rows += 1;
        charged_map_entry(
            ctx,
            &mut self.by_type,
            type_byte,
            "creo curve coverage type nodes",
        )?
        .0 += 1;
        charged_map_entry(
            ctx,
            &mut self.unknown_by_type,
            type_byte,
            "creo curve coverage unknown type nodes",
        )?;
        Ok(())
    }

    fn record_transferred_row(
        &mut self,
        ctx: &DecodeContext<'_>,
        type_byte: u8,
    ) -> Result<(), CodecError> {
        self.transferred_rows += 1;
        charged_map_entry(
            ctx,
            &mut self.by_type,
            type_byte,
            "creo curve coverage type nodes",
        )?
        .1 += 1;
        Ok(())
    }

    fn record_retained_unknown_row(
        &mut self,
        ctx: &DecodeContext<'_>,
        type_byte: u8,
    ) -> Result<(), CodecError> {
        self.retained_unknown_rows += 1;
        *charged_map_entry(
            ctx,
            &mut self.unknown_by_type,
            type_byte,
            "creo curve coverage unknown type nodes",
        )? += 1;
        Ok(())
    }

    /// Source and transferred counts by native curve type.
    pub(super) fn by_type(&self) -> &BTreeMap<u8, (usize, usize)> {
        &self.by_type
    }

    /// Retained unknown counts by native curve type.
    pub(super) fn unknown_by_type(&self) -> &BTreeMap<u8, usize> {
        &self.unknown_by_type
    }
}

#[derive(Default)]
pub(super) struct SketchSegmentTransferCoverage {
    decoded_rows: usize,
    resolved_geometry: usize,
    missing_rows: usize,
    by_family: [Option<(usize, usize)>; 9],
}

impl SketchSegmentTransferCoverage {
    /// Records decoded and missing rows for a segment table.
    pub(super) fn record_table_rows(
        &mut self,
        decoded: usize,
        expected: usize,
    ) -> Result<(), CodecError> {
        let missing = expected.checked_sub(decoded).ok_or_else(|| {
            CodecError::malformed("decoded sketch rows exceed the declared count")
        })?;
        self.decoded_rows = self
            .decoded_rows
            .checked_add(decoded)
            .ok_or_else(|| CodecError::malformed("decoded sketch row count exceeds usize"))?;
        self.missing_rows = self
            .missing_rows
            .checked_add(missing)
            .ok_or_else(|| CodecError::malformed("missing sketch row count exceeds usize"))?;
        Ok(())
    }

    /// Records decoded rows in one segment family.
    pub(super) fn record_family_rows(
        &mut self,
        family: crate::coverage::SketchSegmentFamily,
        count: usize,
    ) {
        self.family_mut(family).0 += count;
    }

    /// Records resolved geometry instances.
    pub(super) fn record_resolved_geometry(&mut self, count: usize) {
        self.resolved_geometry += count;
    }

    /// Records resolved rows in one segment family.
    pub(super) fn record_family_resolution(
        &mut self,
        family: crate::coverage::SketchSegmentFamily,
        count: usize,
    ) {
        self.family_mut(family).1 += count;
    }

    /// Recorded decoded rows.
    pub(super) fn decoded_rows(&self) -> usize {
        self.decoded_rows
    }

    /// Recorded resolved geometry.
    pub(super) fn resolved_geometry(&self) -> usize {
        self.resolved_geometry
    }

    /// Recorded missing rows.
    pub(super) fn missing_rows(&self) -> usize {
        self.missing_rows
    }

    pub(super) fn families(
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
pub(in crate::decode) struct DesignConstraintTransferCoverage {
    pub(super) transferred: usize,
    pub(super) native: usize,
    pub(super) active: usize,
    pub(super) active_native: usize,
    pub(super) native_by_kind: BTreeMap<u32, usize>,
    pub(super) active_native_by_kind: BTreeMap<u32, usize>,
}

impl DesignConstraintTransferCoverage {
    pub(super) fn typed(&self) -> Result<usize, CodecError> {
        self.transferred.checked_sub(self.native).ok_or_else(|| {
            CodecError::malformed("native constraint count exceeds transferred count")
        })
    }

    pub(super) fn active_typed(&self) -> Result<usize, CodecError> {
        self.active.checked_sub(self.active_native).ok_or_else(|| {
            CodecError::malformed("native constraint count exceeds transferred count")
        })
    }
}

pub(super) fn design_constraint_transfer_coverage(
    ctx: &DecodeContext<'_>,
    constraints: &[SketchConstraint],
    id_marker: &str,
    native_kind_prefix: &str,
) -> Result<DesignConstraintTransferCoverage, CodecError> {
    let mut coverage = DesignConstraintTransferCoverage::default();
    for constraint in constraints
        .iter()
        .filter(|constraint| constraint.id.as_str().contains(id_marker))
    {
        coverage.transferred += 1;
        let native_kind_text = match constraint.definition.kind() {
            SketchConstraintDefinitionInput::Native { native_kind, .. }
                if native_kind.as_str().starts_with(native_kind_prefix) =>
            {
                Some(native_kind.as_str())
            }
            _ => None,
        };
        let native_kind_suffix = match native_kind_text {
            Some(kind) => ctx.strip_prefix(kind, native_kind_prefix, "creo native constraint kind prefix")?,
            None => None,
        };
        let native_kind = match native_kind_suffix {
            Some(kind) => ctx.parse_text::<u32>(kind, "creo scalar text parsing")?.ok(),
            None => None,
        };
        if native_kind_text.is_some() {
            coverage.native += 1;
        }
        if let Some(native_kind) = native_kind {
            *charged_map_entry(
                ctx,
                &mut coverage.native_by_kind,
                native_kind,
                "creo native constraint kind nodes",
            )? += 1;
            if constraint.active == Some(true) {
                *charged_map_entry(
                    ctx,
                    &mut coverage.active_native_by_kind,
                    native_kind,
                    "creo active native constraint kind nodes",
                )? += 1;
            }
        }
        if constraint.active == Some(true) {
            coverage.active += 1;
            if native_kind_text.is_some() {
                coverage.active_native += 1;
            }
        }
    }
    Ok(coverage)
}

pub(super) fn constraint_kind_breakdown<'a>(
    coverage: &'a cadmpeg_ir::report::decode::Coverage,
    prefix: &'a str,
) -> impl std::fmt::Display + 'a {
    struct Breakdown<'a> {
        coverage: &'a cadmpeg_ir::report::decode::Coverage,
        prefix: &'a str,
    }
    impl std::fmt::Display for Breakdown<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut first = true;
            for (key, count) in self.coverage.iter() {
                let Some(kind) = key
                    .strip_prefix(self.prefix)
                    .and_then(|name| name.strip_suffix("_constraint_count"))
                else {
                    continue;
                };
                if *count == 0 {
                    continue;
                }
                if !first {
                    f.write_str(", ")?;
                }
                write!(f, "type {kind}={count}")?;
                first = false;
            }
            Ok(())
        }
    }
    Breakdown { coverage, prefix }
}

pub(super) fn curve_transfer_coverage(
    ctx: &DecodeContext<'_>,
    rows: &[crate::curve::CurveTopologyRow],
    curves: &[Curve],
) -> Result<CurveTransferCoverage, CodecError> {
    let unique_rows = crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)?;
    let mut transferred_ids = BTreeSet::new();
    for curve in curves.iter().filter(|curve| {
        !matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        )
    }) {
        let Some(source) = curve.source_object.as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)
        else {
            continue;
        };
        let Some(digits) = ctx.strip_prefix(source.object_id.as_str(), "VisibGeom:", "creo coverage identity prefix")? else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        ctx.insert_btree_set(&mut transferred_ids, id, "creo transferred curve ID nodes")?;
    }
    let mut unknown_ids = BTreeSet::new();
    for curve in curves.iter().filter(|curve| {
        matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        )
    }) {
        let Some(source) = curve.source_object.as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)
        else {
            continue;
        };
        let Some(digits) = ctx.strip_prefix(source.object_id.as_str(), "VisibGeom:", "creo coverage identity prefix")? else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        ctx.insert_btree_set(&mut unknown_ids, id, "creo unknown curve ID nodes")?;
    }
    let mut coverage = CurveTransferCoverage::default();
    coverage.record_ambiguous_rows(
        rows.len()
            .checked_sub(unique_rows.len())
            .ok_or_else(|| CodecError::malformed("unique row count exceeds source row count"))?,
    );
    for row in unique_rows {
        coverage.record_source_row(ctx, row.type_byte)?;
        if transferred_ids.contains(&row.id) {
            coverage.record_transferred_row(ctx, row.type_byte)?;
        }
        if unknown_ids.contains(&row.id) {
            coverage.record_retained_unknown_row(ctx, row.type_byte)?;
        }
    }
    Ok(coverage)
}

pub(super) fn surface_transfer_coverage(
    ctx: &DecodeContext<'_>,
    rows: &[crate::surface::SurfaceRow],
    surfaces: &[Surface],
    procedural_surfaces: &[ProceduralSurface],
) -> Result<SurfaceTransferCoverage, CodecError> {
    let unique_rows = crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)?;
    let mut extrusion_constructions = BTreeSet::new();
    for id in procedural_surfaces
        .iter()
        .filter(|procedural| {
            matches!(
                procedural.definition(),
                ProceduralSurfaceDefinition::Extrusion(_)
            )
        })
        .map(|procedural| &procedural.id)
    {
        ctx.insert_btree_set(
            &mut extrusion_constructions,
            id,
            "creo extrusion construction nodes",
        )?;
    }
    let mut extrusion_surfaces = BTreeSet::new();
    for id in surfaces
        .iter()
        .filter(|surface| {
            surface
                .geometry
                .procedural_construction()
                .is_some_and(|id| extrusion_constructions.contains(id))
        })
        .map(|surface| &surface.id)
    {
        ctx.insert_btree_set(&mut extrusion_surfaces, id, "creo extrusion surface nodes")?;
    }
    let mut transferred = Vec::new();
    for surface in surfaces {
        let Some(source) = surface.source_object.as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)
        else {
            continue;
        };
        let Some(digits) = ctx.strip_prefix(source.object_id.as_str(), "VisibGeom:", "creo coverage identity prefix")? else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        let Some(kind) = surface_kind_for_geometry(&surface.geometry) else {
            continue;
        };
        let extra = extrusion_surfaces.contains(&surface.id).then_some(
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        );
        ctx.reserve_vec(&mut transferred, 1, "creo transferred surface rows")?;
        transferred.push((id, [Some(kind), extra]));
    }
    let mut unknown_ids = BTreeSet::new();
    for surface in surfaces.iter().filter(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        )
    }) {
        let Some(source) = surface.source_object.as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)
        else {
            continue;
        };
        let Some(digits) = ctx.strip_prefix(source.object_id.as_str(), "VisibGeom:", "creo coverage identity prefix")? else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        ctx.insert_btree_set(&mut unknown_ids, id, "creo unknown surface ID nodes")?;
    }
    let mut coverage = SurfaceTransferCoverage::default();
    coverage.record_ambiguous_rows(
        rows.len()
            .checked_sub(unique_rows.len())
            .ok_or_else(|| CodecError::malformed("unique row count exceeds source row count"))?,
    );
    for row in unique_rows {
        coverage.record_source_row(row.kind);
        if transferred.iter().any(|(id, kinds)| {
            *id == row.id
                && kinds
                    .iter()
                    .flatten()
                    .any(|kind| kind.same_family(row.kind))
        }) {
            coverage.record_transferred_row(row.kind);
        }
        if unknown_ids.contains(&row.id) {
            coverage.record_retained_unknown_row(row.kind);
        }
    }
    Ok(coverage)
}

pub(super) fn surface_variant(kind: crate::surface::SurfaceKind) -> Option<&'static str> {
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

#[cfg(test)]
mod tests {
    use super::{DesignConstraintTransferCoverage, SketchSegmentTransferCoverage};
    use cadmpeg_core::CodecError;

    #[test]
    fn sketch_row_coverage_refuses_overfull_table() {
        let mut coverage = SketchSegmentTransferCoverage::default();
        assert!(matches!(
            coverage.record_table_rows(2, 1),
            Err(CodecError::Malformed(_))
        ));
        assert_eq!(coverage.decoded_rows(), 0);
        assert_eq!(coverage.missing_rows(), 0);
    }

    #[test]
    fn typed_constraint_coverage_refuses_native_count_above_total() {
        let coverage = DesignConstraintTransferCoverage {
            native: 1,
            ..Default::default()
        };
        assert!(matches!(coverage.typed(), Err(CodecError::Malformed(_))));
    }

    #[test]
    fn active_typed_constraint_coverage_refuses_native_count_above_total() {
        let coverage = DesignConstraintTransferCoverage {
            active_native: 1,
            ..Default::default()
        };
        assert!(matches!(
            coverage.active_typed(),
            Err(CodecError::Malformed(_))
        ));
    }
    mod prefixes;

}
