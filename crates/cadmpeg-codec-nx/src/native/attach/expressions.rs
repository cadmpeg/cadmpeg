use super::feature_projection::{insert_parameter_property, parameter_consumer_identity};
use super::{
    Angle, AnnotationBuilder, BTreeMap, CadIr, CodecError, DecodeContext, DesignParameter,
    DistinctMembers, Exactness, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    FeatureSourceContent, FeatureTreeNodeRole, IdScope, Length, ParameterId, ParameterValue,
    StreamHandle, TreeChildren,
};
use std::collections::btree_map::Entry;

pub(in crate::native) fn attach_expression_parameters(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    expressions: &[crate::native::om::ParameterFormula],
    declarations: &[crate::native::om::ExpressionDeclaration],
    parameter_uses: &[crate::native::features::FeatureParameterUse],
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "NX expression parameter indexes")?;
    let mut declaration_index = BTreeMap::new();
    for declaration in ctx.admit_iter(declarations, "NX expression declaration traversal")? {
        reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut declaration_index,
                declaration.id.as_str(),
                declaration,
                "NX expression declaration index",
            )
        })?;
    }
    let mut tables = BTreeMap::<&str, Vec<&crate::native::om::ParameterFormula>>::new();
    for expression in ctx.admit_iter(expressions, "NX expression table traversal")? {
        let table = expression.source_table.as_str();

        let table_expressions = reservation
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut tables,
                    table,
                    "NX attach expression parameters tables entry",
                )
            })?
            .or_default();
        ctx.reserve_scoped_vec(
            &mut reservation,
            table_expressions,
            1,
            "NX expression table members",
        )?;
        table_expressions.push(expression);
    }
    let stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    let mut uses_by_expression =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in ctx.admit_iter(parameter_uses, "NX expression use traversal")? {
        let key = parameter_use.expression.as_str();

        let uses = reservation
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut uses_by_expression,
                    key,
                    "NX attach expression parameters uses by expression entry",
                )
            })?
            .or_default();
        ctx.reserve_scoped_vec(&mut reservation, uses, 1, "NX expression use member")?;
        uses.push(parameter_use);
    }
    for (_, uses) in ctx.admit_iter(&mut uses_by_expression, "NX expression use groups")? {
        ctx.stable_sort_by_key(
            uses,
            |value| {
                let record = *value;
                (
                    record.bindings.first().map(|binding| binding.source_offset),
                    record.id.as_str(),
                )
            },
            Ord::cmp,
            "NX expression use sort",
        )?;
    }
    let mut ordered_tables = Vec::new();
    for entry in ctx.admit_iter(tables, "NX expression tables")? {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut ordered_tables,
            1,
            "NX expression ordered tables",
        )?;
        ordered_tables.push(entry);
    }
    for (_, expressions) in ctx.admit_iter(&mut ordered_tables, "NX expression table ordering")? {
        ctx.stable_sort_by_key(
            expressions,
            |value| {
                let record = *value;
                (record.source_offset, record.id.as_str())
            },
            Ord::cmp,
            "NX expression table sort",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut ordered_tables,
        |value| {
            (
                value.1.first().map(|expression| expression.source_offset),
                value.0,
            )
        },
        Ord::cmp,
        "NX ordered table sort",
    )?;
    let base_ordinal = cadmpeg_core::decode::u64_from_index(ir.model.features.len());
    for (table_ordinal, (table, mut expressions)) in ctx
        .admit_iter(ordered_tables, "NX ordered expression tables")?
        .enumerate()
    {
        let ordered_count = order_expression_dependencies(ctx, &mut reservation, &mut expressions)?;
        let (feature_id, _feature_id_storage) =
            ctx.with_scoped_storage("NX expression table feature identity", || {
                match ctx.split_once(
                    table,
                    ":expression-table#",
                    "NX expression table identity parsing",
                )? {
                    None => {
                        let (scope, _scope_storage) =
                            ctx.with_scoped_storage("NX expression feature scope", || {
                                let Some(scope_text) = table.strip_prefix("nx:") else {
                                    return Err(CodecError::malformed(
                                        "NX expression table id is not an NX identity",
                                    ));
                                };
                                ctx.charge_formatted_retained(
                                    format_args!("{scope_text}"),
                                    "NX expression feature scope text",
                                )?;
                                IdScope::of(table).ok_or_else(|| {
                                    CodecError::malformed(
                                        "NX expression table id is not an NX identity",
                                    )
                                })
                            })?;
                        scope.id_charged::<FeatureId>(
                            ctx,
                            &cadmpeg_ir::identity_component!("feature"),
                            cadmpeg_ir::identity_key!("equations"),
                        )
                    }
                    Some((scope, key)) => {
                        let (key, _key_storage) =
                            ctx.with_scoped_storage("NX expression table key", || {
                                let key_text =
                                    ctx.copy_retained_text(key, "NX expression table key text")?;
                                cadmpeg_ir::ids::IdentityKey::try_new(key_text).map_err(|error| {
                                    CodecError::malformed(format_args!(
                                        "NX expression table key is not identity key text: {error}"
                                    ))
                                })
                            })?;
                        let (scope, _scope_storage) =
                            ctx.with_scoped_storage("NX expression feature scope", || {
                                let Some(scope_text) = scope.strip_prefix("nx:") else {
                                    return Err(CodecError::malformed(
                                        "NX expression table id is not an NX identity",
                                    ));
                                };
                                ctx.charge_formatted_retained(
                                    format_args!("{scope_text}"),
                                    "NX expression feature scope text",
                                )?;
                                IdScope::of(scope).ok_or_else(|| {
                                    CodecError::malformed(
                                        "NX expression table id is not an NX identity",
                                    )
                                })
                            })?;
                        scope.id_charged::<FeatureId>(
                            ctx,
                            &cadmpeg_ir::identity_component!("feature"),
                            format_args!("equations-{key}"),
                        )
                    }
                }
            })?;
        let first_offset = ctx
            .admit_iter(&expressions, "NX expression table first offset")?
            .map(|expression| expression.source_offset)
            .min()
            .unwrap_or(0);
        annotations.note(
            ctx,
            &feature_id,
            &stream,
            first_offset,
            Some("hostglobalvariables"),
        )?;
        annotations.exactness(ctx, &feature_id, Exactness::Derived)?;
        let mut source_content = Vec::new();
        for expression in ctx.admit_iter(&expressions, "NX expression parameter traversal")? {
            let Some(parameter) = expression_parameter_id(ctx, &expression.id)? else {
                continue;
            };
            ctx.charge_collection_items(1, "NX expression feature content")?;
            ctx.reserve_capacity(&mut source_content, 1, "NX expression feature content")?;
            source_content.push(FeatureSourceContent::Parameter(parameter));
        }
        let source_content = cadmpeg_ir::features::FeatureContent::new(
            source_content,
            ctx,
            "NX expression feature content validation",
        )?;
        if !source_content.is_empty() {
            annotations
                .derived(ctx, &feature_id, "source_content")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        ctx.charge_collection_items(1, "NX expression feature")?;
        ctx.reserve_capacity(&mut ir.model.features, 1, "NX expression feature")?;
        let ordinal = base_ordinal
            .checked_add(cadmpeg_core::decode::u64_from_index(table_ordinal))
            .ok_or_else(|| ctx.refuse_codec_limit("NX expression feature ordinal", 0, 1))?;
        ir.model.features.push(Feature {
            id: feature_id.try_clone_for_decode(ctx, "NX expression feature identity")?,
            ordinal,
            name: Some(ctx.copy_retained_text("NX expressions", "NX expression feature name")?),
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some(
                ctx.copy_retained_text("hostglobalvariables", "NX expression feature source tag")?,
            ),
            source_text: None,
            source_content,

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: FeatureTreeNodeRole::Equations,
                    children: TreeChildren::default(),
                }),
            ),
            native_ref: None,
        });
        let mut parameter_index_storage =
            ctx.reserve_scoped(0, "NX parameter identity index storage")?;
        let mut parameter_ids =
            BTreeMap::<(&str, &crate::native::om::ExpressionUnit), Vec<ParameterId>>::new();
        for expression in ctx.admit_iter(&expressions, "NX expression parameter traversal")? {
            let Some(id) = parameter_index_storage
                .with_storage(|| expression_parameter_id(ctx, &expression.id))?
            else {
                continue;
            };
            let key = (expression.name.as_str(), &expression.unit);

            let ids = parameter_index_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut parameter_ids,
                        key,
                        "NX attach expression parameters parameter ids entry",
                    )
                })?
                .or_default();
            ctx.reserve_scoped_vec(
                &mut parameter_index_storage,
                ids,
                1,
                "NX parameter identity candidates",
            )?;
            ids.push(id);
        }
        for (ordinal, expression) in ctx
            .admit_iter(&expressions, "NX ordered expression traversal")?
            .copied()
            .enumerate()
        {
            let Some(id) = expression_parameter_id(ctx, &expression.id)? else {
                continue;
            };
            annotations.note(
                ctx,
                id.as_str(),
                &stream,
                expression.source_offset,
                Some("Number"),
            )?;
            annotations
                .derived(ctx, id.as_str(), "owner")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(ctx, id.as_str(), "ordinal")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(ctx, id.as_str(), "value")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(ctx, id.as_str(), "native_ref")
                .map_err(cadmpeg_core::CodecError::from)?;
            let mut dependencies = Vec::new();
            if ordinal < ordered_count {
                for name in
                    crate::native::om::expression_parameter_names(ctx, &expression.expression)
                {
                    let name = name?;
                    let Some([candidate]) = ctx
                        .get_btree_map(
                            &parameter_ids,
                            &(name, &expression.unit),
                            "NX parameter dependency lookup",
                        )?
                        .map(Vec::as_slice)
                    else {
                        continue;
                    };
                    if ctx.any_by(
                        &dependencies,
                        |existing: &ParameterId| {
                            ctx.equal_bytes(
                                existing.as_str().as_bytes(),
                                candidate.as_str().as_bytes(),
                                "NX parameter dependency identity",
                            )
                        },
                        "NX parameter dependency uniqueness",
                    )? {
                        continue;
                    }
                    ctx.charge_collection_items(1, "NX parameter dependencies")?;
                    ctx.reserve_capacity(&mut dependencies, 1, "NX parameter dependencies")?;
                    dependencies.push(
                        candidate.try_clone_for_decode(ctx, "NX expression dependency identity")?,
                    );
                }
            }
            if !dependencies.is_empty() {
                annotations
                    .derived(ctx, id.as_str(), "dependencies")
                    .map_err(cadmpeg_core::CodecError::from)?;
            }
            let value = expression.value.and_then(|value| match &expression.unit {
                crate::native::om::ExpressionUnit::Millimeter => {
                    Some(ParameterValue::Length(Length::from_assigned_real(value)))
                }
                crate::native::om::ExpressionUnit::Inch => {
                    crate::native::om::expression_length_in_millimeters(
                        &expression.unit,
                        value.get(),
                    )
                    .and_then(Length::new)
                    .map(ParameterValue::Length)
                }
                crate::native::om::ExpressionUnit::Degree => {
                    Some(ParameterValue::Angle(Angle::new(value.get().to_radians())?))
                }
                crate::native::om::ExpressionUnit::Native(_) => None,
            });
            let mut properties = BTreeMap::new();
            insert_parameter_property(
                ctx,
                &mut properties,
                format_args!("unit"),
                expression.unit.property_name(ctx)?,
            )?;
            annotations
                .derived(ctx, id.as_str(), "properties")
                .map_err(cadmpeg_core::CodecError::from)?;
            if let Some(declaration) = expression
                .declaration
                .as_deref()
                .map(|id| {
                    ctx.get_btree_map(
                        &declaration_index,
                        id,
                        "NX attach expression parameters declaration index lookup",
                    )
                })
                .transpose()?
                .flatten()
            {
                insert_parameter_property(
                    ctx,
                    &mut properties,
                    format_args!("declaration"),
                    ctx.format_retained(
                        format_args!("{}", declaration.id),
                        "NX expression declaration identity text",
                    )?,
                )?;
                insert_parameter_property(
                    ctx,
                    &mut properties,
                    format_args!("declaration_object_id"),
                    ctx.format_retained(
                        format_args!("{}", declaration.object_id),
                        "NX expression declaration object identity text",
                    )?,
                )?;
                annotations
                    .derived(ctx, id.as_str(), "properties")
                    .map_err(cadmpeg_core::CodecError::from)?;
            }
            if let Some(feature_property_records) = ctx.get_btree_map(
                &uses_by_expression,
                expression.id.as_str(),
                "NX attach expression parameters uses by expression lookup",
            )? {
                for (consumer_ordinal, parameter_use) in ctx
                    .admit_iter(feature_property_records, "NX expression consumer traversal")?
                    .enumerate()
                {
                    insert_parameter_property(
                        ctx,
                        &mut properties,
                        format_args!("consumer.{consumer_ordinal}"),
                        parameter_consumer_identity(ctx, &parameter_use.operation_label)?,
                    )?;
                    insert_parameter_property(
                        ctx,
                        &mut properties,
                        format_args!("parameter_use.{consumer_ordinal}"),
                        ctx.format_retained(
                            format_args!("{}", parameter_use.id),
                            "NX expression parameter-use identity text",
                        )?,
                    )?;
                    annotations
                        .derived(ctx, id.as_str(), "properties")
                        .map_err(cadmpeg_core::CodecError::from)?;
                }
            }
            ctx.charge_collection_items(1, "NX expression parameters")?;
            ctx.reserve_capacity(&mut ir.model.parameters, 1, "NX expression parameters")?;
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit(
                    "NX expression parameter ordinal",
                    0,
                    cadmpeg_core::decode::u64_from_index(ordinal),
                )
            })?;
            ir.model.parameters.push(DesignParameter {
                id,
                owner: Some(
                    feature_id.try_clone_for_decode(ctx, "NX expression feature identity")?,
                ),
                ordinal,
                name: ctx.format_retained(
                    format_args!("{}", expression.name.as_str()),
                    "NX expression parameter name text",
                )?,
                expression: ctx.format_retained(
                    format_args!("{}", expression.expression),
                    "NX expression formula text",
                )?,
                display: None,
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
                properties,
                pmi: None,
                native_ref: Some(ctx.format_retained(
                    format_args!("{}", expression.id),
                    "NX native expression reference text",
                )?),
            });
        }
    }
    Ok(())
}

fn order_expression_dependencies(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    expressions: &mut Vec<&crate::native::om::ParameterFormula>,
) -> Result<usize, CodecError> {
    let count = expressions.len();
    let mut index_storage = ctx.reserve_scoped(0, "NX expression dependency index storage")?;
    let mut dependency_indices = BTreeMap::new();
    for (index, expression) in ctx
        .admit_iter(&*expressions, "NX expression dependency identity index")?
        .enumerate()
    {
        match index_storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut dependency_indices,
                (expression.name.as_str(), &expression.unit),
                "NX order expression dependencies dependency indices entry",
            )
        })? {
            Entry::Vacant(entry) => {
                entry.insert(Some(index));
            }
            Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
    }
    let mut emitted_storage = ctx.reserve_scoped(0, "NX expression emitted flags")?;
    let mut emitted = emitted_storage
        .with_storage(|| ctx.alloc_filled(count, false, "NX expression dependency order"))?;
    let mut order = Vec::new();
    let mut records_iter = 0..count;
    while ctx
        .next_charged(
            &mut records_iter,
            "NX order expression dependencies range traversal",
        )?
        .is_some()
    {
        let mut ready = None;
        let mut records_iter = expressions.iter().enumerate();
        while let Some((index, expression)) =
            ctx.next_charged(&mut records_iter, "NX expression dependency candidate")?
        {
            if emitted[index] {
                continue;
            }
            let mut dependencies_ready = true;
            for name in crate::native::om::expression_parameter_names(ctx, &expression.expression) {
                let name = name?;
                let dependency = ctx
                    .get_btree_map(
                        &dependency_indices,
                        &(name, &expression.unit),
                        "NX expression dependency lookup",
                    )?
                    .and_then(Option::as_ref);
                if dependency.is_some_and(|dependency| !emitted[*dependency]) {
                    dependencies_ready = false;
                    break;
                }
            }
            if dependencies_ready {
                ready = Some(index);
                break;
            }
        }
        let Some(index) = ready else { break };
        emitted[index] = true;
        ctx.reserve_scoped_vec(reservation, &mut order, 1, "NX expression dependency order")?;
        order.push(expressions[index]);
    }
    let ordered_count = order.len();
    for (index, expression) in ctx
        .admit_iter(expressions.as_slice(), "NX unresolved expression traversal")?
        .enumerate()
    {
        if !emitted[index] {
            ctx.reserve_scoped_vec(reservation, &mut order, 1, "NX expression dependency order")?;
            order.push(expression);
        }
    }
    *expressions = order;
    Ok(ordered_count)
}

pub(super) fn attach_block_dimension_parameter_consumers(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    dimensions: &[crate::native::features::FeatureBlockDimensions],
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut records_iter = dimensions.iter();
    while let Some(dimension_set) =
        ctx.next_charged(&mut records_iter, "NX block dimension traversal")?
    {
        let mut reservation = ctx.reserve_scoped(0, "NX block dimension consumers")?;
        let consumer = match ctx.split_once(
            &dimension_set.operation_label,
            "operation-label",
            "NX block consumer label parsing",
        )? {
            Some((prefix, suffix)) => ctx.format_scoped_text(
                &mut reservation,
                format_args!("{prefix}feature{suffix}"),
                "NX block parameter consumer identity text",
            )?,
            None => ctx.format_scoped_text(
                &mut reservation,
                format_args!("{}", dimension_set.operation_label),
                "NX block parameter consumer identity text",
            )?,
        };
        for (ordinal, dimension) in dimension_set.dimensions.iter().enumerate() {
            let (parameter_id, _parameter_id_storage) = ctx
                .with_scoped_storage("NX block dimension parameter identity", || {
                    expression_parameter_id(ctx, &dimension.expression)
                })?;
            let Some(parameter_id) = parameter_id else {
                continue;
            };
            let Some(parameter_index) = ctx.rposition_by(
                &ir.model.parameters,
                |parameter| {
                    ctx.equal_bytes(
                        parameter.id.as_str().as_bytes(),
                        parameter_id.as_str().as_bytes(),
                        "NX attach block dimension parameter consumers equality",
                    )
                },
                "NX block dimension parameter lookup",
            )?
            else {
                continue;
            };
            let parameter = &mut ir.model.parameters[parameter_index];
            insert_parameter_property(
                ctx,
                &mut parameter.properties,
                format_args!("block_dimension.{ordinal}"),
                ctx.format_retained(
                    format_args!("{}", dimension_set.id),
                    "NX block dimension source property value",
                )?,
            )?;
            let has_consumer = ctx.any_by(
                &parameter.properties,
                |(_, value)| {
                    ctx.equal_bytes(
                        value.as_bytes(),
                        consumer.as_bytes(),
                        "NX block consumer identity equality",
                    )
                },
                "NX block parameter property traversal",
            )?;
            if !has_consumer {
                // One more candidate than the map holds entries, so one of
                // them is free; the `else` states that rather than asserting it.
                let mut consumer_ordinal = None;
                let mut records_iter = 0..=parameter.properties.len();
                while let Some(candidate) =
                    ctx.next_charged(&mut records_iter, "NX block dimension consumer ordinal")?
                {
                    let mut key_reservation =
                        ctx.reserve_scoped(0, "NX block dimension consumer key")?;
                    let key = ctx.format_scoped_text(
                        &mut key_reservation,
                        format_args!("consumer.{candidate}"),
                        "NX block consumer property candidate key",
                    )?;
                    if !ctx.contains_key_btree_map(
                        &parameter.properties,
                        key.as_str(),"NX attach block dimension parameter consumers parameter properties membership",
                    )? {
                        consumer_ordinal = Some(candidate);
                        break;
                    }
                }
                let Some(consumer_ordinal) = consumer_ordinal else {
                    return Err(cadmpeg_core::CodecError::malformed(format_args!(
                        "NX parameter properties hold no free consumer ordinal"
                    )));
                };
                insert_parameter_property(
                    ctx,
                    &mut parameter.properties,
                    format_args!("consumer.{consumer_ordinal}"),
                    ctx.format_retained(
                        format_args!("{consumer}"),
                        "NX block consumer property value",
                    )?,
                )?;
            }
            annotations
                .derived(ctx, parameter.id.as_str(), "properties")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    Ok(())
}

pub(super) fn expression_parameter_id(
    ctx: &DecodeContext<'_>,
    expression_id: &str,
) -> Result<Option<ParameterId>, CodecError> {
    let Some((section, key)) = ctx.split_once(
        expression_id,
        ":expression#",
        "NX expression parameter source parsing",
    )?
    else {
        return Ok(None);
    };
    if !section.starts_with("nx:") || key.is_empty() {
        return Ok(None);
    }
    let text = ctx.format_retained(
        format_args!("{section}:parameter#{key}"),
        "NX expression parameter identity",
    )?;
    Ok(ParameterId::mint(text).ok())
}

#[cfg(test)]
mod tests;
