// SPDX-License-Identifier: Apache-2.0
//! The parsed form of one USDA layer (spec §3 to §7).
//!
//! The tree keeps every statement in source order and evaluates no
//! composition. Values stay untyped: [`Value`] is the value syntax of §7.1,
//! and the declared type name beside it gives the type (§7.2).

/// One parsed layer.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Layer {
    /// Layer metadata items in source order (§4.3).
    pub(crate) metadata: Vec<MetaItem>,
    /// Root prims in source order.
    pub(crate) prims: Vec<PrimSpec>,
    /// The `reorder rootPrims` name list, when stated.
    pub(crate) root_order: Option<Vec<String>>,
}

/// A prim specifier (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Specifier {
    Def,
    Over,
    Class,
}

/// One prim specification (§5.1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrimSpec {
    pub(crate) specifier: Specifier,
    /// Dot-joined schema type name, when stated.
    pub(crate) type_name: Option<String>,
    pub(crate) name: String,
    pub(crate) metadata: Vec<MetaItem>,
    pub(crate) body: PrimBody,
    /// Byte offset of the specifier keyword.
    pub(crate) offset: usize,
}

/// The statements of a prim or variant body (§5.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PrimBody {
    pub(crate) properties: Vec<PropertySpec>,
    pub(crate) children: Vec<PrimSpec>,
    pub(crate) variant_sets: Vec<VariantSet>,
    /// `reorder nameChildren` lists in source order.
    pub(crate) child_orders: Vec<Vec<String>>,
    /// `reorder properties` lists in source order.
    pub(crate) property_orders: Vec<Vec<String>>,
}

/// A `variantSet` statement (§5.4).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VariantSet {
    pub(crate) name: String,
    pub(crate) variants: Vec<Variant>,
}

/// One variant of a variant set.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Variant {
    pub(crate) name: String,
    pub(crate) metadata: Vec<MetaItem>,
    pub(crate) body: PrimBody,
}

/// A list-operation prefix (§4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListOp {
    Add,
    Delete,
    Append,
    Prepend,
    Reorder,
}

impl ListOp {
    pub(crate) fn from_keyword(word: &str) -> Option<Self> {
        Some(match word {
            "add" => Self::Add,
            "delete" => Self::Delete,
            "append" => Self::Append,
            "prepend" => Self::Prepend,
            "reorder" => Self::Reorder,
            _ => return None,
        })
    }
}

/// A property statement (§6.1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropertySpec {
    Attribute(AttributeSpec),
    Relationship(RelationshipSpec),
}

impl PropertySpec {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Attribute(attribute) => &attribute.name,
            Self::Relationship(relationship) => &relationship.name,
        }
    }
}

/// Attribute variability (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variability {
    Varying,
    Uniform,
    Config,
}

/// One attribute statement (§6.2).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttributeSpec {
    /// The list-op prefix of a `.connect` statement.
    pub(crate) list_op: Option<ListOp>,
    pub(crate) custom: bool,
    pub(crate) variability: Variability,
    pub(crate) type_name: String,
    pub(crate) array: bool,
    pub(crate) name: String,
    pub(crate) field: AttributeField,
    pub(crate) metadata: Vec<MetaItem>,
    pub(crate) offset: usize,
}

/// The field one attribute statement sets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AttributeField {
    /// A declaration with no assignment.
    Declared,
    /// An assignment of the default value.
    Default(AttributeValue),
    /// A `.timeSamples` map, in source order.
    TimeSamples(Vec<TimeSample>),
    /// A `.connect` target list.
    Connect(PathList),
    /// A `.spline` value, kept as source text.
    Spline(String),
}

/// The right side of an attribute assignment.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AttributeValue {
    /// `None`: no default value.
    None,
    /// `AnimationBlock`.
    AnimationBlock,
    /// An `edit [ … ]` array edit, kept as source text.
    ArrayEdit(String),
    Value(Value),
}

/// One time sample (§6.2.1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimeSample {
    pub(crate) time: Number,
    pub(crate) value: AttributeValue,
}

/// A path-valued right side: `None`, one path, or a bracketed list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PathList {
    None,
    Paths(Vec<String>),
}

/// One relationship statement (§6.3).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RelationshipSpec {
    pub(crate) list_op: Option<ListOp>,
    pub(crate) custom: bool,
    pub(crate) varying: bool,
    pub(crate) name: String,
    pub(crate) field: RelationshipField,
    pub(crate) metadata: Vec<MetaItem>,
    pub(crate) offset: usize,
}

/// The field one relationship statement sets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RelationshipField {
    /// A declaration with no assignment.
    Declared,
    /// The target list.
    Targets(PathList),
    /// `.default = <path>`.
    DefaultTarget(String),
    /// The `rel name [<path>]` form.
    BracketTarget(String),
}

/// One metadata item (§4.1, §4.3, §4.4).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetaItem {
    pub(crate) list_op: Option<ListOp>,
    /// The key; a bare documentation string has the key `documentation`.
    pub(crate) key: String,
    pub(crate) value: MetaValue,
}

/// The value of one metadata item.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MetaValue {
    None,
    Value(Value),
    Dictionary(Dictionary),
    References(Vec<Reference>),
    Payloads(Vec<Reference>),
    Paths(Vec<String>),
    Names(Vec<String>),
    SubLayers(Vec<SubLayer>),
    Relocates(Vec<(String, String)>),
    StringMap(Vec<(String, String)>),
    /// `permission`, `symmetryFunction`, and `displayUnit` identifiers.
    Identifier(Option<String>),
}

/// One reference or payload list item (§4.4).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reference {
    pub(crate) asset: Option<String>,
    pub(crate) prim_path: Option<String>,
    pub(crate) offset: Option<Number>,
    pub(crate) scale: Option<Number>,
    pub(crate) custom_data: Option<Dictionary>,
}

/// One `subLayers` item (§4.3).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SubLayer {
    pub(crate) asset: String,
    pub(crate) offset: Option<Number>,
    pub(crate) scale: Option<Number>,
}

/// A dictionary value (§7.1).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Dictionary {
    pub(crate) entries: Vec<DictionaryEntry>,
}

/// One dictionary entry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DictionaryEntry {
    pub(crate) key: String,
    pub(crate) value: DictionaryValue,
}

/// A dictionary entry value: a typed value or a nested dictionary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DictionaryValue {
    Typed {
        type_name: String,
        array: bool,
        value: Value,
    },
    Dictionary(Dictionary),
}

/// A number literal (§2.6): its binary64 value, and its exact integer value
/// when the literal is an optional minus and digits only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Number {
    pub(crate) float: f64,
    pub(crate) integer: Option<i128>,
}

/// The value syntax of §7.1.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Number(Number),
    /// A bare identifier.
    Identifier(String),
    String(String),
    Asset(String),
    Path(String),
    Tuple(Vec<Value>),
    List(Vec<Value>),
    /// A list of numbers, or of tuples of numbers, held flat.
    Packed(Packed),
}

/// A list whose elements are numbers or tuples of numbers, held as the
/// components of each element in order: `width` components per element,
/// the rows of a matrix in turn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Packed {
    pub(crate) numbers: Numbers,
    pub(crate) width: usize,
}

/// The components of a packed list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Numbers {
    Reals(Vec<f64>),
    Integers(Vec<i128>),
    /// Integer literals: the exact value of each and its binary64 value,
    /// which keeps the sign of `-0`.
    Literals {
        integers: Vec<i128>,
        floats: Vec<f64>,
    },
}
