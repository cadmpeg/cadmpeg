// SPDX-License-Identifier: Apache-2.0
//! Structural joins for legacy ASCII family-table persistence.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::legacy::{
    self, NumericPayload, ObjectPayload, ObjectRecord, Persistence, StringPayload,
};

const FAMILY_ROOT: &str = "drv_tbl_ptr";
const FAMILY_PARENT_NAMES: [&str; 2] = ["Solid", "Sld_FamilyInfo"];
const ITEMS_ARRAY: &str = "items";
const INSTANCES_ARRAY: &str = "instances";
const VALUES_ARRAY: &str = "values";
const VALUE_REAL: &str = "value(d_val)";
const VALUE_INTEGER: &str = "value(i_val)";
const VALUE_STRING: &str = "value(s_val)";

/// One complete legacy family-table root and its ordered rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FamilyTable {
    /// Direct owning model object identity.
    pub(crate) root_parent_id: String,
    /// Direct owning model object name.
    pub(crate) root_parent_name: String,
    /// Source offset of the root object row.
    pub(crate) offset: usize,
    /// Optional root generic-name field.
    pub(crate) generic_name: Option<legacy::StringValue>,
    /// Ordered table-column descriptors.
    pub(crate) items: Vec<FamilyTableItem>,
    /// Ordered instance rows.
    pub(crate) instances: Vec<FamilyTableInstance>,
}

/// One ordered family-table column descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FamilyTableItem {
    /// Source offset of the item object row.
    pub(crate) offset: usize,
    /// Stored item identifier.
    pub(crate) item_id: i32,
    /// Stored item type code.
    pub(crate) type_code: i32,
    /// Stored visibility flag.
    pub(crate) invisible: i32,
    /// Stored item name, including null or non-UTF-8 forms.
    pub(crate) name: legacy::StringValue,
}

/// One ordered family-table instance row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FamilyTableInstance {
    /// Source offset of the instance object row.
    pub(crate) offset: usize,
    /// Stored instance name. This field is required to be non-empty UTF-8.
    pub(crate) name: String,
    /// Stored instance attributes bitfield.
    pub(crate) attributes: i32,
    /// Direct model object referenced by the instance row.
    pub(crate) model_object_id: String,
    /// Values aligned by ordinal with [`FamilyTable::items`].
    pub(crate) values: Vec<FamilyTableValue>,
}

/// One typed family-table cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FamilyTableValue {
    /// Legacy value-row object identity.
    pub(crate) source_object_id: String,
    /// Source offset of the typed value field.
    pub(crate) offset: usize,
    /// Typed value payload.
    pub(crate) value: FamilyTableValuePayload,
}

/// Typed payload forms admitted by the legacy family-table row grammar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum FamilyTableValuePayload {
    /// `type=50` with one `value(d_val)` real field.
    Real {
        /// Exact source real value.
        value: legacy::Real,
    },
    /// `type=51` with one `value(s_val)` string field.
    String {
        /// Exact source string value.
        value: legacy::StringValue,
    },
    /// `type=52` with one `value(i_val)` integer field.
    Integer {
        /// Exact source integer value.
        value: i32,
    },
}

impl FamilyTableValuePayload {
    pub(crate) fn type_code(&self) -> i32 {
        match self {
            Self::Real { .. } => 50,
            Self::String { .. } => 51,
            Self::Integer { .. } => 52,
        }
    }
}

impl Serialize for FamilyTableItem {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            source_object_id: String,
            offset: &'a usize,
            item_id: &'a i32,
            type_code: &'a i32,
            invisible: &'a i32,
            name: &'a legacy::StringValue,
        }
        Wire {
            source_object_id: legacy::object_node_id(self.offset),
            offset: &self.offset,
            item_id: &self.item_id,
            type_code: &self.type_code,
            invisible: &self.invisible,
            name: &self.name,
        }
        .serialize(serializer)
    }
}

impl Serialize for FamilyTableInstance {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            source_object_id: String,
            offset: &'a usize,
            name: &'a String,
            attributes: &'a i32,
            model_object_id: &'a String,
            #[serde(serialize_with = "serialize_ordered")]
            values: &'a Vec<FamilyTableValue>,
        }
        Wire {
            source_object_id: legacy::object_node_id(self.offset),
            offset: &self.offset,
            name: &self.name,
            attributes: &self.attributes,
            model_object_id: &self.model_object_id,
            values: &self.values,
        }
        .serialize(serializer)
    }
}

impl Serialize for FamilyTableValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut record = serializer.serialize_struct("FamilyTableValue", 4)?;
        record.serialize_field("source_object_id", &self.source_object_id)?;
        record.serialize_field("offset", &self.offset)?;
        record.serialize_field("type_code", &self.value.type_code())?;
        record.serialize_field("value", &self.value)?;
        record.end()
    }
}

fn serialize_ordered<T: Serialize, S: serde::Serializer>(
    rows: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct OrderedRow<'a, T> {
        ordinal: usize,
        #[serde(flatten)]
        row: &'a T,
    }

    serializer.collect_seq(
        rows.iter()
            .enumerate()
            .map(|(ordinal, row)| OrderedRow { ordinal, row }),
    )
}

struct Index<'a> {
    object_by_id: BTreeMap<String, &'a ObjectRecord>,
    objects_by_parent_name: BTreeMap<(usize, &'a str), Vec<&'a ObjectRecord>>,
    integers_by_parent_name: BTreeMap<(usize, &'a str), Vec<&'a legacy::IntegerRecord>>,
    reals_by_parent_name: BTreeMap<(usize, &'a str), Vec<&'a legacy::RealRecord>>,
    strings_by_parent_name: BTreeMap<(usize, &'a str), Vec<&'a legacy::StringRecord>>,
    typed_field_names: BTreeMap<usize, Vec<&'a str>>,
}

impl<'a> Index<'a> {
    fn build(persistence: &'a Persistence) -> Option<Self> {
        let mut object_by_id = BTreeMap::new();
        let mut objects_by_parent_name = BTreeMap::new();
        for object in &persistence.objects {
            if object_by_id.insert(object.id(), object).is_some() {
                return None;
            }
            if let Some(parent) = object.parent {
                objects_by_parent_name
                    .entry((parent, object.name.as_str()))
                    .or_insert_with(Vec::new)
                    .push(object);
            }
        }

        let mut integers_by_parent_name = BTreeMap::new();
        add_value_index(&mut integers_by_parent_name, &persistence.integer_values);
        let mut reals_by_parent_name = BTreeMap::new();
        add_value_index(&mut reals_by_parent_name, &persistence.real_values);
        let mut strings_by_parent_name = BTreeMap::new();
        add_value_index(&mut strings_by_parent_name, &persistence.string_values);

        let mut typed_field_names = BTreeMap::new();
        add_typed_field_names(&mut typed_field_names, &persistence.integer_values);
        add_typed_field_names(&mut typed_field_names, &persistence.real_values);
        add_typed_field_names(&mut typed_field_names, &persistence.string_values);
        add_typed_field_names(&mut typed_field_names, &persistence.type_3_values.rows);
        add_typed_field_names(&mut typed_field_names, &persistence.type_4_values.rows);
        add_typed_field_names(&mut typed_field_names, &persistence.type_5_values.rows);
        add_typed_field_names(&mut typed_field_names, &persistence.type_6_values.rows);
        add_typed_field_names(&mut typed_field_names, &persistence.type_7_values.rows);
        add_typed_field_names(&mut typed_field_names, &persistence.type_9_values.rows);
        add_typed_field_names(&mut typed_field_names, &persistence.type_11_values.rows);

        Some(Self {
            object_by_id,
            objects_by_parent_name,
            integers_by_parent_name,
            reals_by_parent_name,
            strings_by_parent_name,
            typed_field_names,
        })
    }
}

fn add_value_index<'a, T>(
    index: &mut BTreeMap<(usize, &'a str), Vec<&'a legacy::ValueRecord<T>>>,
    records: &'a [legacy::ValueRecord<T>],
) {
    for record in records {
        if let Some(parent) = record.parent {
            index
                .entry((parent, record.name.as_str()))
                .or_default()
                .push(record);
        }
    }
}

fn add_typed_field_names<'a, T>(
    index: &mut BTreeMap<usize, Vec<&'a str>>,
    records: &'a [legacy::ValueRecord<T>],
) {
    for record in records {
        if !record.name.starts_with("value(") {
            continue;
        }
        if let Some(parent) = record.parent {
            index.entry(parent).or_default().push(record.name.as_str());
        }
    }
}

fn one_object<'a>(index: &Index<'a>, parent: usize, name: &str) -> Option<&'a ObjectRecord> {
    let records = index.objects_by_parent_name.get(&(parent, name))?;
    let [record] = records.as_slice() else {
        return None;
    };
    Some(*record)
}

fn array_elements<'a>(
    index: &Index<'a>,
    parent: usize,
    name: &str,
) -> Option<Vec<&'a ObjectRecord>> {
    let array = one_object(index, parent, name)?;
    let ObjectPayload::Array {
        dimensions,
        elements,
    } = &array.payload
    else {
        return None;
    };
    if !array.payload.is_complete() || dimensions.len() != 1 {
        return None;
    }
    elements
        .iter()
        .map(|element_id| {
            let element = index.object_by_id.get(element_id.as_str()).copied()?;
            (element.parent == Some(array.offset)).then_some(element)
        })
        .collect()
}

fn optional_integer(index: &Index<'_>, parent: usize, name: &str) -> Result<Option<i32>, ()> {
    let Some(records) = index.integers_by_parent_name.get(&(parent, name)) else {
        return Ok(None);
    };
    if records.len() != 1 {
        return Err(());
    }
    match &records[0].payload {
        NumericPayload::Scalar { value } => Ok(Some(*value)),
        NumericPayload::Array(_) => Err(()),
    }
}

fn optional_string(
    index: &Index<'_>,
    parent: usize,
    name: &str,
) -> Result<Option<legacy::StringValue>, ()> {
    let Some(records) = index.strings_by_parent_name.get(&(parent, name)) else {
        return Ok(None);
    };
    if records.len() != 1 {
        return Err(());
    }
    match &records[0].payload {
        StringPayload::Scalar { value } => Ok(Some(value.clone())),
        StringPayload::Array { .. } => Err(()),
    }
}

fn typed_value(
    index: &Index<'_>,
    value_object: &ObjectRecord,
    type_code: i32,
) -> Option<(usize, FamilyTableValuePayload)> {
    let names = index.typed_field_names.get(&value_object.offset)?;
    if names.len() != 1 {
        return None;
    }
    let expected_name = match type_code {
        50 => VALUE_REAL,
        51 => VALUE_STRING,
        52 => VALUE_INTEGER,
        _ => return None,
    };
    if names[0] != expected_name {
        return None;
    }
    match type_code {
        50 => {
            let records = index
                .reals_by_parent_name
                .get(&(value_object.offset, expected_name))?;
            if records.len() != 1 {
                return None;
            }
            let value = match &records[0].payload {
                NumericPayload::Scalar { value } => *value,
                NumericPayload::Array(_) => return None,
            };
            Some((records[0].offset, FamilyTableValuePayload::Real { value }))
        }
        51 => {
            let records = index
                .strings_by_parent_name
                .get(&(value_object.offset, expected_name))?;
            if records.len() != 1 {
                return None;
            }
            let value = match &records[0].payload {
                StringPayload::Scalar { value } => value.clone(),
                StringPayload::Array { .. } => return None,
            };
            Some((records[0].offset, FamilyTableValuePayload::String { value }))
        }
        52 => {
            let records = index
                .integers_by_parent_name
                .get(&(value_object.offset, expected_name))?;
            if records.len() != 1 {
                return None;
            }
            let value = match &records[0].payload {
                NumericPayload::Scalar { value } => *value,
                NumericPayload::Array(_) => return None,
            };
            Some((
                records[0].offset,
                FamilyTableValuePayload::Integer { value },
            ))
        }
        _ => None,
    }
}

/// Parse one complete legacy family-table object graph.
///
/// The root is selected only from a unique direct `drv_tbl_ptr` child of
/// `Solid` or `Sld_FamilyInfo`. Nested `drv_tbl_ptr` objects are instance
/// targets and are never competing roots. Every admitted array is one
/// dimensional and complete; instance values join item columns by ordinal.
pub(crate) fn parse(persistence: &Persistence) -> Option<FamilyTable> {
    let index = Index::build(persistence)?;
    let roots = persistence
        .objects
        .iter()
        .filter(|object| {
            if object.name != FAMILY_ROOT {
                return false;
            }
            let Some(parent_id) = object.parent else {
                return false;
            };
            index
                .object_by_id
                .get(&legacy::object_node_id(parent_id))
                .is_some_and(|parent| FAMILY_PARENT_NAMES.contains(&parent.name.as_str()))
        })
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return None;
    };
    if !matches!(root.payload, ObjectPayload::Arrow) {
        return None;
    }
    let root_parent_id = root.parent?;
    let root_parent = index
        .object_by_id
        .get(&legacy::object_node_id(root_parent_id))?;
    let generic_name = optional_string(&index, root.offset, "gen_name").ok()?;
    let item_rows = array_elements(&index, root.offset, ITEMS_ARRAY)?;
    let instance_rows = array_elements(&index, root.offset, INSTANCES_ARRAY)?;
    if item_rows.is_empty() || instance_rows.is_empty() {
        return None;
    }

    let items = item_rows
        .into_iter()
        .map(|item| {
            if !matches!(item.payload, ObjectPayload::Inline) {
                return None;
            }
            Some(FamilyTableItem {
                offset: item.offset,
                item_id: optional_integer(&index, item.offset, "id").ok()??,
                type_code: optional_integer(&index, item.offset, "type").ok()??,
                invisible: optional_integer(&index, item.offset, "invisible").ok()??,
                name: optional_string(&index, item.offset, "name").ok()??,
            })
        })
        .collect::<Option<Vec<_>>>()?;

    let mut instance_names = BTreeSet::new();
    let instances = instance_rows
        .into_iter()
        .map(|instance| {
            if !matches!(instance.payload, ObjectPayload::Arrow) {
                return None;
            }
            let name = match optional_string(&index, instance.offset, "name").ok()?? {
                legacy::StringValue::Utf8 { text } if !text.is_empty() => text,
                _ => return None,
            };
            if !instance_names.insert(name.clone()) {
                return None;
            }
            let attributes = optional_integer(&index, instance.offset, "attributes").ok()??;
            let model = one_object(&index, instance.offset, FAMILY_ROOT)?;
            if !matches!(model.payload, ObjectPayload::Arrow) {
                return None;
            }
            let value_rows = array_elements(&index, instance.offset, VALUES_ARRAY)?;
            if value_rows.len() != items.len() {
                return None;
            }
            let values = value_rows
                .into_iter()
                .map(|value_row| {
                    if !matches!(value_row.payload, ObjectPayload::Inline) {
                        return None;
                    }
                    let type_code = optional_integer(&index, value_row.offset, "type").ok()??;
                    let (offset, value) = typed_value(&index, value_row, type_code)?;
                    Some(FamilyTableValue {
                        source_object_id: value_row.id(),
                        offset,
                        value,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(FamilyTableInstance {
                offset: instance.offset,
                name,
                attributes,
                model_object_id: model.id(),
                values,
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(FamilyTable {
        root_parent_id: root_parent.id(),
        root_parent_name: root_parent.name.clone(),
        offset: root.offset,
        generic_name,
        items,
        instances,
    })
}

impl Serialize for FamilyTable {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: String,
            root_object_id: String,
            root_parent_id: &'a str,
            root_parent_name: &'a str,
            offset: usize,
            generic_name: &'a Option<legacy::StringValue>,
            #[serde(serialize_with = "serialize_ordered")]
            items: &'a [FamilyTableItem],
            #[serde(serialize_with = "serialize_ordered")]
            instances: &'a [FamilyTableInstance],
        }
        Wire {
            id: self.id(),
            root_object_id: legacy::object_node_id(self.offset),
            root_parent_id: &self.root_parent_id,
            root_parent_name: &self.root_parent_name,
            offset: self.offset,
            generic_name: &self.generic_name,
            items: &self.items,
            instances: &self.instances,
        }
        .serialize(serializer)
    }
}

impl FamilyTable {
    /// Native identity derived from the root offset.
    pub(crate) fn id(&self) -> String {
        format!("creo:legacy_family:driver_table#{}", self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_offset(id: &str) -> usize {
        match id {
            "solid" => 1,
            "root" => 2,
            "items-array" => 3,
            "item" => 4,
            "instances-array" => 5,
            "instance" => 6,
            "model" => 7,
            "values-array" => 8,
            "value" => 9,
            _ => panic!("unknown fixture object"),
        }
    }
    fn object(
        _id: &str,
        name: &str,
        parent: Option<&str>,
        mut payload: ObjectPayload,
        offset: usize,
    ) -> ObjectRecord {
        if let ObjectPayload::Array { elements, .. } = &mut payload {
            for element in elements {
                *element = crate::legacy::object_node_id(fixture_offset(element));
            }
        }
        ObjectRecord {
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: parent.map(fixture_offset),
            depth: 0,
            payload,
            offset,
        }
    }

    fn integer(parent: &str, name: &str, value: i32, offset: usize) -> legacy::IntegerRecord {
        legacy::ValueRecord {
            kind: crate::legacy::ValueKind::Integer,
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: NumericPayload::Scalar { value },
            offset,
        }
    }

    fn real(parent: &str, name: &str, value: f64, offset: usize) -> legacy::RealRecord {
        legacy::ValueRecord {
            kind: crate::legacy::ValueKind::Real,
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: NumericPayload::Scalar {
                value: legacy::Real::from_bits(value.to_bits()),
            },
            offset,
        }
    }

    fn string(parent: &str, name: &str, value: &str, offset: usize) -> legacy::StringRecord {
        legacy::ValueRecord {
            kind: crate::legacy::ValueKind::String,
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: StringPayload::Scalar {
                value: legacy::StringValue::Utf8 {
                    text: value.to_string(),
                },
            },
            offset,
        }
    }

    fn complete_table() -> Persistence {
        let solid = "solid";
        let root = "root";
        let item_array = "items-array";
        let item = "item";
        let instance_array = "instances-array";
        let instance = "instance";
        let model = "model";
        let value_array = "values-array";
        let value = "value";
        Persistence {
            objects: vec![
                object(solid, "Solid", None, ObjectPayload::Inline, 1),
                object(root, FAMILY_ROOT, Some(solid), ObjectPayload::Arrow, 2),
                object(
                    item_array,
                    ITEMS_ARRAY,
                    Some(root),
                    ObjectPayload::Array {
                        dimensions: vec![1],
                        elements: vec![item.to_string()],
                    },
                    3,
                ),
                object(
                    item,
                    ITEMS_ARRAY,
                    Some(item_array),
                    ObjectPayload::Inline,
                    4,
                ),
                object(
                    instance_array,
                    INSTANCES_ARRAY,
                    Some(root),
                    ObjectPayload::Array {
                        dimensions: vec![1],
                        elements: vec![instance.to_string()],
                    },
                    5,
                ),
                object(
                    instance,
                    INSTANCES_ARRAY,
                    Some(instance_array),
                    ObjectPayload::Arrow,
                    6,
                ),
                object(model, FAMILY_ROOT, Some(instance), ObjectPayload::Arrow, 7),
                object(
                    value_array,
                    VALUES_ARRAY,
                    Some(instance),
                    ObjectPayload::Array {
                        dimensions: vec![1],
                        elements: vec![value.to_string()],
                    },
                    8,
                ),
                object(
                    value,
                    VALUES_ARRAY,
                    Some(value_array),
                    ObjectPayload::Inline,
                    9,
                ),
            ],
            integer_values: vec![
                integer(item, "id", 17, 10),
                integer(item, "type", 2, 11),
                integer(item, "invisible", 0, 12),
                integer(instance, "attributes", 0, 13),
                integer(value, "type", 50, 14),
            ],
            real_values: vec![real(value, VALUE_REAL, 2.5, 15)],
            string_values: vec![
                string(item, "name", "d0", 16),
                string(instance, "name", "SMALL", 17),
            ],
            ..Persistence::default()
        }
    }

    #[test]
    fn joins_complete_ordered_table_rows() {
        let table = parse(&complete_table()).expect("complete family table");
        assert_eq!(table.items[0].item_id, 17);
        assert_eq!(table.instances[0].name, "SMALL");
        let wire = serde_json::to_value(&table).expect("serialized family table");
        assert_eq!(wire["instances"][0]["values"][0]["ordinal"], 0);
        assert_eq!(table.instances[0].values[0].value.type_code(), 50);
        assert!(matches!(
            table.instances[0].values[0].value,
            FamilyTableValuePayload::Real { .. }
        ));
    }

    #[test]
    fn nested_pointer_is_not_a_family_root() {
        let mut persistence = complete_table();
        persistence
            .objects
            .retain(|object| object.offset != fixture_offset("root"));
        assert!(parse(&persistence).is_none());
    }

    #[test]
    fn null_root_and_duplicate_roots_are_retained() {
        let mut null_root = complete_table();
        null_root
            .objects
            .iter_mut()
            .find(|object| object.offset == fixture_offset("root"))
            .expect("synthetic family-table root")
            .payload = ObjectPayload::Null;
        assert!(parse(&null_root).is_none());

        let mut duplicate = complete_table();
        duplicate.objects.push(object(
            "root-2",
            FAMILY_ROOT,
            Some("solid"),
            ObjectPayload::Arrow,
            20,
        ));
        assert!(parse(&duplicate).is_none());
    }

    #[test]
    fn incomplete_value_form_is_retained() {
        let mut persistence = complete_table();
        persistence.integer_values.retain(|record| {
            record.name != "type" || record.parent != Some(fixture_offset("value"))
        });
        assert!(parse(&persistence).is_none());
    }

    #[test]
    fn integer_and_string_value_forms_are_typed_by_their_source_field() {
        let mut persistence = complete_table();
        persistence.real_values.clear();
        persistence
            .integer_values
            .retain(|record| record.parent != Some(fixture_offset("value")));
        persistence
            .integer_values
            .push(integer("value", "type", 52, 30));
        persistence
            .integer_values
            .push(integer("value", VALUE_INTEGER, 3, 31));
        let mut table = parse(&persistence).expect("integer family table");
        assert!(matches!(
            table
                .instances
                .pop()
                .expect("synthetic family-table instance")
                .values[0]
                .value,
            FamilyTableValuePayload::Integer { value: 3 }
        ));

        let mut persistence = complete_table();
        persistence.real_values.clear();
        persistence
            .integer_values
            .retain(|record| record.parent != Some(fixture_offset("value")));
        persistence
            .integer_values
            .push(integer("value", "type", 51, 40));
        persistence
            .string_values
            .push(string("value", VALUE_STRING, "yes", 41));
        let table = parse(&persistence).expect("string family table");
        assert!(matches!(
            table.instances[0].values[0].value,
            FamilyTableValuePayload::String { .. }
        ));
    }
}
