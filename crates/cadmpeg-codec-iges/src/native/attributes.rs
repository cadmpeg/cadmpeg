// SPDX-License-Identifier: Apache-2.0
//! Canonical neutral values for typed IGES property and attribute records.

use super::{
    NativeAttributeTableDefinition, NativeAttributeTableInstance, NativeProductProperty,
    NativeProperty, NativePropertyValue, ProductPropertyKind,
};
use crate::entities::geometry::SourceSequences;
use crate::loss::IgesLossCode;
use crate::parameter::TokenValue;
use cadmpeg_core::{CodecError, decode::DecodeContext};
use cadmpeg_ir::CadIr;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AttributeId, IdentityKey};
use cadmpeg_ir::report::loss::LossNote;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

pub(super) type ProjectedValues = BTreeMap<String, Vec<AttributeId>>;

pub(super) fn serialize_product_property<S: Serializer>(
    record: &NativeProductProperty,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Wire<'a> {
        id: &'a str,
        source_entity: &'a str,
        form: i64,
        property_kind: ProductPropertyKind,
        value: &'a Option<cadmpeg_ir::native::bytes::NativeBytes<Vec<u8>>>,
        owners: &'a [String],
        #[serde(skip_serializing_if = "Option::is_none")]
        neutral_values: Option<&'a ProjectedValues>,
    }
    Wire {
        id: &record.id,
        source_entity: &record.source_entity,
        form: record.property_kind.form(),
        property_kind: record.property_kind,
        value: &record.value,
        owners: &record.owners,
        neutral_values: record.neutral_values.as_ref(),
    }
    .serialize(serializer)
}

pub(super) fn serialize_property<S: Serializer>(
    record: &NativeProperty,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Wire<'a> {
        id: &'a str,
        source_entity: &'a str,
        form: i64,
        declared_value_count: Option<i64>,
        owners: &'a [String],
        #[serde(flatten)]
        value: &'a NativePropertyValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        neutral_values: Option<&'a ProjectedValues>,
    }
    Wire {
        id: &record.id,
        source_entity: &record.source_entity,
        form: record.form,
        declared_value_count: record.declared_value_count,
        owners: &record.owners,
        value: &record.value,
        neutral_values: record.neutral_values.as_ref(),
    }
    .serialize(serializer)
}

pub(super) fn serialize_definition<S: Serializer>(
    record: &NativeAttributeTableDefinition,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Wire<'a> {
        id: &'a str,
        source_entity: &'a str,
        form: i64,
        name: &'a Option<cadmpeg_ir::native::bytes::NativeBytes<Vec<u8>>>,
        attribute_list_type: Option<i64>,
        declared_attribute_count: Option<i64>,
        attributes: &'a [super::NativeAttributeDefinition],
        #[serde(skip_serializing_if = "Option::is_none")]
        neutral_values: Option<&'a ProjectedValues>,
    }
    Wire {
        id: &record.id,
        source_entity: &record.source_entity,
        form: record.form,
        name: &record.name,
        attribute_list_type: record.attribute_list_type,
        declared_attribute_count: record.declared_attribute_count,
        attributes: &record.attributes,
        neutral_values: record.neutral_values.as_ref(),
    }
    .serialize(serializer)
}

pub(super) fn serialize_instance<S: Serializer>(
    record: &NativeAttributeTableInstance,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Wire<'a> {
        id: &'a str,
        source_entity: &'a str,
        form: i64,
        definition: &'a Option<String>,
        declared_row_count: Option<i64>,
        rows: &'a [Vec<TokenValue>],
        #[serde(skip_serializing_if = "Option::is_none")]
        neutral_values: Option<&'a ProjectedValues>,
    }
    Wire {
        id: &record.id,
        source_entity: &record.source_entity,
        form: record.form,
        definition: &record.definition,
        declared_row_count: record.declared_row_count,
        rows: &record.rows,
        neutral_values: record.neutral_values.as_ref(),
    }
    .serialize(serializer)
}

fn property_kind(value: &NativePropertyValue) -> &'static str {
    use NativePropertyValue::{
        AssociativityGroupType, BasicDimension, Closure, DimensionDisplayData, DimensionTolerance,
        DimensionUnits, DrawingSheetApproval, DrawingSheetId, DrilledHole,
        ExternalReferenceFileList, FlowLineSpecification, GenericData, Hierarchy, Highlight,
        IntercharacterSpacing, LepArtworkStackup, LepDrilledHole, LevelFunction,
        LevelToLepLayerMap, LineFont, LineWidening, Name, NominalSize, Overscore, PartNumber, Pick,
        PinNumber, ReferenceDesignator, RegionFill, RegionRestriction, TabularData, Underscore,
        UniformRectangularGrid,
    };
    match value {
        RegionRestriction { .. } => "region_restriction",
        LevelFunction { .. } => "level_function",
        RegionFill { .. } => "region_fill",
        LineWidening { .. } => "line_widening",
        DrilledHole { .. } => "drilled_hole",
        ReferenceDesignator { .. } => "reference_designator",
        PinNumber { .. } => "pin_number",
        PartNumber { .. } => "part_number",
        Hierarchy { .. } => "hierarchy",
        TabularData { .. } => "tabular_data",
        ExternalReferenceFileList { .. } => "external_reference_file_list",
        NominalSize { .. } => "nominal_size",
        FlowLineSpecification { .. } => "flow_line_specification",
        Name { .. } => "name",
        IntercharacterSpacing { .. } => "intercharacter_spacing",
        LineFont { .. } => "line_font",
        Highlight { .. } => "highlight",
        Pick { .. } => "pick",
        UniformRectangularGrid { .. } => "uniform_rectangular_grid",
        AssociativityGroupType { .. } => "associativity_group_type",
        LevelToLepLayerMap { .. } => "level_to_lep_layer_map",
        LepArtworkStackup { .. } => "lep_artwork_stackup",
        LepDrilledHole { .. } => "lep_drilled_hole",
        GenericData { .. } => "generic_data",
        DimensionUnits { .. } => "dimension_units",
        DimensionTolerance { .. } => "dimension_tolerance",
        DimensionDisplayData { .. } => "dimension_display_data",
        BasicDimension { .. } => "basic_dimension",
        DrawingSheetApproval { .. } => "drawing_sheet_approval",
        DrawingSheetId { .. } => "drawing_sheet_id",
        Underscore { .. } => "underscore",
        Overscore { .. } => "overscore",
        Closure { .. } => "closure",
    }
}

fn source(sequence: u32) -> String {
    format!("iges:entity:directory#{sequence}")
}

fn targets(ir: &CadIr, sequences: &SourceSequences) -> BTreeMap<String, Vec<AttributeTarget>> {
    let mut targets = BTreeMap::<String, Vec<AttributeTarget>>::new();
    let mut add = |sequence, target| {
        targets.entry(source(sequence)).or_default().push(target);
    };
    for body in &ir.model.bodies {
        if let Some(sequence) = sequences.body(&body.id) {
            add(sequence, AttributeTarget::Body(body.id.clone()));
        }
    }
    for face in &ir.model.faces {
        if let Some(sequence) = sequences.face(&face.id) {
            add(sequence, AttributeTarget::Face(face.id.clone()));
        }
    }
    for curve in &ir.model.curves {
        if let Some(sequence) = sequences.curve(&curve.id) {
            add(sequence, AttributeTarget::Curve(curve.id.clone()));
        }
    }
    for surface in &ir.model.surfaces {
        if let Some(sequence) = sequences.surface(&surface.id) {
            add(sequence, AttributeTarget::Surface(surface.id.clone()));
        }
    }
    for point in &ir.model.points {
        if let Some(sequence) = sequences.point(&point.id) {
            add(sequence, AttributeTarget::Point(point.id.clone()));
        }
    }
    let namespace = ir.native.namespace("iges");
    let native_sources: BTreeMap<_, _> = namespace
        .into_iter()
        .flat_map(|namespace| namespace.arenas().values())
        .flatten()
        .filter_map(|record| {
            Some((
                record.id(),
                record.field("source_entity")?.as_str()?.to_owned(),
            ))
        })
        .collect();
    for product in &ir.model.product_definitions {
        if let Some(source) = product
            .native_ref
            .as_deref()
            .and_then(|id| native_sources.get(id))
        {
            targets
                .entry(source.clone())
                .or_default()
                .push(AttributeTarget::ProductDefinition(product.id.clone()));
        }
    }
    for occurrence in &ir.model.occurrences {
        if let Some(source) = occurrence
            .native_ref
            .as_deref()
            .and_then(|id| native_sources.get(id))
        {
            targets
                .entry(source.clone())
                .or_default()
                .push(AttributeTarget::Occurrence(occurrence.id.clone()));
        }
    }
    targets
}

type Fields = Vec<(String, Vec<AttributeValue>)>;

fn project_fields(
    ir: &mut CadIr,
    source_id: &str,
    owners: &[String],
    name: &str,
    fields: Fields,
    targets: &BTreeMap<String, Vec<AttributeTarget>>,
    ctx: &DecodeContext<'_>,
) -> Result<ProjectedValues, CodecError> {
    let sequence = source_id
        .rsplit_once('#')
        .and_then(|(_, sequence)| sequence.parse::<u32>().ok())
        .ok_or_else(|| CodecError::malformed("IGES attribute source has no directory sequence"))?;
    let mut assigned = Vec::new();
    for owner in owners {
        match targets.get(owner) {
            Some(targets) => assigned.extend(targets.iter().cloned()),
            None => assigned.push(AttributeTarget::Source {
                source_id: owner.clone(),
            }),
        }
    }
    if assigned.is_empty() {
        assigned.push(AttributeTarget::Source {
            source_id: source_id.to_owned(),
        });
    }
    assigned.sort();
    assigned.dedup();
    let mut result = BTreeMap::new();
    for (field_index, (field, value)) in fields.into_iter().enumerate() {
        let mut references = Vec::new();
        for (owner_index, target) in assigned.iter().enumerate() {
            let key =
                IdentityKey::encode_key_text(&format!("D{sequence}:{field_index}:{owner_index}"));
            let id = AttributeId::compose(
                &cadmpeg_ir::identity_namespace!("iges", "application", "attribute"),
                key,
            );
            ctx.charge_entities(1, "iges_neutral_attributes")?;
            ir.model.attributes.push(SourceAttribute {
                id: id.clone(),
                target: target.clone(),
                name: if field.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}.{field}")
                },
                values: value.clone(),
            });
            references.push(id);
        }
        result.insert(field, references);
    }
    Ok(result)
}

mod fields;
mod labels;

pub(super) struct PropertyRecords<'a> {
    pub(super) properties: &'a mut [NativeProperty],
    pub(super) products: &'a mut [NativeProductProperty],
    pub(super) definitions: &'a mut [NativeAttributeTableDefinition],
    pub(super) instances: &'a mut [NativeAttributeTableInstance],
}

pub(super) fn project(
    ir: &mut CadIr,
    sequences: &SourceSequences,
    entities: &mut [super::NativeEntity<'_>],
    records: PropertyRecords<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let PropertyRecords {
        properties,
        products,
        definitions,
        instances,
    } = records;
    let targets = targets(ir, sequences);
    let mut losses = Vec::new();
    for property in properties.iter_mut().filter(|property| property.form != 15) {
        match fields::property(&property.value)
            .and_then(|fields| Ok((fields::name(&property.value)?, fields)))
        {
            Ok((name, fields)) => {
                property.neutral_values = Some(project_fields(
                    ir,
                    &property.source_entity,
                    &property.owners,
                    &name,
                    fields,
                    &targets,
                    ctx,
                )?);
            }
            Err(reason) => losses.push(
                IgesLossCode::NeutralProjectionUnavailable
                    .note(format!("{}: {reason}", property.id)),
            ),
        }
    }
    losses.extend(labels::project(ir, entities, properties, &targets, ctx)?);
    for product in products.iter_mut() {
        product.neutral_values = properties
            .iter()
            .find(|property| property.source_entity == product.source_entity)
            .and_then(|property| property.neutral_values.clone());
    }
    for definition in definitions.iter_mut() {
        let mut fields = Vec::new();
        let mut refusal = None;
        for (index, attribute) in definition.attributes.iter().enumerate() {
            for (value_index, value) in attribute.values.iter().enumerate() {
                match fields::typed_token(&value.value, attribute.value_data_type) {
                    Ok(Some(value)) => fields.push((
                        format!("attributes.{index}.values.{value_index}"),
                        vec![value],
                    )),
                    Ok(None) => {}
                    Err(reason) => refusal = Some(reason),
                }
            }
        }
        if let Some(reason) = refusal {
            losses.push(
                IgesLossCode::NeutralProjectionUnavailable
                    .note(format!("{}: {reason}", definition.id)),
            );
        } else {
            definition.neutral_values = Some(project_fields(
                ir,
                &definition.source_entity,
                &[],
                "iges.attribute_table.default",
                fields,
                &targets,
                ctx,
            )?);
        }
    }
    for instance in instances {
        let definition = instance
            .definition
            .as_ref()
            .and_then(|id| definitions.iter().find(|definition| definition.id == *id));
        let Some(definition) = definition else {
            losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                "{}: attribute row definition does not resolve",
                instance.id
            )));
            continue;
        };
        let mut fields = Vec::new();
        let mut refusal = None;
        for (row, values) in instance.rows.iter().enumerate() {
            let Some(types) = row_types(definition, values.len()) else {
                refusal = Some("attribute row does not fit its typed descriptor widths");
                break;
            };
            for (column, (value, data_type)) in values.iter().zip(types).enumerate() {
                match fields::typed_token(value, data_type) {
                    Ok(Some(value)) => fields.push((format!("rows.{row}.{column}"), vec![value])),
                    Ok(None) => {}
                    Err(reason) => refusal = Some(reason),
                }
            }
        }
        if let Some(reason) = refusal {
            losses.push(
                IgesLossCode::NeutralProjectionUnavailable
                    .note(format!("{}: {reason}", instance.id)),
            );
        } else {
            instance.neutral_values = Some(project_fields(
                ir,
                &instance.source_entity,
                &[],
                "iges.attribute_table.value",
                fields,
                &targets,
                ctx,
            )?);
        }
    }

    Ok(losses)
}

fn row_types(
    definition: &NativeAttributeTableDefinition,
    width: usize,
) -> Option<Vec<Option<i64>>> {
    let mut types = Vec::new();
    for attribute in &definition.attributes {
        let count = usize::try_from(attribute.declared_value_count.unwrap_or(1)).ok()?;
        let end = types.len().checked_add(count).filter(|end| *end <= width)?;
        for _ in types.len()..end {
            types.push(attribute.value_data_type);
        }
    }
    (types.len() == width).then_some(types)
}

#[cfg(test)]
mod tests;
