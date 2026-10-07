use super::{
    BTreeMap, BodyId, BodySelection, BodyWriterHistory, CodecError, DecodeContext,
    FeatureDefinition, FeatureId, FeatureOperation,
};

/// Identity namespace used to prove that two Boolean selections are disjoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FeatureBodyIdentity {
    Segment(u32),
    OffsetStore(String),
}

fn offset_store_identity<'a>(
    ctx: &DecodeContext<'_>,
    data_block: &'a str,
) -> Result<Option<&'a str>, CodecError> {
    let Some(data_block) = data_block.strip_prefix("nx:om-data-blocks-") else {
        return Ok(None);
    };
    Ok(ctx.split_once(data_block, ":block#", "NX body selection offset store parsing")?
        .map(|(store, _)| store))
}

pub(super) enum FeatureBodySelection<'ctx> {
    Native(String),
    Local {
        bodies: Vec<String>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
        _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
    },
    Resolved {
        bodies: Vec<BodyId>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
        _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
    },
}

pub(super) fn local_body_selection(
    ctx: &DecodeContext<'_>,
    bodies: &[String],
    native: String,
) -> Result<BodySelection, CodecError> {
    let bodies =
        ctx.try_collect_retained_with(bodies.iter(), "NX local body selection members", |body| {
            ctx.copy_retained_text(body, "NX local body selection identity")
        })?;
    let native_copy = ctx.copy_retained_text(&native, "NX feature projection text")?;
    Ok(BodySelection::local(bodies, native_copy, ctx)?.unwrap_or(BodySelection::Native(native)))
}

impl FeatureBodySelection<'_> {
    pub(super) fn into_selection(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<BodySelection, CodecError> {
        match self {
            Self::Native(native) => Ok(BodySelection::Native(native)),
            Self::Local { bodies, native, .. } => local_body_selection(ctx, &bodies, native),
            Self::Resolved { bodies, native, .. } => {
                if bodies.is_empty() {
                    return Ok(BodySelection::Native(native));
                }
                let bodies = ctx.try_collect_retained_with(
                    bodies.iter(),
                    "NX resolved body selection members",
                    |body| body.try_clone_for_decode(ctx, "NX resolved body selection identity"),
                )?;
                let bodies = match cadmpeg_ir::features::DistinctMembers::try_from(bodies, ctx) {
                    Ok(bodies) => bodies,
                    Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                        return Err(limit.into())
                    }
                    Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => {
                        return Ok(BodySelection::Native(native))
                    }
                };
                Ok(BodySelection::Resolved { bodies, native })
            }
        }
    }

    fn into_native(self) -> BodySelection {
        let (Self::Native(native) | Self::Local { native, .. } | Self::Resolved { native, .. }) =
            self;
        BodySelection::Native(native)
    }
}

/// Resolve a complete object-index selection only when every alias root owns one
/// decoded body image. Retain the complete feature-input-local identities when
/// current topology cannot represent a consumed historical body. An offset-store
/// selection uses the exact data-block identities from its feature-history
/// section. A complete operation-local offset-store map takes precedence over
/// a segment alias with the same integer; mixed namespace coverage remains
/// native.
pub(super) fn feature_body_selection<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> Result<FeatureBodySelection<'ctx>, CodecError> {
    feature_body_selection_with_offset_blocks(
        ctx,
        object_indices,
        body_alias_roots,
        &BTreeMap::new(),
        bodies_by_object_index,
        native,
    )
}

pub(super) fn feature_body_selection_with_offset_blocks<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_body_blocks: &BTreeMap<u32, String>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> Result<FeatureBodySelection<'ctx>, CodecError> {
    let mut roots = Vec::new();
    let mut offset_blocks: Vec<&String> = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX feature body selection")?;
    let mut indices = object_indices.iter();
    while let Some(index) = ctx.next_charged(&mut indices, "NX body selection object indices")? {
        match (
            ctx.get_btree_map(body_alias_roots, index, "NX body selection alias root lookup")?,
            ctx.get_btree_map(offset_store_body_blocks, index, "NX body selection offset block lookup")?,
        ) {
            (_, Some(data_block)) => {
                if !ctx.any_by(&offset_blocks, |block| ctx.equal_bytes(block.as_bytes(), data_block.as_bytes(), "NX body selection offset block equality"), "NX body selection offset block uniqueness")? {
                    ctx.reserve_scoped_vec(&mut reservation, &mut offset_blocks, 1, "NX feature body offset blocks")?;
                    offset_blocks.push(data_block);
                }
            }
            (Some(root), None) => {
                if !ctx.any_by(&roots, |candidate| Ok(candidate == root), "NX feature body root uniqueness")? {
                    ctx.reserve_scoped_vec(&mut reservation, &mut roots, 1, "NX feature body roots")?;
                    roots.push(*root);
                }
            }
            (None, None) => return Ok(FeatureBodySelection::Native(native)),
        }
    }
    if !roots.is_empty() && !offset_blocks.is_empty() {
        return Ok(FeatureBodySelection::Native(native));
    }
    let offset_store = match offset_blocks.first() {
        Some(block) => offset_store_identity(ctx, block)?,
        None => None,
    };
    if !offset_blocks.is_empty()
        && (offset_store.is_none()
            || ctx.any_by(
                &offset_blocks,
                |block| {
                    let candidate = offset_store_identity(ctx, block)?;
                    Ok(!match (candidate, offset_store) {
                        (Some(left), Some(right)) => ctx.equal_bytes(left.as_bytes(), right.as_bytes(), "NX body selection offset store identity")?,
                        (None, None) => true,
                        _ => false,
                    })
                },
                "NX body selection offset store consistency",
            )?)
    {
        return Ok(FeatureBodySelection::Native(native));
    }
    if !offset_blocks.is_empty() {
        let mut bodies = Vec::new();
        let mut identity_keys = Vec::new();
        for block in ctx
            .admit_iter(&offset_blocks, "NX body selection offset blocks")?
            .copied()
        {
            ctx.charge_collection_items(2, "NX feature body offset selection")?;
            reservation.with_storage(|| {
                ctx.reserve_capacity(&mut bodies, 1, "NX feature body offset selection")
            })?;
            reservation.with_storage(|| {
                ctx.reserve_capacity(&mut identity_keys, 1, "NX feature body offset identities")
            })?;
            bodies.push(reservation.with_storage(|| {
                ctx.copy_retained_text(block, "NX feature body offset selection")
            })?);
            identity_keys.push(FeatureBodyIdentity::OffsetStore(reservation.with_storage(
                || ctx.copy_retained_text(block, "NX feature body offset identities"),
            )?));
        }
        return Ok(FeatureBodySelection::Local {
            bodies,
            native,
            identity_keys,
            _reservation: reservation,
        });
    }
    let mut resolved = Vec::new();
    let mut all_resolved = true;
    let mut root_iter = roots.iter();
    while let Some(root) = ctx.next_charged(&mut root_iter, "NX body selection resolved roots")? {
        let Some([body]) = ctx.get_btree_map(bodies_by_object_index, root, "NX body selection resolved body lookup")?.map(Vec::as_slice) else {
            all_resolved = false;
            break;
        };
        if ctx.any_by(&resolved, |candidate: &BodyId| ctx.equal_bytes(candidate.as_str().as_bytes(), body.as_str().as_bytes(), "NX body selection resolved identity equality"), "NX feature body resolved uniqueness")? {
            all_resolved = false;
            break;
        }
        ctx.charge_collection_items(1, "NX feature body resolved candidates")?;
        reservation.with_storage(|| {
            ctx.reserve_capacity(&mut resolved, 1, "NX feature body resolved candidates")
        })?;
        resolved.push(
            reservation
                .with_storage(|| body.try_clone_for_decode(ctx, "NX resolved body identity"))?,
        );
    }
    if all_resolved {
        let mut identity_keys = Vec::new();
        for root in ctx
            .admit_iter(&roots, "NX body selection local roots")?
            .copied()
        {
            ctx.reserve_scoped_vec(
                &mut reservation,
                &mut identity_keys,
                1,
                "NX feature body segment identities",
            )?;
            identity_keys.push(FeatureBodyIdentity::Segment(root));
        }
        return Ok(FeatureBodySelection::Resolved {
            bodies: resolved,
            native,
            identity_keys,
            _reservation: reservation,
        });
    }
    let mut bodies = Vec::new();
    let mut identity_keys = Vec::new();
    for root in ctx
        .admit_iter(&roots, "NX body selection local roots")?
        .copied()
    {
        ctx.charge_collection_items(2, "NX feature body local selection")?;
        reservation.with_storage(|| {
            ctx.reserve_capacity(&mut bodies, 1, "NX feature body local selection")
        })?;
        reservation.with_storage(|| {
            ctx.reserve_capacity(&mut identity_keys, 1, "NX feature body local identities")
        })?;
        bodies.push(ctx.format_scoped_text(
            &mut reservation,
            format_args!("nx:om-body-object#{root}"),
            "NX body selection text",
        )?);
        identity_keys.push(FeatureBodyIdentity::Segment(root));
    }
    Ok(FeatureBodySelection::Local {
        bodies,
        native,
        identity_keys,
        _reservation: reservation,
    })
}

/// Resolve one complete body set when possible. Otherwise retain every exact
/// input-local object identity. A body set needs no cross-role disjointness
/// proof, so an identity outside the segment alias table remains its own root.
pub(super) fn feature_body_set_selection(
    ctx: &DecodeContext<'_>,
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> Result<BodySelection, CodecError> {
    let mut roots = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX feature body set")?;
    for index in ctx.admit_iter(object_indices, "NX body selection object indices")? {
        let root = ctx.get_btree_map(body_alias_roots, index, "NX body set alias root lookup")?.copied().unwrap_or(*index);
        if !ctx.any_by(&roots, |candidate| Ok(*candidate == root), "NX feature body set roots")? {
            ctx.reserve_scoped_vec(&mut reservation, &mut roots, 1, "NX feature body set roots")?;
            roots.push(root);
        }
    }
    let mut resolved = Vec::new();
    let mut all_resolved = true;
    let mut root_iter = roots.iter();
    while let Some(root) = ctx.next_charged(&mut root_iter, "NX body selection resolved roots")? {
        let Some([body]) = ctx.get_btree_map(bodies_by_object_index, root, "NX body selection resolved body lookup")?.map(Vec::as_slice) else {
            all_resolved = false;
            break;
        };
        if ctx.any_by(&resolved, |candidate: &BodyId| ctx.equal_bytes(candidate.as_str().as_bytes(), body.as_str().as_bytes(), "NX body selection resolved identity equality"), "NX feature body set uniqueness")? {
            all_resolved = false;
            break;
        }
        ctx.charge_collection_items(1, "NX feature body set resolved candidates")?;
        reservation.with_storage(|| {
            ctx.reserve_capacity(&mut resolved, 1, "NX feature body set resolved candidates")
        })?;
        resolved.push(
            reservation
                .with_storage(|| body.try_clone_for_decode(ctx, "NX resolved body identity"))?,
        );
    }
    if all_resolved && !resolved.is_empty() {
        return FeatureBodySelection::Resolved {
            bodies: resolved,
            native,
            identity_keys: Vec::new(),
            _reservation: reservation,
        }
        .into_selection(ctx);
    }
    let mut bodies = Vec::new();
    for root in ctx
        .admit_iter(&roots, "NX body selection local roots")?
        .copied()
    {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut bodies,
            1,
            "NX feature body set local selection",
        )?;
        bodies.push(ctx.format_scoped_text(
            &mut reservation,
            format_args!("nx:om-body-object#{root}"),
            "NX body selection text",
        )?);
    }
    FeatureBodySelection::Local {
        bodies,
        native,
        identity_keys: Vec::new(),
        _reservation: reservation,
    }
    .into_selection(ctx)
}

pub(super) fn atomic_disjoint_body_selections(
    ctx: &DecodeContext<'_>,
    left: FeatureBodySelection<'_>,
    right: FeatureBodySelection<'_>,
) -> Result<(BodySelection, BodySelection), CodecError> {
    let complete = match (&left, &right) {
        (
            FeatureBodySelection::Local {
                identity_keys: left,
                ..
            }
            | FeatureBodySelection::Resolved {
                identity_keys: left,
                ..
            },
            FeatureBodySelection::Local {
                identity_keys: right,
                ..
            }
            | FeatureBodySelection::Resolved {
                identity_keys: right,
                ..
            },
        ) => {
            let same_namespace = match left.first().zip(right.first()) {
                None => true,
                Some((left, right)) => match (left, right) {
                    (FeatureBodyIdentity::Segment(_), FeatureBodyIdentity::Segment(_)) => true,
                    (
                        FeatureBodyIdentity::OffsetStore(left),
                        FeatureBodyIdentity::OffsetStore(right),
                    ) => match (offset_store_identity(ctx, left)?, offset_store_identity(ctx, right)?) {
                        (Some(left), Some(right)) => ctx.equal_bytes(left.as_bytes(), right.as_bytes(), "NX atomic disjoint body selections equality")?,
                        (None, None) => true,
                        _ => false,
                    },
                    _ => false,
                },
            };
            same_namespace
                && !ctx.any_by(left, |key| ctx.any_by(right, |candidate| match (key, candidate) {
                    (FeatureBodyIdentity::Segment(left), FeatureBodyIdentity::Segment(right)) => Ok(left == right),
                    (FeatureBodyIdentity::OffsetStore(left), FeatureBodyIdentity::OffsetStore(right)) => ctx.equal_bytes(left.as_bytes(), right.as_bytes(), "NX body selection disjoint identity equality"),
                    _ => Ok(false),
                }, "NX body selection right disjointness"), "NX body selection disjointness")?
        }
        _ => false,
    };
    if complete {
        Ok((left.into_selection(ctx)?, right.into_selection(ctx)?))
    } else {
        Ok((left.into_native(), right.into_native()))
    }
}

/// Resolve one Boolean participant through the namespace selected by the
/// complete Boolean definition. Native integer identity is used only when the
/// definition did not establish one exact offset-store selection.
pub(super) fn boolean_participant_writer<'a>(
    ctx: &DecodeContext<'_>,
    selection: &BodySelection,
    object_index: u32,
    offset_store_body_blocks: Option<&BTreeMap<u32, String>>,
    body_alias_roots: &BTreeMap<u32, u32>,
    history: &'a BodyWriterHistory,
) -> Result<Option<&'a FeatureId>, CodecError> {
    let offset_store_selection = matches!(
        selection,
        BodySelection::Local { bodies, .. }
            if !bodies.is_empty()
                && ctx.all_by(bodies.as_slice(), |body| Ok(offset_store_identity(ctx, body)?.is_some()), "NX Boolean participant local bodies")?
    );
    if offset_store_selection {
        let block = match offset_store_body_blocks {
            Some(blocks) => ctx.get_btree_map(blocks, &object_index, "NX Boolean participant offset block lookup")?,
            None => None,
        };
        return match block {
            Some(data_block) => history.offset_store_writer(ctx, data_block),
            None => Ok(None),
        };
    }
    history.native_writer(
        ctx,
        ctx.get_btree_map(body_alias_roots, &object_index, "NX Boolean participant alias root lookup")?
            .copied()
            .unwrap_or(object_index),
    )
}

/// Register a Boolean's target in the namespace established by its complete
/// target selection. An offset-store target must not create a native writer
/// for the same integer object index.
pub(super) fn boolean_target_writer<'a>(
    ctx: &DecodeContext<'_>,
    definition: &'a FeatureDefinition,
    native_body: u32,
) -> Result<(Option<u32>, Option<&'a str>), CodecError> {
    if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = definition {
        if let BodySelection::Local { bodies, .. } = operands.target() {
            if let [body] = bodies.as_slice() {
                if offset_store_identity(ctx, body)?.is_some() {
                    return Ok((None, Some(body.as_str())));
                }
            }
        }
    }
    Ok((Some(native_body), None))
}

pub(super) fn boolean_target_output(definition: Option<&FeatureDefinition>) -> Option<&BodyId> {
    let Some(FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })) = definition
    else {
        return None;
    };
    let BodySelection::Resolved { bodies, .. } = operands.target() else {
        return None;
    };
    bodies.first()
}
