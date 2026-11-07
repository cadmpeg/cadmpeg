// SPDX-License-Identifier: Apache-2.0
//! Neutral precedence and explicit synthesis omissions.

use cadmpeg_ir::codec::write::{ArenaDisposition, ArenaDispositions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::EntitySchema;

use crate::loss::F3dLossCode;

pub(super) fn pcurves(
    target: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    use super::super::generate::native_geometry::{native_support_pcurve, pcurve_support_geometry};
    use cadmpeg_ir::geometry::pcurve::PcurveMetadata;
    use cadmpeg_ir::topology::{LoopBoundary, LoopRing};
    use std::collections::BTreeSet;
    let mut omitted = BTreeSet::new();
    for pcurve in &target.model.pcurves {
        let referenced = target
            .model
            .coedges
            .iter()
            .any(|coedge| coedge.pcurves.iter().any(|use_| use_.pcurve == pcurve.id))
            || target.model.loops.iter().any(|boundary| {
                boundary
                    .vertex_pcurves()
                    .any(|use_| use_.pcurve == pcurve.id)
            });
        let metadata_supported = match &pcurve.metadata {
            PcurveMetadata::AsmInline { .. } => true,
            PcurveMetadata::General { form } => {
                form.wrapper_reversed.is_none()
                    && form.fit_tolerance().is_none()
                    && form.parameter_range().is_some()
            }
        };
        let supported = if referenced && metadata_supported {
            match pcurve_support_geometry(&target.model, &pcurve.id)
                .and_then(|support| native_support_pcurve(support, &pcurve.geometry))
            {
                Ok(_) => true,
                Err(error) if super::declines(&error) => false,
                Err(error) => return Err(error),
            }
        } else {
            false
        };
        if !supported {
            omitted.insert(pcurve.id.clone());
        }
    }
    for coedge in &mut target.model.coedges {
        coedge
            .pcurves
            .retain(|use_| !omitted.contains(&use_.pcurve));
        if coedge.pcurves.len() > 1 {
            for use_ in coedge.pcurves.drain(1..) {
                omitted.insert(use_.pcurve);
            }
        }
    }
    for coedge in &mut target.model.coedges {
        coedge
            .pcurves
            .retain(|use_| !omitted.contains(&use_.pcurve));
    }
    for boundary in &mut target.model.loops {
        match &mut boundary.boundary {
            LoopBoundary::Vertex { pcurves, .. } => {
                pcurves.retain(|use_| !omitted.contains(&use_.pcurve));
            }
            LoopBoundary::Ring(ring) => {
                let mut vertices = ring.vertex_uses().to_vec();
                for vertex in &mut vertices {
                    vertex
                        .pcurves
                        .retain(|use_| !omitted.contains(&use_.pcurve));
                }
                *ring = LoopRing::new(ring.coedges().to_vec(), vertices)
                    .map_err(|error| cadmpeg_core::CodecError::Malformed(error.to_string()))?;
            }
        }
    }
    target
        .model
        .pcurves
        .retain(|pcurve| !omitted.contains(&pcurve.id));
    if !omitted.is_empty() {
        let identities = omitted
            .iter()
            .map(cadmpeg_ir::ids::PcurveId::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(F3dLossCode::WriterPcurveOmitted.note(format!(
            "{} unsupported parameter-space curves are omitted: {identities}; spatial edge carriers remain",
            omitted.len(),
        )));
    }
    Ok(())
}

pub(super) fn auxiliary(
    target: &mut CadIr,
    native: &crate::native::F3dNative,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut attributes = std::collections::BTreeSet::new();
    target.model.attributes.retain(|attribute| {
        if attributes.insert(attribute.id.clone()) {
            true
        } else {
            losses.push(F3dLossCode::WriterAuxiliaryOmitted.note(format!(
                "attribute {} has a duplicate identity and is omitted",
                attribute.id
            )));
            false
        }
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy {
        limits,
        ..Default::default()
    };
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let mut retained = Vec::new();
    for mut surface in std::mem::take(&mut target.model.subds) {
        let cage = &surface.cage;
        let mut symmetries = Vec::new();
        for (index, symmetry) in cage.symmetries().iter().enumerate() {
            if super::super::generate::subd::encode_symmetry(
                &ctx,
                false,
                &mut String::new(),
                symmetry,
                cage.faces().len(),
                cage.edges().len(),
                cage.vertices().len(),
            )? {
                symmetries.push(symmetry.clone());
            } else {
                losses.push(F3dLossCode::WriterSubdOmitted.note(format!(
                    "subd {} symmetry {index} has unsupported T-spline controls",
                    surface.id
                )));
            }
        }
        if symmetries.len() != cage.symmetries().len() {
            surface.cage = cadmpeg_ir::subd::SubdCage::new(
                cage.vertices().to_vec(),
                cage.edges().to_vec(),
                cage.faces().to_vec(),
                symmetries,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        if super::super::generate::subd::encode(&ctx, &surface, false)?.is_some() {
            retained.push(surface);
        } else {
            losses.push(F3dLossCode::WriterSubdOmitted.note(format!(
                "subd {} has unsupported T-spline topology or controls",
                surface.id
            )));
        }
    }
    target.model.subds = retained;
    let canvas_asset =
        super::super::generate::canvas::select(target, native).map(|canvas| canvas.asset);
    let decal_asset =
        super::super::generate::decal::select(target, native).map(|decal| decal.asset);
    target.model.assets.retain(|asset| {
        canvas_asset.as_ref() == Some(&asset.id) || decal_asset.as_ref() == Some(&asset.id)
    });
    target.model.tessellations.clear();
    for face in &mut target.model.faces {
        if matches!(
            face.loops,
            cadmpeg_ir::topology::FaceLoops::Classified { .. }
        ) {
            face.loops = cadmpeg_ir::topology::FaceLoops::unspecified(face.loops.to_vec());
            losses.push(F3dLossCode::WriterLoopClassificationOmitted.note(format!(
                "face {} outer and inner loop classification is not encoded",
                face.id
            )));
        }
        if face.tolerance.take().is_some() {
            losses.push(
                F3dLossCode::WriterAuxiliaryOmitted
                    .note(format!("face {} tolerance is not encoded", face.id)),
            );
        }
    }
    for point in &mut target.model.points {
        if point.source_object.take().is_some() {
            losses.push(F3dLossCode::WriterSourceAssociationOmitted.note(format!(
                "point {} source-object association is not encoded",
                point.id,
            )));
        }
    }
    for surface in &mut target.model.surfaces {
        if surface.source_object.take().is_some() {
            losses.push(F3dLossCode::WriterSourceAssociationOmitted.note(format!(
                "surface {} source-object association is not encoded",
                surface.id,
            )));
        }
    }
    for curve in &mut target.model.curves {
        if curve.source_object.take().is_some() {
            losses.push(F3dLossCode::WriterSourceAssociationOmitted.note(format!(
                "curve {} source-object association is not encoded",
                curve.id,
            )));
        }
    }
    Ok(())
}

pub(super) fn solved_carriers(
    target: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    use super::super::generate::native_geometry::{
        native_procedural_curve, native_procedural_surface,
    };
    use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
    let mut surfaces = std::collections::BTreeSet::new();
    for surface in &target.model.surfaces {
        if let SurfaceGeometry::Procedural {
            cache: Some(cache), ..
        } = &surface.geometry
        {
            let supported = match cache {
                SolvedSurfaceGeometry::Nurbs(cache) => {
                    match native_procedural_surface(&mut Vec::new(), target, surface, cache) {
                        Ok(value) => value,
                        Err(error) if super::declines(&error) => false,
                        Err(error) => return Err(error),
                    }
                }
                _ => false,
            };
            if !supported {
                surfaces.insert(surface.id.clone());
            }
        }
    }
    let mut curves = std::collections::BTreeSet::new();
    for curve in &target.model.curves {
        if let CurveGeometry::Procedural {
            cache: Some(cache), ..
        } = &curve.geometry
        {
            let supported = match cache {
                SolvedCurveGeometry::Nurbs(cache) => {
                    match native_procedural_curve(&mut Vec::new(), target, &curve.id, cache) {
                        Ok(value) => value,
                        Err(error) if super::declines(&error) => false,
                        Err(error) => return Err(error),
                    }
                }
                _ => false,
            };
            if !supported {
                curves.insert(curve.id.clone());
            }
        }
    }
    for surface in &mut target.model.surfaces {
        if !surfaces.contains(&surface.id) {
            continue;
        }
        if let SurfaceGeometry::Procedural {
            construction,
            cache: Some(cache),
        } = &surface.geometry
        {
            losses.push(F3dLossCode::WriterProceduralReduced.note(format!(
                "surface {} is encoded from its solved carrier; construction {} is omitted",
                surface.id, construction,
            )));
            surface.geometry = SurfaceGeometry::Solved(cache.clone());
        }
    }
    for curve in &mut target.model.curves {
        if !curves.contains(&curve.id) {
            continue;
        }
        if let CurveGeometry::Procedural {
            construction,
            cache: Some(cache),
        } = &curve.geometry
        {
            losses.push(F3dLossCode::WriterProceduralReduced.note(format!(
                "curve {} is encoded from its solved carrier; construction {} is omitted",
                curve.id, construction,
            )));
            curve.geometry = CurveGeometry::Solved(cache.clone());
        }
    }
    let surface_ids: std::collections::BTreeSet<_> = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| surface.geometry.procedural_construction().cloned())
        .collect();
    target
        .model
        .procedural_surfaces
        .retain(|record| surface_ids.contains(&record.id));
    let curve_ids: std::collections::BTreeSet<_> = target
        .model
        .curves
        .iter()
        .filter_map(|curve| curve.geometry.procedural_construction().cloned())
        .collect();
    target
        .model
        .procedural_curves
        .retain(|record| curve_ids.contains(&record.id));
    let local_curves = super::super::primitives::f3d_native(target)?
        .unwrap_or_default()
        .tolerant_coedge_parameters
        .into_iter()
        .filter(|record| {
            matches!(
                record.extension,
                cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                    target: None,
                    ..
                }
            )
        })
        .filter_map(|record| {
            cadmpeg_asm::brep::records::native_entity_id(
                &record.source_namespace,
                record.record_index,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    for coedge in &mut target.model.coedges {
        if local_curves.contains(coedge.id.as_str())
            && coedge.use_curve.as_ref().is_some_and(|use_| {
                target
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == use_.curve)
                    .is_some_and(|curve| {
                        matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Nurbs(_)))
                    })
            })
        {
            continue;
        }
        if coedge.use_curve.take().is_some() {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "coedge {} local curve is omitted; the shared edge carrier is encoded",
                coedge.id,
            )));
        }
    }
    Ok(())
}

pub(super) fn unreferenced_composite_curves(
    target: &mut CadIr,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    let candidates: std::collections::BTreeSet<String> = target
        .model
        .curves
        .iter()
        .filter(|curve| {
            matches!(
                curve.geometry.solved(),
                Some(SolvedCurveGeometry::Composite { .. })
            )
        })
        .map(|curve| curve.id.as_str().to_owned())
        .collect();
    if candidates.is_empty() {
        return Ok(());
    }
    for edge in &mut target.model.edges {
        if edge.param_range().is_some() {
            continue;
        }
        let Some(original) = edge.curve().cloned() else {
            continue;
        };
        let mut current = original.clone();
        let mut visited = std::collections::BTreeSet::new();
        while visited.insert(current.clone()) {
            let Some(curve) = target.model.curves.iter().find(|curve| curve.id == current) else {
                break;
            };
            let Some(SolvedCurveGeometry::Composite { segments, .. }) = curve.geometry.solved()
            else {
                break;
            };
            if curve.parameter_range.is_some() || segments.len() != 1 || !segments[0].same_sense {
                break;
            }
            current = segments[0].curve.clone();
        }
        if current != original
            && !candidates.contains(current.as_str())
            && target.model.curves.iter().any(|curve| curve.id == current)
        {
            edge.map_curve(|_| current.clone());
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "edge {} uses curve {} after composite curve {} is reduced",
                edge.id, current, original
            )));
        }
    }
    let needs = super::design::with_context(limits, |ctx| target.neutral_needs(ctx))?;
    let mut retained = std::collections::BTreeSet::new();
    let mut pending: Vec<String> = needs
        .iter()
        .filter(|(owner, _)| !candidates.contains(*owner))
        .flat_map(|(_, refs)| refs.iter().filter(|id| candidates.contains(*id)).cloned())
        .collect();
    while let Some(id) = pending.pop() {
        if retained.insert(id.clone()) {
            if let Some(refs) = needs.get(&id) {
                pending.extend(refs.iter().filter(|id| candidates.contains(*id)).cloned());
            }
        }
    }
    target.model.curves.retain(|curve| {
        if candidates.contains(curve.id.as_str()) && !retained.contains(curve.id.as_str()) {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "composite curve {} has no F3D carrier and is omitted",
                curve.id
            )));
            false
        } else {
            true
        }
    });
    Ok(())
}

pub(super) fn dispositions(
    original: &CadIr,
    written: &CadIr,
    mut coverage: ArenaDispositions,
    represented_attributes: &std::collections::BTreeSet<String>,
    losses: &mut Vec<LossNote>,
) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Omitted, Reported};
    macro_rules! report {
        ($code:ident; $($field:ident),* $(,)?) => {$({
            let before = original.model.$field.len();
            let after = written.model.$field.len();
            if !coverage.$field.carries() {
                coverage.$field = Dropped;
                if before != 0 {
                    let mut ids: Vec<_> = original.model.$field.iter().map(EntitySchema::identity).collect();
                    ids.sort_unstable();
                    losses.push(F3dLossCode::$code.note(format!(
                        "model.{}: {before} record(s) are not encoded: {}", stringify!($field), ids.join(", "),
                    )));
                }
            } else if before != after {
                coverage.$field = if after == 0 { Dropped } else { Reported };
                let live: std::collections::BTreeSet<_> = written.model.$field.iter().map(EntitySchema::identity).collect();
                let mut omitted: Vec<_> = original.model.$field.iter().map(EntitySchema::identity)
                    .filter(|id| !live.contains(id)).collect();
                omitted.sort_unstable();
                losses.push(F3dLossCode::$code.note(format!(
                    "model.{}: {before} document record(s) produce {after} encoded record(s); omitted identities: {}", stringify!($field), omitted.join(", "),
                )));
            }
        })*};
    }
    report!(WriterGeometryOmitted; bodies, regions, shells, faces, loops, coedges,
        edges, vertices, points, surfaces, curves);
    if coverage.faces.carries()
        && original.model.faces.iter().any(|face| {
            matches!(
                face.loops,
                cadmpeg_ir::topology::FaceLoops::Classified { .. }
            )
        })
    {
        coverage.faces = Reported;
    }
    report!(WriterPcurveOmitted; pcurves);
    report!(WriterDesignRecordsOmitted; features, feature_input_topologies,
        feature_result_topologies, configurations, parameters, sketches,
        sketch_entities, sketch_constraints, spatial_sketches, spatial_sketch_entities,
        spatial_sketch_constraints, spreadsheets);
    report!(WriterSubdOmitted; subds);
    report!(WriterAssetOmitted; assets);
    report!(WriterTessellationOmitted; tessellations);
    report!(WriterProductOmitted; product_definitions, occurrences, assembly_joints);
    report!(WriterPmiOmitted; pmi);
    report!(WriterAppearancePropertiesOmitted; appearances, appearance_bindings);
    report!(WriterProceduralReduced; procedural_surfaces, procedural_curves);
    report!(WriterAuxiliaryOmitted; drawings,
        semantic_annotations, presentation_documents, view_presentations,
        presentation_layers);
    let omitted_attributes = original
        .model
        .attributes
        .iter()
        .filter(|attribute| !represented_attributes.contains(attribute.id.as_str()))
        .collect::<Vec<_>>();
    coverage.attributes = if omitted_attributes.is_empty() {
        ArenaDisposition::Written
    } else if omitted_attributes.len() == original.model.attributes.len() {
        Dropped
    } else {
        Reported
    };
    for attribute in omitted_attributes {
        losses.push(F3dLossCode::WriterAuxiliaryOmitted.note(format!(
            "model.attributes: 1 record(s) are not encoded: {}{}",
            attribute.id,
            MissingAttributePositions(&attribute.values)
        )));
    }
    // A field reduction is still partial handling when the record count is
    // unchanged. Identity reminting also takes this conservative disposition.
    macro_rules! changed {
        ($($field:ident),* $(,)?) => {$({
            if coverage.$field == ArenaDisposition::Written
                && original.model.$field != written.model.$field
            {
                coverage.$field = Reported;
            }
        })*};
    }
    changed!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        curves,
        pcurves,
        procedural_surfaces,
        procedural_curves,
        parameters,
        configurations,
        appearances
    );
    // Coverage has no implicit omission path: every dropped arena above has
    // a local, taxonomy-bearing loss at the appropriate severity.
    debug_assert!(!matches!(coverage.bodies, Omitted));
    coverage
}

struct MissingAttributePositions<'a>(&'a [Option<cadmpeg_ir::attributes::AttributeValue>]);

impl std::fmt::Display for MissingAttributePositions<'_> {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.0.iter().any(Option::is_none) {
            return Ok(());
        }
        output.write_str("; unavailable value positions [")?;
        let mut separator = "";
        for (index, value) in self.0.iter().enumerate() {
            if value.is_none() {
                write!(output, "{separator}{index}")?;
                separator = ", ";
            }
        }
        output.write_str("]")
    }
}
