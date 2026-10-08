// SPDX-License-Identifier: Apache-2.0
//! Surface, curve, sketch-segment, and design-constraint transfer coverage.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::sketches::{SketchConstraint, SketchConstraintDefinitionInput};

use crate::container::ContainerScan;

use super::feature_history::link::surface_kind_for_geometry;

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

pub(super) fn source_section_ref<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    offset: usize,
) -> Result<&'a str, CodecError> {
    match ctx.find_by(
        &scan.framing.sections,
        |section| Ok(section.contains(offset)),
        "creo source section search",
    )? {
        Some(section) => Ok(section.name()),
        None => Ok(
            if matches!(
                scan.framing.layout,
                crate::container::Layout::LegacyAscii(_)
            ) {
                "legacy_ascii"
            } else {
                "unknown"
            },
        ),
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
        ctx.entry_btree_map(
            &mut self.by_type,
            type_byte,
            "creo curve coverage type nodes",
        )?
        .or_default()
        .0 += 1;
        ctx.entry_btree_map(
            &mut self.unknown_by_type,
            type_byte,
            "creo curve coverage unknown type nodes",
        )?
        .or_default();
        Ok(())
    }

    fn record_transferred_row(
        &mut self,
        ctx: &DecodeContext<'_>,
        type_byte: u8,
    ) -> Result<(), CodecError> {
        self.transferred_rows += 1;
        ctx.entry_btree_map(
            &mut self.by_type,
            type_byte,
            "creo curve coverage type nodes",
        )?
        .or_default()
        .1 += 1;
        Ok(())
    }

    fn record_retained_unknown_row(
        &mut self,
        ctx: &DecodeContext<'_>,
        type_byte: u8,
    ) -> Result<(), CodecError> {
        self.retained_unknown_rows += 1;
        *ctx.entry_btree_map(
            &mut self.unknown_by_type,
            type_byte,
            "creo curve coverage unknown type nodes",
        )?
        .or_default() += 1;
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

pub(super) fn design_constraint_transfer_coverage<const N: usize>(
    ctx: &DecodeContext<'_>,
    constraints: &[SketchConstraint],
    selectors: [(&str, &str); N],
) -> Result<[DesignConstraintTransferCoverage; N], CodecError> {
    let mut coverages = std::array::from_fn(|_| DesignConstraintTransferCoverage::default());
    for constraint in ctx.admit_iter(constraints, "creo constraint coverage traversal")? {
        for ((id_marker, native_kind_prefix), coverage) in selectors.into_iter().zip(&mut coverages)
        {
            if !ctx.contains_text(
                constraint.id.as_str(),
                id_marker,
                "creo constraint identity marker",
            )? {
                continue;
            }
            coverage.transferred += 1;
            let native_kind_suffix = match constraint.definition.kind() {
                SketchConstraintDefinitionInput::Native { native_kind, .. } => ctx.strip_prefix(
                    native_kind.as_str(),
                    native_kind_prefix,
                    "creo native constraint kind prefix",
                )?,
                _ => None,
            };
            let native_kind = match native_kind_suffix {
                Some(kind) => ctx
                    .parse_text::<u32>(kind, "creo scalar text parsing")?
                    .ok(),
                None => None,
            };
            if native_kind_suffix.is_some() {
                coverage.native += 1;
            }
            if let Some(native_kind) = native_kind {
                *ctx.entry_btree_map(
                    &mut coverage.native_by_kind,
                    native_kind,
                    "creo native constraint kind nodes",
                )?
                .or_default() += 1;
                if constraint.active == Some(true) {
                    *ctx.entry_btree_map(
                        &mut coverage.active_native_by_kind,
                        native_kind,
                        "creo active native constraint kind nodes",
                    )?
                    .or_default() += 1;
                }
            }
            if constraint.active == Some(true) {
                coverage.active += 1;
                if native_kind_suffix.is_some() {
                    coverage.active_native += 1;
                }
            }
        }
    }
    Ok(coverages)
}

pub(super) struct ConstraintKindBreakdown<'a, 'ctx> {
    rows: Vec<(&'a str, usize)>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl std::fmt::Display for ConstraintKindBreakdown<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (kind, count)) in self.rows.iter().enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            write!(f, "type {kind}={count}")?;
        }
        Ok(())
    }
}

pub(super) fn constraint_kind_breakdown<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    coverage: &'a cadmpeg_ir::report::decode::Coverage,
    prefix: &str,
) -> Result<ConstraintKindBreakdown<'a, 'ctx>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo constraint kind breakdown storage")?;
    let mut rows = Vec::new();
    for (key, count) in ctx.admit_iter(&**coverage, "creo constraint kind breakdown traversal")? {
        if *count == 0 {
            continue;
        }
        let Some(name) = ctx.strip_prefix(key, prefix, "creo constraint kind breakdown prefix")?
        else {
            continue;
        };
        let Some(kind) = ctx.strip_suffix(
            name,
            "_constraint_count",
            "creo constraint kind breakdown suffix",
        )?
        else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut rows,
            (kind, *count),
            "creo constraint kind breakdown rows",
        )?;
    }
    Ok(ConstraintKindBreakdown {
        rows,
        _storage: storage,
    })
}

pub(super) fn curve_transfer_coverage(
    ctx: &DecodeContext<'_>,
    rows: &[crate::curve::CurveTopologyRow],
    curves: &[Curve],
) -> Result<CurveTransferCoverage, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo curve coverage lookup storage")?;
    let unique_rows = scratch.with_storage(|| {
        crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)
    })?;
    let mut transferred_ids = BTreeSet::new();
    let mut unknown_ids = BTreeSet::new();
    for curve in ctx.admit_iter(curves, "creo curve coverage traversal")? {
        let Some(source) = curve
            .source_object
            .as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)
        else {
            continue;
        };
        let Some(digits) = ctx.strip_prefix(
            source.object_id.as_str(),
            "VisibGeom:",
            "creo coverage identity prefix",
        )?
        else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        scratch.with_storage(|| {
            if matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            ) {
                ctx.insert_btree_set(&mut unknown_ids, id, "creo unknown curve ID nodes")
            } else {
                ctx.insert_btree_set(&mut transferred_ids, id, "creo transferred curve ID nodes")
            }
        })?;
    }
    let mut coverage = CurveTransferCoverage::default();
    coverage.record_ambiguous_rows(
        rows.len()
            .checked_sub(unique_rows.len())
            .ok_or_else(|| CodecError::malformed("unique row count exceeds source row count"))?,
    );
    for row in ctx.admit_iter(unique_rows, "creo unique coverage row traversal")? {
        coverage.record_source_row(ctx, row.type_byte)?;
        if ctx.contains_btree_set(
            &transferred_ids,
            &row.id,
            "creo transferred curve ID lookup",
        )? {
            coverage.record_transferred_row(ctx, row.type_byte)?;
        }
        if ctx.contains_btree_set(&unknown_ids, &row.id, "creo unknown coverage ID lookup")? {
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
    let mut scratch = ctx.reserve_scoped(0, "creo surface coverage lookup storage")?;
    let unique_rows = scratch.with_storage(|| {
        crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)
    })?;
    let mut extrusion_constructions = BTreeSet::new();
    for procedural in ctx.admit_iter(
        procedural_surfaces,
        "creo procedural surface coverage traversal",
    )? {
        if matches!(
            procedural.definition(),
            ProceduralSurfaceDefinition::Extrusion(_)
        ) {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut extrusion_constructions,
                    &procedural.id,
                    "creo extrusion construction nodes",
                )
            })?;
        }
    }
    let mut extrusion_surfaces = BTreeSet::new();
    for surface in ctx.admit_iter(surfaces, "creo extrusion surface coverage traversal")? {
        let extrusion = match surface.geometry.procedural_construction() {
            Some(construction) => ctx.contains_btree_set(
                &extrusion_constructions,
                construction,
                "creo extrusion construction lookup",
            )?,
            None => false,
        };
        if extrusion {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut extrusion_surfaces,
                    &surface.id,
                    "creo extrusion surface nodes",
                )
            })?;
        }
    }
    let mut transferred = HashMap::<u32, [bool; 7]>::new();
    let mut unknown_ids = BTreeSet::new();
    for surface in ctx.admit_iter(surfaces, "creo surface coverage traversal")? {
        let Some(source) = surface
            .source_object
            .as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Creo)
        else {
            continue;
        };
        let Some(digits) = ctx.strip_prefix(
            source.object_id.as_str(),
            "VisibGeom:",
            "creo coverage identity prefix",
        )?
        else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        if matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        ) {
            scratch.with_storage(|| {
                ctx.insert_btree_set(&mut unknown_ids, id, "creo unknown surface ID nodes")
            })?;
        }
        let Some(kind) = surface_kind_for_geometry(&surface.geometry) else {
            continue;
        };
        let extrusion = ctx.contains_btree_set(
            &extrusion_surfaces,
            &surface.id,
            "creo extrusion surface lookup",
        )?;
        scratch.with_storage(|| {
            let kinds = ctx
                .entry_hash_map(&mut transferred, id, "creo transferred surface rows")?
                .or_insert([false; 7]);
            kinds[surface_family_index(kind)] = true;
            if extrusion {
                kinds[surface_family_index(crate::surface::SurfaceKind::Extrusion(
                    crate::surface::ExtrusionVariant::Linear,
                ))] = true;
            }
            Ok::<_, CodecError>(())
        })?;
    }
    let mut coverage = SurfaceTransferCoverage::default();
    coverage.record_ambiguous_rows(
        rows.len()
            .checked_sub(unique_rows.len())
            .ok_or_else(|| CodecError::malformed("unique row count exceeds source row count"))?,
    );
    for row in ctx.admit_iter(unique_rows, "creo unique coverage row traversal")? {
        coverage.record_source_row(row.kind);
        if transferred
            .get(&row.id)
            .is_some_and(|kinds| kinds[surface_family_index(row.kind)])
        {
            coverage.record_transferred_row(row.kind);
        }
        if ctx.contains_btree_set(&unknown_ids, &row.id, "creo unknown coverage ID lookup")? {
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
