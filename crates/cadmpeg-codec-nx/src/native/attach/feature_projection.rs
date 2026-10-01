use super::{extrude_feature_definition, feature_body_outputs};
use crate::native::history::BodyWriterHistory;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{BodyId, LoopId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{BodyKind, Face, FaceLoops, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferSpec, RadiusSpec},
        holes::{HoleForm, HoleKind, HolePlacement},
        patterns::PatternKind,
        BodySelection, BodyTrimSide, BooleanOp, CurveProjectionDirection,
        CurveProjectionDirectionState, EdgeSelection, FaceSelection, FeatureDefinition, FeatureId,
        FeatureOperation, FeatureSourceContent, FeatureTreeNodeRole, LinearTermination, PathRef,
        RibConstruction, RibDraft, SurfaceExtension, ThickenSide, TreeChildren, TrimRegion,
        UnresolvedFamily,
    },
    scalar::{FiniteReal, Length, NonZeroLength, PositiveLength},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn body_faces<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &'a CadIr,
    body_id: &BodyId,
) -> Result<Option<ScopedFaces<'a, 'ctx>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()),
        "NX body face body lookup",
    )?;
    let Some(body) = ir.model.bodies.iter().find(|body| body.id == *body_id) else {
        return Ok(None);
    };
    let mut faces = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body faces")?;
    for region_id in &body.regions {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.regions.len()),
            "NX body face region lookup",
        )?;
        let Some(region) = ir
            .model
            .regions
            .iter()
            .find(|region| region.id == *region_id && region.body == body.id)
        else {
            return Ok(None);
        };
        for shell_id in &region.shells {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(ir.model.shells.len()),
                "NX body face shell lookup",
            )?;
            let Some(shell) = ir
                .model
                .shells
                .iter()
                .find(|shell| shell.id == *shell_id && shell.region == region.id)
            else {
                return Ok(None);
            };
            for face_id in shell.faces() {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(ir.model.faces.len()),
                    "NX body face lookup",
                )?;
                let Some(face) = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id && face.shell == shell.id)
                else {
                    return Ok(None);
                };
                ctx.reserve_scoped_vec(&mut reservation, &mut faces, 1, "NX body faces")?;
                faces.push(face);
            }
        }
    }
    Ok(Some(ScopedFaces {
        faces,
        _reservation: reservation,
    }))
}

pub(super) struct ScopedFaces<'a, 'ctx> {
    faces: Vec<&'a Face>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a> std::ops::Deref for ScopedFaces<'a, '_> {
    type Target = [&'a Face];

    fn deref(&self) -> &Self::Target {
        &self.faces
    }
}

pub(super) fn connected_solid_body_faces<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &'a CadIr,
    body_id: &BodyId,
) -> Result<Option<ScopedFaces<'a, 'ctx>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()),
        "NX connected solid body lookup",
    )?;
    let Some(body) = ir.model.bodies.iter().find(|body| body.id == *body_id) else {
        return Ok(None);
    };
    if body.kind != cadmpeg_ir::topology::BodyKind::Solid {
        return Ok(None);
    }
    let [region_id] = body.regions.as_slice() else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.regions.len()),
        "NX connected solid region lookup",
    )?;
    let Some(region) = ir
        .model
        .regions
        .iter()
        .find(|region| region.id == *region_id && region.body == body.id)
    else {
        return Ok(None);
    };
    let [shell_id] = region.shells.as_slice() else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.shells.len()),
        "NX connected solid shell lookup",
    )?;
    let Some(shell) = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == *shell_id && shell.region == region.id)
    else {
        return Ok(None);
    };
    let mut faces = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX connected solid faces")?;
    for face_id in shell.faces() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.faces.len()),
            "NX connected solid face lookup",
        )?;
        let Some(face) = ir
            .model
            .faces
            .iter()
            .find(|face| face.id == *face_id && face.shell == shell.id)
        else {
            return Ok(None);
        };
        ctx.reserve_scoped_vec(&mut reservation, &mut faces, 1, "NX connected solid faces")?;
        faces.push(face);
    }
    Ok(Some(ScopedFaces {
        faces,
        _reservation: reservation,
    }))
}

pub(super) fn connected_solid_body_exists(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body: &cadmpeg_ir::topology::Body,
) -> Result<bool, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()),
        "NX connected solid identity scan",
    )?;
    let Some(body) = ir
        .model
        .bodies
        .iter()
        .find(|candidate| candidate.id == body.id)
    else {
        return Ok(false);
    };
    if body.kind != BodyKind::Solid {
        return Ok(false);
    }
    let [region_id] = body.regions.as_slice() else {
        return Ok(false);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.regions.len()),
        "NX connected solid region scan",
    )?;
    let Some(region) = ir
        .model
        .regions
        .iter()
        .find(|region| region.id == *region_id && region.body == body.id)
    else {
        return Ok(false);
    };
    let [shell_id] = region.shells.as_slice() else {
        return Ok(false);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.shells.len()),
        "NX connected solid shell scan",
    )?;
    let Some(shell) = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == *shell_id && shell.region == region.id)
    else {
        return Ok(false);
    };
    let face_work = shell
        .faces()
        .len()
        .checked_mul(ir.model.faces.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX connected solid face scan",
                0,
                cadmpeg_core::decode::u64_from_index(shell.faces().len()),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(face_work),
        "NX connected solid face scan",
    )?;
    Ok(shell.faces().iter().all(|face_id| {
        ir.model
            .faces
            .iter()
            .any(|face| face.id == *face_id && face.shell == shell.id)
    }))
}

pub(super) struct ScopedSurfaceIds<'ctx> {
    ids: BTreeSet<SurfaceId>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn selection_indices_native(
    ctx: &DecodeContext<'_>,
    indices: impl Iterator<Item = u32> + Clone,
) -> Result<String, CodecError> {
    const PREFIX: &str = "nx:om-object-indices#";
    let mut count = 0usize;
    let mut length = PREFIX.len();
    for index in indices.clone() {
        let digits = if index == 0 { 1 } else { index.ilog10() + 1 };
        length = length
            .checked_add(
                usize::try_from(digits)
                    .map_err(|_| ctx.refuse_codec_limit("NX body selection indices", 0, 1))?,
            )
            .and_then(|length| length.checked_add(usize::from(count != 0)))
            .ok_or_else(|| ctx.refuse_codec_limit("NX body selection indices", 0, 1))?;
        count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX body selection indices", 0, 1))?;
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(length),
        "NX body selection indices",
    )?;
    let mut text = ctx.retained_string(length, "NX body selection indices")?;
    text.push_str(PREFIX);
    for (ordinal, index) in indices.enumerate() {
        if ordinal != 0 {
            text.push(',');
        }
        std::fmt::write(&mut text, format_args!("{index}")).map_err(|_| {
            CodecError::InvalidInput("NX body selection index formatting failed".to_string())
        })?;
    }
    Ok(text)
}

pub(super) fn parameter_consumer_identity(
    ctx: &DecodeContext<'_>,
    operation_label: &str,
) -> Result<String, CodecError> {
    match operation_label.split_once("operation-label") {
        Some((prefix, suffix)) => ctx.format_retained(
            format_args!("{prefix}feature{suffix}"),
            "NX feature projection text",
        ),
        None => ctx.format_retained(
            format_args!("{operation_label}"),
            "NX feature projection text",
        ),
    }
}

pub(super) fn insert_parameter_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: String,
) -> Result<(), CodecError> {
    let key = ctx.format_retained(key, "NX feature projection text")?;
    let key = cadmpeg_core::text::NonBlankString::new(key).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format_args!("NX parameter property key is blank"))
    })?;
    if !properties.contains_key(&key) {
        ctx.charge_collection_items(1, "NX parameter properties")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<(cadmpeg_core::text::NonBlankString, String)>() * 4,
            ),
            "NX parameter property node",
        )?;
    }
    properties.insert(key, value);
    Ok(())
}

impl std::ops::Deref for ScopedSurfaceIds<'_> {
    type Target = BTreeSet<SurfaceId>;

    fn deref(&self) -> &Self::Target {
        &self.ids
    }
}

pub(super) fn body_surface_ids<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    body_id: &BodyId,
) -> Result<Option<ScopedSurfaceIds<'ctx>>, CodecError> {
    let Some(faces) = body_faces(ctx, ir, body_id)? else {
        return Ok(None);
    };
    let mut ids = BTreeSet::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body surface identities")?;
    for face in faces.iter() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ids.len()),
            "NX body surface uniqueness",
        )?;
        if ids.contains(&face.surface) {
            continue;
        }
        let bytes = std::mem::size_of::<SurfaceId>()
            .checked_mul(4)
            
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX body surface identity",
                    0,
                    cadmpeg_core::decode::u64_from_index(face.surface.as_str().len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX body surface identities")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        ids.insert(reservation.with_storage(|| face.surface.try_clone_for_decode(ctx, "NX body surface identity copy"))?);
    }
    Ok(Some(ScopedSurfaceIds {
        ids,
        _reservation: reservation,
    }))
}

/// Neutral operand family named by an NX rolling-ball blend operation.
#[derive(Clone, Copy)]
pub(super) enum NxBlendFamily {
    /// Edge-selected `BLEND` operation.
    Edge,
    /// Face-selected `FACE_BLEND` operation.
    Face,
}

/// Project complete owned rolling-ball carriers into their named blend family.
pub(super) fn blend_feature_definition(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
    family: NxBlendFamily,
) -> Result<Option<(FeatureDefinition, Vec<SurfaceId>)>, CodecError> {
    let [body] = outputs else {
        return Ok(None);
    };
    let Some(body_surfaces) = body_surface_ids(ctx, ir, body)? else {
        return Ok(None);
    };
    let mut surfaces = Vec::new();
    let mut first_radius = None;
    let mut constant_radii = true;
    let mut uniform_radii = true;
    let mut variable_radii = true;
    let mut pairs = Vec::new();
    let mut pairs_reservation = ctx.reserve_scoped(0, "NX blend support pairs")?;
    let mut complete_pairs = true;
    for procedural in &ir.model.procedural_surfaces {
        ctx.charge_work(1, "NX blend procedural surface scan")?;
        let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
            continue;
        };
        if !body_surfaces.contains(owner) {
            continue;
        }
        let ProceduralSurfaceDefinition::Blend(definition_payload) = procedural.definition() else {
            continue;
        };
        let supports = definition_payload.supports();
        let radius = definition_payload.radius();
        let cross_section = definition_payload.cross_section();

        if *cross_section != BlendCrossSection::Circular {
            return Ok(None);
        }
        ctx.reserve_vec(&mut surfaces, 1, "NX blend result surfaces")?;
        surfaces.push(owner.try_clone_for_decode(ctx, "NX feature projection surface identity")?);
        match radius {
            BlendRadiusLaw::Constant { signed_radius } if signed_radius.get() != 0.0 => {
                let magnitude = signed_radius.get().abs();
                if first_radius.is_some_and(|first: f64| first.to_bits() != magnitude.to_bits()) {
                    uniform_radii = false;
                }
                first_radius.get_or_insert(magnitude);
                variable_radii = false;
            }
            BlendRadiusLaw::Linear { .. } | BlendRadiusLaw::Law { .. } => {
                constant_radii = false;
            }
            BlendRadiusLaw::Constant { .. } => {
                constant_radii = false;
                variable_radii = false;
            }
        }
        if matches!(family, NxBlendFamily::Face) {
            if let [Some(first), Some(second)] = supports {
                if first.surface == second.surface {
                    complete_pairs = false;
                } else {
                    let bytes = std::mem::size_of::<[SurfaceId; 2]>();
                    ctx.charge_collection_items(1, "NX blend support pairs")?;
                    pairs_reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
                    cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                        &mut pairs,
                        1,
                        "NX blend support pairs",
                    )?;
                    pairs.push(pairs_reservation.with_storage(|| Ok::<_, CodecError>([first.surface.try_clone_for_decode(ctx, "NX blend first support identity copy")?, second.surface.try_clone_for_decode(ctx, "NX blend second support identity copy")?]))?);
                }
            } else {
                complete_pairs = false;
            }
        }
    }
    if surfaces.is_empty() {
        return Ok(None);
    }
    let sort_work = surfaces
        .len()
        .checked_mul(
            usize::try_from(usize::BITS - surfaces.len().leading_zeros())
                .map_err(|_| ctx.refuse_codec_limit("NX blend result sort", 0, 1))?,
        )
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX blend result sort",
                0,
                cadmpeg_core::decode::u64_from_index(surfaces.len()),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sort_work),
        "NX blend result sort",
    )?;
    surfaces.sort();
    let radius = if constant_radii {
        if uniform_radii {
            first_radius
                .and_then(cadmpeg_ir::scalar::PositiveLength::new)
                .map_or(
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
                    },
                    |radius| RadiusSpec::Constant { radius },
                )
        } else {
            RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
            }
        }
    } else if variable_radii {
        RadiusSpec::Unresolved {
            form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable),
        }
    } else {
        RadiusSpec::Unresolved { form: None }
    };
    let face_blend = if matches!(family, NxBlendFamily::Face) && complete_pairs {
        if let Some(sides) = blend_support_bipartition(ctx, &pairs)? {
            let first_native = ctx.format_retained(
                format_args!("{body}:blend-first-support-surfaces"),
                "NX feature projection text",
            )?;
            let second_native = ctx.format_retained(
                format_args!("{body}:blend-second-support-surfaces"),
                "NX feature projection text",
            )?;
            let (first_faces, _) = support_face_projection(ctx, ir, &sides.first, first_native)?;
            let (second_faces, _) = support_face_projection(ctx, ir, &sides.second, second_native)?;
            match (&first_faces, &second_faces) {
                (FaceSelection::Resolved { .. }, FaceSelection::Resolved { .. }) => {
                    cadmpeg_ir::features::FaceBlendOperands::new(first_faces, second_faces)
                        .ok()
                        .map(|operands| {
                            FeatureDefinition::Operation(FeatureOperation::FaceBlend {
                                operands,
                                radius: radius.clone(),
                            })
                        })
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    let Some(unresolved_operands) = cadmpeg_ir::features::FaceBlendOperands::new(
        FaceSelection::Unresolved,
        FaceSelection::Unresolved,
    )
    .ok() else {
        return Ok(None);
    };
    let unresolved = match family {
        NxBlendFamily::Edge => FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius,
                    tangency_weight: None,
                },
            ),
        }),
        NxBlendFamily::Face => FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: unresolved_operands,
            radius,
        }),
    };
    Ok(Some((face_blend.unwrap_or(unresolved), surfaces)))
}

/// Split an unordered rolling-ball support graph into two deterministic face
/// sets. Face blending is symmetric, so each connected component starts with
/// its lowest surface identity on the first side. The support graph must be
/// complete bipartite: odd cycles and missing cross-pairs cannot be represented
/// by one neutral face-blend operation.
pub(super) struct ScopedBlendSides<'ctx> {
    pub(super) first: Vec<SurfaceId>,
    pub(super) second: Vec<SurfaceId>,
    pub(super) _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn blend_support_bipartition<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    pairs: &[[SurfaceId; 2]],
) -> Result<Option<ScopedBlendSides<'ctx>>, CodecError> {
    let mut adjacent = BTreeMap::<&SurfaceId, BTreeSet<&SurfaceId>>::new();
    let mut reservation = ctx.reserve_scoped(0, "NX blend support graph")?;
    for [first, second] in pairs {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(adjacent.len()),
            "NX blend support graph lookup",
        )?;
        if first == second {
            return Ok(None);
        }
        for (from, to) in [(first, second), (second, first)] {
            if !adjacent.contains_key(from) {
                ctx.charge_collection_items(1, "NX blend support graph nodes")?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<(&SurfaceId, BTreeSet<&SurfaceId>)>() * 4,
                ))?;
            }
            let neighbors = adjacent.entry(from).or_default();
            if !neighbors.contains(to) {
                ctx.charge_collection_items(1, "NX blend support graph edges")?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<&SurfaceId>() * 4,
                ))?;
                neighbors.insert(to);
            }
        }
    }
    let mut sides = BTreeMap::<&SurfaceId, bool>::new();
    let mut pending = Vec::new();
    for seed in adjacent.keys() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(sides.len()),
            "NX blend support side lookup",
        )?;
        if sides.contains_key(seed) {
            continue;
        }
        ctx.charge_collection_items(1, "NX blend support sides")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(&SurfaceId, bool)>() * 4,
        ))?;
        sides.insert(*seed, false);
        ctx.reserve_scoped_vec(&mut reservation, &mut pending, 1, "NX blend support queue")?;
        pending.push(*seed);
        while let Some(surface) = pending.pop() {
            let side = sides[&surface];
            for neighbor in &adjacent[&surface] {
                ctx.charge_work(1, "NX blend support bipartition")?;
                match sides.get(neighbor) {
                    Some(neighbor_side) if *neighbor_side == side => return Ok(None),
                    Some(_) => {}
                    None => {
                        ctx.charge_collection_items(1, "NX blend support sides")?;
                        reservation.grow(cadmpeg_core::decode::u64_from_index(
                            std::mem::size_of::<(&SurfaceId, bool)>() * 4,
                        ))?;
                        sides.insert(*neighbor, !side);
                        ctx.reserve_scoped_vec(
                            &mut reservation,
                            &mut pending,
                            1,
                            "NX blend support queue",
                        )?;
                        pending.push(*neighbor);
                    }
                }
            }
        }
    }
    let mut first = Vec::new();
    let mut second = Vec::new();
    for (&surface, &second_side) in &sides {
        let output = if second_side { &mut second } else { &mut first };
        let bytes = std::mem::size_of::<SurfaceId>();
        ctx.charge_collection_items(1, "NX blend support output")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            output,
            1,
            "NX blend support output",
        )?;
        output.push(reservation.with_storage(|| surface.try_clone_for_decode(ctx, "NX blend support identity copy"))?);
    }
    for surface in &first {
        for other in &second {
            ctx.charge_work(1, "NX blend complete support graph")?;
            if !adjacent[surface].contains(other) {
                return Ok(None);
            }
        }
    }
    Ok(Some(ScopedBlendSides {
        first,
        second,
        _reservation: reservation,
    }))
}

pub(super) fn offset_surface_feature_definition(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
) -> Result<Option<(FeatureDefinition, Vec<SurfaceId>)>, CodecError> {
    let Some(OwnedOffsetSurfaceData {
        body,
        distance,
        supports,
    }) = owned_offset_surface_data(ctx, ir, outputs)?
    else {
        return Ok(None);
    };
    let native = ctx.format_retained(
        format_args!("{}:offset-support-surfaces", body.as_str()),
        "NX feature projection text",
    )?;
    let (faces, senses) = support_face_projection(ctx, ir, &supports, native)?;
    let distance = senses
        .as_deref()
        .and_then(uniform_face_sense)
        .map(|sense| match sense {
            Sense::Forward => distance,
            Sense::Reversed => distance.negated(),
        });
    Ok(Some((
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces,
            distance: distance.map(Length::from_assigned_real),
        }),
        supports,
    )))
}

pub(super) struct OwnedOffsetSurfaceData<'a> {
    body: &'a BodyId,
    distance: FiniteReal,
    supports: Vec<SurfaceId>,
}

pub(super) fn owned_offset_surface_data<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    outputs: &'a [BodyId],
) -> Result<Option<OwnedOffsetSurfaceData<'a>>, CodecError> {
    let Some((body, carriers)) = owned_offset_carriers(ctx, ir, outputs)? else {
        return Ok(None);
    };
    let distance = carriers.values[0].1;
    if carriers
        .values
        .iter()
        .any(|(_, candidate)| candidate.get().to_bits() != distance.get().to_bits())
    {
        return Ok(None);
    }
    let supports = unique_carrier_supports(ctx, &carriers.values)?;
    Ok(Some(OwnedOffsetSurfaceData {
        body,
        distance,
        supports,
    }))
}

pub(super) struct ScopedOffsetCarriers<'a, 'ctx> {
    values: Vec<(&'a SurfaceId, FiniteReal)>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn unique_carrier_supports(
    ctx: &DecodeContext<'_>,
    carriers: &[(&SurfaceId, FiniteReal)],
) -> Result<Vec<SurfaceId>, CodecError> {
    let mut supports = Vec::new();
    for &(support, _) in carriers {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(supports.len()),
            "NX offset support uniqueness",
        )?;
        if supports.contains(support) {
            continue;
        }
        ctx.reserve_vec(&mut supports, 1, "NX offset support output")?;
        supports.push(support.try_clone_for_decode(ctx, "NX feature projection surface identity")?);
    }
    let sort_work = supports
        .len()
        .checked_mul(
            usize::try_from(usize::BITS - supports.len().leading_zeros())
                .map_err(|_| ctx.refuse_codec_limit("NX offset support sort", 0, 1))?,
        )
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX offset support sort",
                0,
                cadmpeg_core::decode::u64_from_index(supports.len()),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sort_work),
        "NX offset support sort",
    )?;
    supports.sort();
    Ok(supports)
}

pub(super) fn owned_offset_carriers<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &'a CadIr,
    outputs: &'a [BodyId],
) -> Result<Option<(&'a BodyId, ScopedOffsetCarriers<'a, 'ctx>)>, CodecError> {
    let [body] = outputs else {
        return Ok(None);
    };
    let Some(body_surfaces) = body_surface_ids(ctx, ir, body)? else {
        return Ok(None);
    };
    let mut carriers = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX offset carriers")?;
    for procedural in &ir.model.procedural_surfaces {
        ctx.charge_work(1, "NX offset procedural surface scan")?;
        let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
            continue;
        };
        if !body_surfaces.contains(owner) {
            continue;
        }
        let ProceduralSurfaceDefinition::Offset(definition_payload) = procedural.definition()
        else {
            continue;
        };
        let support = definition_payload.support();
        let candidate = definition_payload.distance();
        ctx.reserve_scoped_vec(&mut reservation, &mut carriers, 1, "NX offset carriers")?;
        carriers.push((support, candidate));
    }
    Ok((!carriers.is_empty()).then_some((
        body,
        ScopedOffsetCarriers {
            values: carriers,
            _reservation: reservation,
        },
    )))
}

pub(super) fn thicken_feature_definition(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
) -> Result<Option<(FeatureDefinition, Vec<SurfaceId>)>, CodecError> {
    let Some(OwnedThickenSurfaceData {
        body,
        thickness,
        supports,
        direction,
    }) = owned_thicken_surface_data(ctx, ir, outputs)?
    else {
        return Ok(None);
    };
    let native = ctx.format_retained(
        format_args!("{}:thicken-support-surfaces", body.as_str()),
        "NX feature projection text",
    )?;
    let (faces, senses) = support_face_projection(ctx, ir, &supports, native)?;
    let side = match direction {
        ThickenDirection::Both => Some(ThickenSide::Both),
        ThickenDirection::Signed(distance) => senses
            .as_deref()
            .and_then(uniform_face_sense)
            .map(|sense| thicken_side(distance, sense)),
    };
    Ok(Some((
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces,
            thickness: Some(thickness),
            side,
        }),
        supports,
    )))
}

pub(super) enum ThickenDirection {
    Signed(NonZeroLength),
    Both,
}

pub(super) struct OwnedThickenSurfaceData<'a> {
    body: &'a BodyId,
    thickness: PositiveLength,
    supports: Vec<SurfaceId>,
    direction: ThickenDirection,
}

pub(super) fn owned_thicken_surface_data<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    outputs: &'a [BodyId],
) -> Result<Option<OwnedThickenSurfaceData<'a>>, CodecError> {
    let Some((body, carriers)) = owned_offset_carriers(ctx, ir, outputs)? else {
        return Ok(None);
    };
    let Some(output_body) = ir
        .model
        .bodies
        .iter()
        .find(|candidate| candidate.id == *body)
    else {
        return Ok(None);
    };
    if output_body.kind != BodyKind::Solid {
        return Ok(None);
    }
    let distance = carriers.values[0].1;
    if carriers
        .values
        .iter()
        .all(|(_, candidate)| candidate.get().to_bits() == distance.get().to_bits())
    {
        if let Ok(distance) = NonZeroLength::try_from(Length::from_assigned_real(distance)) {
            let supports = unique_carrier_supports(ctx, &carriers.values)?;
            return Ok(Some(OwnedThickenSurfaceData {
                body,
                thickness: distance.abs(),
                supports,
                direction: ThickenDirection::Signed(distance),
            }));
        }
        return Ok(None);
    }

    let mut magnitude = None::<PositiveLength>;
    let mut positive = BTreeSet::new();
    let mut negative = BTreeSet::new();
    let mut support_reservation = ctx.reserve_scoped(0, "NX thicken signed supports")?;
    for &(support, distance) in &carriers.values {
        let Ok(distance) = NonZeroLength::try_from(Length::from_assigned_real(distance)) else {
            return Ok(None);
        };
        let candidate = distance.abs();
        if magnitude.is_some_and(|magnitude| magnitude.get().to_bits() != candidate.get().to_bits())
        {
            return Ok(None);
        }
        magnitude = Some(candidate);
        if distance.get().is_sign_positive() {
            if !positive.contains(support) {
                ctx.charge_collection_items(1, "NX thicken positive supports")?;
                support_reservation.grow(cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<&SurfaceId>() * 4,
                ))?;
            }
            positive.insert(support);
        } else {
            if !negative.contains(support) {
                ctx.charge_collection_items(1, "NX thicken negative supports")?;
                support_reservation.grow(cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<&SurfaceId>() * 4,
                ))?;
            }
            negative.insert(support);
        }
    }
    if positive.is_empty() || positive != negative {
        return Ok(None);
    }
    let Some(magnitude) = magnitude else {
        return Ok(None);
    };
    let Some(thickness) = PositiveLength::new(magnitude.get() * 2.0) else {
        return Ok(None);
    };
    let mut supports = Vec::new();
    for support in positive {
        ctx.reserve_vec(&mut supports, 1, "NX thicken support output")?;
        supports.push(support.try_clone_for_decode(ctx, "NX feature projection surface identity")?);
    }
    Ok(Some(OwnedThickenSurfaceData {
        body,
        thickness,
        supports,
        direction: ThickenDirection::Both,
    }))
}

pub(super) fn support_face_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    supports: &[SurfaceId],
    native: String,
) -> Result<(FaceSelection, Option<Vec<Sense>>), CodecError> {
    let mut selected = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX support face projection")?;
    for support in supports {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.faces.len()),
            "NX support face lookup",
        )?;
        let mut matches = ir
            .model
            .faces
            .iter()
            .filter(|face| face.surface == *support);
        let Some(face) = matches.next() else {
            return Ok((FaceSelection::Native(native), None));
        };
        if matches.next().is_some() {
            return Ok((FaceSelection::Native(native), None));
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(selected.len()),
            "NX support face uniqueness",
        )?;
        if selected.iter().any(|(id, _)| *id == face.id) {
            return Ok((FaceSelection::Native(native), None));
        }
        let bytes = std::mem::size_of_val(&face.id)
            .checked_add(std::mem::size_of::<Sense>())
            .and_then(|bytes| bytes.checked_add(face.id.as_str().len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX support face projection", 0, 1))?;
        ctx.charge_collection_items(1, "NX support face projection")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut selected,
            1,
            "NX support face projection",
        )?;
        selected.push((face.id.clone(), face.sense));
    }
    let mut faces = Vec::new();
    let mut senses = Vec::new();
    for (face, sense) in selected {
        ctx.charge_collection_items(2, "NX resolved support faces")?;
        let bytes = std::mem::size_of_val(&face)
            .checked_add(std::mem::size_of::<Sense>())
            .and_then(|bytes| bytes.checked_add(face.as_str().len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX resolved support faces", 0, 1))?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX resolved support faces",
        )?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut faces,
            1,
            "NX resolved support faces",
        )?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut senses,
            1,
            "NX resolved support senses",
        )?;
        faces.push(face);
        senses.push(sense);
    }
    Ok((FaceSelection::Resolved { faces, native }, Some(senses)))
}

pub(super) fn thicken_side(distance: NonZeroLength, sense: Sense) -> ThickenSide {
    match (distance.get().is_sign_positive(), sense) {
        (true, Sense::Forward) | (false, Sense::Reversed) => ThickenSide::Forward,
        (true, Sense::Reversed) | (false, Sense::Forward) => ThickenSide::Reverse,
    }
}

pub(super) fn uniform_face_sense(senses: &[Sense]) -> Option<Sense> {
    let (first, rest) = senses.split_first()?;
    rest.iter().all(|sense| sense == first).then_some(*first)
}

pub(in crate::native) fn feature_source_content(
    ctx: &DecodeContext<'_>,
    payload_strings: &[&crate::native::features::FeaturePayloadString],
) -> Result<cadmpeg_ir::features::FeatureContent, CodecError> {
    let mut sorted = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX feature source text order")?;
    for &value in payload_strings {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut sorted,
            1,
            "NX feature source text order",
        )?;
        sorted.push(value);
    }
    let count = sorted.len();
    let passes = usize::try_from(usize::BITS - count.leading_zeros()).map_err(|_| {
        ctx.refuse_codec_limit(
            "NX feature source text sort",
            0,
            cadmpeg_core::decode::u64_from_index(count),
        )
    })?;
    let work = count.checked_mul(passes).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX feature source text sort",
            0,
            cadmpeg_core::decode::u64_from_index(count),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "NX feature source text sort",
    )?;
    sorted.sort_by_key(|value| value.source_offset);
    let mut content = Vec::new();
    for value in sorted {
        let text = value.value.as_str();
        let bytes = std::mem::size_of::<FeatureSourceContent>()
            .checked_add(text.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX feature source text",
                    0,
                    cadmpeg_core::decode::u64_from_index(text.len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX feature source text")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX feature source text",
        )?;
        let mut owned = String::new();
        cadmpeg_core::decode::DecodeContext::reserve_admitted_string(
            &mut owned,
            text.len(),
            "allocate NX feature source text",
        )?;
        owned.push_str(text);
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut content,
            1,
            "NX feature source text",
        )?;
        content.push(FeatureSourceContent::Text(owned));
    }
    cadmpeg_ir::features::FeatureContent::try_from_for_decode(content, ctx, "NX feature source content validation").map_err(CodecError::from)
}

pub(super) fn simple_hole_native_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    operation_label: &str,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    repeated_lanes: &[crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLane],
    block_references: &[crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
    construction_groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
) -> Result<(), CodecError> {
    fn insert_property(
        ctx: &DecodeContext<'_>,
        properties: &mut BTreeMap<String, String>,
        key: &'static str,
        value: &str,
    ) -> Result<(), CodecError> {
        let bytes = std::mem::size_of::<(String, String)>()
            .checked_add(key.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX simple hole native property",
                    0,
                    cadmpeg_core::decode::u64_from_index(value.len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX simple hole native property")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX simple hole native property",
        )?;
        properties.insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    if let Some(template) = templates
        .iter()
        .find(|template| template.operation_label == operation_label)
    {
        insert_property(ctx, properties, "simple_hole_template", &template.id)?;
    }
    if let Some(pair) = repeated_lanes
        .iter()
        .find(|pair| pair.operation_label == operation_label)
    {
        insert_property(
            ctx,
            properties,
            "simple_hole_repeated_scalar_lane",
            &pair.id,
        )?;
    }
    if let Some(references) = block_references
        .iter()
        .find(|references| references.operation_label == operation_label)
    {
        insert_property(
            ctx,
            properties,
            "simple_hole_repeated_scalar_lane_block_references",
            &references.id,
        )?;
    }
    if let Some(group) = construction_groups.iter().find(|group| {
        group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|label| label == operation_label)
    }) {
        insert_property(ctx, properties, "simple_hole_construction_group", &group.id)?;
    }
    Ok(())
}

pub(super) fn block_placement(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    dimensions: [f64; 3],
    outputs: &[BodyId],
) -> Result<Option<(BodyId, Transform)>, CodecError> {
    struct PlaneBand {
        normal: Vector3,
        offsets: Vec<f64>,
    }

    #[derive(Clone, Copy)]
    struct PlaneExtent {
        normal: Vector3,
        minimum: f64,
        maximum: f64,
    }

    fn canonical_normal(
        normal: cadmpeg_ir::units::UnitVector3,
        angular_tolerance: f64,
    ) -> Option<Vector3> {
        let mut normal = *normal.to_unit_length_charted().as_raw();
        let leading = [normal.x, normal.y, normal.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)?;
        if leading < 0.0 {
            normal = Vector3::new(-normal.x, -normal.y, -normal.z);
        }
        Some(normal)
    }

    fn plane_extent(
        ctx: &DecodeContext<'_>,
        band: &mut PlaneBand,
        linear_tolerance: f64,
    ) -> Result<Option<PlaneExtent>, CodecError> {
        let count = band.offsets.len();
        let passes = usize::try_from(usize::BITS - count.leading_zeros()).map_err(|_| {
            ctx.refuse_codec_limit(
                "NX block plane sort",
                0,
                cadmpeg_core::decode::u64_from_index(count),
            )
        })?;
        let work = count.checked_mul(passes).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX block plane sort",
                0,
                cadmpeg_core::decode::u64_from_index(count),
            )
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "NX block plane sort",
        )?;
        band.offsets.sort_by(f64::total_cmp);
        let mut first: Option<[f64; 2]> = None;
        let mut second: Option<[f64; 2]> = None;
        for &offset in &band.offsets {
            if !offset.is_finite() {
                return Ok(None);
            }
            if let Some(cluster) = second.as_mut().or(first.as_mut()) {
                if offset - cluster[0] <= linear_tolerance {
                    cluster[1] = offset;
                    continue;
                }
            }
            if first.is_none() {
                first = Some([offset, offset]);
            } else if second.is_none() {
                second = Some([offset, offset]);
            } else {
                return Ok(None);
            }
        }
        let (Some(minimum), Some(maximum)) = (first, second) else {
            return Ok(None);
        };
        if maximum[1] - minimum[0] <= linear_tolerance {
            return Ok(None);
        }
        Ok(Some(PlaneExtent {
            normal: band.normal,
            minimum: minimum[0],
            maximum: maximum[1],
        }))
    }

    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    if dimensions
        .iter()
        .any(|dimension| *dimension <= linear_tolerance)
    {
        return Ok(None);
    }
    let body = match outputs {
        [body] => body,
        [] => {
            let mut unique = None;
            for candidate in &ir.model.bodies {
                if connected_solid_body_faces(ctx, ir, &candidate.id)?.is_some()
                    && unique.replace(&candidate.id).is_some()
                {
                    return Ok(None);
                }
            }
            let Some(body) = unique else {
                return Ok(None);
            };
            body
        }
        _ => return Ok(None),
    };
    let Some(faces) = connected_solid_body_faces(ctx, ir, body)? else {
        return Ok(None);
    };
    let mut bands = Vec::<PlaneBand>::new();
    let mut band_reservation = ctx.reserve_scoped(0, "NX block plane bands")?;
    for face in faces.iter().copied() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()),
            "NX block plane surface lookup",
        )?;
        let Some(geometry) = ir
            .model
            .surfaces
            .iter()
            .rev()
            .find(|surface| surface.id == face.surface)
            .map(|surface| &surface.geometry)
        else {
            return Ok(None);
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = geometry else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let Some(normal) = canonical_normal(*plane_surface.frame().axis(), angular_tolerance)
        else {
            return Ok(None);
        };
        let offset = normal.dot(Vector3::new(origin.x, origin.y, origin.z));
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(bands.len()),
            "NX block plane band lookup",
        )?;
        let existing = bands
            .iter_mut()
            .find(|band| (1.0 - band.normal.dot(normal)).abs() <= angular_tolerance);
        ctx.charge_collection_items(1, "NX block plane offsets")?;
        band_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<f64>(),
        ))?;
        if let Some(band) = existing {
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut band.offsets,
                1,
                "NX block plane offsets",
            )?;
            band.offsets.push(offset);
        } else {
            ctx.charge_collection_items(1, "NX block plane bands")?;
            band_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                PlaneBand,
            >()))?;
            let mut offsets = Vec::new();
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut offsets,
                1,
                "NX block plane offsets",
            )?;
            offsets.push(offset);
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut bands,
                1,
                "NX block plane bands",
            )?;
            bands.push(PlaneBand { normal, offsets });
        }
    }
    if bands.len() != 3
        || (0..3).any(|first| {
            (first + 1..3).any(|second| {
                bands[first].normal.dot(bands[second].normal).abs() > angular_tolerance
            })
        })
    {
        return Ok(None);
    }
    let [first, second, third] = bands.as_mut_slice() else {
        return Ok(None);
    };
    let (Some(first), Some(second), Some(third)) = (
        plane_extent(ctx, first, linear_tolerance)?,
        plane_extent(ctx, second, linear_tolerance)?,
        plane_extent(ctx, third, linear_tolerance)?,
    ) else {
        return Ok(None);
    };
    let mut extents = [first, second, third];
    extents.sort_by(|left, right| {
        right
            .normal
            .x
            .total_cmp(&left.normal.x)
            .then_with(|| right.normal.y.total_cmp(&left.normal.y))
            .then_with(|| right.normal.z.total_cmp(&left.normal.z))
    });
    let permutations = [
        [0usize, 1usize, 2usize],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut matched = None;
    for permutation in permutations {
        if (0..3).all(|axis| {
            let band = extents[permutation[axis]];
            ((band.maximum - band.minimum) - dimensions[axis]).abs() <= linear_tolerance
        }) && matched.replace(permutation).is_some()
        {
            return Ok(None);
        }
    }
    let Some(permutation) = matched else {
        return Ok(None);
    };
    let mut ordered = permutation.map(|index| extents[index]);
    if ordered[0]
        .normal
        .cross(ordered[1].normal)
        .dot(ordered[2].normal)
        < 0.0
    {
        let third = &mut ordered[2];
        third.normal = Vector3::new(-third.normal.x, -third.normal.y, -third.normal.z);
        (third.minimum, third.maximum) = (-third.maximum, -third.minimum);
    }
    let origin = Point3::new(
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.x)
            .sum(),
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.y)
            .sum(),
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.z)
            .sum(),
    );
    let [x_axis, y_axis, z_axis] = ordered.map(|band| band.normal);
    let Some(placement) = Transform::affine([
        [x_axis.x, y_axis.x, z_axis.x, origin.x],
        [x_axis.y, y_axis.y, z_axis.y, origin.y],
        [x_axis.z, y_axis.z, z_axis.z, origin.z],
    ]) else {
        return Ok(None);
    };
    let body_bytes = std::mem::size_of::<BodyId>()
        .checked_add(body.as_str().len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX block output body",
                0,
                cadmpeg_core::decode::u64_from_index(body.as_str().len()),
            )
        })?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(body_bytes),
        "NX block output body",
    )?;
    Ok(Some((body.clone(), placement)))
}

/// Return the complete primitive witness for an NX `SPHERE` operation.
///
/// A spherical surface inside a larger result is not enough: a Boolean or a
/// later feature can leave the same carrier in the output. The primitive
/// projection therefore accepts only one connected solid body with exactly
/// one face whose surface is a finite positive-radius sphere. With no native
/// output relation, the candidate must also be unique across the model.
pub(super) fn sphere_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
) -> Result<
    Option<(
        BodyId,
        cadmpeg_ir::features::FinitePoint3,
        cadmpeg_ir::scalar::PositiveLength,
    )>,
    CodecError,
> {
    let body = match outputs {
        [body] => body,
        [] => {
            let mut unique = None;
            for candidate in &ir.model.bodies {
                let Some(faces) = connected_solid_body_faces(ctx, ir, &candidate.id)? else {
                    continue;
                };
                let [face] = &faces[..] else {
                    continue;
                };
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()),
                    "NX sphere fallback surface scan",
                )?;
                if !ir.model.surfaces.iter().any(|surface| {
                    surface.id == face.surface
                        && matches!(
                            surface.geometry.solved(),
                            Some(SolvedSurfaceGeometry::Sphere(_))
                        )
                }) {
                    continue;
                }
                if unique.replace(&candidate.id).is_some() {
                    return Ok(None);
                }
            }
            let Some(body) = unique else {
                return Ok(None);
            };
            body
        }
        _ => return Ok(None),
    };
    let Some(faces) = connected_solid_body_faces(ctx, ir, body)? else {
        return Ok(None);
    };
    let [face] = &faces[..] else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()),
        "NX sphere surface lookup",
    )?;
    let Some(surface) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
    else {
        return Ok(None);
    };
    let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
        return Ok(None);
    };
    let center = sphere_surface.center();
    let Ok(radius) = cadmpeg_ir::scalar::PositiveLength::try_from(sphere_surface.radius()) else {
        return Ok(None);
    };
    let body_bytes = std::mem::size_of::<BodyId>()
        .checked_add(body.as_str().len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX sphere output body",
                0,
                cadmpeg_core::decode::u64_from_index(body.as_str().len()),
            )
        })?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(body_bytes),
        "NX sphere output body",
    )?;
    Ok(Some((body.clone(), center, radius)))
}

pub(super) struct NewBodyEvidence<'a> {
    pub(super) has_complete_projection: bool,
    pub(super) has_complete_primitive_construction: bool,
    pub(super) outputs: &'a [BodyId],
    pub(super) body_reference_count: usize,
    pub(super) provisional_feature: Option<&'a FeatureId>,
    pub(super) native_primary_body: Option<u32>,
    pub(super) offset_store_primary_body: Option<&'a str>,
    pub(super) history: &'a BodyWriterHistory,
}

pub(super) fn new_body_boolean_op(evidence: &NewBodyEvidence<'_>) -> BooleanOp {
    // A unique offset-store body field proves the operation's local writer
    // namespace, but the fallback body selected for placement is not that
    // writer. Likewise, multiple body fields have no primary role until the
    // operation-specific relation identifies one. Do not let a placement
    // fallback turn either case into a neutral body Boolean.
    if evidence.body_reference_count > 1
        && evidence.native_primary_body.is_none()
        && evidence.offset_store_primary_body.is_none()
        && !evidence.has_complete_primitive_construction
    {
        return BooleanOp::Unresolved;
    }
    if evidence.has_complete_projection
        && matches!(evidence.outputs, [_])
        && !evidence.history.has_preceding_writer(
            evidence.provisional_feature,
            evidence.native_primary_body,
            evidence.offset_store_primary_body,
            evidence.outputs,
        )
    {
        BooleanOp::NewBody
    } else {
        BooleanOp::Unresolved
    }
}

pub(super) fn body_writing_unresolved_feature_definition(
    kind: &str,
    source_properties: &BTreeMap<String, String>,
) -> Option<FeatureDefinition> {
    if !source_properties
        .keys()
        .any(|key| key.starts_with("body_write."))
    {
        return None;
    }
    match kind {
        "BREP" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Brep,
        })),
        "CONE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Cone,
        })),
        "SPHERE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Sphere,
        })),
        "BLEND" => Some(FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                },
            ),
        })),
        "FACE_BLEND" => Some(FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
            )
            .ok()?,

            radius: RadiusSpec::Unresolved { form: None },
        })),
        "DELETE FACE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DeleteFace,
        })),
        "MIRROR_FACE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MirrorFace,
        })),
        "SUBDIVISION_BODY" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::SubdivisionBody,
        })),
        "TOPOLOGY_OPTIMIZATION" => {
            Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::TopologyOptimization,
            }))
        }
        "THREADS" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Thread,
        })),
        "DETAILED_THREAD" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DetailedThread,
        })),
        _ => None,
    }
}

#[cfg(test)]
pub(super) fn non_boolean_feature_definition(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole_diameter: Option<Length>,
) -> FeatureDefinition {
    non_boolean_feature_definition_with_parameters(
        kind,
        payload_strings,
        block_dimensions,
        block_placement,
        HoleProjection {
            diameter: hole_diameter,
            ..HoleProjection::default()
        },
        BTreeMap::new(),
    )
    .unwrap()
}

/// Project one operation as a history node only when its bounded record has
/// no modeling relation or value lane.
pub(super) fn non_modeling_history_definition(
    kind: &str,
    object_indices: &[Option<u32>; 4],
    outputs: &[BodyId],
    body_reference_count: usize,
    body_operand_count: usize,
    payload_string_count: usize,
    source_properties: &BTreeMap<String, String>,
) -> Option<FeatureDefinition> {
    let operation_identity_only = source_properties.keys().all(|key| {
        matches!(
            key.as_str(),
            "operation_record" | "operation_terminal_frame"
        ) || (key
            .strip_prefix("object_index.")
            .is_some_and(|slot| matches!(slot, "0" | "1" | "2" | "3")))
    });
    (kind == "EXTRACT_STRING"
        && object_indices.iter().all(Option::is_none)
        && outputs.is_empty()
        && body_reference_count == 0
        && body_operand_count == 0
        && payload_string_count == 0
        && source_properties.contains_key("operation_record")
        && source_properties.contains_key("operation_terminal_frame")
        && operation_identity_only)
        .then_some(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: TreeChildren::default(),
        }))
}

/// Permutation-invariant hole properties derived from one complete body partition.
#[derive(Default)]
pub(super) struct HoleProjection {
    pub(super) placements: Vec<HolePlacement>,
    pub(super) diameter: Option<Length>,
    pub(super) extent: Option<LinearTermination>,
    pub(super) counterbore: Option<CounterboreDimensions>,
    pub(super) chamfer: Option<HoleKind>,
    pub(super) grouped_simple_through: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CounterboreDimensions {
    pub(super) diameter: cadmpeg_ir::scalar::PositiveLength,
    pub(super) depth: cadmpeg_ir::scalar::PositiveLength,
}

pub(super) fn non_boolean_feature_definition_with_parameters(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole: HoleProjection,
    native_parameters: BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<FeatureDefinition, CodecError> {
    let hole_template = unique_simple_hole_template(payload_strings);
    if matches!(kind, "BLEND" | "FACE_BLEND") {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: kind.into(),
            parameters: native_parameters,
        }));
    }
    if let ("BLOCK", Some([Some(length), Some(width), Some(height)])) = (
        kind,
        block_dimensions.map(|dimensions| dimensions.map(cadmpeg_ir::scalar::PositiveLength::new)),
    ) {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions: Some([length, width, height]),
            placement: block_placement.and_then(cadmpeg_ir::features::FeatureRigidPlacement::new),
            op: BooleanOp::Unresolved,
        }));
    }
    if let Some(op) = match kind {
        "UNITE" => Some(cadmpeg_ir::features::BooleanKind::Join),
        "SUBTRACT" => Some(cadmpeg_ir::features::BooleanKind::Cut),
        "INTERSECT" => Some(cadmpeg_ir::features::BooleanKind::Intersect),
        _ => None,
    } {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            op,
            keep_tools: false,
        }));
    }
    Ok(match kind {
        "DATUM_PLANE" | "EXTRACT_DATUM_PLANE" => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            })
        }
        "DATUM_AXIS" | "EXTRACT_DATUM_AXIS" => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumAxis,
            })
        }
        "BRIDGE_CURVE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::BridgeCurve,
        }),
        "POINT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPoint,
        }),
        "DATUM_CSYS" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem,
        }),
        "BLOCK" => FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        }),
        "SKETCH" => FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
        }),
        "EXTRACT_BODY" => FeatureDefinition::Operation(FeatureOperation::ExtractBody {
            source: BodySelection::Unresolved,
        }),
        "MASTER SNAPSHOT BODY" => FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        }),
        "SKIN" | "THRU_CURVE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Loft,
        }),
        "THRU_CURVE_MESH" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::ThroughCurveMesh,
        }),
        "Studio Surface" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::FreeformSurface,
        }),
        "SWP104" => FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::unresolved(None),
            path: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: None,
            allow_multi_profile_faces: None,
        }),
        "DRAFT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Draft,
        }),
        "CPROJ" | "CPROJ_CMB" => FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
            source: PathRef::Unresolved("nx:unresolved".into()),
            target_faces: FaceSelection::Unresolved,
            direction: CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved),
            bidirectional: None,
        }),
        "TRIMMED_SH" => FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: PathRef::Unresolved("nx:unresolved".into()),
            keep: TrimRegion::Unresolved,
        }),
        "EXTRACT_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::ExtractFace,
        }),
        "COPY_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::CopyFace,
        }),
        "LINKED_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::LinkedFace,
        }),
        "FILL_HOLE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::FillHole,
        }),
        "MOVE_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MoveFace,
        }),
        "MOVE_OBJECT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MoveObject,
        }),
        "CYLINDER" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Cylinder,
        }),
        "SYMBOLIC_THREAD" => symbolic_thread_feature_definition(),
        "EXTEND_SHEET" => FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        }),
        "SIMPLE HOLE" | "CBORE_HOLE" | "CSUNK_HOLE" => {
            let measured_chamfer = hole.chamfer;
            let (template_kind, template_exit_kind, template_extent) = hole_template.map_or(
                (
                    if matches!(kind, "CBORE_HOLE" | "CSUNK_HOLE") {
                        HoleKind::Unresolved(None)
                    } else {
                        HoleKind::Simple
                    },
                    None,
                    None,
                ),
                |(form, extent, start_treatment, end_treatment)| {
                    let kind = match start_treatment {
                        crate::native::features::holes::SimpleHoleEndTreatment::Chamfer => {
                            HoleKind::Unresolved(Some(HoleForm::Chamfer))
                        }
                        crate::native::features::holes::SimpleHoleEndTreatment::None => {
                            match form {
                                crate::native::features::holes::SimpleHoleForm::Simple => {
                                    HoleKind::Simple
                                }
                                crate::native::features::holes::SimpleHoleForm::Counterbored => {
                                    HoleKind::Unresolved(Some(HoleForm::Counterbore))
                                }
                                crate::native::features::holes::SimpleHoleForm::Countersunk => {
                                    HoleKind::Unresolved(Some(HoleForm::Countersink))
                                }
                            }
                        }
                    };
                    let exit_kind = match end_treatment {
                        crate::native::features::holes::SimpleHoleEndTreatment::Chamfer => {
                            Some(HoleKind::Unresolved(Some(HoleForm::Chamfer)))
                        }
                        crate::native::features::holes::SimpleHoleEndTreatment::None => None,
                    };
                    let extent = match extent {
                        crate::native::features::holes::SimpleHoleExtent::Through => {
                            Some(cadmpeg_ir::features::LinearTermination::ThroughAll {})
                        }
                        crate::native::features::holes::SimpleHoleExtent::Blind => None,
                    };
                    (kind, exit_kind, extent)
                },
            );
            let template_kind = match (
                hole.counterbore,
                matches!(
                    &template_kind,
                    HoleKind::Unresolved(Some(HoleForm::Counterbore))
                        | HoleKind::PartialCounterbore(..)
                ),
            ) {
                (Some(dimensions), true) => HoleKind::Counterbore {
                    diameter: dimensions.diameter,
                    depth: dimensions.depth,
                },
                _ => template_kind,
            };
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: Some(hole.placements).filter(|placements| !placements.is_empty()),
                shape: cadmpeg_ir::features::holes::HoleShape::new(
                    cadmpeg_ir::features::holes::HoleConstruction::Form {
                        kind: match (measured_chamfer, hole_template) {
                            (
                                Some(chamfer),
                                Some((
                                    crate::native::features::holes::SimpleHoleForm::Simple,
                                    crate::native::features::holes::SimpleHoleExtent::Through,
                                    crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                    crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                )),
                            ) => chamfer,
                            _ => template_kind,
                        },
                        specification: None,
                    },
                    match (measured_chamfer, hole_template) {
                        (
                            Some(chamfer),
                            Some((
                                crate::native::features::holes::SimpleHoleForm::Simple,
                                crate::native::features::holes::SimpleHoleExtent::Through,
                                crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                            )),
                        ) => Some(chamfer),
                        _ => template_exit_kind,
                    },
                    hole.diameter.and_then(|diameter| {
                        cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()
                    }),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,

                extent: hole.extent.or(template_extent),
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            })
        }
        "HOLE PACKAGE" => FeatureDefinition::Operation(FeatureOperation::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            direction: None,
            placements: Some(hole.placements).filter(|placements| !placements.is_empty()),
            shape: cadmpeg_ir::features::holes::HoleShape::new(
                cadmpeg_ir::features::holes::HoleConstruction::Form {
                    kind: if hole.grouped_simple_through {
                        hole.chamfer.unwrap_or(HoleKind::Simple)
                    } else {
                        HoleKind::Unresolved(None)
                    },
                    specification: None,
                },
                hole.grouped_simple_through
                    .then_some(hole.chamfer)
                    .flatten(),
                hole.diameter.and_then(|diameter| {
                    cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()
                }),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            extent: hole
                .grouped_simple_through
                .then_some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        }),
        "RIB" => FeatureDefinition::Operation(FeatureOperation::Rib {
            construction: RibConstruction {
                profile: None,
                direction: None,
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Unresolved,
        }),
        "SHELL" => shell_feature_definition(),
        "ENLARGE" => enlarge_feature_definition(),
        "CHAMFER" => FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Unresolved { form: None },
                },
            ),
            flip_direction: false,
        }),
        "BLEND" => FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                },
            ),
        }),
        "FACE_BLEND" => FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            radius: RadiusSpec::Unresolved { form: None },
        }),
        "SEW" => FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: BodySelection::Unresolved
                .try_into()
                .map_err(CodecError::malformed)?,
            gap_tolerance: None,
        }),
        "TRIM BODY" => FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }),
        "EXTRUDE" => extrude_feature_definition(None, None, BooleanOp::Unresolved, &[]),
        "OFFSET" => FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        }),
        "THICKEN_SHEET" => FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        }),
        "Pattern Feature"
        | "Pattern Geometry"
        | "Geometry Instance"
        | "Multi Instance Output"
        | "IDENTICAL INSTANCE OUTPUT"
        | "Instance Feature" => FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::UNRESOLVED,
        }),
        "ASSOCIATIVE_INTERSECTION" | "Intersection Curve" => {
            FeatureDefinition::Operation(FeatureOperation::SectionShape {
                operands: cadmpeg_ir::features::SectionOperands::new(
                    BodySelection::Unresolved,
                    BodySelection::Unresolved,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,

                approximate: None,
            })
        }
        _ => FeatureDefinition::Operation(FeatureOperation::Native {
            kind: kind.into(),
            parameters: native_parameters,
        }),
    })
}

/// Project a BREP operation as direct stored geometry when its result bodies
/// are resolved. A BREP record carries boundary representation rather than a
/// replayable parametric construction; without a closed result-body relation,
/// retaining the native definition preserves the unresolved history edge.
pub(super) fn brep_feature_definition(
    ctx: &DecodeContext<'_>,
    outputs: &[BodyId],
) -> Result<Option<FeatureDefinition>, CodecError> {
    for (index, body) in outputs.iter().enumerate() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(index),
            "NX BREP output uniqueness",
        )?;
        if outputs[..index].contains(body) {
            return Ok(None);
        }
    }
    Ok(
        (!outputs.is_empty()).then_some(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
    )
}

/// Preserve a SHELL operation as a typed neutral family while its construction roles remain
/// unresolved. The operation label identifies the family, but does not assign bodies, opening
/// faces, thickness, side, offset mode, corner join, or intersection policy.
pub(super) fn shell_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: FaceSelection::Unresolved,
        thickness: None,
        outward: None,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    })
}

/// Preserve an ENLARGE operation as a typed surface-extension family while its selected faces,
/// extension law, and extent remain unresolved.
pub(super) fn enlarge_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces: FaceSelection::Unresolved,
        distance: None,
        method: SurfaceExtension::Unresolved,
    })
}

/// Preserve a `SYMBOLIC_THREAD` operation as a cosmetic-thread family while its cylindrical face,
/// nominal diameter, and axial extent remain unresolved.
pub(super) fn symbolic_thread_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
        face: FaceSelection::Unresolved,
        diameter: None,
        extent: None,
    })
}

pub(super) fn native_feature_parameters(
    ctx: &DecodeContext<'_>,
    uses: &[&crate::native::features::FeatureParameterUse],
    expressions: &[crate::native::om::ParameterFormula],
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut parameters = BTreeMap::new();
    for parameter_use in uses {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(expressions.len()),
            "NX native parameter expression lookup",
        )?;
        let Some(expression) = expressions
            .iter()
            .rev()
            .find(|expression| expression.id == parameter_use.expression)
        else {
            return Ok(BTreeMap::new());
        };
        if parameters.contains_key(expression.name.as_str()) {
            return Ok(BTreeMap::new());
        }
        let bytes = std::mem::size_of::<(String, String)>()
            .checked_add(expression.name.as_str().len())
            .and_then(|bytes| bytes.checked_add(expression.expression.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX native feature parameter",
                    0,
                    cadmpeg_core::decode::u64_from_index(expression.expression.len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX native feature parameter")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX native feature parameter",
        )?;
        parameters.insert(
            expression.name.as_str().to_owned(),
            expression.expression.clone(),
        );
    }
    Ok(parameters)
}

/// Resolve explicit hole outputs only from the proven segment-body namespace.
/// Offset-store body fields remain absent so a complete unique-solid topology
/// witness can apply the documented fallback.
pub(super) fn primary_hole_outputs(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    body_references: &BTreeMap<&str, u32>,
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<BTreeMap<String, Vec<BodyId>>, CodecError> {
    let mut outputs = BTreeMap::new();
    for template in templates {
        ctx.charge_work(1, "NX primary hole output scan")?;
        let Some(object_index) = body_references.get(template.operation_label.as_str()) else {
            continue;
        };
        let bodies =
            feature_body_outputs(ctx, *object_index, body_bindings, bodies_by_object_index)?;
        ctx.admit_retained_btree_record::<String, Vec<BodyId>>(
            template.operation_label.len(),
            "NX primary hole output map",
        )?;
        outputs.insert(template.operation_label.clone(), bodies);
    }
    Ok(outputs)
}

pub(super) fn charge_hole_sort_work(
    ctx: &DecodeContext<'_>,
    count: usize,
) -> Result<(), CodecError> {
    let work = count.checked_mul(count).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX hole operation sort",
            0,
            cadmpeg_core::decode::u64_from_index(count),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "NX hole operation sort",
    )
}

pub(super) fn simple_hole_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
    operation_positions: &BTreeMap<&str, usize>,
) -> Result<Option<Vec<String>>, CodecError> {
    let mut ordered_templates = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX simple hole selected templates")?;
    for template in templates {
        ctx.charge_work(1, "NX simple hole template scan")?;
        if template.form != crate::native::features::holes::SimpleHoleForm::Simple
            || template.extent != crate::native::features::holes::SimpleHoleExtent::Through
        {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(templates.len()),
            "NX simple hole template identity scan",
        )?;
        if templates
            .iter()
            .filter(|candidate| candidate.operation_label == template.operation_label)
            .count()
            != 1
            || !operation_positions.contains_key(template.operation_label.as_str())
        {
            return Ok(None);
        }
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut ordered_templates,
            1,
            "NX simple hole selected templates",
        )?;
        ordered_templates.push(template);
    }
    if ordered_templates.is_empty() {
        return Ok(None);
    }
    charge_hole_sort_work(ctx, ordered_templates.len())?;
    ordered_templates.sort_by(|first, second| {
        operation_positions
            .get(first.operation_label.as_str())
            .cmp(&operation_positions.get(second.operation_label.as_str()))
            .then_with(|| first.operation_label.cmp(&second.operation_label))
    });
    let mut selected_group = None;
    for group in groups {
        let comparisons = group
            .members
            .len()
            .checked_mul(ordered_templates.len())
            .and_then(|count| count.checked_mul(2))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX simple hole group membership",
                    0,
                    cadmpeg_core::decode::u64_from_index(group.members.len()),
                )
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(comparisons),
            "NX simple hole group membership",
        )?;
        let same_operations = group.members.iter().all(|member| {
            ordered_templates
                .iter()
                .any(|template| template.operation_label == member.operation_label)
        }) && ordered_templates.iter().all(|template| {
            group
                .members
                .iter()
                .any(|member| member.operation_label == template.operation_label)
        });
        if same_operations && selected_group.replace(group).is_some() {
            return Ok(None);
        }
    }
    let mut operations = Vec::new();
    if let Some(group) = selected_group {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(group.members.len()),
            "NX simple hole group order",
        )?;
        if group
            .members
            .iter()
            .any(|member| !operation_positions.contains_key(member.operation_label.as_str()))
            || group.members.windows(2).any(|pair| {
                operation_positions[pair[0].operation_label.as_str()]
                    >= operation_positions[pair[1].operation_label.as_str()]
            })
        {
            return Ok(None);
        }
        for member in group.members.iter() {
            ctx.push_retained_vec(
                &mut operations,
                ctx.copy_retained_text(&member.operation_label, "NX hole operation labels")?,
                "NX hole operation labels",
            )?;
        }
    } else {
        for template in ordered_templates {
            ctx.push_retained_vec(
                &mut operations,
                ctx.copy_retained_text(&template.operation_label, "NX hole operation labels")?,
                "NX hole operation labels",
            )?;
        }
    }
    Ok(Some(operations))
}

/// Select uniquely typed hole operations in feature-history order.
pub(super) fn selected_hole_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
    accepts: impl Fn(&crate::native::features::holes::FeatureSimpleHoleTemplate) -> bool,
) -> Result<Option<Vec<String>>, CodecError> {
    let mut operations = Vec::new();
    for template in templates {
        ctx.charge_work(1, "NX selected hole template scan")?;
        if !accepts(template) {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(templates.len()),
            "NX selected hole template identity scan",
        )?;
        if templates
            .iter()
            .filter(|candidate| candidate.operation_label == template.operation_label)
            .count()
            == 1
        {
            ctx.push_retained_vec(
                &mut operations,
                ctx.copy_retained_text(&template.operation_label, "NX hole operation labels")?,
                "NX hole operation labels",
            )?;
        }
    }
    if operations.is_empty() || !hole_operations_are_unique(ctx, &operations)? {
        return Ok(None);
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(operations.len()),
        "NX selected hole operation positions",
    )?;
    if operations
        .iter()
        .any(|operation| !operation_positions.contains_key(operation.as_str()))
    {
        return Ok(None);
    }
    charge_hole_sort_work(ctx, operations.len())?;
    operations.sort_by(|first, second| {
        operation_positions
            .get(first.as_str())
            .cmp(&operation_positions.get(second.as_str()))
            .then_with(|| first.cmp(second))
    });
    Ok(Some(operations))
}

/// Return simple blind-hole operations in feature-history order. A blind
/// operation with competing typed templates is not assignable to one body
/// witness and remains native-only.
pub(super) fn blind_hole_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Result<Option<Vec<String>>, CodecError> {
    selected_hole_operations(ctx, templates, operation_positions, |template| {
        template.form == crate::native::features::holes::SimpleHoleForm::Simple
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Blind
    })
}

/// Return counterbored through-hole operations in feature-history order.
/// Counterbore construction groups are not inferred from the scalar lanes:
/// each operation must have its own unambiguous body and topology witness.
pub(super) fn counterbore_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Result<Option<Vec<String>>, CodecError> {
    selected_hole_operations(ctx, templates, operation_positions, |template| {
        template.form == crate::native::features::holes::SimpleHoleForm::Counterbored
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Through
            && template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::None
            && template.end_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::None
    })
}

#[derive(Default)]
pub(super) struct HolePackageProjection {
    pub(super) internal_operations: BTreeSet<String>,
    pub(super) outputs: BTreeMap<String, Vec<BodyId>>,
    pub(super) diameters: BTreeMap<String, Length>,
    pub(super) chamfers: BTreeMap<String, HoleKind>,
    pub(super) placements: BTreeMap<String, Vec<HolePlacement>>,
}

#[derive(Clone, Copy)]
pub(super) struct HolePackageSources<'a> {
    pub(super) outputs: &'a BTreeMap<String, Vec<BodyId>>,
    pub(super) diameters: &'a BTreeMap<String, Length>,
    pub(super) chamfers: &'a BTreeMap<String, HoleKind>,
}

pub(super) fn hole_package_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
    uses: &[crate::native::features::holes::FeatureHolePackageConstructionGroupUse],
    sources: HolePackageSources<'_>,
) -> Result<HolePackageProjection, CodecError> {
    let HolePackageSources {
        outputs,
        diameters,
        chamfers,
    } = sources;
    let mut projection = HolePackageProjection::default();
    for use_ in uses {
        let use_scans = uses.len().checked_mul(2).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX hole package use uniqueness",
                0,
                cadmpeg_core::decode::u64_from_index(uses.len()),
            )
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(use_scans),
            "NX hole package use uniqueness",
        )?;
        if uses
            .iter()
            .filter(|candidate| candidate.operation_label == use_.operation_label)
            .count()
            != 1
            || uses
                .iter()
                .filter(|candidate| {
                    candidate.simple_hole_construction_group == use_.simple_hole_construction_group
                })
                .count()
                != 1
        {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(groups.len()),
            "NX hole package group lookup",
        )?;
        let Some(group) = groups
            .iter()
            .find(|group| group.id == use_.simple_hole_construction_group)
        else {
            continue;
        };
        if group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|operation| projection.internal_operations.contains(operation))
        {
            continue;
        }
        let template_scans = group
            .members
            .len()
            .checked_mul(templates.len())
            .and_then(|count| count.checked_mul(2))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX hole package template lookup",
                    0,
                    cadmpeg_core::decode::u64_from_index(group.members.len()),
                )
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(template_scans),
            "NX hole package template lookup",
        )?;
        let mut requests_chamfer = true;
        let mut requests_no_treatment = true;
        let mut complete_templates = true;
        for member in group.members.iter() {
            let mut matches = templates
                .iter()
                .filter(|template| template.operation_label == member.operation_label);
            let Some(template) = matches.next() else {
                complete_templates = false;
                break;
            };
            if matches.next().is_some()
                || template.form != crate::native::features::holes::SimpleHoleForm::Simple
                || template.extent != crate::native::features::holes::SimpleHoleExtent::Through
            {
                complete_templates = false;
                break;
            }
            requests_chamfer &= template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
                && template.end_treatment
                    == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer;
            requests_no_treatment &= template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::None
                && template.end_treatment
                    == crate::native::features::holes::SimpleHoleEndTreatment::None;
        }
        if !complete_templates {
            continue;
        }
        let Some(body) = group
            .members
            .first()
            .and_then(|member| outputs.get(&member.operation_label))
            .and_then(|bodies| bodies.as_slice().first().filter(|_| bodies.len() == 1))
        else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(group.members.len()),
            "NX hole package output lookup",
        )?;
        if group.members.iter().any(|member| !matches!(outputs.get(&member.operation_label).map(Vec::as_slice), Some([candidate]) if candidate == body))
        {
            continue;
        }
        let Some(diameter) = group
            .members
            .first()
            .map(|member| &member.operation_label)
            .and_then(|operation| diameters.get(operation))
            .copied()
        else {
            continue;
        };
        if group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|operation| diameters.get(operation).copied() != Some(diameter))
        {
            continue;
        }
        if !requests_chamfer && !requests_no_treatment {
            continue;
        }
        let chamfer = if requests_chamfer {
            let Some(chamfer) = group
                .members
                .first()
                .map(|member| &member.operation_label)
                .and_then(|operation| chamfers.get(operation))
                .copied()
            else {
                continue;
            };
            if group
                .members
                .iter()
                .map(|member| &member.operation_label)
                .any(|operation| chamfers.get(operation).copied() != Some(chamfer))
            {
                continue;
            }
            Some(chamfer)
        } else {
            None
        };
        for member in group.members.iter() {
            ctx.charge_work(1, "NX hole package internal operation")?;
            if projection
                .internal_operations
                .contains(&member.operation_label)
            {
                continue;
            }
            ctx.charge_collection_items(1, "NX hole package internal operations")?;
            let bytes = std::mem::size_of::<String>()
                .checked_add(member.operation_label.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX hole package internal operations",
                        0,
                        cadmpeg_core::decode::u64_from_index(member.operation_label.len()),
                    )
                })?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(bytes),
                "NX hole package internal operations",
            )?;
            projection
                .internal_operations
                .insert(member.operation_label.clone());
        }
        insert_hole_output_body(ctx, &mut projection.outputs, &use_.operation_label, body)?;
        ctx.admit_retained_btree_record::<String, Length>(
            use_.operation_label.len(),
            "NX hole package diameter map",
        )?;
        projection
            .diameters
            .insert(use_.operation_label.clone(), diameter);
        if let Some(chamfer) = chamfer {
            ctx.admit_retained_btree_record::<String, HoleKind>(
                use_.operation_label.len(),
                "NX hole package chamfer map",
            )?;
            projection
                .chamfers
                .insert(use_.operation_label.clone(), chamfer);
        }
        let placements = hole_axis_placements_for_body(ctx, ir, body)?;
        if placements.len() == group.members.len() {
            ctx.admit_retained_btree_record::<String, Vec<HolePlacement>>(
                use_.operation_label.len(),
                "NX hole package placement map",
            )?;
            projection
                .placements
                .insert(use_.operation_label.clone(), placements);
        }
    }
    Ok(projection)
}

pub(super) struct HoleBodyProjection {
    pub(super) outputs: BTreeMap<String, Vec<BodyId>>,
    pub(super) diameters: BTreeMap<String, Length>,
    pub(super) blind_depths: BTreeMap<String, cadmpeg_ir::scalar::NonZeroLength>,
    pub(super) counterbores: BTreeMap<String, CounterboreDimensions>,
}

pub(super) fn extend_hole_projection_map<V>(
    ctx: &DecodeContext<'_>,
    target: &mut BTreeMap<String, V>,
    source: BTreeMap<String, V>,
    operation: &'static str,
) -> Result<(), CodecError> {
    for (key, value) in source {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(target.len()),
            operation,
        )?;
        if !target.contains_key(&key) {
            ctx.charge_collection_items(1, operation)?;
        }
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(String, V)>()),
            operation,
        )?;
        target.insert(key, value);
    }
    Ok(())
}

pub(super) fn hole_operations_are_unique(
    ctx: &DecodeContext<'_>,
    operations: &[String],
) -> Result<bool, CodecError> {
    for (index, operation) in operations.iter().enumerate() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(index),
            "NX hole operation uniqueness",
        )?;
        if operations[..index].contains(operation) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn insert_hole_output_body(
    ctx: &DecodeContext<'_>,
    outputs: &mut BTreeMap<String, Vec<BodyId>>,
    operation: &str,
    body: &BodyId,
) -> Result<(), CodecError> {
    let nested_bytes = std::mem::size_of::<BodyId>()
        .checked_add(body.as_str().len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX hole output body",
                0,
                cadmpeg_core::decode::u64_from_index(body.as_str().len()),
            )
        })?;
    ctx.charge_collection_items(1, "NX hole output body")?;
    ctx.admit_retained_btree_record::<String, Vec<BodyId>>(
        operation
            .len()
            .checked_add(nested_bytes)
            .ok_or_else(|| ctx.refuse_codec_limit("NX hole output map", u64::MAX, u64::MAX))?,
        "NX hole output map",
    )?;
    let mut bodies = Vec::new();
    cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
        &mut bodies,
        1,
        "NX hole output body",
    )?;
    bodies.push(body.clone());
    outputs.insert(operation.to_owned(), bodies);
    Ok(())
}

pub(super) fn hole_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<HoleBodyProjection>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)? {
        return Ok(None);
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(None);
    };

    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let Some(bores) = cylindrical_face_witnesses(ctx, ir, &body_faces)? else {
            return Ok(None);
        };
        let Some(radius) = bores.first().map(|bore| bore.radius) else {
            return Ok(None);
        };
        if bores.len() != operations.len()
            || bores
                .iter()
                .any(|bore| bore.radius.to_bits() != radius.to_bits())
        {
            return Ok(None);
        }
        for operation in operations {
            insert_hole_output_body(ctx, &mut projected_outputs, &operation, &body)?;
            let Some(diameter) = Length::new(radius * 2.0) else {
                return Ok(None);
            };
            ctx.admit_retained_btree_record::<String, Length>(
                operation.len(),
                "NX hole diameter map",
            )?;
            diameters.insert(operation, diameter);
        }
    }
    Ok(Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores: BTreeMap::new(),
    }))
}

pub(super) fn counterbore_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<HoleBodyProjection>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)? {
        return Ok(None);
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(None);
    };
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut counterbores = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            // A counterbore pair has no serialized operation-to-pair relation
            // once multiple operations share one result body. Do not assign
            // geometry to history order.
            return Ok(None);
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let Some(witnesses) = counterbore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(None);
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(None);
        };
        insert_hole_output_body(ctx, &mut projected_outputs, operation, &body)?;
        let Some(diameter) = Length::new(witness.bore_radius * 2.0) else {
            return Ok(None);
        };
        ctx.admit_retained_btree_record::<String, Length>(
            operation.len(),
            "NX counterbore diameter map",
        )?;
        diameters.insert(operation.clone(), diameter);
        let (Some(diameter), Some(depth)) = (
            cadmpeg_ir::scalar::PositiveLength::new(witness.counterbore_radius * 2.0),
            cadmpeg_ir::scalar::PositiveLength::new(witness.depth),
        ) else {
            return Ok(None);
        };
        ctx.admit_retained_btree_record::<String, CounterboreDimensions>(
            operation.len(),
            "NX counterbore dimension map",
        )?;
        counterbores.insert(operation.clone(), CounterboreDimensions { diameter, depth });
    }
    Ok(Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores,
    }))
}

pub(super) fn blind_hole_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<HoleBodyProjection>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)? {
        return Ok(None);
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(None);
    };
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut blind_depths = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return Ok(None);
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let Some(witnesses) = blind_bore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(None);
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(None);
        };
        insert_hole_output_body(ctx, &mut projected_outputs, operation, &body)?;
        let Some(diameter) = Length::new(witness.bore_radius * 2.0) else {
            return Ok(None);
        };
        ctx.admit_retained_btree_record::<String, Length>(
            operation.len(),
            "NX blind hole diameter map",
        )?;
        diameters.insert(operation.clone(), diameter);
        let Some(depth) = cadmpeg_ir::scalar::NonZeroLength::new(witness.depth) else {
            return Ok(None);
        };
        ctx.admit_retained_btree_record::<String, cadmpeg_ir::scalar::NonZeroLength>(
            operation.len(),
            "NX blind hole depth map",
        )?;
        blind_depths.insert(operation.clone(), depth);
    }
    Ok(Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths,
        counterbores: BTreeMap::new(),
    }))
}

/// Derive one complete unoriented placement when one operation owns exactly
/// one through bore. The closest point to the model origin is invariant under
/// axial shifts of the serialized cylinder origin. Canonical axis sign makes
/// serialization deterministic but carries no drilling-direction semantics.
pub(super) fn hole_axis_placements_for_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HolePlacement>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)? {
        return Ok(BTreeMap::new());
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(BTreeMap::new());
    };

    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            continue;
        };
        let mut body_placements = hole_axis_placements_for_body(ctx, ir, &body)?;
        if body_placements.len() != 1 {
            continue;
        }
        ctx.admit_retained_btree_record::<String, HolePlacement>(
            operation.len(),
            "NX hole placement map",
        )?;
        placements.insert(operation.clone(), body_placements.remove(0));
    }
    Ok(placements)
}

pub(super) fn counterbore_axis_placements_for_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HolePlacement>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)? {
        return Ok(BTreeMap::new());
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(BTreeMap::new());
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let Some(witnesses) = counterbore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(BTreeMap::new());
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let (Some(point), Some(direction)) = (
            cadmpeg_ir::features::FinitePoint3::new(witness.line_origin),
            cadmpeg_ir::features::FeatureDirection3::new(witness.axis),
        ) else {
            return Ok(BTreeMap::new());
        };
        ctx.admit_retained_btree_record::<String, HolePlacement>(
            operation.len(),
            "NX counterbore placement map",
        )?;
        placements.insert(
            operation.clone(),
            HolePlacement::Axis {
                origin: point,
                axis: direction,
            },
        );
    }
    Ok(placements)
}

pub(super) fn blind_hole_axis_placements_for_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HolePlacement>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)? {
        return Ok(BTreeMap::new());
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(BTreeMap::new());
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let Some(witnesses) = blind_bore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(BTreeMap::new());
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let (Some(point), Some(direction)) = (
            cadmpeg_ir::features::FinitePoint3::new(witness.position),
            cadmpeg_ir::features::FeatureDirection3::new(witness.direction),
        ) else {
            return Ok(BTreeMap::new());
        };
        ctx.admit_retained_btree_record::<String, HolePlacement>(
            operation.len(),
            "NX blind hole placement map",
        )?;
        placements.insert(
            operation.clone(),
            HolePlacement::Directed {
                position: point,
                direction,
            },
        );
    }
    Ok(placements)
}

pub(super) fn hole_axis_placements_for_body(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body: &BodyId,
) -> Result<Vec<HolePlacement>, CodecError> {
    let Some(body_faces) = connected_solid_body_faces(ctx, ir, body)? else {
        return Ok(Vec::new());
    };
    let Some(bores) = cylindrical_face_witnesses(ctx, ir, &body_faces)? else {
        return Ok(Vec::new());
    };
    let angular_tolerance = ir.tolerances.angular.get();
    let mut placements = Vec::new();
    for bore in bores {
        let origin = bore.line_origin;
        let axis = bore.axis;
        let Some(mut axis) = FiniteVector3::new(axis).and_then(FiniteVector3::unit_nonzero) else {
            return Ok(Vec::new());
        };
        let Some(leading) = [axis.x, axis.y, axis.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)
        else {
            return Ok(Vec::new());
        };
        if leading < 0.0 {
            axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        }
        let axial_offset = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
        let origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        let (Some(origin), Some(axis)) = (
            cadmpeg_ir::features::FinitePoint3::new(origin),
            cadmpeg_ir::features::FeatureDirection3::new(axis),
        ) else {
            return Ok(Vec::new());
        };
        ctx.reserve_retained_vec(&mut placements, 1, "NX hole axis placements")?;
        placements.push(HolePlacement::Axis { origin, axis });
    }
    placements.sort_by_key(hole_placement_key);
    Ok(placements)
}

pub(super) fn hole_placement_key(placement: &HolePlacement) -> [u64; 6] {
    let HolePlacement::Axis { origin, axis } = placement else {
        return [0; 6];
    };
    [
        origin.x.to_bits(),
        origin.y.to_bits(),
        origin.z.to_bits(),
        axis.x.to_bits(),
        axis.y.to_bits(),
        axis.z.to_bits(),
    ]
}

#[derive(Clone, Debug)]
pub(super) struct CylindricalFaceWitness {
    line_origin: Point3,
    axis: Vector3,
    radius: f64,
    stations: [f64; 2],
    loop_ids: [LoopId; 2],
}

#[derive(Clone, Copy)]
pub(super) struct CounterboreCylinderWitness {
    line_origin: Point3,
    axis: Vector3,
    bore_radius: f64,
    counterbore_radius: f64,
    depth: f64,
}

#[derive(Clone, Copy)]
pub(super) struct BlindBoreCylinderWitness {
    position: Point3,
    direction: Vector3,
    bore_radius: f64,
    depth: f64,
}

pub(super) fn canonical_axis(
    axis: cadmpeg_ir::units::UnitVector3,
    angular_tolerance: f64,
) -> Option<Vector3> {
    let mut axis = *axis.to_unit_length_charted().as_raw();
    let leading = [axis.x, axis.y, axis.z]
        .into_iter()
        .find(|component| component.abs() > angular_tolerance)?;
    if leading < 0.0 {
        axis = Vector3::new(-axis.x, -axis.y, -axis.z);
    }
    Some(axis)
}

pub(super) fn face_two_loops(face: &Face) -> Option<[&LoopId; 2]> {
    match &face.loops {
        FaceLoops::Unspecified { loops } => {
            let [first, second] = loops.as_slice() else {
                return None;
            };
            Some([first, second])
        }
        FaceLoops::Classified { outer, inner } => {
            let [second] = inner.as_slice() else {
                return None;
            };
            Some([outer, second])
        }
    }
}

pub(super) fn face_one_loop(face: &Face) -> Option<&LoopId> {
    match &face.loops {
        FaceLoops::Unspecified { loops } => {
            let [only] = loops.as_slice() else {
                return None;
            };
            Some(only)
        }
        FaceLoops::Classified { outer, inner } if inner.is_empty() => Some(outer),
        FaceLoops::Classified { .. } => None,
    }
}

pub(super) fn circular_loop_geometry(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    loop_id: &LoopId,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> Result<Option<(Point3, Vector3, f64)>, CodecError> {
    let coedge_scans = ir.model.coedges.len().checked_mul(2).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX circular loop coedge scan",
            0,
            cadmpeg_core::decode::u64_from_index(ir.model.coedges.len()),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(coedge_scans),
        "NX circular loop coedge scan",
    )?;
    if !ir
        .model
        .coedges
        .iter()
        .any(|coedge| &coedge.owner_loop == loop_id)
    {
        return Ok(None);
    }
    let mut witness: Option<(Point3, Vector3, f64)> = None;
    for coedge in ir
        .model
        .coedges
        .iter()
        .filter(|coedge| &coedge.owner_loop == loop_id)
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.edges.len()),
            "NX circular loop edge scan",
        )?;
        let Some(curve_id) = ir
            .model
            .edges
            .iter()
            .rev()
            .find(|edge| edge.id == coedge.edge)
            .and_then(|edge| edge.curve())
        else {
            return Ok(None);
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.curves.len()),
            "NX circular loop curve scan",
        )?;
        let Some(curve) = ir
            .model
            .curves
            .iter()
            .rev()
            .find(|curve| &curve.id == curve_id)
        else {
            return Ok(None);
        };
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = &curve.geometry
        else {
            return Ok(None);
        };
        let center = circle_curve.center().get();
        let axis = circle_curve.frame().axis();
        let radius = circle_curve.radius().get();
        let Some(axis) = canonical_axis(*axis, angular_tolerance) else {
            return Ok(None);
        };
        if let Some((previous_center, previous_axis, previous_radius)) = witness {
            if (radius - previous_radius).abs() > linear_tolerance
                || (1.0 - axis.dot(previous_axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - previous_center.x,
                    center.y - previous_center.y,
                    center.z - previous_center.z,
                )
                .norm()
                    > linear_tolerance
            {
                return Ok(None);
            }
        }
        witness = Some((center, axis, radius));
    }
    Ok(witness)
}

pub(super) fn same_loop_edges(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    first: &LoopId,
    second: &LoopId,
) -> Result<bool, CodecError> {
    let coedge_count = ir.model.coedges.len();
    let work = coedge_count
        .checked_mul(coedge_count)
        .and_then(|count| count.checked_mul(2))
        .and_then(|count| {
            coedge_count
                .checked_mul(4)
                .and_then(|scans| count.checked_add(scans))
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX loop edge comparison",
                0,
                cadmpeg_core::decode::u64_from_index(coedge_count),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "NX loop edge comparison",
    )?;
    let first_has_edges = ir
        .model
        .coedges
        .iter()
        .any(|coedge| &coedge.owner_loop == first);
    let second_has_edges = ir
        .model
        .coedges
        .iter()
        .any(|coedge| &coedge.owner_loop == second);
    if first_has_edges != second_has_edges {
        return Ok(false);
    }
    let first_in_second = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| &coedge.owner_loop == first)
        .all(|coedge| {
            ir.model
                .coedges
                .iter()
                .any(|other| &other.owner_loop == second && other.edge == coedge.edge)
        });
    let second_in_first = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| &coedge.owner_loop == second)
        .all(|coedge| {
            ir.model
                .coedges
                .iter()
                .any(|other| &other.owner_loop == first && other.edge == coedge.edge)
        });
    Ok(first_in_second && second_in_first)
}

pub(super) fn cylindrical_face_witnesses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
) -> Result<Option<Vec<CylindricalFaceWitness>>, CodecError> {
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut witnesses = Vec::new();
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(body_faces.len()),
        "NX cylindrical face scan",
    )?;
    for face in body_faces
        .iter()
        .copied()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()),
            "NX cylindrical surface lookup",
        )?;
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .rev()
            .find(|surface| surface.id == face.surface)
        else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
            &surface.geometry
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis();
        let radius = cylinder_surface.radius().get();
        let Some(axis) = canonical_axis(*axis, angular_tolerance) else {
            return Ok(None);
        };
        let axial_offset = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
        let line_origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        let Some([first_loop, second_loop]) = face_two_loops(face) else {
            return Ok(None);
        };
        let mut stations = [0.0; 2];
        for (ordinal, loop_id) in [first_loop, second_loop].into_iter().enumerate() {
            let Some((center, circle_axis, circle_radius)) =
                circular_loop_geometry(ctx, ir, loop_id, linear_tolerance, angular_tolerance)?
            else {
                return Ok(None);
            };
            if (circle_radius - radius).abs() > linear_tolerance
                || (1.0 - axis.dot(circle_axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                )
                .cross(axis)
                .norm()
                    > linear_tolerance
            {
                return Ok(None);
            }
            let station = Vector3::new(center.x, center.y, center.z).dot(axis);
            if !station.is_finite() {
                return Ok(None);
            }
            stations[ordinal] = station;
        }
        let [first, second] = stations.as_slice() else {
            return Ok(None);
        };
        if (first - second).abs() <= linear_tolerance {
            return Ok(None);
        }
        let bytes = std::mem::size_of::<CylindricalFaceWitness>()
            .checked_add(first_loop.as_str().len())
            .and_then(|bytes| bytes.checked_add(second_loop.as_str().len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX cylindrical face witness",
                    0,
                    cadmpeg_core::decode::u64_from_index(first_loop.as_str().len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX cylindrical face witnesses")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX cylindrical face witness",
        )?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut witnesses,
            1,
            "NX cylindrical face witnesses",
        )?;
        witnesses.push(CylindricalFaceWitness {
            line_origin,
            axis,
            radius,
            stations: [*first, *second],
            loop_ids: [first_loop.clone(), second_loop.clone()],
        });
    }
    Ok(Some(witnesses))
}

pub(super) fn plane_annulus_witness(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
    small: &CylindricalFaceWitness,
    small_station_ordinal: usize,
    large: &CylindricalFaceWitness,
    large_station_ordinal: usize,
) -> Result<bool, CodecError> {
    let line_origin = small.line_origin;
    let axis = small.axis;
    let station = small.stations[small_station_ordinal];
    let inner_radius = small.radius;
    let outer_radius = large.radius;
    let inner_loop = &small.loop_ids[small_station_ordinal];
    let outer_loop = &large.loop_ids[large_station_ordinal];
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut matches = 0;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(body_faces.len()),
        "NX annulus face scan",
    )?;
    for face in body_faces {
        if face.loops.len() != 2 {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()),
            "NX annulus plane lookup",
        )?;
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .rev()
            .find(|surface| surface.id == face.surface)
        else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
            &surface.geometry
        else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = plane_surface.frame().axis();
        let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
            continue;
        };
        if (1.0 - normal.dot(axis).abs()) > angular_tolerance
            || (Vector3::new(
                origin.x - line_origin.x,
                origin.y - line_origin.y,
                origin.z - line_origin.z,
            )
            .dot(axis)
                - station)
                .abs()
                > linear_tolerance
        {
            continue;
        }
        let Some([first_loop, second_loop]) = face_two_loops(face) else {
            continue;
        };
        let mut boundaries = [(0.0, first_loop), (0.0, second_loop)];
        let mut valid = true;
        for (ordinal, loop_id) in [first_loop, second_loop].into_iter().enumerate() {
            let Some((center, circle_axis, radius)) =
                circular_loop_geometry(ctx, ir, loop_id, linear_tolerance, angular_tolerance)?
            else {
                valid = false;
                break;
            };
            if (1.0 - circle_axis.dot(normal).abs()) > angular_tolerance
                || (Vector3::new(
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                )
                .dot(normal))
                .abs()
                    > linear_tolerance
                || Vector3::new(
                    center.x - line_origin.x,
                    center.y - line_origin.y,
                    center.z - line_origin.z,
                )
                .cross(axis)
                .norm()
                    > linear_tolerance
                || (Vector3::new(center.x, center.y, center.z).dot(axis) - station).abs()
                    > linear_tolerance
            {
                valid = false;
                break;
            }
            boundaries[ordinal] = (radius, loop_id);
        }
        if !valid {
            continue;
        }
        boundaries.sort_by(|(first, _), (second, _)| first.total_cmp(second));
        let [(inner, inner_boundary), (outer, outer_boundary)] = boundaries.as_slice() else {
            continue;
        };
        if (inner - inner_radius).abs() <= linear_tolerance
            && (outer - outer_radius).abs() <= linear_tolerance
            && same_loop_edges(ctx, ir, inner_boundary, inner_loop)?
            && same_loop_edges(ctx, ir, outer_boundary, outer_loop)?
        {
            matches += 1;
        }
    }
    Ok(matches == 1)
}

pub(super) fn counterbore_cylinders(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
) -> Result<Option<Vec<CounterboreCylinderWitness>>, CodecError> {
    let Some(cylinders) = cylindrical_face_witnesses(ctx, ir, body_faces)? else {
        return Ok(None);
    };
    if cylinders.is_empty() || cylinders.len() % 2 != 0 {
        return Ok(None);
    }
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let pair_work = cylinders
        .len()
        .checked_mul(cylinders.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX counterbore pair scan",
                0,
                cadmpeg_core::decode::u64_from_index(cylinders.len()),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(pair_work),
        "NX counterbore pair scan",
    )?;
    let mut candidates = ctx.alloc_filled(
        cylinders.len(),
        Vec::<(usize, CounterboreCylinderWitness)>::new(),
        "nx counterbore cylinder candidates",
    )?;
    for (first_index, first) in cylinders.iter().enumerate() {
        for (second_index, second) in cylinders.iter().enumerate().skip(first_index + 1) {
            let (small, large) = if first.radius < second.radius {
                (first, second)
            } else {
                (second, first)
            };
            if large.radius - small.radius <= linear_tolerance
                || (1.0 - small.axis.dot(large.axis).abs()) > angular_tolerance
                || Vector3::new(
                    large.line_origin.x - small.line_origin.x,
                    large.line_origin.y - small.line_origin.y,
                    large.line_origin.z - small.line_origin.z,
                )
                .cross(small.axis)
                .norm()
                    > linear_tolerance
            {
                continue;
            }
            let mut common = None;
            let mut multiple_common = false;
            for (small_ordinal, small_station) in small.stations.iter().enumerate() {
                for (large_ordinal, large_station) in large.stations.iter().enumerate() {
                    if (small_station - large_station).abs() <= linear_tolerance
                        && common
                            .replace((small_ordinal, large_ordinal, *small_station))
                            .is_some()
                    {
                        multiple_common = true;
                    }
                }
            }
            let Some((small_shared, large_shared, shared_station)) = common else {
                continue;
            };
            if multiple_common {
                continue;
            }
            let small_other = small.stations[1 - small_shared];
            let large_other = large.stations[1 - large_shared];
            let depth = (large_other - shared_station).abs();
            if depth <= linear_tolerance
                || (small_other - shared_station).abs() <= linear_tolerance
                || !plane_annulus_witness(
                    ctx,
                    ir,
                    body_faces,
                    small,
                    small_shared,
                    large,
                    large_shared,
                )?
            {
                continue;
            }
            let witness = CounterboreCylinderWitness {
                line_origin: small.line_origin,
                axis: small.axis,
                bore_radius: small.radius,
                counterbore_radius: large.radius,
                depth,
            };
            ctx.charge_collection_items(2, "nx counterbore candidate pair")?;
            let pair_bytes = std::mem::size_of::<(usize, CounterboreCylinderWitness)>()
                .checked_mul(2)
                .ok_or_else(|| ctx.refuse_codec_limit("nx counterbore candidate pair", 0, 2))?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(pair_bytes),
                "nx counterbore candidate pair",
            )?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut candidates[first_index],
                1,
                "nx counterbore candidate pair",
            )?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut candidates[second_index],
                1,
                "nx counterbore candidate pair",
            )?;
            candidates[first_index].push((second_index, witness));
            candidates[second_index].push((first_index, witness));
        }
    }
    if candidates.iter().any(|candidates| candidates.len() != 1) {
        return Ok(None);
    }
    let mut witnesses =
        ctx.retained_vec(cylinders.len() / 2, "nx counterbore cylinder witnesses")?;
    let mut used = ctx.alloc_filled(
        cylinders.len(),
        false,
        "nx counterbore cylinder assignments",
    )?;
    for first_index in 0..cylinders.len() {
        if used[first_index] {
            continue;
        }
        let (second_index, witness) = candidates[first_index][0];
        if used[second_index]
            || candidates[second_index][0].0 != first_index
            || first_index == second_index
        {
            return Ok(None);
        }
        used[first_index] = true;
        used[second_index] = true;
        witnesses.push(witness);
    }
    Ok(Some(witnesses))
}

/// Identify one blind bore from its unique planar termination. The cylinder
/// boundary and cap loop must share the exact edge identities; a radius or
/// station match alone is not a topology relation.
pub(super) fn blind_bore_cylinders(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
) -> Result<Option<Vec<BlindBoreCylinderWitness>>, CodecError> {
    let Some(cylinders) = cylindrical_face_witnesses(ctx, ir, body_faces)? else {
        return Ok(None);
    };
    let [cylinder] = cylinders.as_slice() else {
        return Ok(None);
    };
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut cap_station = None;
    let mut cap_count = 0usize;
    for (station_ordinal, station) in cylinder.stations.iter().enumerate() {
        let cylinder_loop = &cylinder.loop_ids[station_ordinal];
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.coedges.len()),
            "NX blind bore cylinder edge scan",
        )?;
        if !ir
            .model
            .coedges
            .iter()
            .any(|coedge| &coedge.owner_loop == cylinder_loop)
        {
            return Ok(None);
        }
        for face in body_faces {
            let Some(cap_loop) = face_one_loop(face) else {
                continue;
            };
            if !same_loop_edges(ctx, ir, cap_loop, cylinder_loop)? {
                continue;
            }
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()),
                "NX blind bore cap surface lookup",
            )?;
            let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .rev()
                .find(|surface| surface.id == face.surface)
            else {
                continue;
            };
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
                &surface.geometry
            else {
                continue;
            };
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis();
            let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
                continue;
            };
            let Some((center, circle_axis, circle_radius)) =
                circular_loop_geometry(ctx, ir, cap_loop, linear_tolerance, angular_tolerance)?
            else {
                continue;
            };
            if (circle_radius - cylinder.radius).abs() > linear_tolerance
                || (1.0 - circle_axis.dot(normal).abs()) > angular_tolerance
                || (1.0 - normal.dot(cylinder.axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - cylinder.line_origin.x,
                    center.y - cylinder.line_origin.y,
                    center.z - cylinder.line_origin.z,
                )
                .cross(cylinder.axis)
                .norm()
                    > linear_tolerance
                || (Vector3::new(center.x, center.y, center.z).dot(cylinder.axis) - *station).abs()
                    > linear_tolerance
                || (Vector3::new(origin.x, origin.y, origin.z).dot(cylinder.axis) - *station).abs()
                    > linear_tolerance
            {
                continue;
            }
            cap_count = cap_count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX blind bore cap count",
                    0,
                    cadmpeg_core::decode::u64_from_index(cap_count),
                )
            })?;
            cap_station = Some((station_ordinal, *station));
        }
    }
    let Some((cap_ordinal, cap_station)) = cap_station.filter(|_| cap_count == 1) else {
        return Ok(None);
    };
    let entry_ordinal = 1 - cap_ordinal;
    let entry_station = cylinder.stations[entry_ordinal];
    let depth = (cap_station - entry_station).abs();
    if !depth.is_finite() || depth <= linear_tolerance {
        return Ok(None);
    }
    let position = Point3::new(
        cylinder.line_origin.x + entry_station * cylinder.axis.x,
        cylinder.line_origin.y + entry_station * cylinder.axis.y,
        cylinder.line_origin.z + entry_station * cylinder.axis.z,
    );
    let direction = if cap_station > entry_station {
        cylinder.axis
    } else {
        Vector3::new(-cylinder.axis.x, -cylinder.axis.y, -cylinder.axis.z)
    };
    let mut witnesses = ctx.retained_vec(1, "NX blind bore witness")?;
    witnesses.push(BlindBoreCylinderWitness {
        position,
        direction,
        bore_radius: cylinder.radius,
        depth,
    });
    Ok(Some(witnesses))
}

/// Resolve hole operations to their explicit output bodies, or to the one
/// connected solid when NX omits every operation-output relation. An output
/// entry with no body is an explicit unresolved relation and blocks fallback.
pub(super) fn hole_operations_by_body(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<BTreeMap<BodyId, Vec<String>>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(operations.len()),
        "NX hole output relation scan",
    )?;
    let related = operations
        .iter()
        .filter(|operation| outputs.contains_key(*operation))
        .count();
    if related != 0 && related != operations.len() {
        return Ok(None);
    }
    if related == operations.len() {
        let mut operations_by_body = BTreeMap::<BodyId, Vec<String>>::new();
        for operation in operations {
            let Some([body]) = outputs.get(operation).map(Vec::as_slice) else {
                return Ok(None);
            };
            if !operations_by_body.contains_key(body) {
                ctx.charge_collection_items(1, "NX hole operation body groups")?;
                let bytes = std::mem::size_of::<(BodyId, Vec<String>)>()
                    .checked_add(body.as_str().len())
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "NX hole operation body groups",
                            0,
                            cadmpeg_core::decode::u64_from_index(body.as_str().len()),
                        )
                    })?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(bytes),
                    "NX hole operation body groups",
                )?;
                operations_by_body.insert(body.clone(), Vec::new());
            }
            let group = operations_by_body
                .get_mut(body)
                .ok_or_else(|| ctx.refuse_codec_limit("NX hole operation body groups", 0, 1))?;
            ctx.charge_collection_items(1, "NX hole operations per body")?;
            let bytes = std::mem::size_of::<String>()
                .checked_add(operation.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX hole operations per body",
                        0,
                        cadmpeg_core::decode::u64_from_index(operation.len()),
                    )
                })?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(bytes),
                "NX hole operations per body",
            )?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                group,
                1,
                "NX hole operations per body",
            )?;
            group.push(operation.clone());
        }
        return Ok(Some(operations_by_body));
    }

    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()),
        "NX connected solid body scan",
    )?;
    let mut selected = None;
    for body in &ir.model.bodies {
        if connected_solid_body_exists(ctx, ir, body)? && selected.replace(body).is_some() {
            return Ok(None);
        }
    }
    let Some(body) = selected else {
        return Ok(None);
    };
    ctx.charge_collection_items(1, "NX hole operation body groups")?;
    let map_bytes = std::mem::size_of::<(BodyId, Vec<String>)>()
        .checked_add(body.id.as_str().len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX hole operation body groups",
                0,
                cadmpeg_core::decode::u64_from_index(body.id.as_str().len()),
            )
        })?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(map_bytes),
        "NX hole operation body groups",
    )?;
    let mut group = Vec::new();
    for operation in operations {
        ctx.charge_collection_items(1, "NX hole operations per body")?;
        let bytes = std::mem::size_of::<String>()
            .checked_add(operation.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX hole operations per body",
                    0,
                    cadmpeg_core::decode::u64_from_index(operation.len()),
                )
            })?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX hole operations per body",
        )?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut group,
            1,
            "NX hole operations per body",
        )?;
        group.push(operation.clone());
    }
    Ok(Some(BTreeMap::from([(body.id.clone(), group)])))
}

/// Derive identical entry and exit chamfer treatments only when every simple
/// through-hole bore has exactly two coaxial conical faces and every cone is
/// bounded by the bore circle and one equal larger circle.
pub(super) fn simple_hole_chamfers(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HoleKind>, CodecError> {
    let mut operations = Vec::new();
    let mut operation_reservation = ctx.reserve_scoped(0, "NX chamfer selected operations")?;
    for template in templates {
        ctx.charge_work(1, "NX chamfer template scan")?;
        if !(template.form == crate::native::features::holes::SimpleHoleForm::Simple
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Through
            && template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
            && template.end_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer)
        {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(templates.len()),
            "NX chamfer template identity scan",
        )?;
        if templates
            .iter()
            .filter(|candidate| candidate.operation_label == template.operation_label)
            .count()
            != 1
        {
            continue;
        }
        let bytes = std::mem::size_of::<String>()
            .checked_add(template.operation_label.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX chamfer selected operations",
                    0,
                    cadmpeg_core::decode::u64_from_index(template.operation_label.len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX chamfer selected operations")?;
        operation_reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut operations,
            1,
            "NX chamfer selected operations",
        )?;
        operations.push(template.operation_label.clone());
    }
    if operations.is_empty() {
        return Ok(BTreeMap::new());
    }
    charge_hole_sort_work(ctx, operations.len())?;
    operations.sort();
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, &operations, outputs)? else {
        return Ok(BTreeMap::new());
    };

    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut treatments = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let Some(bores) = cylindrical_face_witnesses(ctx, ir, &body_faces)? else {
            return Ok(BTreeMap::new());
        };
        let [first_bore, ..] = bores.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let bore_radius = first_bore.radius;
        if bores.len() != operations.len()
            || bores
                .iter()
                .any(|bore| bore.radius.to_bits() != bore_radius.to_bits())
        {
            return Ok(BTreeMap::new());
        }
        let cone_count_bytes = bores
            .len()
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX chamfer cone counts",
                    0,
                    cadmpeg_core::decode::u64_from_index(bores.len()),
                )
            })?;
        let _cone_count_reservation = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(cone_count_bytes),
            "NX chamfer cone counts",
        )?;
        let mut cone_counts =
            ctx.alloc_filled(bores.len(), 0usize, "nx simple-hole chamfer cone counts")?;
        let mut outer_radii = Vec::new();
        let mut included_angles = Vec::new();
        let mut geometry_reservation = ctx.reserve_scoped(0, "NX chamfer cone geometry")?;
        for face in body_faces
            .faces
            .iter()
            .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
        {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()),
                "NX chamfer cone surface scan",
            )?;
            let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))) = ir
                .model
                .surfaces
                .iter()
                .rev()
                .find(|surface| surface.id == face.surface)
                .map(|surface| &surface.geometry)
            else {
                continue;
            };
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let half_angle = cone_surface.half_angle().get();
            if half_angle <= 0.0 || half_angle >= std::f64::consts::FRAC_PI_2 {
                return Ok(BTreeMap::new());
            }
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(bores.len()),
                "nx chamfer bore matching",
            )?;
            let mut matching_bore = None;
            let mut multiple_bores = false;
            for (ordinal, bore) in bores.iter().enumerate() {
                let bore_origin = bore.line_origin;
                let bore_axis = bore.axis;
                let dot = axis.dot(bore_axis);
                if (1.0 - dot.abs()) > angular_tolerance {
                    continue;
                }
                let delta = Vector3::new(
                    origin.x - bore_origin.x,
                    origin.y - bore_origin.y,
                    origin.z - bore_origin.z,
                );
                if delta.cross(bore_axis).norm() <= linear_tolerance
                    && matching_bore.replace(ordinal).is_some()
                {
                    multiple_bores = true;
                }
            }
            let Some(bore_ordinal) = matching_bore else {
                return Ok(BTreeMap::new());
            };
            if multiple_bores {
                return Ok(BTreeMap::new());
            }
            cone_counts[bore_ordinal] += 1;

            let mut radii = [None, None];
            let Some(loops) = face_two_loops(face) else {
                return Ok(BTreeMap::new());
            };
            let mut radius_count = 0;
            for loop_id in loops {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(ir.model.coedges.len()),
                    "NX chamfer coedge scan",
                )?;
                for coedge in ir
                    .model
                    .coedges
                    .iter()
                    .filter(|coedge| coedge.owner_loop == *loop_id)
                {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(ir.model.edges.len()),
                        "NX chamfer edge lookup",
                    )?;
                    let Some(curve_id) = ir
                        .model
                        .edges
                        .iter()
                        .rev()
                        .find(|edge| edge.id == coedge.edge)
                        .and_then(|edge| edge.curve())
                    else {
                        continue;
                    };
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(ir.model.curves.len()),
                        "NX chamfer curve lookup",
                    )?;
                    let Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))) = ir
                        .model
                        .curves
                        .iter()
                        .rev()
                        .find(|curve| curve.id == *curve_id)
                        .map(|curve| &curve.geometry)
                    else {
                        continue;
                    };
                    let radius = circle_curve.radius().get();
                    if radius_count == radii.len() {
                        return Ok(BTreeMap::new());
                    }
                    radii[radius_count] = Some(radius);
                    radius_count += 1;
                }
            }
            let [Some(mut inner), Some(mut outer)] = radii else {
                return Ok(BTreeMap::new());
            };
            if inner.total_cmp(&outer).is_gt() {
                std::mem::swap(&mut inner, &mut outer);
            }
            if inner.to_bits() != bore_radius.to_bits() || outer <= inner {
                return Ok(BTreeMap::new());
            }
            ctx.charge_collection_items(2, "nx chamfer cone geometry")?;
            geometry_reservation.grow(cadmpeg_core::decode::u64_from_index(
                2 * std::mem::size_of::<f64>(),
            ))?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut outer_radii,
                1,
                "nx chamfer outer radii",
            )?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut included_angles,
                1,
                "nx chamfer included angles",
            )?;
            outer_radii.push(outer);
            included_angles.push(half_angle * 2.0);
        }
        if cone_counts.iter().any(|count| *count != 2)
            || outer_radii.len() != bores.len() * 2
            || included_angles.len() != outer_radii.len()
        {
            return Ok(BTreeMap::new());
        }
        charge_hole_sort_work(ctx, outer_radii.len())?;
        charge_hole_sort_work(ctx, included_angles.len())?;
        outer_radii.sort_by(f64::total_cmp);
        included_angles.sort_by(f64::total_cmp);
        let (Some(&widest), Some(&narrowest), Some(&largest), Some(&smallest)) = (
            outer_radii.last(),
            outer_radii.first(),
            included_angles.last(),
            included_angles.first(),
        ) else {
            return Ok(BTreeMap::new());
        };
        if widest - narrowest > linear_tolerance || largest - smallest > angular_tolerance {
            return Ok(BTreeMap::new());
        }
        let (Some(diameter), Some(angle)) = (
            cadmpeg_ir::scalar::PositiveLength::new(
                2.0 * outer_radii.iter().sum::<f64>()
                    / cadmpeg_core::convert::f64_from_index(outer_radii.len()).ok_or_else(
                        || {
                            ctx.refuse_codec_limit(
                                "NX chamfer radii count",
                                9_007_199_254_740_992,
                                cadmpeg_core::decode::u64_from_index(outer_radii.len()),
                            )
                        },
                    )?,
            ),
            cadmpeg_ir::scalar::InteriorAngle::new(
                included_angles.iter().sum::<f64>()
                    / cadmpeg_core::convert::f64_from_index(included_angles.len()).ok_or_else(
                        || {
                            ctx.refuse_codec_limit(
                                "NX chamfer angles count",
                                9_007_199_254_740_992,
                                cadmpeg_core::decode::u64_from_index(included_angles.len()),
                            )
                        },
                    )?,
            ),
        ) else {
            return Ok(BTreeMap::new());
        };
        let treatment = HoleKind::Chamfer { diameter, angle };
        for operation in operations {
            let bytes = std::mem::size_of::<(String, HoleKind)>()
                .checked_add(operation.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX chamfer treatments",
                        0,
                        cadmpeg_core::decode::u64_from_index(operation.len()),
                    )
                })?;
            ctx.charge_collection_items(1, "NX chamfer treatments")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(bytes),
                "NX chamfer treatments",
            )?;
            treatments.insert(operation, treatment);
        }
    }
    Ok(treatments)
}

pub(super) fn unique_simple_hole_template(
    payload_strings: &[&str],
) -> Option<(
    crate::native::features::holes::SimpleHoleForm,
    crate::native::features::holes::SimpleHoleExtent,
    crate::native::features::holes::SimpleHoleEndTreatment,
    crate::native::features::holes::SimpleHoleEndTreatment,
)> {
    let mut candidates = payload_strings
        .iter()
        .copied()
        .filter(|value| value.starts_with("Hole_"));
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    crate::native::features::holes::parse_simple_hole_template(candidate)
}
