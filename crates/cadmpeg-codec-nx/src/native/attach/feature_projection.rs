use super::{extrude_feature_definition, feature_body_outputs};
use crate::native::history::BodyWriterHistory;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FiniteVector3, NativeFeatureKind};
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
    let Some(body) = ctx.find_by(
        &ir.model.bodies,
        |body| {
            Ok(ctx.equal_bytes(
                body.id.as_str().as_bytes(),
                body_id.as_str().as_bytes(),
                "NX body faces equality",
            )?)
        },
        "NX body topology lookup",
    )?
    else {
        return Ok(None);
    };
    let mut faces = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body faces")?;
    let mut records_iter = (&body.regions).into_iter();
    while let Some(region_id) = ctx.next_charged(&mut records_iter, "NX body regions")? {
        let Some(region) = ctx.find_by(
            &ir.model.regions,
            |region| {
                Ok(ctx.equal_bytes(
                    region.id.as_str().as_bytes(),
                    region_id.as_str().as_bytes(),
                    "NX body faces equality",
                )? && ctx.equal_bytes(
                    region.body.as_str().as_bytes(),
                    body.id.as_str().as_bytes(),
                    "NX body faces equality",
                )?)
            },
            "NX body topology lookup",
        )?
        else {
            return Ok(None);
        };
        let mut records_iter = (&region.shells).into_iter();
        while let Some(shell_id) = ctx.next_charged(&mut records_iter, "NX region shells")? {
            let Some(shell) = ctx.find_by(
                &ir.model.shells,
                |shell| {
                    Ok(ctx.equal_bytes(
                        shell.id.as_str().as_bytes(),
                        shell_id.as_str().as_bytes(),
                        "NX body faces equality",
                    )? && ctx.equal_bytes(
                        shell.region.as_str().as_bytes(),
                        region.id.as_str().as_bytes(),
                        "NX body faces equality",
                    )?)
                },
                "NX body topology lookup",
            )?
            else {
                return Ok(None);
            };
            let mut records_iter = (shell.faces()).into_iter();
            while let Some(face_id) =
                ctx.next_charged(&mut records_iter, "NX shell face identities")?
            {
                let Some(face) = ctx.find_by(
                    &ir.model.faces,
                    |face| {
                        Ok(ctx.equal_bytes(
                            face.id.as_str().as_bytes(),
                            face_id.as_str().as_bytes(),
                            "NX body faces equality",
                        )? && ctx.equal_bytes(
                            face.shell.as_str().as_bytes(),
                            shell.id.as_str().as_bytes(),
                            "NX body faces equality",
                        )?)
                    },
                    "NX body topology lookup",
                )?
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
    let Some(body) = ctx.find_by(
        &ir.model.bodies,
        |body| {
            Ok(ctx.equal_bytes(
                body.id.as_str().as_bytes(),
                body_id.as_str().as_bytes(),
                "NX connected solid body faces equality",
            )?)
        },
        "NX body topology lookup",
    )?
    else {
        return Ok(None);
    };
    if body.kind != cadmpeg_ir::topology::BodyKind::Solid {
        return Ok(None);
    }
    let [region_id] = body.regions.as_slice() else {
        return Ok(None);
    };
    let Some(region) = ctx.find_by(
        &ir.model.regions,
        |region| {
            Ok(ctx.equal_bytes(
                region.id.as_str().as_bytes(),
                region_id.as_str().as_bytes(),
                "NX connected solid body faces equality",
            )? && ctx.equal_bytes(
                region.body.as_str().as_bytes(),
                body.id.as_str().as_bytes(),
                "NX connected solid body faces equality",
            )?)
        },
        "NX body topology lookup",
    )?
    else {
        return Ok(None);
    };
    let [shell_id] = region.shells.as_slice() else {
        return Ok(None);
    };
    let Some(shell) = ctx.find_by(
        &ir.model.shells,
        |shell| {
            Ok(ctx.equal_bytes(
                shell.id.as_str().as_bytes(),
                shell_id.as_str().as_bytes(),
                "NX connected solid body faces equality",
            )? && ctx.equal_bytes(
                shell.region.as_str().as_bytes(),
                region.id.as_str().as_bytes(),
                "NX connected solid body faces equality",
            )?)
        },
        "NX body topology lookup",
    )?
    else {
        return Ok(None);
    };
    let mut faces = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX connected solid faces")?;
    let mut records_iter = (shell.faces()).into_iter();
    while let Some(face_id) = ctx.next_charged(&mut records_iter, "NX shell face identities")? {
        let Some(face) = ctx.find_by(
            &ir.model.faces,
            |face| {
                Ok(ctx.equal_bytes(
                    face.id.as_str().as_bytes(),
                    face_id.as_str().as_bytes(),
                    "NX connected solid body faces equality",
                )? && ctx.equal_bytes(
                    face.shell.as_str().as_bytes(),
                    shell.id.as_str().as_bytes(),
                    "NX connected solid body faces equality",
                )?)
            },
            "NX body topology lookup",
        )?
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
    let Some(body) = ctx.find_by(
        &ir.model.bodies,
        |candidate| {
            Ok(ctx.equal_bytes(
                candidate.id.as_str().as_bytes(),
                body.id.as_str().as_bytes(),
                "NX connected solid body exists equality",
            )?)
        },
        "NX body topology lookup",
    )?
    else {
        return Ok(false);
    };
    if body.kind != BodyKind::Solid {
        return Ok(false);
    }
    let [region_id] = body.regions.as_slice() else {
        return Ok(false);
    };
    let Some(region) = ctx.find_by(
        &ir.model.regions,
        |region| {
            Ok(ctx.equal_bytes(
                region.id.as_str().as_bytes(),
                region_id.as_str().as_bytes(),
                "NX connected solid body exists equality",
            )? && ctx.equal_bytes(
                region.body.as_str().as_bytes(),
                body.id.as_str().as_bytes(),
                "NX connected solid body exists equality",
            )?)
        },
        "NX body topology lookup",
    )?
    else {
        return Ok(false);
    };
    let [shell_id] = region.shells.as_slice() else {
        return Ok(false);
    };
    let Some(shell) = ctx.find_by(
        &ir.model.shells,
        |shell| {
            Ok(ctx.equal_bytes(
                shell.id.as_str().as_bytes(),
                shell_id.as_str().as_bytes(),
                "NX connected solid body exists equality",
            )? && ctx.equal_bytes(
                shell.region.as_str().as_bytes(),
                region.id.as_str().as_bytes(),
                "NX connected solid body exists equality",
            )?)
        },
        "NX body topology lookup",
    )?
    else {
        return Ok(false);
    };
    let mut records_iter = (shell.faces()).into_iter();
    while let Some(face_id) =
        ctx.next_charged(&mut records_iter, "NX connected solid shell faces")?
    {
        if !ctx.any_by(
            &ir.model.faces,
            |face| {
                Ok(ctx.equal_bytes(
                    face.id.as_str().as_bytes(),
                    face_id.as_str().as_bytes(),
                    "NX connected solid body exists equality",
                )? && ctx.equal_bytes(
                    face.shell.as_str().as_bytes(),
                    shell.id.as_str().as_bytes(),
                    "NX connected solid body exists equality",
                )?)
            },
            "NX connected solid face candidates",
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) struct ScopedSurfaceIds<'ctx> {
    ids: BTreeSet<SurfaceId>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn selection_indices_native<T>(
    ctx: &DecodeContext<'_>,
    first: Option<&u32>,
    indices: &[T],
    value: impl Fn(&T) -> u32,
) -> Result<String, CodecError> {
    const PREFIX: &str = "nx:om-object-indices#";
    let first = first.map_or(&[][..], std::slice::from_ref);
    let mut text = ctx.copy_retained_text(PREFIX, "NX body selection prefix")?;
    for (ordinal, index) in first
        .iter()
        .copied()
        .chain(
            ctx.admit_iter(indices, "NX selection index text traversal")?
                .map(value),
        )
        .enumerate()
    {
        if ordinal != 0 {
            ctx.push_retained_char(&mut text, ',', "NX body selection separator")?;
        }
        ctx.append_formatted_retained(
            &mut text,
            format_args!("{index}"),
            "NX body selection index text",
        )?;
    }
    Ok(text)
}

pub(super) fn parameter_consumer_identity(
    ctx: &DecodeContext<'_>,
    operation_label: &str,
) -> Result<String, CodecError> {
    match ctx.split_once(
        operation_label,
        "operation-label",
        "NX parameter consumer label parsing",
    )? {
        Some((prefix, suffix)) => ctx.format_retained(
            format_args!("{prefix}feature{suffix}"),
            "NX parameter consumer identity text",
        ),
        None => ctx.format_retained(
            format_args!("{operation_label}"),
            "NX parameter consumer identity text",
        ),
    }
}

pub(super) fn insert_parameter_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: String,
) -> Result<(), CodecError> {
    let key = ctx.format_retained(key, "NX insert parameter property text")?;
    let key = cadmpeg_core::text::NonBlankString::for_decode(
        ctx,
        key,
        "NX insert parameter property key validation",
    )?
    .ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format_args!("NX parameter property key is blank"))
    })?;
    ctx.insert_btree_map(properties, key, value, "NX parameter property membership")?;
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
    for face in ctx.admit_iter(&*faces, "NX body surface face visits")? {
        if ctx.contains_btree_set(&ids, &face.surface, "NX body surface uniqueness")? {
            continue;
        }
        let surface = reservation.with_storage(|| {
            face.surface
                .try_clone_for_decode(ctx, "NX body surface identity copy")
        })?;
        reservation.with_storage(|| {
            ctx.insert_btree_set(&mut ids, surface, "NX body surface identities")
        })?;
    }
    Ok(Some(ScopedSurfaceIds {
        ids,
        _reservation: reservation,
    }))
}

fn unique_procedural_surface_owner<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    id: &cadmpeg_ir::ids::ProceduralSurfaceId,
) -> Result<Option<&'a SurfaceId>, CodecError> {
    let mut owner = None;
    let mut surfaces = ir.model.surfaces.iter();
    while let Some(surface) =
        ctx.next_charged(&mut surfaces, "NX procedural surface owner candidates")?
    {
        if let Some(construction) = surface.geometry.procedural_construction() {
            if ctx.equal_bytes(
                construction.as_str().as_bytes(),
                id.as_str().as_bytes(),
                "NX procedural surface owner identity",
            )? && owner.replace(&surface.id).is_some()
            {
                return Ok(None);
            }
        }
    }
    Ok(owner)
}

pub(super) struct ScopedSurfaceWitnesses<'ctx> {
    pub(super) values: Vec<SurfaceId>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) struct ScopedFaceSenses<'ctx> {
    values: Vec<Sense>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl std::ops::Deref for ScopedFaceSenses<'_> {
    type Target = [Sense];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
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
pub(super) fn blend_feature_definition<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
    family: NxBlendFamily,
) -> Result<Option<(FeatureDefinition, ScopedSurfaceWitnesses<'ctx>)>, CodecError> {
    let [body] = outputs else {
        return Ok(None);
    };
    let Some(body_surfaces) = body_surface_ids(ctx, ir, body)? else {
        return Ok(None);
    };
    let mut surface_storage = ctx.reserve_scoped(0, "NX blend result surface witnesses")?;
    let mut surfaces = Vec::new();
    let mut first_radius = None;
    let mut constant_radii = true;
    let mut uniform_radii = true;
    let mut variable_radii = true;
    let mut pairs = Vec::new();
    let mut pairs_reservation = ctx.reserve_scoped(0, "NX blend support pairs")?;
    let mut complete_pairs = true;
    let mut records_iter = (&ir.model.procedural_surfaces).into_iter();
    while let Some(procedural) =
        ctx.next_charged(&mut records_iter, "NX blend procedural surface scan")?
    {
        let Some(owner) = unique_procedural_surface_owner(ctx, ir, &procedural.id)? else {
            continue;
        };
        if !ctx.contains_btree_set(
            &body_surfaces.ids,
            owner,
            "NX body surface owner membership",
        )? {
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
        ctx.reserve_scoped_vec(
            &mut surface_storage,
            &mut surfaces,
            1,
            "NX blend result surfaces",
        )?;
        surfaces.push(surface_storage.with_storage(|| {
            owner.try_clone_for_decode(ctx, "NX feature projection surface identity")
        })?);
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
                if ctx.equal_bytes(
                    first.surface.as_str().as_bytes(),
                    second.surface.as_str().as_bytes(),
                    "NX blend feature definition equality",
                )? {
                    complete_pairs = false;
                } else {
                    ctx.charge_collection_items(1, "NX blend support pairs")?;
                    pairs_reservation.with_storage(|| {
                        ctx.reserve_capacity(&mut pairs, 1, "NX blend support pairs")
                    })?;
                    pairs.push(pairs_reservation.with_storage(|| {
                        Ok::<_, CodecError>([
                            first.surface.try_clone_for_decode(
                                ctx,
                                "NX blend first support identity copy",
                            )?,
                            second.surface.try_clone_for_decode(
                                ctx,
                                "NX blend second support identity copy",
                            )?,
                        ])
                    })?);
                }
            } else {
                complete_pairs = false;
            }
        }
    }
    if surfaces.is_empty() {
        return Ok(None);
    }
    ctx.stable_sort_by(
        &mut surfaces,
        |value| value,
        Ord::cmp,
        "NX blend result sort",
    )?;
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
                "NX blend feature definition text",
            )?;
            let second_native = ctx.format_retained(
                format_args!("{body}:blend-second-support-surfaces"),
                "NX blend feature definition text",
            )?;
            let (first_faces, _) = support_face_projection(ctx, ir, &sides.first, first_native)?;
            let (second_faces, _) = support_face_projection(ctx, ir, &sides.second, second_native)?;
            match (&first_faces, &second_faces) {
                (FaceSelection::Resolved { .. }, FaceSelection::Resolved { .. }) => {
                    cadmpeg_ir::features::FaceBlendOperands::new(first_faces, second_faces, ctx)?
                        .ok()
                        .map(|operands| {
                            radius
                                .try_clone_for_decode(ctx, "NX face blend radius copy")
                                .map(|radius| {
                                    FeatureDefinition::Operation(FeatureOperation::FaceBlend {
                                        operands,
                                        radius,
                                    })
                                })
                        })
                        .transpose()?
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
        ctx,
    )?
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
    Ok(Some((
        face_blend.unwrap_or(unresolved),
        ScopedSurfaceWitnesses {
            values: surfaces,
            _storage: surface_storage,
        },
    )))
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
    let mut records_iter = (pairs).into_iter();
    while let Some([first, second]) =
        ctx.next_charged(&mut records_iter, "NX blend support pairs")?
    {
        if ctx.equal_bytes(
            first.as_str().as_bytes(),
            second.as_str().as_bytes(),
            "NX blend support bipartition equality",
        )? {
            return Ok(None);
        }
        for (from, to) in [(first, second), (second, first)] {
            let neighbors = reservation
                .with_storage(|| {
                    ctx.entry_btree_map(&mut adjacent, from, "NX blend support graph nodes")
                })?
                .or_default();
            reservation.with_storage(|| {
                ctx.insert_btree_set(neighbors, to, "NX blend support graph edges")
            })?;
        }
    }
    let mut sides = BTreeMap::<&SurfaceId, bool>::new();
    let mut pending = Vec::new();
    let mut records_iter = (&adjacent).into_iter();
    while let Some((seed, _)) = ctx.next_charged(&mut records_iter, "NX blend support seeds")? {
        if ctx.contains_key_btree_map(
            &sides,
            seed,
            "NX blend support bipartition sides membership",
        )? {
            continue;
        }

        reservation.with_storage(|| {
            ctx.insert_btree_map(&mut sides, *seed, false, "NX blend support sides")
        })?;
        ctx.reserve_scoped_vec(&mut reservation, &mut pending, 1, "NX blend support queue")?;
        pending.push(*seed);
        while let Some(surface) = ctx.next_charged(
            &mut std::iter::from_fn(|| pending.pop()),
            "NX blend support queue visit",
        )? {
            let Some(&side) =
                ctx.get_btree_map(&sides, &surface, "NX blend queued support side")?
            else {
                return Ok(None);
            };
            let Some(neighbors) =
                ctx.get_btree_map(&adjacent, &surface, "NX blend queued support neighbors")?
            else {
                return Ok(None);
            };
            let mut neighbors = neighbors.iter();
            while let Some(neighbor) =
                ctx.next_charged(&mut neighbors, "NX blend support bipartition")?
            {
                match ctx.get_btree_map(
                    &sides,
                    neighbor,
                    "NX blend support bipartition sides lookup",
                )? {
                    Some(neighbor_side) if *neighbor_side == side => return Ok(None),
                    Some(_) => {}
                    None => {
                        reservation.with_storage(|| {
                            ctx.insert_btree_map(
                                &mut sides,
                                *neighbor,
                                !side,
                                "NX blend support sides",
                            )
                        })?;
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
    for (&surface, &second_side) in ctx.admit_iter(&sides, "NX blend support sides projection")? {
        let output = if second_side { &mut second } else { &mut first };
        ctx.charge_collection_items(1, "NX blend support output")?;
        reservation.with_storage(|| ctx.reserve_capacity(output, 1, "NX blend support output"))?;
        output.push(reservation.with_storage(|| {
            surface.try_clone_for_decode(ctx, "NX blend support identity copy")
        })?);
    }
    if !ctx.all_by(
        &first,
        |surface| {
            let Some(neighbors) =
                ctx.get_btree_map(&adjacent, &surface, "NX blend complete graph neighbors")?
            else {
                return Ok(false);
            };
            ctx.all_by(
                &second,
                |other| ctx.contains_btree_set(neighbors, &other, "NX blend complete graph edge"),
                "NX blend complete graph cross supports",
            )
        },
        "NX blend complete graph first supports",
    )? {
        return Ok(None);
    }
    Ok(Some(ScopedBlendSides {
        first,
        second,
        _reservation: reservation,
    }))
}

pub(super) fn offset_surface_feature_definition<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
) -> Result<Option<(FeatureDefinition, ScopedSurfaceWitnesses<'ctx>)>, CodecError> {
    let (data, support_storage) = ctx
        .with_scoped_storage("NX projected support surface witnesses", || {
            owned_offset_surface_data(ctx, ir, outputs)
        })?;
    let Some(OwnedOffsetSurfaceData {
        body,
        distance,
        supports,
    }) = data
    else {
        return Ok(None);
    };
    let native = ctx.format_retained(
        format_args!("{}:offset-support-surfaces", body.as_str()),
        "NX offset surface feature definition text",
    )?;
    let (faces, senses) = support_face_projection(ctx, ir, &supports, native)?;
    let distance = senses
        .as_deref()
        .map(|senses| uniform_face_sense(ctx, senses))
        .transpose()?
        .flatten()
        .map(|sense| match sense {
            Sense::Forward => distance,
            Sense::Reversed => distance.negated(),
        });
    Ok(Some((
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces,
            distance: distance.map(Length::from_assigned_real),
        }),
        ScopedSurfaceWitnesses {
            values: supports,
            _storage: support_storage,
        },
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
    if ctx.any_by(
        &carriers.values,
        |(_, candidate)| Ok(candidate.get().to_bits() != distance.get().to_bits()),
        "NX offset carrier distances",
    )? {
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
    for &(support, _) in ctx.admit_iter(carriers, "NX offset carrier supports")? {
        if ctx.any_by(
            &supports,
            |existing: &SurfaceId| {
                ctx.equal_bytes(
                    existing.as_str().as_bytes(),
                    support.as_str().as_bytes(),
                    "NX offset support identity equality",
                )
            },
            "NX offset support uniqueness",
        )? {
            continue;
        }
        ctx.reserve_vec(&mut supports, 1, "NX offset support output")?;
        supports.push(support.try_clone_for_decode(ctx, "NX feature projection surface identity")?);
    }
    ctx.stable_sort_by(
        &mut supports,
        |value| value,
        Ord::cmp,
        "NX offset support sort",
    )?;
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
    for procedural in ctx.admit_iter(
        &ir.model.procedural_surfaces,
        "NX offset procedural surface scan",
    )? {
        let Some(owner) = unique_procedural_surface_owner(ctx, ir, &procedural.id)? else {
            continue;
        };
        if !ctx.contains_btree_set(
            &body_surfaces.ids,
            owner,
            "NX body surface owner membership",
        )? {
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

pub(super) fn thicken_feature_definition<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
) -> Result<Option<(FeatureDefinition, ScopedSurfaceWitnesses<'ctx>)>, CodecError> {
    let (data, support_storage) = ctx
        .with_scoped_storage("NX projected support surface witnesses", || {
            owned_thicken_surface_data(ctx, ir, outputs)
        })?;
    let Some(OwnedThickenSurfaceData {
        body,
        thickness,
        supports,
        direction,
    }) = data
    else {
        return Ok(None);
    };
    let native = ctx.format_retained(
        format_args!("{}:thicken-support-surfaces", body.as_str()),
        "NX thicken feature definition text",
    )?;
    let (faces, senses) = support_face_projection(ctx, ir, &supports, native)?;
    let side = match direction {
        ThickenDirection::Both => Some(ThickenSide::Both),
        ThickenDirection::Signed(distance) => senses
            .as_deref()
            .map(|senses| uniform_face_sense(ctx, senses))
            .transpose()?
            .flatten()
            .map(|sense| thicken_side(distance, sense)),
    };
    Ok(Some((
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces,
            thickness: Some(thickness),
            side,
        }),
        ScopedSurfaceWitnesses {
            values: supports,
            _storage: support_storage,
        },
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
    let Some(output_body) = ctx.find_by(
        &ir.model.bodies,
        |candidate| {
            Ok(ctx.equal_bytes(
                candidate.id.as_str().as_bytes(),
                body.as_str().as_bytes(),
                "NX owned thicken surface data equality",
            )?)
        },
        "NX thicken output body lookup",
    )?
    else {
        return Ok(None);
    };
    if output_body.kind != BodyKind::Solid {
        return Ok(None);
    }
    let distance = carriers.values[0].1;
    if ctx.all_by(
        &carriers.values,
        |(_, candidate)| Ok(candidate.get().to_bits() == distance.get().to_bits()),
        "NX thicken carrier distances",
    )? {
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
    let mut records_iter = (&carriers.values).into_iter();
    while let Some(&(support, distance)) =
        ctx.next_charged(&mut records_iter, "NX thicken signed supports")?
    {
        let Ok(distance) = NonZeroLength::try_from(Length::from_assigned_real(distance)) else {
            return Ok(None);
        };
        let candidate = distance.abs();
        if magnitude.is_some_and(|magnitude| magnitude.get().to_bits() != candidate.get().to_bits())
        {
            return Ok(None);
        }
        magnitude = Some(candidate);
        let set = if distance.get().is_sign_positive() {
            &mut positive
        } else {
            &mut negative
        };
        support_reservation.with_storage(|| {
            ctx.insert_btree_set(set, support, "NX thicken signed support membership")
        })?;
    }
    if positive.is_empty()
        || positive.len() != negative.len()
        || !ctx.all_by(
            &positive,
            |support| {
                ctx.contains_btree_set(&negative, support, "NX thicken negative support membership")
            },
            "NX thicken signed support equality",
        )?
    {
        return Ok(None);
    }
    let Some(magnitude) = magnitude else {
        return Ok(None);
    };
    let Some(thickness) = PositiveLength::new(magnitude.get() * 2.0) else {
        return Ok(None);
    };
    let mut supports = Vec::new();
    for &support in ctx.admit_iter(&positive, "NX thicken support output")? {
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

pub(super) fn support_face_projection<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    supports: &[SurfaceId],
    native: String,
) -> Result<(FaceSelection, Option<ScopedFaceSenses<'ctx>>), CodecError> {
    let mut selected: Vec<(&cadmpeg_ir::ids::FaceId, Sense)> = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX support face projection")?;
    let mut records_iter = (supports).into_iter();
    while let Some(support) = ctx.next_charged(&mut records_iter, "NX support face identities")? {
        let mut matching_face = None;
        let mut records_iter = (&ir.model.faces).into_iter();
        while let Some(face) = ctx.next_charged(&mut records_iter, "NX support face lookup")? {
            if ctx.equal_bytes(
                face.surface.as_str().as_bytes(),
                support.as_str().as_bytes(),
                "NX support face projection equality",
            )? {
                if matching_face.is_some() {
                    return Ok((FaceSelection::Native(native), None));
                }
                matching_face = Some(face);
            }
        }
        let Some(face) = matching_face else {
            return Ok((FaceSelection::Native(native), None));
        };
        if ctx.any_by(
            &selected,
            |(id, _)| {
                Ok(ctx.equal_bytes(
                    id.as_str().as_bytes(),
                    face.id.as_str().as_bytes(),
                    "NX support face projection equality",
                )?)
            },
            "NX support face uniqueness",
        )? {
            return Ok((FaceSelection::Native(native), None));
        }
        ctx.charge_collection_items(1, "NX support face projection")?;
        reservation.with_storage(|| {
            ctx.reserve_capacity(&mut selected, 1, "NX support face projection")
        })?;
        selected.push((&face.id, face.sense));
    }
    let mut faces = Vec::new();
    let mut senses = Vec::new();
    for (face, sense) in ctx.admit_iter(selected, "NX support face result projection")? {
        ctx.charge_collection_items(2, "NX resolved support faces")?;
        ctx.reserve_capacity(&mut faces, 1, "NX resolved support faces")?;
        reservation
            .with_storage(|| ctx.reserve_capacity(&mut senses, 1, "NX resolved support senses"))?;
        faces.push(face.try_clone_for_decode(ctx, "NX support face identity copy")?);
        senses.push(sense);
    }
    Ok((
        FaceSelection::Resolved { faces, native },
        Some(ScopedFaceSenses {
            values: senses,
            _storage: reservation,
        }),
    ))
}

pub(super) fn thicken_side(distance: NonZeroLength, sense: Sense) -> ThickenSide {
    match (distance.get().is_sign_positive(), sense) {
        (true, Sense::Forward) | (false, Sense::Reversed) => ThickenSide::Forward,
        (true, Sense::Reversed) | (false, Sense::Forward) => ThickenSide::Reverse,
    }
}

pub(super) fn uniform_face_sense(
    ctx: &DecodeContext<'_>,
    senses: &[Sense],
) -> Result<Option<Sense>, CodecError> {
    let Some((first, rest)) = senses.split_first() else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(rest, |sense| Ok(sense == first), "NX uniform face senses")?
        .then_some(*first))
}

pub(in crate::native) fn feature_source_content(
    ctx: &DecodeContext<'_>,
    payload_strings: &[&crate::native::features::FeaturePayloadString],
) -> Result<cadmpeg_ir::features::FeatureContent, CodecError> {
    let mut sorted = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX feature source text order")?;
    for &value in ctx.admit_iter(payload_strings, "NX feature source strings")? {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut sorted,
            1,
            "NX feature source text order",
        )?;
        sorted.push(value);
    }
    ctx.stable_sort_by(
        &mut sorted,
        |value| &value.source_offset,
        Ord::cmp,
        "NX feature source text sort",
    )?;
    let mut content = Vec::new();
    for value in ctx
        .admit_iter(&sorted, "NX ordered feature source strings")?
        .copied()
    {
        let text = value.value.as_str();

        ctx.charge_collection_items(1, "NX feature source text")?;

        let mut owned = String::new();
        ctx.try_reserve_retained_text(&mut owned, text.len(), "NX feature source text")?;
        ctx.append_retained(&mut owned, text, "NX feature source content text append")?;
        ctx.reserve_capacity(&mut content, 1, "NX feature source text")?;
        content.push(FeatureSourceContent::Text(owned));
    }
    cadmpeg_ir::features::FeatureContent::new(content, ctx, "NX feature source content validation")
        .map_err(CodecError::from)
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
        ctx.insert_btree_map(
            properties,
            ctx.copy_retained_text(key, "NX simple hole native property")?,
            ctx.copy_retained_text(value, "NX simple hole native property")?,
            "NX simple hole native property",
        )?;
        Ok(())
    }
    if let Some(template) = ctx.find_by(
        templates,
        |template| {
            Ok(ctx.equal_bytes(
                template.operation_label.as_bytes(),
                operation_label.as_bytes(),
                "NX simple hole native properties equality",
            )?)
        },
        "NX simple hole templates",
    )? {
        insert_property(ctx, properties, "simple_hole_template", &template.id)?;
    }
    if let Some(pair) = ctx.find_by(
        repeated_lanes,
        |pair| {
            Ok(ctx.equal_bytes(
                pair.operation_label.as_bytes(),
                operation_label.as_bytes(),
                "NX simple hole native properties equality",
            )?)
        },
        "NX simple hole scalar lanes",
    )? {
        insert_property(
            ctx,
            properties,
            "simple_hole_repeated_scalar_lane",
            &pair.id,
        )?;
    }
    if let Some(references) = ctx.find_by(
        block_references,
        |references| {
            Ok(ctx.equal_bytes(
                references.operation_label.as_bytes(),
                operation_label.as_bytes(),
                "NX simple hole native properties equality",
            )?)
        },
        "NX simple hole block references",
    )? {
        insert_property(
            ctx,
            properties,
            "simple_hole_repeated_scalar_lane_block_references",
            &references.id,
        )?;
    }
    let mut records_iter = (construction_groups).into_iter();
    while let Some(group) =
        ctx.next_charged(&mut records_iter, "NX simple hole construction groups")?
    {
        if ctx.any_by(
            &*group.members,
            |member| {
                Ok(ctx.equal_bytes(
                    member.operation_label.as_bytes(),
                    operation_label.as_bytes(),
                    "NX simple hole native properties equality",
                )?)
            },
            "NX simple hole construction members",
        )? {
            insert_property(ctx, properties, "simple_hole_construction_group", &group.id)?;
            break;
        }
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
        ctx.stable_sort_by(
            &mut band.offsets,
            |value| value,
            f64::total_cmp,
            "NX block plane sort",
        )?;
        let mut first: Option<[f64; 2]> = None;
        let mut second: Option<[f64; 2]> = None;
        let mut records_iter = (&band.offsets).into_iter();
        while let Some(&offset) = ctx.next_charged(&mut records_iter, "NX block plane offsets")? {
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
            let mut records_iter = (&ir.model.bodies).into_iter();
            while let Some(candidate) =
                ctx.next_charged(&mut records_iter, "NX primitive fallback bodies")?
            {
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
    let mut records_iter = (&*faces).into_iter().copied();
    while let Some(face) = ctx.next_charged(&mut records_iter, "NX primitive face visits")? {
        let Some(geometry) = ctx
            .rposition_by(
                &(ir.model.surfaces),
                |surface| {
                    Ok(ctx.equal_bytes(
                        surface.id.as_str().as_bytes(),
                        face.surface.as_str().as_bytes(),
                        "NX block placement equality",
                    )?)
                },
                "NX block plane surface lookup",
            )?
            .and_then(|index| (ir.model.surfaces).get(index))
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
        let existing = ctx.position_by(
            &bands,
            |band| Ok((1.0 - band.normal.dot(normal)).abs() <= angular_tolerance),
            "NX block plane band lookup",
        )?;
        ctx.charge_collection_items(1, "NX block plane offsets")?;
        if let Some(index) = existing {
            let band = &mut bands[index];
            band_reservation.with_storage(|| {
                ctx.reserve_capacity(&mut band.offsets, 1, "NX block plane offsets")
            })?;
            band.offsets.push(offset);
        } else {
            ctx.charge_collection_items(1, "NX block plane bands")?;
            let mut offsets = Vec::new();
            band_reservation
                .with_storage(|| ctx.reserve_capacity(&mut offsets, 1, "NX block plane offsets"))?;
            offsets.push(offset);
            band_reservation
                .with_storage(|| ctx.reserve_capacity(&mut bands, 1, "NX block plane bands"))?;
            bands.push(PlaneBand { normal, offsets });
        }
    }
    if bands.len() != 3 {
        return Ok(None);
    }
    for first in 0usize..3 {
        if (first + 1..3)
            .any(|second| bands[first].normal.dot(bands[second].normal).abs() > angular_tolerance)
        {
            return Ok(None);
        }
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
    for (first, second) in [(0, 1), (1, 2), (0, 1)] {
        let left = extents[first].normal;
        let right = extents[second].normal;
        if right
            .x
            .total_cmp(&left.x)
            .then_with(|| right.y.total_cmp(&left.y))
            .then_with(|| right.z.total_cmp(&left.z))
            .is_gt()
        {
            extents.swap(first, second);
        }
    }
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

    Ok(Some((
        body.try_clone_for_decode(ctx, "NX block output body")?,
        placement,
    )))
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
            let mut records_iter = (&ir.model.bodies).into_iter();
            while let Some(candidate) =
                ctx.next_charged(&mut records_iter, "NX primitive fallback bodies")?
            {
                let Some(faces) = connected_solid_body_faces(ctx, ir, &candidate.id)? else {
                    continue;
                };
                let [face] = &faces[..] else {
                    continue;
                };
                if !ctx.any_by(
                    &ir.model.surfaces,
                    |surface| {
                        Ok({
                            ctx.equal_bytes(
                                surface.id.as_str().as_bytes(),
                                face.surface.as_str().as_bytes(),
                                "NX sphere body projection equality",
                            )? && matches!(
                                surface.geometry.solved(),
                                Some(SolvedSurfaceGeometry::Sphere(_))
                            )
                        })
                    },
                    "NX sphere surface lookup",
                )? {
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
    let Some(surface) = ctx.find_by(
        &ir.model.surfaces,
        |surface| {
            Ok(ctx.equal_bytes(
                surface.id.as_str().as_bytes(),
                face.surface.as_str().as_bytes(),
                "NX sphere body projection equality",
            )?)
        },
        "NX sphere surface lookup",
    )?
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

    Ok(Some((
        body.try_clone_for_decode(ctx, "NX sphere output body")?,
        center,
        radius,
    )))
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

pub(super) fn new_body_boolean_op(
    ctx: &DecodeContext<'_>,
    evidence: &NewBodyEvidence<'_>,
) -> Result<BooleanOp, CodecError> {
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
        return Ok(BooleanOp::Unresolved);
    }
    if evidence.has_complete_projection
        && matches!(evidence.outputs, [_])
        && !evidence.history.has_preceding_writer(
            ctx,
            evidence.provisional_feature,
            evidence.native_primary_body,
            evidence.offset_store_primary_body,
            evidence.outputs,
        )?
    {
        Ok(BooleanOp::NewBody)
    } else {
        Ok(BooleanOp::Unresolved)
    }
}

pub(super) fn body_writing_unresolved_feature_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    source_properties: &BTreeMap<String, String>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let body_write = ctx.any_by(
        source_properties,
        |(key, _)| Ok(key.starts_with("body_write.")),
        "NX body-writing property visit",
    )?;
    if !body_write {
        return Ok(None);
    }
    Ok(match kind {
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
                ctx,
            )?
            .map_err(CodecError::malformed)?,

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
    })
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
        &cadmpeg_test_support::service_decode_context(),
        kind,
        payload_strings,
        block_dimensions,
        block_placement,
        HoleProjection {
            diameter: hole_diameter,
            ..HoleProjection::default()
        },
        || Ok(BTreeMap::new()),
    )
    .unwrap()
}

/// Project one operation as a history node only when its bounded record has
/// no modeling relation or value lane.
pub(super) fn non_modeling_history_definition(
    ctx: &DecodeContext<'_>,
    kind: &str,
    object_indices: &[Option<u32>; 4],
    outputs: &[BodyId],
    body_reference_count: usize,
    body_operand_count: usize,
    payload_string_count: usize,
    source_properties: &BTreeMap<String, String>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let operation_identity_only = ctx.all_by(
        source_properties,
        |(key, _)| {
            Ok({
                matches!(
                    key.as_str(),
                    "operation_record" | "operation_terminal_frame"
                ) || (key
                    .strip_prefix("object_index.")
                    .is_some_and(|slot| matches!(slot, "0" | "1" | "2" | "3")))
            })
        },
        "NX history source property keys",
    )?;
    Ok((kind == "EXTRACT_STRING"
        && object_indices.iter().all(Option::is_none)
        && outputs.is_empty()
        && body_reference_count == 0
        && body_operand_count == 0
        && payload_string_count == 0
        && ctx.contains_key_btree_map(
            source_properties,
            "operation_record",
            "NX history operation record membership",
        )?
        && ctx.contains_key_btree_map(
            source_properties,
            "operation_terminal_frame",
            "NX history terminal frame membership",
        )?
        && operation_identity_only)
        .then_some(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: TreeChildren::default(),
        })))
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

fn native_feature_kind(
    ctx: &DecodeContext<'_>,
    value: &str,
) -> Result<NativeFeatureKind, CodecError> {
    let (kind, storage) = ctx.with_scoped_storage("NX native feature kind", || {
        ctx.copy_retained_text(value, "NX native feature kind")
            .map(NativeFeatureKind::from)
    })?;
    if matches!(&kind, NativeFeatureKind::Other(_)) {
        storage.commit()?;
    }
    Ok(kind)
}

pub(super) fn non_boolean_feature_definition_with_parameters(
    ctx: &DecodeContext<'_>,
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole: HoleProjection,
    native_parameters: impl FnOnce() -> Result<
        BTreeMap<cadmpeg_core::text::NonBlankString, String>,
        CodecError,
    >,
) -> Result<FeatureDefinition, CodecError> {
    if matches!(kind, "BLEND" | "FACE_BLEND") {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: native_feature_kind(ctx, kind)?,
            parameters: native_parameters()?,
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
                ctx,
            )?
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
            source: PathRef::Unresolved(
                ctx.copy_retained_text("nx:unresolved", "NX unresolved path identity")?,
            ),
            target_faces: FaceSelection::Unresolved,
            direction: CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved),
            bidirectional: None,
        }),
        "TRIMMED_SH" => FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: PathRef::Unresolved(
                ctx.copy_retained_text("nx:unresolved", "NX unresolved path identity")?,
            ),
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
            let hole_template = unique_simple_hole_template(ctx, payload_strings)?;
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
                ctx,
            )?
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
                ctx,
            )?
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }),
        "EXTRUDE" => extrude_feature_definition(ctx, None, None, BooleanOp::Unresolved, &[])?,
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
                    ctx,
                )?
                .map_err(cadmpeg_core::CodecError::malformed)?,

                approximate: None,
            })
        }
        _ => FeatureDefinition::Operation(FeatureOperation::Native {
            kind: native_feature_kind(ctx, kind)?,
            parameters: native_parameters()?,
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
    let mut records_iter = (outputs).into_iter().enumerate();
    while let Some((index, body)) = ctx.next_charged(&mut records_iter, "NX BREP output bodies")? {
        if ctx.any_by(
            &outputs[..index],
            |existing| {
                ctx.equal_bytes(
                    existing.as_str().as_bytes(),
                    body.as_str().as_bytes(),
                    "NX BREP output uniqueness identity",
                )
            },
            "NX BREP output uniqueness",
        )? {
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

pub(super) fn native_feature_parameters<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    uses: &[&crate::native::features::FeatureParameterUse],
    expressions: &[crate::native::om::ParameterFormula],
) -> Result<
    (
        BTreeMap<String, String>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut nodes = ctx.reserve_scoped(0, "NX native parameter staging nodes")?;
    if uses.is_empty() {
        return Ok((BTreeMap::new(), nodes));
    }
    let (expressions_by_id, _expression_index_storage) = ctx.collect_scoped_btree_map(
        expressions
            .iter()
            .map(|expression| (expression.id.as_str(), expression)),
        "NX native parameter expression index",
    )?;
    let mut selected = BTreeMap::new();
    let mut selection_storage = ctx.reserve_scoped(0, "NX native parameter selection")?;

    let mut uses = uses.iter();
    while let Some(parameter_use) =
        ctx.next_charged(&mut uses, "NX native feature parameter uses")?
    {
        let Some(expression) = ctx.get_btree_map(
            &expressions_by_id,
            parameter_use.expression.as_str(),
            "NX native parameter expression lookup",
        )?
        else {
            return Ok((BTreeMap::new(), nodes));
        };
        match selection_storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut selected,
                expression.name.as_str(),
                "NX native parameter name selection",
            )
        })? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(*expression);
            }
            std::collections::btree_map::Entry::Occupied(_) => return Ok((BTreeMap::new(), nodes)),
        }
    }
    let mut parameters = BTreeMap::new();
    for (name, expression) in ctx.admit_iter(selected, "NX kept native parameter values")? {
        let name = ctx.copy_retained_text(name, "NX native feature parameter")?;
        let value =
            ctx.copy_retained_text(&expression.expression, "NX native feature parameter")?;
        nodes.with_storage(|| {
            ctx.insert_btree_map(&mut parameters, name, value, "NX native feature parameter")
        })?;
    }
    Ok((parameters, nodes))
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
    for template in ctx.admit_iter(templates, "NX primary hole output scan")? {
        let Some(object_index) = ctx.get_btree_map(
            body_references,
            template.operation_label.as_str(),
            "NX primary hole outputs body references lookup",
        )?
        else {
            continue;
        };
        let bodies =
            feature_body_outputs(ctx, *object_index, body_bindings, bodies_by_object_index)?;
        ctx.insert_btree_map(
            &mut outputs,
            ctx.copy_retained_text(&template.operation_label, "NX primary hole output map")?,
            bodies,
            "NX primary hole output map",
        )?;
    }
    Ok(outputs)
}

pub(super) fn simple_hole_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
    operation_positions: &BTreeMap<&str, usize>,
) -> Result<Option<Vec<String>>, CodecError> {
    let (unique_templates, _unique_template_storage) = ctx.unique_index(
        templates
            .iter()
            .map(|template| (template.operation_label.as_str(), template)),
        "NX unique hole template index",
    )?;
    let mut ordered_templates = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX simple hole selected templates")?;
    let mut records_iter = (templates).into_iter();
    while let Some(template) =
        ctx.next_charged(&mut records_iter, "NX simple hole template scan")?
    {
        if template.form != crate::native::features::holes::SimpleHoleForm::Simple
            || template.extent != crate::native::features::holes::SimpleHoleExtent::Through
        {
            continue;
        }
        if ctx
            .get_hash_map(
                &unique_templates,
                template.operation_label.as_str(),
                "NX unique hole template lookup",
            )?
            .and_then(Option::as_ref)
            .is_none()
        {
            return Ok(None);
        }
        let Some(&position) = ctx.get_btree_map(
            operation_positions,
            template.operation_label.as_str(),
            "NX simple hole operation position",
        )?
        else {
            return Ok(None);
        };
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut ordered_templates,
            1,
            "NX simple hole selected templates",
        )?;
        ordered_templates.push((position, template));
    }
    if ordered_templates.is_empty() {
        return Ok(None);
    }
    ctx.stable_sort_by_key(
        &mut ordered_templates,
        |value| (value.0, value.1.operation_label.as_str()),
        Ord::cmp,
        "sort NX simple hole templates",
    )?;
    let mut selected_group = None;
    let mut records_iter = (groups).into_iter();
    while let Some(group) =
        ctx.next_charged(&mut records_iter, "NX simple hole construction groups")?
    {
        let mut same_operations = true;
        let mut records_iter = (&*group.members).into_iter();
        while let Some(member) =
            ctx.next_charged(&mut records_iter, "NX simple hole group members")?
        {
            if !ctx.any_by(
                &ordered_templates,
                |(_, template)| {
                    Ok(ctx.equal_bytes(
                        template.operation_label.as_bytes(),
                        member.operation_label.as_bytes(),
                        "NX simple hole operations equality",
                    )?)
                },
                "NX simple hole selected template membership",
            )? {
                same_operations = false;
                break;
            }
        }
        if same_operations {
            let mut records_iter = (&ordered_templates).into_iter();
            while let Some((_, template)) = ctx.next_charged(
                &mut records_iter,
                "NX simple hole selected template coverage",
            )? {
                if !ctx.any_by(
                    &*group.members,
                    |member| {
                        Ok(ctx.equal_bytes(
                            member.operation_label.as_bytes(),
                            template.operation_label.as_bytes(),
                            "NX simple hole operations equality",
                        )?)
                    },
                    "NX simple hole group member coverage",
                )? {
                    same_operations = false;
                    break;
                }
            }
        }
        if same_operations && selected_group.replace(group).is_some() {
            return Ok(None);
        }
    }
    let mut operations = Vec::new();
    if let Some(group) = selected_group {
        let mut previous = None;
        let mut members = group.members.iter();
        while let Some(member) =
            ctx.next_charged(&mut members, "NX simple hole adjacent group order")?
        {
            let Some(&position) = ctx.get_btree_map(
                operation_positions,
                member.operation_label.as_str(),
                "NX simple hole group operation position",
            )?
            else {
                return Ok(None);
            };
            if previous
                .replace(position)
                .is_some_and(|prior| prior >= position)
            {
                return Ok(None);
            }
        }
        for member in ctx.admit_iter(&*group.members, "NX simple hole ordered group members")? {
            ctx.push_vec(
                &mut operations,
                ctx.copy_retained_text(&member.operation_label, "NX hole operation labels")?,
                "NX hole operation labels",
            )?;
        }
    } else {
        for (_, template) in ctx
            .admit_iter(&ordered_templates, "NX simple hole ordered labels")?
            .copied()
        {
            ctx.push_vec(
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
    let (unique_templates, _unique_template_storage) = ctx.unique_index(
        templates
            .iter()
            .map(|template| (template.operation_label.as_str(), template)),
        "NX unique hole template index",
    )?;
    let mut ordered = Vec::new();
    let mut storage = ctx.reserve_scoped(0, "NX selected hole operation ordering")?;
    let mut candidates = templates.iter();
    while let Some(template) =
        ctx.next_charged(&mut candidates, "NX selected hole template scan")?
    {
        if !accepts(template) {
            continue;
        }
        let operation = template.operation_label.as_str();
        if ctx
            .get_hash_map(
                &unique_templates,
                operation,
                "NX unique hole template lookup",
            )?
            .and_then(Option::as_ref)
            .is_none()
        {
            continue;
        }
        let Some(&position) = ctx.get_btree_map(
            operation_positions,
            operation,
            "NX selected hole operation position lookup",
        )?
        else {
            return Ok(None);
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut ordered,
            (position, operation),
            "NX selected hole ordering records",
        )?;
    }
    if ordered.is_empty() {
        return Ok(None);
    }
    ctx.stable_sort_by_key(
        &mut ordered,
        |value| *value,
        Ord::cmp,
        "sort NX hole operations",
    )?;
    let mut operations = ctx.collection_vec(ordered.len(), "NX ordered hole operation labels")?;
    for (_, operation) in ctx.admit_iter(ordered, "NX ordered hole operation labels")? {
        operations.push(ctx.copy_retained_text(operation, "NX hole operation labels")?);
    }
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
    let (uses_by_operation, _operation_use_storage) = ctx.unique_index(
        uses.iter()
            .map(|use_| (use_.operation_label.as_str(), use_)),
        "NX hole package operation use index",
    )?;
    let (uses_by_group, _group_use_storage) = ctx.unique_index(
        uses.iter()
            .map(|use_| (use_.simple_hole_construction_group.as_str(), use_)),
        "NX hole package group use index",
    )?;
    let mut projection = HolePackageProjection::default();
    let mut records_iter = (uses).into_iter();
    while let Some(use_) =
        ctx.next_charged(&mut records_iter, "NX hole package construction uses")?
    {
        if ctx
            .get_hash_map(
                &uses_by_operation,
                use_.operation_label.as_str(),
                "NX hole package unique operation use",
            )?
            .and_then(Option::as_ref)
            .is_none()
            || ctx
                .get_hash_map(
                    &uses_by_group,
                    use_.simple_hole_construction_group.as_str(),
                    "NX hole package unique group use",
                )?
                .and_then(Option::as_ref)
                .is_none()
        {
            continue;
        }
        let Some(group) = ctx.find_by(
            groups,
            |group| {
                Ok(ctx.equal_bytes(
                    group.id.as_str().as_bytes(),
                    use_.simple_hole_construction_group.as_str().as_bytes(),
                    "NX hole package projection equality",
                )?)
            },
            "NX hole package group lookup",
        )?
        else {
            continue;
        };
        if ctx.any_by(
            &*group.members,
            |member| {
                ctx.contains_btree_set(
                    &projection.internal_operations,
                    &member.operation_label,
                    "NX hole package internal membership",
                )
            },
            "NX hole package internal member conflicts",
        )? {
            continue;
        }
        let mut requests_chamfer = true;
        let mut requests_no_treatment = true;
        let mut complete_templates = true;
        let mut records_iter = (&*group.members).into_iter();
        while let Some(member) = ctx.next_charged(&mut records_iter, "NX hole package members")? {
            let mut matching_template = None;
            let mut multiple_templates = false;
            let mut records_iter = (templates).into_iter();
            while let Some(template) =
                ctx.next_charged(&mut records_iter, "NX hole package member templates")?
            {
                if ctx.equal_bytes(
                    template.operation_label.as_bytes(),
                    member.operation_label.as_bytes(),
                    "NX hole package projection equality",
                )? {
                    if matching_template.is_some() {
                        multiple_templates = true;
                        break;
                    }
                    matching_template = Some(template);
                }
            }
            let Some(template) = matching_template else {
                complete_templates = false;
                break;
            };
            if multiple_templates
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
            .map(|member| {
                ctx.get_btree_map(
                    outputs,
                    &member.operation_label,
                    "NX hole package first output lookup",
                )
            })
            .transpose()?
            .flatten()
            .and_then(|bodies| bodies.as_slice().first().filter(|_| bodies.len() == 1))
        else {
            continue;
        };
        let mut complete_outputs = true;
        let mut records_iter = (&*group.members).into_iter();
        while let Some(member) = ctx.next_charged(&mut records_iter, "NX hole package members")? {
            if !matches!(ctx.get_btree_map(outputs, &member.operation_label, "NX hole package member output lookup")?.map(Vec::as_slice), Some([candidate]) if ctx.equal_bytes(candidate.as_str().as_bytes(), body.as_str().as_bytes(), "NX hole package output identity equality")?)
            {
                complete_outputs = false;
                break;
            }
        }
        if !complete_outputs {
            continue;
        }
        let Some(diameter) = group
            .members
            .first()
            .map(|member| &member.operation_label)
            .map(|operation| {
                ctx.get_btree_map(
                    diameters,
                    operation,
                    "NX hole package first diameters lookup",
                )
            })
            .transpose()?
            .flatten()
            .copied()
        else {
            continue;
        };
        let mut complete_diameters = true;
        let mut records_iter = (&*group.members).into_iter();
        while let Some(member) = ctx.next_charged(&mut records_iter, "NX hole package members")? {
            if ctx
                .get_btree_map(
                    diameters,
                    &member.operation_label,
                    "NX hole package member diameters lookup",
                )?
                .copied()
                != Some(diameter)
            {
                complete_diameters = false;
                break;
            }
        }
        if !complete_diameters {
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
                .map(|operation| {
                    ctx.get_btree_map(chamfers, operation, "NX hole package first chamfers lookup")
                })
                .transpose()?
                .flatten()
                .copied()
            else {
                continue;
            };
            let mut complete_chamfers = true;
            let mut records_iter = (&*group.members).into_iter();
            while let Some(member) =
                ctx.next_charged(&mut records_iter, "NX hole package members")?
            {
                if ctx
                    .get_btree_map(
                        chamfers,
                        &member.operation_label,
                        "NX hole package member chamfers lookup",
                    )?
                    .copied()
                    != Some(chamfer)
                {
                    complete_chamfers = false;
                    break;
                }
            }
            if !complete_chamfers {
                continue;
            }
            Some(chamfer)
        } else {
            None
        };
        for member in ctx.admit_iter(&*group.members, "NX hole package members")? {
            if ctx.contains_btree_set(
                &projection.internal_operations,
                &member.operation_label,
                "NX hole package internal membership",
            )? {
                continue;
            }
            ctx.insert_btree_set(
                &mut projection.internal_operations,
                ctx.copy_retained_text(
                    &member.operation_label,
                    "NX hole package internal operations",
                )?,
                "NX hole package internal operations",
            )?;
        }
        insert_hole_output_body(ctx, &mut projection.outputs, &use_.operation_label, body)?;
        ctx.insert_btree_map(
            &mut projection.diameters,
            ctx.copy_retained_text(&use_.operation_label, "NX hole package diameter map")?,
            diameter,
            "NX hole package diameter map",
        )?;
        if let Some(chamfer) = chamfer {
            ctx.insert_btree_map(
                &mut projection.chamfers,
                ctx.copy_retained_text(&use_.operation_label, "NX hole package chamfer map")?,
                chamfer,
                "NX hole package chamfer map",
            )?;
        }
        let (placements, placement_storage) = ctx
            .with_scoped_storage("NX hole package placement candidate", || {
                hole_axis_placements_for_body(ctx, ir, body)
            })?;
        if placements.len() == group.members.len() {
            placement_storage.commit()?;
            ctx.insert_btree_map(
                &mut projection.placements,
                ctx.copy_retained_text(&use_.operation_label, "NX hole package placement map")?,
                placements,
                "NX hole package placement map",
            )?;
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
    for (key, value) in ctx.admit_iter(source, operation)? {
        ctx.insert_btree_map(target, key, value, operation)?;
    }
    Ok(())
}

pub(super) fn hole_operations_are_unique(
    ctx: &DecodeContext<'_>,
    operations: &[String],
) -> Result<bool, CodecError> {
    let mut seen = BTreeSet::new();
    let mut storage = ctx.reserve_scoped(0, "NX hole operation uniqueness index")?;
    let mut operations = operations.iter();
    while let Some(operation) = ctx.next_charged(&mut operations, "NX hole operation labels")? {
        if !ctx.insert_scoped_btree_value(
            &mut storage,
            &mut seen,
            operation.as_str(),
            "NX hole operation uniqueness",
        )? {
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
    let mut bodies = ctx.collection_vec(1, "NX hole output body")?;
    bodies.push(body.try_clone_for_decode(ctx, "NX hole output body")?);
    ctx.insert_btree_map(
        &mut *outputs,
        ctx.copy_retained_text(operation, "NX hole output map")?,
        bodies,
        "NX hole output map",
    )?;
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
    let (operations_by_body, _operations_by_body_storage) = ctx
        .with_scoped_storage("NX operations by body scratch", || {
            hole_operations_by_body(ctx, ir, operations, outputs)
        })?;
    let Some(operations_by_body) = operations_by_body else {
        return Ok(None);
    };

    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut records_iter = (operations_by_body).into_iter();
    while let Some((body, operations)) =
        ctx.next_charged(&mut records_iter, "NX hole body projection groups")?
    {
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let (bores, _bores_storage) = ctx.with_scoped_storage("NX bores scratch", || {
            cylindrical_face_witnesses(ctx, ir, &body_faces)
        })?;
        let Some(bores) = bores else {
            return Ok(None);
        };
        let Some(radius) = bores.first().map(|bore| bore.radius) else {
            return Ok(None);
        };
        if bores.len() != operations.len()
            || ctx.any_by(
                &bores,
                |bore| Ok(bore.radius.to_bits() != radius.to_bits()),
                "NX hole bore radius uniqueness",
            )?
        {
            return Ok(None);
        }
        let mut records_iter = (operations).into_iter();
        while let Some(operation) =
            ctx.next_charged(&mut records_iter, "NX hole treatment labels")?
        {
            insert_hole_output_body(ctx, &mut projected_outputs, &operation, &body)?;
            let Some(diameter) = Length::new(radius * 2.0) else {
                return Ok(None);
            };
            ctx.insert_btree_map(
                &mut diameters,
                ctx.copy_retained_text(&operation, "NX hole diameter label")?,
                diameter,
                "NX hole diameter map",
            )?;
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
    let (operations_by_body, _operations_by_body_storage) = ctx
        .with_scoped_storage("NX operations by body scratch", || {
            hole_operations_by_body(ctx, ir, operations, outputs)
        })?;
    let Some(operations_by_body) = operations_by_body else {
        return Ok(None);
    };
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut counterbores = BTreeMap::new();
    let mut records_iter = (operations_by_body).into_iter();
    while let Some((body, operations)) =
        ctx.next_charged(&mut records_iter, "NX hole body projection groups")?
    {
        let [operation] = operations.as_slice() else {
            // A counterbore pair has no serialized operation-to-pair relation
            // once multiple operations share one result body. Do not assign
            // geometry to history order.
            return Ok(None);
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let (witnesses, _witness_storage) = ctx
            .with_scoped_storage("NX cylinder projection witnesses", || {
                counterbore_cylinders(ctx, ir, &body_faces)
            })?;
        let Some(witnesses) = witnesses else {
            return Ok(None);
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(None);
        };
        insert_hole_output_body(ctx, &mut projected_outputs, operation, &body)?;
        let Some(diameter) = Length::new(witness.bore_radius * 2.0) else {
            return Ok(None);
        };
        ctx.insert_btree_map(
            &mut diameters,
            ctx.copy_retained_text(operation, "NX counterbore diameter map")?,
            diameter,
            "NX counterbore diameter map",
        )?;
        let (Some(diameter), Some(depth)) = (
            cadmpeg_ir::scalar::PositiveLength::new(witness.counterbore_radius * 2.0),
            cadmpeg_ir::scalar::PositiveLength::new(witness.depth),
        ) else {
            return Ok(None);
        };
        ctx.insert_btree_map(
            &mut counterbores,
            ctx.copy_retained_text(operation, "NX counterbore dimension map")?,
            CounterboreDimensions { diameter, depth },
            "NX counterbore dimension map",
        )?;
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
    let (operations_by_body, _operations_by_body_storage) = ctx
        .with_scoped_storage("NX operations by body scratch", || {
            hole_operations_by_body(ctx, ir, operations, outputs)
        })?;
    let Some(operations_by_body) = operations_by_body else {
        return Ok(None);
    };
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut blind_depths = BTreeMap::new();
    let mut records_iter = (operations_by_body).into_iter();
    while let Some((body, operations)) =
        ctx.next_charged(&mut records_iter, "NX hole body projection groups")?
    {
        let [operation] = operations.as_slice() else {
            return Ok(None);
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let (witnesses, _witness_storage) = ctx
            .with_scoped_storage("NX cylinder projection witnesses", || {
                blind_bore_cylinders(ctx, ir, &body_faces)
            })?;
        let Some(witnesses) = witnesses else {
            return Ok(None);
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(None);
        };
        insert_hole_output_body(ctx, &mut projected_outputs, operation, &body)?;
        let Some(diameter) = Length::new(witness.bore_radius * 2.0) else {
            return Ok(None);
        };
        ctx.insert_btree_map(
            &mut diameters,
            ctx.copy_retained_text(operation, "NX blind hole diameter map")?,
            diameter,
            "NX blind hole diameter map",
        )?;
        let Some(depth) = cadmpeg_ir::scalar::NonZeroLength::new(witness.depth) else {
            return Ok(None);
        };
        ctx.insert_btree_map(
            &mut blind_depths,
            ctx.copy_retained_text(operation, "NX blind hole depth map")?,
            depth,
            "NX blind hole depth map",
        )?;
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
    let (operations_by_body, _operations_by_body_storage) = ctx
        .with_scoped_storage("NX operations by body scratch", || {
            hole_operations_by_body(ctx, ir, operations, outputs)
        })?;
    let Some(operations_by_body) = operations_by_body else {
        return Ok(BTreeMap::new());
    };

    let mut placements = BTreeMap::new();
    for (body, operations) in
        ctx.admit_iter(operations_by_body, "NX hole body projection groups")?
    {
        let [operation] = operations.as_slice() else {
            continue;
        };
        let (mut body_placements, _placement_storage) = ctx
            .with_scoped_storage("NX hole operation placement candidate", || {
                hole_axis_placements_for_body(ctx, ir, &body)
            })?;
        if body_placements.len() != 1 {
            continue;
        }
        ctx.insert_btree_map(
            &mut placements,
            ctx.copy_retained_text(operation, "NX hole placement map")?,
            body_placements.remove(0),
            "NX hole placement map",
        )?;
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
    let (operations_by_body, _operations_by_body_storage) = ctx
        .with_scoped_storage("NX operations by body scratch", || {
            hole_operations_by_body(ctx, ir, operations, outputs)
        })?;
    let Some(operations_by_body) = operations_by_body else {
        return Ok(BTreeMap::new());
    };
    let mut placements = BTreeMap::new();
    let mut records_iter = (operations_by_body).into_iter();
    while let Some((body, operations)) =
        ctx.next_charged(&mut records_iter, "NX hole body projection groups")?
    {
        let [operation] = operations.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let (witnesses, _witness_storage) = ctx
            .with_scoped_storage("NX cylinder projection witnesses", || {
                counterbore_cylinders(ctx, ir, &body_faces)
            })?;
        let Some(witnesses) = witnesses else {
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
        ctx.insert_btree_map(
            &mut placements,
            ctx.copy_retained_text(operation, "NX counterbore placement map")?,
            HolePlacement::Axis {
                origin: point,
                axis: direction,
            },
            "NX counterbore placement map",
        )?;
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
    let (operations_by_body, _operations_by_body_storage) = ctx
        .with_scoped_storage("NX operations by body scratch", || {
            hole_operations_by_body(ctx, ir, operations, outputs)
        })?;
    let Some(operations_by_body) = operations_by_body else {
        return Ok(BTreeMap::new());
    };
    let mut placements = BTreeMap::new();
    let mut records_iter = (operations_by_body).into_iter();
    while let Some((body, operations)) =
        ctx.next_charged(&mut records_iter, "NX hole body projection groups")?
    {
        let [operation] = operations.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let (witnesses, _witness_storage) = ctx
            .with_scoped_storage("NX cylinder projection witnesses", || {
                blind_bore_cylinders(ctx, ir, &body_faces)
            })?;
        let Some(witnesses) = witnesses else {
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
        ctx.insert_btree_map(
            &mut placements,
            ctx.copy_retained_text(operation, "NX blind hole placement map")?,
            HolePlacement::Directed {
                position: point,
                direction,
            },
            "NX blind hole placement map",
        )?;
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
    let (bores, _bores_storage) = ctx.with_scoped_storage("NX bores scratch", || {
        cylindrical_face_witnesses(ctx, ir, &body_faces)
    })?;
    let Some(bores) = bores else {
        return Ok(Vec::new());
    };
    let angular_tolerance = ir.tolerances.angular.get();
    let mut placements = Vec::new();
    let mut records_iter = (bores).into_iter();
    while let Some(bore) = ctx.next_charged(&mut records_iter, "NX bore placement witnesses")? {
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
        ctx.reserve_vec(&mut placements, 1, "NX hole axis placements")?;
        placements.push(HolePlacement::Axis { origin, axis });
    }
    ctx.stable_sort_by_key(
        &mut placements,
        hole_placement_key,
        Ord::cmp,
        "sort NX hole axis placements",
    )?;
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
    let mut witness: Option<(Point3, Vector3, f64)> = None;
    let mut records_iter = (&ir.model.coedges).into_iter();
    while let Some(coedge) = ctx.next_charged(&mut records_iter, "NX circular loop coedges")? {
        if !(ctx.equal_bytes(
            coedge.owner_loop.as_str().as_bytes(),
            loop_id.as_str().as_bytes(),
            "NX circular loop geometry equality",
        )?) {
            continue;
        }

        let Some(curve_id) = ctx
            .rposition_by(
                &(ir.model.edges),
                |edge| {
                    Ok(ctx.equal_bytes(
                        edge.id.as_str().as_bytes(),
                        coedge.edge.as_str().as_bytes(),
                        "NX circular loop geometry equality",
                    )?)
                },
                "NX circular loop edge scan",
            )?
            .and_then(|index| (ir.model.edges).get(index))
            .and_then(|edge| edge.curve())
        else {
            return Ok(None);
        };

        let Some(curve) = ctx
            .rposition_by(
                &(ir.model.curves),
                |curve| {
                    Ok(ctx.equal_bytes(
                        curve.id.as_str().as_bytes(),
                        curve_id.as_str().as_bytes(),
                        "NX circular loop geometry equality",
                    )?)
                },
                "NX circular loop curve scan",
            )?
            .and_then(|index| (ir.model.curves).get(index))
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
    let mut first_edges = BTreeSet::new();
    let mut second_edges = BTreeSet::new();
    let mut storage = ctx.reserve_scoped(0, "NX loop edge membership indexes")?;
    for coedge in ctx.admit_iter(&ir.model.coedges, "NX loop edge membership records")? {
        if ctx.equal_bytes(
            coedge.owner_loop.as_str().as_bytes(),
            first.as_str().as_bytes(),
            "NX first loop owner identity",
        )? {
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut first_edges,
                    coedge.edge.as_str(),
                    "NX first loop edge index",
                )
            })?;
        }
        if ctx.equal_bytes(
            coedge.owner_loop.as_str().as_bytes(),
            second.as_str().as_bytes(),
            "NX second loop owner identity",
        )? {
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut second_edges,
                    coedge.edge.as_str(),
                    "NX second loop edge index",
                )
            })?;
        }
    }
    if first_edges.len() != second_edges.len() {
        return Ok(false);
    }
    ctx.all_by(
        &first_edges,
        |edge| ctx.contains_btree_set(&second_edges, edge, "NX second loop edge membership"),
        "NX loop edge set equality",
    )
}

pub(super) fn cylindrical_face_witnesses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
) -> Result<Option<Vec<CylindricalFaceWitness>>, CodecError> {
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut witnesses = Vec::new();
    let mut records_iter = (body_faces).into_iter().copied();
    while let Some(face) = ctx.next_charged(&mut records_iter, "NX cylindrical face scan")? {
        if face.sense != Sense::Reversed || face.loops.len() != 2 {
            continue;
        }
        let Some(surface) = ctx
            .rposition_by(
                &(ir.model.surfaces),
                |surface| {
                    Ok(ctx.equal_bytes(
                        surface.id.as_str().as_bytes(),
                        face.surface.as_str().as_bytes(),
                        "NX cylindrical face witnesses equality",
                    )?)
                },
                "NX cylindrical surface lookup",
            )?
            .and_then(|index| (ir.model.surfaces).get(index))
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
        ctx.charge_collection_items(1, "NX cylindrical face witnesses")?;
        ctx.reserve_capacity(&mut witnesses, 1, "NX cylindrical face witness")?;
        witnesses.push(CylindricalFaceWitness {
            line_origin,
            axis,
            radius,
            stations: [*first, *second],
            loop_ids: [
                first_loop.try_clone_for_decode(ctx, "NX cylindrical first loop identity")?,
                second_loop.try_clone_for_decode(ctx, "NX cylindrical second loop identity")?,
            ],
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
    let mut records_iter = (body_faces).into_iter();
    while let Some(face) = ctx.next_charged(&mut records_iter, "NX annulus face scan")? {
        if face.loops.len() != 2 {
            continue;
        }

        let Some(surface) = ctx
            .rposition_by(
                &(ir.model.surfaces),
                |surface| {
                    Ok(ctx.equal_bytes(
                        surface.id.as_str().as_bytes(),
                        face.surface.as_str().as_bytes(),
                        "NX plane annulus witness equality",
                    )?)
                },
                "NX annulus plane lookup",
            )?
            .and_then(|index| (ir.model.surfaces).get(index))
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
        if boundaries[0].0.total_cmp(&boundaries[1].0).is_gt() {
            boundaries.swap(0, 1);
        }
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
    let (cylinders, _cylinders_storage) = ctx
        .with_scoped_storage("NX cylinders scratch", || {
            cylindrical_face_witnesses(ctx, ir, body_faces)
        })?;
    let Some(cylinders) = cylinders else {
        return Ok(None);
    };
    if cylinders.is_empty() || cylinders.len() % 2 != 0 {
        return Ok(None);
    }
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut candidate_storage = ctx.reserve_scoped(0, "NX counterbore candidate storage")?;
    let mut candidates = candidate_storage.with_storage(|| {
        ctx.collect_indexed_vec(
            cylinders.len(),
            "nx counterbore cylinder candidates",
            |_| Ok(Vec::<(usize, CounterboreCylinderWitness)>::new()),
        )
    })?;
    for (first_index, first) in ctx
        .admit_iter(&cylinders, "NX counterbore pair first cylinders")?
        .enumerate()
    {
        for (relative_index, second) in ctx
            .admit_iter(
                &cylinders[first_index + 1..],
                "NX counterbore pair second cylinders",
            )?
            .enumerate()
        {
            let second_index = first_index + 1 + relative_index;
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
            candidate_storage.with_storage(|| {
                ctx.reserve_capacity(
                    &mut candidates[first_index],
                    1,
                    "nx counterbore candidate pair",
                )
            })?;
            candidate_storage.with_storage(|| {
                ctx.reserve_capacity(
                    &mut candidates[second_index],
                    1,
                    "nx counterbore candidate pair",
                )
            })?;
            candidates[first_index].push((second_index, witness));
            candidates[second_index].push((first_index, witness));
        }
    }
    if ctx.any_by(
        &candidates,
        |candidates| Ok(candidates.len() != 1),
        "NX counterbore candidate uniqueness",
    )? {
        return Ok(None);
    }
    let mut witnesses =
        ctx.collection_vec(cylinders.len() / 2, "nx counterbore cylinder witnesses")?;
    let mut used = candidate_storage.with_storage(|| {
        ctx.alloc_filled(
            cylinders.len(),
            false,
            "nx counterbore cylinder assignments",
        )
    })?;
    let mut records_iter = (0..cylinders.len()).into_iter();
    while let Some(first_index) = ctx.next_charged(
        &mut records_iter,
        "NX counterbore cylinders range traversal",
    )? {
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
    let (cylinders, _cylinders_storage) = ctx
        .with_scoped_storage("NX cylinders scratch", || {
            cylindrical_face_witnesses(ctx, ir, body_faces)
        })?;
    let Some(cylinders) = cylinders else {
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
        if !ctx.any_by(
            &ir.model.coedges,
            |coedge| {
                Ok(ctx.equal_bytes(
                    coedge.owner_loop.as_str().as_bytes(),
                    cylinder_loop.as_str().as_bytes(),
                    "NX blind bore cylinders equality",
                )?)
            },
            "NX blind bore cylinder edge scan",
        )? {
            return Ok(None);
        }
        for face in ctx.admit_iter(body_faces, "NX blind bore cap faces")? {
            let Some(cap_loop) = face_one_loop(face) else {
                continue;
            };
            if !same_loop_edges(ctx, ir, cap_loop, cylinder_loop)? {
                continue;
            }

            let Some(surface) = ctx
                .rposition_by(
                    &(ir.model.surfaces),
                    |surface| {
                        Ok(ctx.equal_bytes(
                            surface.id.as_str().as_bytes(),
                            face.surface.as_str().as_bytes(),
                            "NX blind bore cylinders equality",
                        )?)
                    },
                    "NX blind bore cap surface lookup",
                )?
                .and_then(|index| (ir.model.surfaces).get(index))
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
    let mut witnesses = ctx.collection_vec(1, "NX blind bore witness")?;
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
    let mut related = 0_usize;
    for operation in ctx.admit_iter(operations, "NX hole output operation labels")? {
        if ctx.contains_key_btree_map(
            outputs,
            operation,
            "NX hole operations by body outputs membership",
        )? {
            related = related.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("NX hole output relation count", u64::MAX, u64::MAX)
            })?;
        }
    }
    if related != 0 && related != operations.len() {
        return Ok(None);
    }
    if related == operations.len() {
        let mut operations_by_body = BTreeMap::<BodyId, Vec<String>>::new();
        let mut records_iter = (operations).into_iter();
        while let Some(operation) =
            ctx.next_charged(&mut records_iter, "NX hole output operation labels")?
        {
            let Some([body]) = ctx
                .get_btree_map(
                    outputs,
                    operation,
                    "NX hole operations by body outputs lookup",
                )?
                .map(Vec::as_slice)
            else {
                return Ok(None);
            };
            if !ctx.contains_key_btree_map(
                &operations_by_body,
                body,
                "NX hole operations by body operations by body membership",
            )? {
                let body_key = body.try_clone_for_decode(ctx, "NX hole operation body identity")?;
                ctx.insert_btree_map(
                    &mut operations_by_body,
                    body_key,
                    Vec::new(),
                    "NX hole operation body groups",
                )?;
            }
            let group = ctx
                .get_mut_btree_map(
                    &mut operations_by_body,
                    body,
                    "NX hole operation body group lookup",
                )?
                .ok_or_else(|| ctx.refuse_codec_limit("NX hole operation body groups", 0, 1))?;
            ctx.charge_collection_items(1, "NX hole operations per body")?;
            ctx.reserve_capacity(group, 1, "NX hole operations per body")?;
            group.push(ctx.copy_retained_text(operation, "NX hole operations per body")?);
        }
        return Ok(Some(operations_by_body));
    }

    let mut selected = None;
    let mut records_iter = (&ir.model.bodies).into_iter();
    while let Some(body) = ctx.next_charged(&mut records_iter, "NX connected solid body scan")? {
        if connected_solid_body_exists(ctx, ir, body)? && selected.replace(body).is_some() {
            return Ok(None);
        }
    }
    let Some(body) = selected else {
        return Ok(None);
    };
    let mut group = Vec::new();
    for operation in ctx.admit_iter(operations, "NX hole output operation labels")? {
        ctx.charge_collection_items(1, "NX hole operations per body")?;
        ctx.reserve_capacity(&mut group, 1, "NX hole operations per body")?;
        group.push(ctx.copy_retained_text(operation, "NX hole operations per body")?);
    }
    let mut grouped = BTreeMap::new();
    ctx.insert_btree_map(
        &mut grouped,
        body.id
            .try_clone_for_decode(ctx, "NX hole operation body identity")?,
        group,
        "NX hole operation body groups",
    )?;
    Ok(Some(grouped))
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
    let (unique_templates, _template_storage) = ctx.unique_index(
        templates
            .iter()
            .map(|template| (template.operation_label.as_str(), template)),
        "NX chamfer unique template index",
    )?;
    let mut operations = Vec::new();
    let mut operation_reservation = ctx.reserve_scoped(0, "NX chamfer selected operations")?;
    for template in ctx.admit_iter(templates, "NX chamfer template scan")? {
        if !(template.form == crate::native::features::holes::SimpleHoleForm::Simple
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Through
            && template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
            && template.end_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer)
        {
            continue;
        }
        if ctx
            .get_hash_map(
                &unique_templates,
                template.operation_label.as_str(),
                "NX chamfer unique template lookup",
            )?
            .and_then(Option::as_ref)
            .is_none()
        {
            continue;
        }
        ctx.charge_collection_items(1, "NX chamfer selected operations")?;
        operation_reservation.with_storage(|| {
            ctx.reserve_capacity(&mut operations, 1, "NX chamfer selected operations")
        })?;
        operations.push(operation_reservation.with_storage(|| {
            ctx.copy_retained_text(&template.operation_label, "NX chamfer selected operations")
        })?);
    }
    if operations.is_empty() {
        return Ok(BTreeMap::new());
    }
    ctx.stable_sort_by(
        &mut operations,
        |value| value,
        Ord::cmp,
        "sort NX chamfer selected operations",
    )?;
    let (operations_by_body, _operations_by_body_storage) = ctx
        .with_scoped_storage("NX operations by body scratch", || {
            hole_operations_by_body(ctx, ir, &operations, outputs)
        })?;
    let Some(operations_by_body) = operations_by_body else {
        return Ok(BTreeMap::new());
    };

    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut treatments = BTreeMap::new();
    let mut records_iter = (operations_by_body).into_iter();
    while let Some((body, operations)) =
        ctx.next_charged(&mut records_iter, "NX hole body projection groups")?
    {
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let (bores, _bores_storage) = ctx.with_scoped_storage("NX bores scratch", || {
            cylindrical_face_witnesses(ctx, ir, &body_faces)
        })?;
        let Some(bores) = bores else {
            return Ok(BTreeMap::new());
        };
        let [first_bore, ..] = bores.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let bore_radius = first_bore.radius;
        if bores.len() != operations.len()
            || ctx.any_by(
                &bores,
                |bore| Ok(bore.radius.to_bits() != bore_radius.to_bits()),
                "NX chamfer bore radius uniqueness",
            )?
        {
            return Ok(BTreeMap::new());
        }
        let mut geometry_reservation = ctx.reserve_scoped(0, "NX chamfer cone geometry")?;
        let mut cone_counts = geometry_reservation.with_storage(|| {
            ctx.alloc_filled(bores.len(), 0usize, "nx simple-hole chamfer cone counts")
        })?;
        let mut outer_radii = Vec::new();
        let mut included_angles = Vec::new();
        let mut records_iter = (&body_faces.faces).into_iter();
        while let Some(face) = ctx.next_charged(&mut records_iter, "NX chamfer candidate faces")? {
            if face.sense != Sense::Reversed || face.loops.len() != 2 {
                continue;
            }
            let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))) = ctx
                .rposition_by(
                    &(ir.model.surfaces),
                    |surface| {
                        Ok(ctx.equal_bytes(
                            surface.id.as_str().as_bytes(),
                            face.surface.as_str().as_bytes(),
                            "NX simple hole chamfers equality",
                        )?)
                    },
                    "NX chamfer cone surface scan",
                )?
                .and_then(|index| (ir.model.surfaces).get(index))
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
            let mut matching_bore = None;
            let mut multiple_bores = false;
            let mut candidates = bores.iter().enumerate();
            while let Some((ordinal, bore)) =
                ctx.next_charged(&mut candidates, "NX chamfer bore candidates")?
            {
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
                    break;
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
                let mut records_iter = (&ir.model.coedges).into_iter();
                while let Some(coedge) =
                    ctx.next_charged(&mut records_iter, "NX chamfer coedge scan")?
                {
                    if !(ctx.equal_bytes(
                        coedge.owner_loop.as_str().as_bytes(),
                        loop_id.as_str().as_bytes(),
                        "NX simple hole chamfers equality",
                    )?) {
                        continue;
                    }

                    let Some(curve_id) = ctx
                        .rposition_by(
                            &(ir.model.edges),
                            |edge| {
                                Ok(ctx.equal_bytes(
                                    edge.id.as_str().as_bytes(),
                                    coedge.edge.as_str().as_bytes(),
                                    "NX simple hole chamfers equality",
                                )?)
                            },
                            "NX chamfer edge lookup",
                        )?
                        .and_then(|index| (ir.model.edges).get(index))
                        .and_then(|edge| edge.curve())
                    else {
                        continue;
                    };

                    let Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))) =
                        ctx.rposition_by(
                            &(ir.model.curves),
                            |curve| {
                                Ok(ctx.equal_bytes(
                                    curve.id.as_str().as_bytes(),
                                    curve_id.as_str().as_bytes(),
                                    "NX simple hole chamfers equality",
                                )?)
                            },
                            "NX chamfer curve lookup",
                        )?
                        .and_then(|index| (ir.model.curves).get(index))
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
            geometry_reservation.with_storage(|| {
                ctx.reserve_capacity(&mut outer_radii, 1, "nx chamfer outer radii")
            })?;
            geometry_reservation.with_storage(|| {
                ctx.reserve_capacity(&mut included_angles, 1, "nx chamfer included angles")
            })?;
            outer_radii.push(outer);
            included_angles.push(half_angle * 2.0);
        }
        if ctx.any_by(
            &cone_counts,
            |count| Ok(*count != 2),
            "NX chamfer cone counts",
        )? || outer_radii.len() != bores.len() * 2
            || included_angles.len() != outer_radii.len()
        {
            return Ok(BTreeMap::new());
        }
        ctx.stable_sort_by(
            &mut outer_radii,
            |value| value,
            f64::total_cmp,
            "sort NX chamfer outer radii",
        )?;
        ctx.stable_sort_by(
            &mut included_angles,
            |value| value,
            f64::total_cmp,
            "sort NX chamfer included angles",
        )?;
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
                2.0 * ctx
                    .admit_iter(&outer_radii, "NX chamfer outer radii mean")?
                    .sum::<f64>()
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
                ctx.admit_iter(&included_angles, "NX chamfer included angle mean")?
                    .sum::<f64>()
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
        for operation in ctx.admit_iter(operations, "NX hole treatment labels")? {
            ctx.insert_btree_map(
                &mut treatments,
                ctx.copy_retained_text(&operation, "NX chamfer treatment label")?,
                treatment,
                "NX chamfer treatments",
            )?;
        }
    }
    Ok(treatments)
}

fn unique_simple_hole_template(
    ctx: &DecodeContext<'_>,
    payload_strings: &[&str],
) -> Result<
    Option<(
        crate::native::features::holes::SimpleHoleForm,
        crate::native::features::holes::SimpleHoleExtent,
        crate::native::features::holes::SimpleHoleEndTreatment,
        crate::native::features::holes::SimpleHoleEndTreatment,
    )>,
    CodecError,
> {
    let mut strings = payload_strings.iter().copied();
    let Some(candidate) = ctx.find_by(
        &mut strings,
        |value| Ok(value.starts_with("Hole_")),
        "NX first hole template payload",
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        strings,
        |value| Ok(value.starts_with("Hole_")),
        "NX competing hole template payload",
    )? {
        return Ok(None);
    }
    const MAX_TEMPLATE_LEN: usize = "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer".len();
    if candidate.len() > MAX_TEMPLATE_LEN {
        return Ok(None);
    }
    Ok(crate::native::features::holes::parse_simple_hole_template(
        candidate,
    ))
}

#[cfg(test)]
mod tests;
