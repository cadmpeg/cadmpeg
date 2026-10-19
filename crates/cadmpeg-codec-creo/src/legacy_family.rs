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
    root_parent_id: String,
    /// Direct owning model object name.
    root_parent_name: String,
    /// Source offset of the root object row.
    pub(crate) offset: usize,
    /// Optional root generic-name field.
    generic_name: Option<legacy::StringValue>,
    /// Ordered table-column descriptors.
    pub(crate) items: Vec<FamilyTableItem>,
    /// Ordered instance rows.
    pub(crate) instances: Vec<FamilyTableInstance>,
}

/// One ordered family-table column descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FamilyTableItem {
    /// Source offset of the item object row.
    offset: usize,
    /// Stored item identifier.
    item_id: i32,
    /// Stored item type code.
    type_code: i32,
    /// Stored visibility flag.
    invisible: i32,
    /// Stored item name, including null or non-UTF-8 forms.
    name: legacy::StringValue,
}

/// One ordered family-table instance row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FamilyTableInstance {
    /// Source offset of the instance object row.
    offset: usize,
    /// Stored instance name. This field is required to be non-empty UTF-8.
    name: String,
    /// Stored instance attributes bitfield.
    attributes: i32,
    /// Direct model object referenced by the instance row.
    model_object_id: String,
    /// Values aligned by ordinal with [`FamilyTable::items`].
    values: Vec<FamilyTableValue>,
}

/// One typed family-table cell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FamilyTableValue {
    /// Legacy value-row object identity.
    source_object_id: String,
    /// Source offset of the typed value field.
    offset: usize,
    /// Typed value payload.
    value: FamilyTableValuePayload,
}

/// Typed payload forms admitted by the legacy family-table row grammar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
enum FamilyTableValuePayload {
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
    fn type_code(&self) -> i32 {
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
            source_object_id: legacy::SerializedOffsetId,
            offset: &'a usize,
            item_id: &'a i32,
            type_code: &'a i32,
            invisible: &'a i32,
            name: &'a legacy::StringValue,
        }
        Wire {
            source_object_id: legacy::serialized_object_node_id(self.offset),
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
            source_object_id: legacy::SerializedOffsetId,
            offset: &'a usize,
            name: &'a String,
            attributes: &'a i32,
            model_object_id: &'a String,
            #[serde(serialize_with = "serialize_ordered")]
            values: &'a Vec<FamilyTableValue>,
        }
        Wire {
            source_object_id: legacy::serialized_object_node_id(self.offset),
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
    object_by_offset: BTreeMap<usize, &'a ObjectRecord>,
    objects_by_parent_name: BTreeMap<(usize, &'a str), Vec<&'a ObjectRecord>>,
    integers_by_parent_name: BTreeMap<(usize, &'a str), Option<&'a legacy::IntegerRecord>>,
    reals_by_parent_name: BTreeMap<(usize, &'a str), Option<&'a legacy::RealRecord>>,
    strings_by_parent_name: BTreeMap<(usize, &'a str), Option<&'a legacy::StringRecord>>,
    typed_field_names: BTreeMap<usize, Vec<&'a str>>,
}

impl<'a> Index<'a> {
    fn build(persistence: &'a Persistence) -> Option<Self> {
        let mut object_by_id = BTreeMap::new();
        let mut object_by_offset = BTreeMap::new();
        let mut objects_by_parent_name = BTreeMap::new();
        for object in &persistence.objects {
            let id = legacy::checked_object_node_id(object.offset);
            match object_by_id.entry(id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(object);
                }
                std::collections::btree_map::Entry::Occupied(_) => return None,
            }
            object_by_offset.insert(object.offset, object);
            if let Some(parent) = object.parent {
                match objects_by_parent_name.entry((parent, object.name.as_str())) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let rows = vec![object];
                        entry.insert(rows);
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let rows = entry.get_mut();
                        rows.push(object);
                    }
                }
            }
        }

        let mut integers_by_parent_name = BTreeMap::new();
        legacy::value_index(
            &persistence.integer_values.rows,
            &mut integers_by_parent_name,
        );
        let mut reals_by_parent_name = BTreeMap::new();
        legacy::value_index(&persistence.real_values.rows, &mut reals_by_parent_name);
        let mut strings_by_parent_name = BTreeMap::new();
        legacy::value_index(&persistence.string_values, &mut strings_by_parent_name);

        let mut typed_field_names = BTreeMap::new();
        add_typed_field_names(&mut typed_field_names, &persistence.integer_values.rows);
        add_typed_field_names(&mut typed_field_names, &persistence.real_values.rows);
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
            object_by_offset,
            objects_by_parent_name,
            integers_by_parent_name,
            reals_by_parent_name,
            strings_by_parent_name,
            typed_field_names,
        })
    }
}

fn add_typed_field_names<'a, K: legacy::LegacyCode>(
    index: &mut BTreeMap<usize, Vec<&'a str>>,
    records: &'a [legacy::ValueRecord<K>],
) {
    for record in records {
        if !record.name.starts_with("value(") {
            continue;
        }
        if let Some(parent) = record.parent {
            match index.entry(parent) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let names = vec![record.name.as_str()];
                    entry.insert(names);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let names = entry.get_mut();
                    names.push(record.name.as_str());
                }
            }
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
    let mut rows = Vec::new();
    for element_id in elements {
        let element = index.object_by_id.get(element_id.as_str()).copied()?;
        if element.parent != Some(array.offset) {
            return None;
        }
        rows.push(element);
    }
    Some(rows)
}

fn optional_integer(index: &Index<'_>, parent: usize, name: &str) -> Result<Option<i32>, ()> {
    let Some(records) = index.integers_by_parent_name.get(&(parent, name)) else {
        return Ok(None);
    };
    let record = records.ok_or(())?;
    match &record.payload {
        NumericPayload::Scalar { value } => Ok(Some(*value)),
        NumericPayload::Array(_) => Err(()),
    }
}

fn optional_string<'a>(
    index: &'a Index<'_>,
    parent: usize,
    name: &str,
) -> Result<Option<&'a legacy::StringValue>, ()> {
    let Some(records) = index.strings_by_parent_name.get(&(parent, name)) else {
        return Ok(None);
    };
    let record = records.ok_or(())?;
    match &record.payload {
        StringPayload::Scalar { value } => Ok(Some(value)),
        StringPayload::Array { .. } => Err(()),
    }
}

fn copy_string_value(value: &legacy::StringValue) -> legacy::StringValue {
    match value {
        legacy::StringValue::Null => legacy::StringValue::Null,
        legacy::StringValue::Utf8 { text } => legacy::StringValue::Utf8 {
            text: text.to_owned(),
        },
        legacy::StringValue::Bytes { bytes } => legacy::StringValue::Bytes {
            bytes: bytes.to_vec().into(),
        },
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
            let Some(record) = records else {
                return None;
            };
            let value = match &record.payload {
                NumericPayload::Scalar { value } => *value,
                NumericPayload::Array(_) => return None,
            };
            Some((record.offset, FamilyTableValuePayload::Real { value }))
        }
        51 => {
            let records = index
                .strings_by_parent_name
                .get(&(value_object.offset, expected_name))?;
            let Some(record) = records else {
                return None;
            };
            let value = match &record.payload {
                StringPayload::Scalar { value } => copy_string_value(value),
                StringPayload::Array { .. } => return None,
            };
            Some((record.offset, FamilyTableValuePayload::String { value }))
        }
        52 => {
            let records = index
                .integers_by_parent_name
                .get(&(value_object.offset, expected_name))?;
            let Some(record) = records else {
                return None;
            };
            let value = match &record.payload {
                NumericPayload::Scalar { value } => *value,
                NumericPayload::Array(_) => return None,
            };
            Some((record.offset, FamilyTableValuePayload::Integer { value }))
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
    if !persistence
        .objects
        .iter()
        .any(|object| object.name == FAMILY_ROOT && matches!(object.payload, ObjectPayload::Arrow))
    {
        return None;
    }
    let index = Index::build(persistence)?;
    parse_indexed(persistence, &index)
}

fn parse_indexed(persistence: &Persistence, index: &Index<'_>) -> Option<FamilyTable> {
    let mut roots = persistence.objects.iter().filter(|object| {
        if object.name != FAMILY_ROOT {
            return false;
        }
        let Some(parent_id) = object.parent else {
            return false;
        };
        index
            .object_by_offset
            .get(&parent_id)
            .is_some_and(|parent| FAMILY_PARENT_NAMES.contains(&parent.name.as_str()))
    });
    let root = roots.next()?;
    if roots.next().is_some() {
        return None;
    }
    if !matches!(root.payload, ObjectPayload::Arrow) {
        return None;
    }
    let root_parent_offset = root.parent?;
    let root_parent = index.object_by_offset.get(&root_parent_offset)?;
    let Ok(generic_name) = optional_string(index, root.offset, "gen_name") else {
        return None;
    };
    let generic_name = generic_name.map(copy_string_value);
    let item_rows = array_elements(index, root.offset, ITEMS_ARRAY)?;
    let instance_rows = array_elements(index, root.offset, INSTANCES_ARRAY)?;
    if item_rows.is_empty() || instance_rows.is_empty() {
        return None;
    }

    let mut items = Vec::new();
    for item in item_rows {
        if !matches!(item.payload, ObjectPayload::Inline) {
            return None;
        }
        let item_id = optional_integer(index, item.offset, "id").ok().flatten()?;
        let type_code = optional_integer(index, item.offset, "type")
            .ok()
            .flatten()?;
        let invisible = optional_integer(index, item.offset, "invisible")
            .ok()
            .flatten()?;
        let name = optional_string(index, item.offset, "name").ok().flatten()?;
        items.push(FamilyTableItem {
            offset: item.offset,
            item_id,
            type_code,
            invisible,
            name: copy_string_value(name),
        });
    }

    let mut instance_names = BTreeSet::new();
    let mut instances = Vec::new();
    for instance in instance_rows {
        if !matches!(instance.payload, ObjectPayload::Arrow) {
            return None;
        }
        let Some(legacy::StringValue::Utf8 { text }) =
            optional_string(index, instance.offset, "name")
                .ok()
                .flatten()
        else {
            return None;
        };
        if text.is_empty() || instance_names.contains(text.as_str()) {
            return None;
        }
        instance_names.insert(text.as_str());
        let name = text.to_owned();
        let attributes = optional_integer(index, instance.offset, "attributes")
            .ok()
            .flatten()?;
        let model = one_object(index, instance.offset, FAMILY_ROOT)?;
        if !matches!(model.payload, ObjectPayload::Arrow) {
            return None;
        }
        let value_rows = array_elements(index, instance.offset, VALUES_ARRAY)?;
        if value_rows.len() != items.len() {
            return None;
        }
        let mut values = Vec::new();
        for value_row in value_rows {
            if !matches!(value_row.payload, ObjectPayload::Inline) {
                return None;
            }
            let type_code = optional_integer(index, value_row.offset, "type")
                .ok()
                .flatten()?;
            let (offset, value) = typed_value(index, value_row, type_code)?;
            values.push(FamilyTableValue {
                source_object_id: legacy::checked_object_node_id(value_row.offset),
                offset,
                value,
            });
        }
        instances.push(FamilyTableInstance {
            offset: instance.offset,
            name,
            attributes,
            model_object_id: legacy::checked_object_node_id(model.offset),
            values,
        });
    }

    Some(FamilyTable {
        root_parent_id: legacy::checked_object_node_id(root_parent.offset),
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
            id: legacy::SerializedOffsetId,
            root_object_id: legacy::SerializedOffsetId,
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
            root_object_id: legacy::serialized_object_node_id(self.offset),
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
    pub(crate) fn id(&self) -> legacy::SerializedOffsetId {
        legacy::SerializedOffsetId {
            namespace: "legacy_family",
            kind: "driver_table",
            offset: self.offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse, FamilyTableValuePayload, FAMILY_ROOT, INSTANCES_ARRAY, ITEMS_ARRAY, VALUES_ARRAY,
        VALUE_INTEGER, VALUE_REAL, VALUE_STRING,
    };
    use crate::legacy;
    use crate::legacy::{NumericPayload, ObjectPayload, ObjectRecord, Persistence, StringPayload};

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
            integer_values: crate::legacy::TypedValues {
                rows: vec![
                    integer(item, "id", 17, 10),
                    integer(item, "type", 2, 11),
                    integer(item, "invisible", 0, 12),
                    integer(instance, "attributes", 0, 13),
                    integer(value, "type", 50, 14),
                ],
                unresolved_count: 0,
            },
            real_values: crate::legacy::TypedValues {
                rows: vec![real(value, VALUE_REAL, 2.5, 15)],
                unresolved_count: 0,
            },
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
        persistence.integer_values.rows.retain(|record| {
            record.name != "type" || record.parent != Some(fixture_offset("value"))
        });
        assert!(parse(&persistence).is_none());
    }

    #[test]
    fn integer_and_string_value_forms_are_typed_by_their_source_field() {
        let mut persistence = complete_table();
        persistence.real_values.rows.clear();
        persistence
            .integer_values
            .rows
            .retain(|record| record.parent != Some(fixture_offset("value")));
        persistence
            .integer_values
            .rows
            .push(integer("value", "type", 52, 30));
        persistence
            .integer_values
            .rows
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
        persistence.real_values.rows.clear();
        persistence
            .integer_values
            .rows
            .retain(|record| record.parent != Some(fixture_offset("value")));
        persistence
            .integer_values
            .rows
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

    #[test]
    fn legacy_family_byte_string_value_parses() {
        let mut fixture = complete_table();
        fixture.string_values[0].payload = StringPayload::Scalar {
            value: legacy::StringValue::Bytes {
                bytes: vec![0xff].into(),
            },
        };
        assert!(parse(&fixture).is_some());
    }

    #[test]
    fn null_root_competing_with_arrow_root_remains_ambiguous() {
        let mut persistence = complete_table();
        persistence.objects.push(object(
            "root-2",
            FAMILY_ROOT,
            Some("solid"),
            ObjectPayload::Null,
            20,
        ));
        assert!(parse(&persistence).is_none());
    }
}
