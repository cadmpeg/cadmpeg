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
    let mut declaration_index = BTreeMap::new();
    for declaration in declarations {
        declaration_index.insert(declaration.id.as_str(), declaration);
    }
    let mut tables = BTreeMap::<&str, Vec<&crate::native::om::ParameterFormula>>::new();
    for expression in expressions {
        let table = expression.source_table.as_str();

        let table_expressions = tables.entry(table).or_default();
        table_expressions.push(expression);
    }
    let stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    let mut uses_by_expression =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        let key = parameter_use.expression.as_str();

        let uses = uses_by_expression.entry(key).or_default();
        uses.push(parameter_use);
    }
    for uses in uses_by_expression.values_mut() {
        uses.sort_by(|first, second| {
            first
                .bindings
                .first()
                .map(|binding| binding.source_offset)
                .cmp(&second.bindings.first().map(|binding| binding.source_offset))
                .then_with(|| first.id.cmp(&second.id))
        });
    }
    let mut ordered_tables = tables.into_iter().collect::<Vec<_>>();
    for (_, expressions) in &mut ordered_tables {
        expressions.sort_by(|first, second| {
            first
                .source_offset
                .cmp(&second.source_offset)
                .then_with(|| first.id.cmp(&second.id))
        });
    }
    ordered_tables.sort_by(|(first_table, first), (second_table, second)| {
        first
            .first()
            .map(|expression| expression.source_offset)
            .cmp(&second.first().map(|expression| expression.source_offset))
            .then_with(|| first_table.cmp(second_table))
    });
    let base_ordinal = cadmpeg_core::decode::u64_from_index(ir.model.features.len());
    for (table_ordinal, (table, mut expressions)) in ordered_tables.into_iter().enumerate() {
        let ordered_count = order_expression_dependencies(ctx, &mut expressions)?;
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
        annotations.note(
            ctx,
            &feature_id,
            &stream,
            first_offset,
            Some("hostglobalvariables"),
        )?;
        annotations.exactness(ctx, &feature_id, Exactness::Derived)?;
        let mut source_content = Vec::new();
        for expression in &expressions {
            let Some(parameter) = expression_parameter_id(&expression.id) else {
                continue;
            };

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
        let ordinal = base_ordinal + cadmpeg_core::decode::u64_from_index(table_ordinal);
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
            let Some(id) = expression_parameter_id(&expression.id) else {
                continue;
            };
            let key = (expression.name.as_str(), &expression.unit);

            let ids = parameter_ids.entry(key).or_default();
            ids.push(id);
        }
        for (ordinal, expression) in expressions.into_iter().enumerate() {
            let Some(id) = expression_parameter_id(&expression.id) else {
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
                for name in crate::native::om::expression_parameter_names(&expression.expression) {
                    let Some([candidate]) = parameter_ids
                        .get(&(name, &expression.unit))
                        .map(Vec::as_slice)
                    else {
                        continue;
                    };

                    if dependencies.contains(candidate) {
                        continue;
                    }
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
                &mut properties,
                format_args!("unit"),
                expression.unit.property_name(),
            )?;
            annotations
                .derived(ctx, id.as_str(), "properties")
                .map_err(cadmpeg_core::CodecError::from)?;
            if let Some(declaration) = expression
                .declaration
                .as_deref()
                .and_then(|id| declaration_index.get(id))
            {
                insert_parameter_property(
                    &mut properties,
                    format_args!("declaration"),
                    declaration.id.clone(),
                )?;
                insert_parameter_property(
                    &mut properties,
                    format_args!("declaration_object_id"),
                    declaration.object_id.to_string(),
                )?;
                annotations
                    .derived(ctx, id.as_str(), "properties")
                    .map_err(cadmpeg_core::CodecError::from)?;
            }
            for (consumer_ordinal, parameter_use) in uses_by_expression
                .get(expression.id.as_str())
                .into_iter()
                .flatten()
                .enumerate()
            {
                insert_parameter_property(
                    &mut properties,
                    format_args!("consumer.{consumer_ordinal}"),
                    parameter_consumer_identity(&parameter_use.operation_label),
                )?;
                insert_parameter_property(
                    &mut properties,
                    format_args!("parameter_use.{consumer_ordinal}"),
                    parameter_use.id.clone(),
                )?;
                annotations
                    .derived(ctx, id.as_str(), "properties")
                    .map_err(cadmpeg_core::CodecError::from)?;
            }
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::malformed(format_args!(
                    "NX expression parameter ordinal {ordinal} exceeds u32"
                ))
            })?;
            ir.model.parameters.push(DesignParameter {
                id,
                owner: Some(
                    feature_id.try_clone_for_decode(ctx, "NX expression feature identity")?,
                ),
                ordinal,
                name: expression.name.as_str().to_owned(),
                expression: expression.expression.clone(),
                display: None,
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
                properties,
                pmi: None,
                native_ref: Some(expression.id.clone()),
            });
        }
    }
    Ok(())
}

fn order_expression_dependencies(
    ctx: &DecodeContext<'_>,
    expressions: &mut Vec<&crate::native::om::ParameterFormula>,
) -> Result<usize, CodecError> {
    let count = expressions.len();
    let mut emitted = ctx.alloc_filled(count, false, "NX expression dependency order")?;
    let mut order = Vec::new();
    for _ in 0..count {
        let mut ready = None;
        for (index, expression) in expressions.iter().enumerate() {
            if emitted[index] {
                continue;
            }
            let mut dependencies_ready = true;
            for name in crate::native::om::expression_parameter_names(&expression.expression) {
                let mut dependency = None;
                let mut ambiguous = false;
                for (candidate_index, candidate) in expressions.iter().enumerate() {
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
        order.push(expressions[index]);
    }
    let ordered_count = order.len();
    for (index, expression) in expressions.iter().enumerate() {
        if !emitted[index] {
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
        let consumer = match dimension_set.operation_label.split_once("operation-label") {
            Some((prefix, suffix)) => format!("{prefix}feature{suffix}"),
            None => dimension_set.operation_label.clone(),
        };
        for (ordinal, dimension) in dimension_set.dimensions.iter().enumerate() {
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
                &mut parameter.properties,
                format_args!("block_dimension.{ordinal}"),
                dimension_set.id.clone(),
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
                    let key = format!("consumer.{candidate}");
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
                    &mut parameter.properties,
                    format_args!("consumer.{consumer_ordinal}"),
                    consumer.clone(),
                )?;
            }
            annotations
                .derived(ctx, parameter.id.as_str(), "properties")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    Ok(())
}

pub(super) fn expression_parameter_id(expression_id: &str) -> Option<ParameterId> {
    let (section, key) = expression_id.split_once(":expression#")?;
    IdScope::of(section)?.try_id(&cadmpeg_ir::identity_component!("parameter"), key)
}
