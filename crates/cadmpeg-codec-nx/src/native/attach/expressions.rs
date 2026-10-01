use super::feature_projection::{insert_parameter_property, parameter_consumer_identity};
use super::{
    Angle, AnnotationBuilder, BTreeMap, CadIr, CodecError, DecodeContext, DesignParameter,
    DistinctMembers, Exactness, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    FeatureSourceContent, FeatureTreeNodeRole, IdScope, Length, ParameterId, ParameterValue,
    StreamHandle, TreeChildren,
};

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
    for declaration in declarations {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(declaration_index.len()),
            "NX expression declaration lookup",
        )?;
        if !declaration_index.contains_key(declaration.id.as_str()) {
            ctx.charge_collection_items(1, "NX expression declaration index")?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<(&str, &crate::native::om::ExpressionDeclaration)>() * 4,
            ))?;
        }
        declaration_index.insert(declaration.id.as_str(), declaration);
    }
    let mut tables = BTreeMap::<&str, Vec<&crate::native::om::ParameterFormula>>::new();
    for expression in expressions {
        let table = expression.source_table.as_str();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(tables.len()),
            "NX expression table lookup",
        )?;
        if !tables.contains_key(table) {
            ctx.charge_collection_items(1, "NX expression table index")?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<(&str, Vec<&crate::native::om::ParameterFormula>)>() * 4,
            ))?;
        }
        let table_expressions = tables.entry(table).or_default();
        ctx.reserve_scoped_vec(
            &mut reservation,
            table_expressions,
            1,
            "NX expression table members",
        )?;
        table_expressions.push(expression);
    }
    let stream = StreamHandle::new_for_decode(ctx, cadmpeg_ir::stream_name!("nx:container"), "allocate annotation stream handle")?;
    let mut uses_by_expression =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        let key = parameter_use.expression.as_str();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(uses_by_expression.len()),
            "NX expression uses lookup",
        )?;
        if !uses_by_expression.contains_key(key) {
            ctx.charge_collection_items(1, "NX expression uses index")?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<(&str, Vec<&crate::native::features::FeatureParameterUse>)>()
                    * 4,
            ))?;
        }
        let uses = uses_by_expression.entry(key).or_default();
        ctx.reserve_scoped_vec(&mut reservation, uses, 1, "NX expression use member")?;
        uses.push(parameter_use);
    }
    for uses in uses_by_expression.values_mut() {
        let count = uses.len();
        let passes = usize::try_from(usize::BITS - count.leading_zeros()).map_err(|_| {
            ctx.refuse_codec_limit(
                "NX expression use sort",
                0,
                cadmpeg_core::decode::u64_from_index(count),
            )
        })?;
        let work = count.checked_mul(passes).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX expression use sort",
                0,
                cadmpeg_core::decode::u64_from_index(count),
            )
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "NX expression use sort",
        )?;
        uses.sort_by(|first, second| {
            first
                .bindings
                .first()
                .map(|binding| binding.source_offset)
                .cmp(&second.bindings.first().map(|binding| binding.source_offset))
                .then_with(|| first.id.cmp(&second.id))
        });
    }
    let mut ordered_tables = Vec::new();
    for entry in tables {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut ordered_tables,
            1,
            "NX expression ordered tables",
        )?;
        ordered_tables.push(entry);
    }
    for (_, expressions) in &mut ordered_tables {
        let count = expressions.len();
        let passes = usize::try_from(usize::BITS - count.leading_zeros()).map_err(|_| {
            ctx.refuse_codec_limit(
                "NX expression table sort",
                0,
                cadmpeg_core::decode::u64_from_index(count),
            )
        })?;
        let work = count.checked_mul(passes).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX expression table sort",
                0,
                cadmpeg_core::decode::u64_from_index(count),
            )
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "NX expression table sort",
        )?;
        expressions.sort_by(|first, second| {
            first
                .source_offset
                .cmp(&second.source_offset)
                .then_with(|| first.id.cmp(&second.id))
        });
    }
    let count = ordered_tables.len();
    let passes = usize::try_from(usize::BITS - count.leading_zeros()).map_err(|_| {
        ctx.refuse_codec_limit(
            "NX ordered table sort",
            0,
            cadmpeg_core::decode::u64_from_index(count),
        )
    })?;
    let work = count.checked_mul(passes).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX ordered table sort",
            0,
            cadmpeg_core::decode::u64_from_index(count),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "NX ordered table sort",
    )?;
    ordered_tables.sort_by(|(first_table, first), (second_table, second)| {
        first
            .first()
            .map(|expression| expression.source_offset)
            .cmp(&second.first().map(|expression| expression.source_offset))
            .then_with(|| first_table.cmp(second_table))
    });
    let base_ordinal = cadmpeg_core::decode::u64_from_index(ir.model.features.len());
    for (table_ordinal, (table, mut expressions)) in ordered_tables.into_iter().enumerate() {
        let ordered_count = order_expression_dependencies(ctx, &mut reservation, &mut expressions)?;
        let feature_id_bytes = std::mem::size_of::<FeatureId>()
            .checked_add(table.len())
            .and_then(|bytes| bytes.checked_add(32))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX expression feature identity",
                    0,
                    cadmpeg_core::decode::u64_from_index(table.len()),
                )
            })?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(feature_id_bytes))?;
        let feature_id: FeatureId = match table.split_once(":expression-table#") {
            None => IdScope::of(table)
                .map(|scope| {
                    scope.id(
                        &cadmpeg_ir::identity_component!("feature"),
                        cadmpeg_ir::identity_key!("equations"),
                    )
                })
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "NX expression table id is not an NX identity"
                    ))
                })?,
            Some((scope, key)) => {
                let key = cadmpeg_ir::ids::IdentityKey::try_new(key).map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "NX expression table key is not identity key text: {error}"
                    ))
                })?;
                IdScope::of(scope)
                    .map(|scope| {
                        scope.id(
                            &cadmpeg_ir::identity_component!("feature"),
                            cadmpeg_ir::identity_key!("equations-").then(key),
                        )
                    })
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(format_args!(
                            "NX expression table id is not an NX identity"
                        ))
                    })?
            }
        };
        let first_offset = expressions
            .iter()
            .map(|expression| expression.source_offset)
            .min()
            .unwrap_or(0);
        annotations
            .note_for_decode(ctx, &feature_id, &stream, first_offset, Some("hostglobalvariables"))?;
        annotations.exactness_for_decode(ctx, &feature_id, Exactness::Derived)?;
        let mut source_content = Vec::new();
        for expression in &expressions {
            let bytes = std::mem::size_of::<FeatureSourceContent>()
                .checked_add(expression.id.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX expression feature content",
                        0,
                        cadmpeg_core::decode::u64_from_index(expression.id.len()),
                    )
                })?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
            let Some(parameter) = expression_parameter_id(&expression.id) else {
                continue;
            };
            ctx.charge_collection_items(1, "NX expression feature content")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(bytes),
                "NX expression feature content",
            )?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut source_content,
                1,
                "NX expression feature content",
            )?;
            source_content.push(FeatureSourceContent::Parameter(parameter));
        }
        let source_content = cadmpeg_ir::features::FeatureContent::try_from_for_decode(source_content, ctx, "NX expression feature content validation")?;
        if !source_content.is_empty() {
            annotations
                .derived_for_decode(ctx, &feature_id, "source_content").map_err(cadmpeg_core::CodecError::from)?;
        }
        let feature_bytes = std::mem::size_of::<Feature>();
        ctx.charge_collection_items(1, "NX expression feature")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(feature_bytes),
            "NX expression feature",
        )?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut ir.model.features,
            1,
            "NX expression feature",
        )?;
        let ordinal = base_ordinal
            .checked_add(cadmpeg_core::decode::u64_from_index(table_ordinal))
            .ok_or_else(|| ctx.refuse_codec_limit("NX expression feature ordinal", 0, 1))?;
        ir.model.features.push(Feature {
            id: feature_id.try_clone_for_decode(ctx, "NX expression feature identity")?,
            ordinal,
            name: Some("NX expressions".to_string()),
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hostglobalvariables".to_string()),
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
        let mut parameter_ids =
            BTreeMap::<(&str, &crate::native::om::ExpressionUnit), Vec<ParameterId>>::new();
        for expression in &expressions {
            let id_bytes = std::mem::size_of::<ParameterId>()
                .checked_add(expression.id.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX parameter lookup identity",
                        0,
                        cadmpeg_core::decode::u64_from_index(expression.id.len()),
                    )
                })?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(id_bytes))?;
            let Some(id) = expression_parameter_id(&expression.id) else {
                continue;
            };
            let key = (expression.name.as_str(), &expression.unit);
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(parameter_ids.len()),
                "NX parameter identity lookup",
            )?;
            if !parameter_ids.contains_key(&key) {
                ctx.charge_collection_items(1, "NX parameter identity index")?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<(
                        (&str, &crate::native::om::ExpressionUnit),
                        Vec<ParameterId>,
                    )>() * 4,
                ))?;
            }
            let ids = parameter_ids.entry(key).or_default();
            ctx.reserve_scoped_vec(&mut reservation, ids, 1, "NX parameter identity candidates")?;
            ids.push(id);
        }
        for (ordinal, expression) in expressions.into_iter().enumerate() {
            let id_bytes = std::mem::size_of::<ParameterId>()
                .checked_add(expression.id.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX expression parameter identity",
                        0,
                        cadmpeg_core::decode::u64_from_index(expression.id.len()),
                    )
                })?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(id_bytes))?;
            let Some(id) = expression_parameter_id(&expression.id) else {
                continue;
            };
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(id_bytes),
                "NX expression parameter identity",
            )?;
            annotations
                .note_for_decode(ctx, id.as_str(), &stream, expression.source_offset, Some("Number"))?;
            annotations
                .derived_for_decode(ctx, id.as_str(), "owner").map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived_for_decode(ctx, id.as_str(), "ordinal").map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived_for_decode(ctx, id.as_str(), "value").map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived_for_decode(ctx, id.as_str(), "native_ref").map_err(cadmpeg_core::CodecError::from)?;
            let mut dependencies = Vec::new();
            if ordinal < ordered_count {
                for name in crate::native::om::expression_parameter_names(&expression.expression) {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(parameter_ids.len()),
                        "NX parameter dependency lookup",
                    )?;
                    let Some([candidate]) = parameter_ids
                        .get(&(name, &expression.unit))
                        .map(Vec::as_slice)
                    else {
                        continue;
                    };
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(dependencies.len()),
                        "NX parameter dependency uniqueness",
                    )?;
                    if dependencies.contains(candidate) {
                        continue;
                    }
                    let bytes = std::mem::size_of::<ParameterId>();
                    ctx.charge_collection_items(1, "NX parameter dependencies")?;
                    ctx.charge_retained(
                        cadmpeg_core::decode::u64_from_index(bytes),
                        "NX parameter dependency",
                    )?;
                    cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                        &mut dependencies,
                        1,
                        "NX parameter dependencies",
                    )?;
                    dependencies.push(candidate.try_clone_for_decode(ctx, "NX expression dependency identity")?);
                }
            }
            if !dependencies.is_empty() {
                annotations
                    .derived_for_decode(ctx, id.as_str(), "dependencies").map_err(cadmpeg_core::CodecError::from)?;
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
                .derived_for_decode(ctx, id.as_str(), "properties").map_err(cadmpeg_core::CodecError::from)?;
            if let Some(declaration) = expression
                .declaration
                .as_deref()
                .and_then(|id| declaration_index.get(id))
            {
                insert_parameter_property(
                    ctx,
                    &mut properties,
                    format_args!("declaration"),
                    ctx.format_retained(
                        format_args!("{}", declaration.id),
                        "NX feature projection text",
                    )?,
                )?;
                insert_parameter_property(
                    ctx,
                    &mut properties,
                    format_args!("declaration_object_id"),
                    ctx.format_retained(
                        format_args!("{}", declaration.object_id),
                        "NX feature projection text",
                    )?,
                )?;
                annotations
                    .derived_for_decode(ctx, id.as_str(), "properties").map_err(cadmpeg_core::CodecError::from)?;
            }
            for (consumer_ordinal, parameter_use) in uses_by_expression
                .get(expression.id.as_str())
                .into_iter()
                .flatten()
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
                        "NX feature projection text",
                    )?,
                )?;
                annotations
                    .derived_for_decode(ctx, id.as_str(), "properties").map_err(cadmpeg_core::CodecError::from)?;
            }
            let bytes = std::mem::size_of::<DesignParameter>();
            ctx.charge_collection_items(1, "NX expression parameters")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(bytes),
                "NX expression parameter",
            )?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut ir.model.parameters,
                1,
                "NX expression parameters",
            )?;
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit(
                    "NX expression parameter ordinal",
                    0,
                    cadmpeg_core::decode::u64_from_index(ordinal),
                )
            })?;
            ir.model.parameters.push(DesignParameter {
                id,
                owner: Some(feature_id.try_clone_for_decode(ctx, "NX expression feature identity")?),
                ordinal,
                name: ctx.format_retained(
                    format_args!("{}", expression.name.as_str()),
                    "NX feature projection text",
                )?,
                expression: ctx.format_retained(
                    format_args!("{}", expression.expression),
                    "NX feature projection text",
                )?,
                display: None,
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::try_from_for_decode(dependencies, ctx).map_err(cadmpeg_core::CodecError::from)?,
                properties,
                pmi: None,
                native_ref: Some(ctx.format_retained(
                    format_args!("{}", expression.id),
                    "NX feature projection text",
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
    let emitted_bytes = count
        .checked_mul(std::mem::size_of::<bool>())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX expression dependency order",
                0,
                cadmpeg_core::decode::u64_from_index(count),
            )
        })?;
    reservation.grow(cadmpeg_core::decode::u64_from_index(emitted_bytes))?;
    let mut emitted = ctx.alloc_filled(count, false, "NX expression dependency order")?;
    let mut order = Vec::new();
    for _ in 0..count {
        let mut ready = None;
        for (index, expression) in expressions.iter().enumerate() {
            ctx.charge_work(1, "NX expression dependency candidate")?;
            if emitted[index] {
                continue;
            }
            let mut dependencies_ready = true;
            for name in crate::native::om::expression_parameter_names(&expression.expression) {
                let mut dependency = None;
                let mut ambiguous = false;
                for (candidate_index, candidate) in expressions.iter().enumerate() {
                    ctx.charge_work(1, "NX expression dependency lookup")?;
                    if candidate.name.as_str() == name
                        && candidate.unit == expression.unit
                        && dependency.replace(candidate_index).is_some()
                    {
                        ambiguous = true;
                        break;
                    }
                }
                if !ambiguous && dependency.is_some_and(|dependency| !emitted[dependency]) {
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
    for (index, expression) in expressions.iter().enumerate() {
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
    for dimension_set in dimensions {
        let mut reservation = ctx.reserve_scoped(0, "NX block dimension consumers")?;
        let consumer = match dimension_set.operation_label.split_once("operation-label") {
            Some((prefix, suffix)) => ctx.format_scoped_text(
                &mut reservation,
                format_args!("{prefix}feature{suffix}"),
                "NX body selection text",
            )?,
            None => ctx.format_scoped_text(
                &mut reservation,
                format_args!("{}", dimension_set.operation_label),
                "NX body selection text",
            )?,
        };
        for (ordinal, dimension) in dimension_set.dimensions.iter().enumerate() {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(ir.model.parameters.len()),
                "NX block dimension parameter lookup",
            )?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(
                dimension.expression.len(),
            ))?;
            let Some(parameter_id) = expression_parameter_id(&dimension.expression) else {
                continue;
            };
            let Some(parameter) = ir
                .model
                .parameters
                .iter_mut()
                .rev()
                .find(|parameter| parameter.id == parameter_id)
            else {
                continue;
            };
            insert_parameter_property(
                ctx,
                &mut parameter.properties,
                format_args!("block_dimension.{ordinal}"),
                ctx.format_retained(
                    format_args!("{}", dimension_set.id),
                    "NX feature projection text",
                )?,
            )?;
            if !parameter
                .properties
                .values()
                .any(|value| value == &consumer)
            {
                // One more candidate than the map holds entries, so one of
                // them is free; the `else` states that rather than asserting it.
                let mut consumer_ordinal = None;
                for candidate in 0..=parameter.properties.len() {
                    ctx.charge_work(1, "NX block dimension consumer ordinal")?;
                    let mut key_reservation =
                        ctx.reserve_scoped(0, "NX block dimension consumer key")?;
                    let key = ctx.format_scoped_text(
                        &mut key_reservation,
                        format_args!("consumer.{candidate}"),
                        "NX body selection text",
                    )?;
                    if !parameter.properties.contains_key(key.as_str()) {
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
                        "NX feature projection text",
                    )?,
                )?;
            }
            annotations
                .derived_for_decode(ctx, parameter.id.as_str(), "properties").map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    Ok(())
}

pub(super) fn expression_parameter_id(expression_id: &str) -> Option<ParameterId> {
    let (section, key) = expression_id.split_once(":expression#")?;
    IdScope::of(section)?.try_id(&cadmpeg_ir::identity_component!("parameter"), key)
}
