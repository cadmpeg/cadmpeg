// SPDX-License-Identifier: Apache-2.0
//! The properties and sketch constraints of a Body's features, and the
//! properties of the Body, as v11 parameters.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::View;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    self, Expression, ExpressionDialect, ExpressionToken, Neutral, Node,
};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, ValueKind};
use cadmpeg_ir::features::parameters::{
    DesignParameter as LegacyParameter, ParameterValue as LegacyValue,
};
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;

use cadmpeg_ir::spreadsheets::CellAddress;

use crate::design::cell_content::formula::{
    self, Class, Kind, Origin, Path, Reference, Scope, Unparsed, Value,
};
use crate::design::cell_content::{self, CellContent, CellKind, Dimensions};
use crate::loss::FreecadLossCode;
use crate::native::{ObjectRecord, PropertyRecord};

use super::DesignSources;

mod python;

/// The property type of a sketch's constraints, whose values `FreeCAD` stores
/// in millimetres and radians.
const CONSTRAINT_LIST: &str = "Sketcher::PropertyConstraintList";

/// The property type of a spreadsheet's cells, one parameter each (§5.12).
const SHEET: &str = "Spreadsheet::PropertySheet";

/// The display attributes of a cell, each kept as `fcstd:<attribute>`.
const CELL_DISPLAY: [&str; 4] = ["alignment", "style", "foregroundColor", "backgroundColor"];

/// A cell's display unit, kept as `fcstd:displayUnit` where it is no `unit`
/// of the cell's parameter.
const DISPLAY_UNIT: &str = "displayUnit";

/// Each parameter of `parameters` that one of `features` or a Body owns, in
/// the scope of `definition`. `parameters` holds the model's parameters and
/// the minted parameters of features that have no model record.
/// `containers` maps a Body's feature to its container.
pub(super) fn project(
    parameters: [&[LegacyParameter]; 2],
    definition: &ProductDefinitionId,
    containers: &BTreeMap<FeatureId, ContainerId>,
    features: &[Feature],
    sources: &DesignSources<'_, '_>,
    losses: &mut Vec<LossNote>,
) -> Vec<DesignParameter> {
    let carriers = sources
        .properties
        .iter()
        .map(|property| (property.id.as_str(), property.type_name.as_str()))
        .collect::<HashMap<_, _>>();
    let features = features
        .iter()
        .map(|feature| &feature.id)
        .collect::<BTreeSet<_>>();
    let [model, minted] = parameters;
    let all = || model.iter().chain(minted);
    let owners = all()
        .filter_map(|parameter| {
            let owner = parameter.owner.as_ref()?;
            let owner = containers.get(owner).map_or_else(
                || {
                    features
                        .contains(owner)
                        .then(|| ParameterOwner::Feature(owner.clone()))
                },
                |container| Some(ParameterOwner::Container(container.clone())),
            )?;
            Some((&parameter.id, owner))
        })
        .collect::<BTreeMap<_, _>>();
    let by_id = all()
        .filter(|parameter| owners.contains_key(&parameter.id))
        .map(|parameter| (&parameter.id, parameter))
        .collect::<BTreeMap<_, _>>();
    let is_cell = |parameter: &LegacyParameter| {
        parameter
            .native_ref
            .as_deref()
            .and_then(|property| carriers.get(property).copied())
            == Some(SHEET)
    };
    let catalog = Catalog::new(&by_id, sources);
    let minted = minted
        .iter()
        .map(|parameter| &parameter.id)
        .collect::<BTreeSet<_>>();
    let formulas = by_id
        .values()
        .filter(|parameter| is_cell(parameter))
        .filter_map(
            |parameter| match cell_content::read(&parameter.expression) {
                CellContent::Formula(text) => Some((&parameter.id, formula::parse(text))),
                CellContent::Text(_)
                | CellContent::Value(_)
                | CellContent::Error
                | CellContent::Unmodeled => None,
            },
        )
        .collect::<BTreeMap<_, _>>();
    let values = cell_values(&by_id, &formulas, &catalog, &is_cell);
    let mut projected = Vec::new();
    for parameter in all() {
        let Some(owner) = owners.get(&parameter.id) else {
            continue;
        };
        if !std::ptr::eq(by_id[&parameter.id], parameter) {
            continue;
        }
        let carrier = parameter
            .native_ref
            .as_deref()
            .and_then(|property| carriers.get(property).copied());
        let sheet = carrier == Some(SHEET);
        let context = Context {
            catalog: &catalog,
            values: &values,
            minted: &minted,
        };
        let (kind, value, parameter_definition) = if sheet {
            let (kind, value, expression) =
                cell(parameter, formulas.get(&parameter.id), &context, losses);
            (kind, value, ParameterDefinition::Expression(expression))
        } else {
            let (kind, value) = kind_and_value(parameter.value.as_ref());
            let parameter_definition = if property(parameter, "is_driving") == Some("0") {
                ParameterDefinition::Driven
            } else {
                ParameterDefinition::Expression(expression(
                    parameter, &context, &kind, carrier, losses,
                ))
            };
            (kind, value, parameter_definition)
        };
        let (name, unit, native_options, source_settings) = if sheet {
            let unit = display_unit(parameter, &kind);
            let (native_options, source_settings) = cell_settings(parameter, &kind, unit.is_none());
            (
                property(parameter, "alias")
                    .and_then(|alias| NonBlankString::new(alias.to_owned())),
                unit,
                native_options,
                source_settings,
            )
        } else {
            (
                name(parameter),
                None,
                BTreeMap::default(),
                BTreeMap::default(),
            )
        };
        projected.push(DesignParameter {
            id: parameter.id.clone(),
            scope: definition.clone(),
            owner: Some(owner.clone()),
            name,
            kind,
            unit,
            definition: parameter_definition,
            value,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options,
            source_settings,
            native_ref: parameter.native_ref.clone(),
        });
    }
    projected.sort_by(|a, b| a.id.cmp(&b.id));
    projected
}

/// The objects, properties, and parameters that a formula's paths name.
struct Catalog<'a, 'data> {
    parameters: &'a BTreeMap<&'a ParameterId, &'a LegacyParameter>,
    /// Each object by its name.
    names: HashMap<&'a str, &'a ObjectRecord>,
    /// Each object by its label, where no other object has the label.
    labels: HashMap<&'a str, Option<&'a ObjectRecord>>,
    /// Each object by its feature.
    objects: HashMap<FeatureId, &'a ObjectRecord>,
    /// The feature of each object, by the object's id.
    features: HashMap<&'a str, FeatureId>,
    /// The property by its owner's id and its name.
    properties: HashMap<(&'a str, &'a str), &'a PropertyRecord>,
    links: HashMap<(&'a str, &'a str), ScalarLink<'a>>,
    python: HashMap<&'a str, OnceCell<Value>>,
    entries: &'a BTreeMap<String, View<'data>>,
    /// Whether each admitted shape payload has a root.
    shapes: HashMap<&'a str, bool>,
    /// Each parameter by its owner and a name a path gives it: a
    /// property's name, `Constraints.<name>`, and a cell's alias and
    /// address. A name that two parameters share names neither.
    named: HashMap<(FeatureId, String), Option<&'a ParameterId>>,
    /// The cells of each sheet, with their addresses.
    cells: HashMap<FeatureId, Vec<(CellAddress, &'a ParameterId)>>,
}

enum ScalarLink<'a> {
    Local(&'a ObjectRecord),
    Null,
    Tuple,
    External,
}

/// What a path names.
enum Target<'a> {
    /// A parameter, with the value of its property where the property has a
    /// stated type.
    Parameter {
        id: &'a ParameterId,
        typed: Option<Value>,
        indirect: bool,
    },
    /// A property that is no parameter, with its value.
    Property(Value),
    /// A property of another document or of a sub-object.
    Outside,
    Unresolved,
}

impl<'a, 'data> Catalog<'a, 'data> {
    fn new(
        parameters: &'a BTreeMap<&'a ParameterId, &'a LegacyParameter>,
        sources: &DesignSources<'a, 'data>,
    ) -> Self {
        let mut catalog = Self {
            parameters,
            names: HashMap::new(),
            labels: HashMap::new(),
            objects: HashMap::new(),
            features: HashMap::new(),
            properties: HashMap::new(),
            links: HashMap::new(),
            python: HashMap::new(),
            entries: sources.entries,
            shapes: sources
                .shapes
                .iter()
                .filter_map(|shape| {
                    let tables = crate::brep::Tables::from_payload(shape)?;
                    Some((shape.property.as_str(), !tables.roots.is_empty()))
                })
                .collect(),
            named: HashMap::new(),
            cells: HashMap::new(),
        };
        for object in sources.objects {
            catalog.names.insert(object.name().as_str(), object);
            if let Ok(feature) = crate::design::feature_id(object) {
                catalog.objects.insert(feature.clone(), object);
                catalog.features.insert(object.id().as_str(), feature);
            }
        }
        let ids = sources
            .objects
            .iter()
            .map(|object| (object.id().as_str(), object))
            .collect::<HashMap<_, _>>();
        let mut holders = HashMap::new();
        for property in sources.properties {
            holders.insert(property.id.as_str(), property.owner.as_str());
            let held = (property.owner.as_str(), property.name.as_str());
            catalog.properties.insert(held, property);
            let carrier = match property.type_name.as_str() {
                "App::PropertyXLink" | "App::PropertyXLinkHidden" => Some(("XLink", "name")),
                "App::PropertyLink"
                | "App::PropertyLinkChild"
                | "App::PropertyLinkGlobal"
                | "App::PropertyLinkHidden" => Some(("Link", "value")),
                _ => None,
            };
            if let Some((tag, key)) = carrier {
                let empty = property.values().iter().any(|value| {
                    value.tag == tag && value.attributes.get(key).is_some_and(String::is_empty)
                });
                let target = match (empty, property.links()) {
                    (true, _) => Some(ScalarLink::Null),
                    (_, [Some(link)]) if link.document().is_some() => Some(ScalarLink::External),
                    (_, [Some(link)]) => {
                        let object = link.object().and_then(|id| ids.get(id));
                        Some(match object {
                            Some(object) if object.id() != &property.owner => {
                                if link.subelements().is_empty() {
                                    ScalarLink::Local(object)
                                } else {
                                    ScalarLink::Tuple
                                }
                            }
                            _ => ScalarLink::Null,
                        })
                    }
                    _ => None,
                };
                if let Some(target) = target {
                    catalog.links.insert(held, target);
                }
            }
            if property.type_name == "App::PropertyPythonObject" {
                catalog.python.insert(property.id.as_str(), OnceCell::new());
            }
            let label = (property.name == "Label" && property.type_name == "App::PropertyString")
                .then(|| property.values().iter().find(|value| value.tag == "String"))
                .flatten()
                .and_then(|value| value.attributes.get("value"));
            if let (Some(label), Some(object)) = (label, ids.get(property.owner.as_str())) {
                catalog
                    .labels
                    .entry(label.as_str())
                    .and_modify(|found| *found = None)
                    .or_insert(Some(object));
            }
        }
        for parameter in parameters.values() {
            let Some(owner) = parameter.owner.as_ref() else {
                continue;
            };
            // A feature that is no object's feature id is the feature of the
            // object that holds the parameter's property.
            if !catalog.objects.contains_key(owner) {
                let object = parameter
                    .native_ref
                    .as_deref()
                    .and_then(|property| holders.get(property))
                    .and_then(|object| ids.get(object));
                if let Some(object) = object {
                    catalog.objects.insert(owner.clone(), object);
                    catalog.features.insert(object.id().as_str(), owner.clone());
                }
            }
            let mut names = vec![parameter.name.clone()];
            names.extend(property(parameter, "alias").map(str::to_owned));
            if let Some(address) = property(parameter, "address") {
                names.push(address.to_owned());
                if let Some(address) = CellAddress::parse(address) {
                    catalog
                        .cells
                        .entry(owner.clone())
                        .or_default()
                        .push((address, &parameter.id));
                }
            }
            names.extend(
                property(parameter, "source_name").map(|name| format!("Constraints.{name}")),
            );
            names.sort();
            names.dedup();
            for name in names {
                catalog
                    .named
                    .entry((owner.clone(), name))
                    .and_modify(|found| *found = None)
                    .or_insert(Some(&parameter.id));
            }
        }
        catalog
    }

    /// What `path` names in a formula of `owner`. A path of one component,
    /// or one that starts with `.`, names a property of `owner`; a longer
    /// path names a property of the object its first component names, by
    /// name and then by label, and of `owner` where no object has the name.
    fn resolve(&self, owner: Option<&FeatureId>, path: &Path) -> Target<'a> {
        if path.origin == Origin::Outside {
            return Target::Outside;
        }
        let Some(own) = owner.and_then(|owner| self.objects.get(owner)) else {
            return Target::Unresolved;
        };
        let components = path.components.as_slice();
        let (mut object, mut rest) = match components.split_first() {
            Some((first, rest)) if path.origin != Origin::Own && !rest.is_empty() => {
                let named = (path.origin != Origin::Label)
                    .then(|| self.names.get(first.as_str()).copied())
                    .flatten();
                match named.or_else(|| self.labels.get(first.as_str()).copied().flatten()) {
                    Some(object) => (object, rest),
                    None => (*own, components),
                }
            }
            _ => (*own, components),
        };
        if path.indexed {
            return Target::Unresolved;
        }
        let mut indirect = false;
        while let Some((name, sub)) = rest.split_first() {
            let held = (object.id().as_str(), name.as_str());
            if !sub.is_empty() {
                if let Some(ScalarLink::Local(target)) = self.links.get(&held) {
                    if matches!(sub, [label] if matches!(label.as_str(), "Label" | "Label2")) {
                        return Target::Property(Value::of(&[Class::Text]));
                    }
                    object = target;
                    rest = sub;
                    indirect = true;
                    continue;
                }
            }
            let typed = self.properties.get(&held).map(|property| {
                match (self.links.get(&held), sub.is_empty()) {
                    (Some(ScalarLink::Null), true) => Value::none(),
                    (Some(ScalarLink::Null | ScalarLink::Tuple), false) => {
                        Value::of(&[Class::Raises])
                    }
                    (Some(ScalarLink::Tuple), true) => Value::of(&[Class::Tuple]),
                    (Some(ScalarLink::Local(_)), true) => Value::of(&[Class::Object]),
                    (Some(ScalarLink::External), _) => Value::any(),
                    _ if sub.is_empty() && property.type_name == "App::PropertyPythonObject" => {
                        self.python[property.id.as_str()]
                            .get_or_init(|| python::object(property, self.entries))
                            .clone()
                    }
                    _ => {
                        let has_root =
                            self.shapes.get(property.id.as_str()).copied().or_else(|| {
                                property
                                    .values()
                                    .iter()
                                    .any(|value| {
                                        value.tag == "Part"
                                            && value
                                                .attributes
                                                .get("file")
                                                .is_some_and(String::is_empty)
                                    })
                                    .then_some(false)
                            });
                        property_value(property, sub, has_root, indirect)
                    }
                }
            });
            let key = std::iter::once(name.replace('$', ""))
                .chain(sub.iter().cloned())
                .collect::<Vec<_>>()
                .join(".");
            if let Some(feature) = self.features.get(object.id().as_str()) {
                if let Some(Some(id)) = self.named.get(&(feature.clone(), key)) {
                    return Target::Parameter {
                        id,
                        typed,
                        indirect,
                    };
                }
            }
            return match typed {
                Some(value) => Target::Property(value),
                None if indirect => Target::Property(Value::any()),
                None => Target::Unresolved,
            };
        }
        Target::Unresolved
    }

    /// The cells of the sheet `owner` from `from` to `to`, each an address or
    /// an alias.
    fn range(
        &self,
        owner: Option<&FeatureId>,
        from: &str,
        to: &str,
    ) -> Option<Vec<&'a ParameterId>> {
        let owner = owner?;
        let cells = self.cells.get(owner)?;
        let corner = |name: &str| {
            CellAddress::parse(&name.replace('$', "")).or_else(|| {
                let id = (*self.named.get(&(owner.clone(), name.to_owned()))?)?;
                cells
                    .iter()
                    .find(|(_, cell)| *cell == id)
                    .map(|(address, _)| *address)
            })
        };
        let (from, to) = (corner(from)?, corner(to)?);
        let rows = from.row().min(to.row())..=from.row().max(to.row());
        let columns = from.col().min(to.col())..=from.col().max(to.col());
        let mut selected = cells
            .iter()
            .filter(|(address, _)| {
                rows.contains(&address.row()) && columns.contains(&address.col())
            })
            .collect::<Vec<_>>();
        selected.sort_unstable_by_key(|(address, _)| (address.col(), address.row()));
        Some(selected.into_iter().map(|(_, id)| *id).collect())
    }
}

/// The value of a property of type `type_name`, or of its component `sub`
/// (`getPyObject`, `getPyPathValue`).
fn property_value(
    property: &PropertyRecord,
    sub: &[String],
    shape_has_root: Option<bool>,
    indirect: bool,
) -> Value {
    let type_name = property.type_name.as_str();
    let sub = sub.iter().map(String::as_str).collect::<Vec<_>>();
    let raw = |tag: &str| match property.values() {
        [value] if value.tag == tag => value.attributes.get("value").map(String::as_str),
        _ => None,
    };
    let quantity = |quantity| Value::of(&[Class::quantity(quantity)]);
    let scalar = |class, tag| Value::numeric(class, raw(tag).and_then(|value| value.parse().ok()));
    match (type_name.strip_prefix("App::Property"), sub.as_slice()) {
        (Some("Integer" | "IntegerConstraint" | "Percent"), []) => Value::numeric(
            Class::Int,
            raw("Integer")
                .and_then(|value| value.parse::<i32>().ok())
                .map(f64::from),
        ),
        (Some("Float" | "FloatConstraint" | "Precision"), []) => scalar(Class::Float, "Float"),
        (
            Some("Placement"),
            ["Rotation", "Axis", "x" | "y" | "z"] | ["Rotation", "Yaw" | "Pitch" | "Roll"],
        ) => Value::of(&[Class::Float]),
        (Some("Bool"), []) => Value::numeric(
            Class::Boolean,
            raw("Bool").map(|value| f64::from(u8::from(value == "true"))),
        ),
        (Some("Placement"), ["Base", "x" | "y" | "z"])
        | (Some("VectorDistance" | "Position" | "Direction"), ["x" | "y" | "z"])
            if !indirect =>
        {
            quantity(Quantity::LENGTH)
        }
        (Some("Placement"), ["Rotation", "Angle"]) | (Some("Rotation"), ["Angle"]) if !indirect => {
            quantity(Quantity::ANGLE)
        }
        (Some("Placement"), ["Base", "x" | "y" | "z"] | ["Rotation", "Angle"])
        | (Some("Rotation"), ["Angle"] | ["Axis", "x" | "y" | "z"])
        | (
            Some("Vector" | "VectorDistance" | "Position" | "Direction"),
            ["x" | "y" | "z" | "Length"],
        ) => Value::of(&[Class::Float]),
        (Some("Placement"), ["Base"] | ["Rotation", "Axis"]) | (Some("Rotation"), ["Axis"]) => {
            Value::of(&[Class::Vector])
        }
        (Some("Placement"), ["Rotation"]) => Value::of(&[Class::Object]),
        (Some("Quantity" | "QuantityConstraint"), []) => Value::of(&[Class::Quantity(None)]),
        (Some("Enumeration"), []) if indirect => {
            if enumeration_index(property).is_some() {
                Value::of(&[Class::Text])
            } else {
                Value::of(&[Class::Text, Class::Refused])
            }
        }
        (Some("Enumeration"), []) => Value::numeric(Class::Int, enumeration_index(property)),
        (Some("Enumeration"), ["String"]) if !indirect => {
            if enumeration_index(property).is_some() {
                Value::of(&[Class::Text])
            } else {
                Value::of(&[Class::Text, Class::Raises])
            }
        }
        (Some("Enumeration"), ["Enum" | "All"]) if !indirect => {
            Value::of(&[Class::Tuple, Class::Raises])
        }
        (Some("String" | "File" | "FileIncluded" | "Font" | "UUID"), []) => {
            raw("String").map_or_else(|| Value::of(&[Class::Text]), Value::text)
        }
        (Some("Path"), []) => raw("Path").map_or_else(|| Value::of(&[Class::Text]), Value::text),
        (Some("Vector" | "VectorDistance" | "Position" | "Direction"), []) => {
            Value::of(&[Class::Vector])
        }
        (
            Some(
                "LinkSub" | "LinkSubChild" | "LinkSubGlobal" | "LinkSubHidden" | "XLinkSub"
                | "XLinkSubHidden",
            ),
            [],
        ) => Value::of(&[Class::Tuple, Class::Refused]),
        (Some("Color"), []) => Value::of(&[Class::Tuple]),
        (Some("IntegerSet"), []) => Value::of(&[Class::Refused]),
        (Some("Placement"), []) => Value::of(&[Class::Placement]),
        (
            Some(
                "LinkList" | "LinkListChild" | "LinkListGlobal" | "LinkListHidden" | "XLinkList"
                | "LinkSubList" | "LinkSubListChild" | "LinkSubListGlobal" | "LinkSubListHidden"
                | "XLinkSubList" | "IntegerList" | "FloatList" | "StringList" | "BoolList"
                | "ColorList" | "MaterialList" | "VectorList" | "PlacementList",
            ),
            [],
        ) => Value::of(&[Class::List]),
        (Some("Map"), []) => Value::of(&[Class::Dict]),
        (Some("Rotation" | "Matrix" | "Material"), []) => Value::of(&[Class::Object]),
        (Some(name), []) => property_quantity(name)
            .map(Dimensions::from_quantity)
            .or_else(|| property_native_dimensions(name))
            .map_or_else(Value::any, |dimensions| {
                scalar(Class::Quantity(Some(dimensions)), "Float")
            }),
        (None, ["Length" | "Area" | "Volume"]) if type_name == "Part::PropertyPartShape" => {
            Value::of(match shape_has_root {
                Some(true) => &[Class::Float],
                Some(false) => &[Class::Raises],
                None => &[Class::Float, Class::Raises],
            })
        }
        (None, []) if type_name == "Part::PropertyShapeHistory" => Value::none(),
        (None, [])
            if matches!(
                type_name,
                "Part::PropertyGeometryList"
                    | "Part::PropertyTopoShapeList"
                    | "Part::PropertyFilletEdges"
                    | "Part::PropertyShapeCache"
                    | "Sketcher::PropertyConstraintList"
            ) =>
        {
            Value::of(&[Class::List])
        }
        (None, [])
            if matches!(
                type_name,
                "Part::PropertyPartShape"
                    | "Spreadsheet::PropertySheet"
                    | "Spreadsheet::PropertyColumnWidths"
                    | "Spreadsheet::PropertyRowHeights"
            ) =>
        {
            Value::of(&[Class::Object])
        }
        _ => Value::any(),
    }
}

fn enumeration_index(property: &PropertyRecord) -> Option<f64> {
    let [index, items] = property.values() else {
        return None;
    };
    if index.tag != "Integer"
        || !index.attributes.contains_key("CustomEnum")
        || items.tag != "CustomEnumList"
    {
        return None;
    }
    let index: i32 = index.attributes.get("value")?.parse().ok()?;
    let count: i32 = items.attributes.get("count")?.parse().ok()?;
    (0..count).contains(&index).then_some(f64::from(index))
}

fn property_native_dimensions(name: &str) -> Option<Dimensions> {
    let dimensions = match name {
        "AmountOfSubstance" => [0, 0, 0, 0, 0, 0, 1, 0],
        "CurrentDensity" => [-2, 0, 0, 0, 1, 0, 0, 0],
        "ElectricalCapacitance" => [-2, -1, 0, 4, 2, 0, 0, 0],
        "ElectricalInductance" => [2, 1, 0, -2, -2, 0, 0, 0],
        "ElectricalConductance" => [-2, -1, 0, 3, 2, 0, 0, 0],
        "ElectricalConductivity" => [-3, -1, 0, 3, 2, 0, 0, 0],
        "ElectricalResistance" => [2, 1, 0, -3, -2, 0, 0, 0],
        "ElectricCharge" => [0, 0, 0, 1, 1, 0, 0, 0],
        "SurfaceChargeDensity" => [-2, 0, 0, 1, 1, 0, 0, 0],
        "VolumeChargeDensity" => [-3, 0, 0, 1, 1, 0, 0, 0],
        "ElectricCurrent" => [0, 0, 0, 0, 1, 0, 0, 0],
        "ElectricPotential" => [2, 1, 0, -3, -1, 0, 0, 0],
        "LuminousIntensity" => [0, 0, 0, 0, 0, 0, 0, 1],
        "MagneticFieldStrength" | "Magnetization" => [-1, 0, 0, 0, 1, 0, 0, 0],
        "MagneticFlux" => [2, 1, 0, -2, -1, 0, 0, 0],
        "MagneticFluxDensity" => [0, 1, 0, -2, -1, 0, 0, 0],
        "ElectromagneticPotential" => [1, 1, 0, -2, -1, 0, 0, 0],
        "SpecificHeat" => [2, 0, 0, -2, 0, -1, 0, 0],
        "Temperature" => [0, 0, 0, 0, 0, 1, 0, 0],
        "ThermalConductivity" => [1, 1, 0, -3, 0, -1, 0, 0],
        "ThermalExpansionCoefficient" | "VolumetricThermalExpansionCoefficient" => {
            [0, 0, 0, 0, 0, -1, 0, 0]
        }
        "ThermalTransferCoefficient" => [0, 1, 0, -3, 0, -1, 0, 0],
        "VacuumPermittivity" => [-3, -1, 0, 4, 2, 0, 0, 0],
        _ => return None,
    };
    Some(Dimensions(dimensions))
}

/// The quantity of a quantity property `App::Property<name>`.
fn property_quantity(name: &str) -> Option<Quantity> {
    Some(match name {
        "Length" | "Distance" => Quantity::LENGTH,
        "Angle" => Quantity::ANGLE,
        "Area" => Quantity::of(2, 0, 0, 0),
        "Volume" => Quantity::of(3, 0, 0, 0),
        "Mass" => Quantity::of(0, 1, 0, 0),
        "Time" => Quantity::of(0, 0, 0, 1),
        "Frequency" => Quantity::of(0, 0, 0, -1),
        "Force" => Quantity::of(1, 1, 0, -2),
        "Pressure"
        | "Stress"
        | "ShearModulus"
        | "YoungsModulus"
        | "YieldStrength"
        | "UltimateTensileStrength"
        | "CompressiveStrength" => Quantity::of(-1, 1, 0, -2),
        "Speed" | "Velocity" => Quantity::of(1, 0, 0, -1),
        "Acceleration" => Quantity::of(1, 0, 0, -2),
        "Power" => Quantity::of(2, 1, 0, -3),
        "Work" | "Moment" => Quantity::of(2, 1, 0, -2),
        "Density" => Quantity::of(-3, 1, 0, 0),
        "InverseLength" => Quantity::of(-1, 0, 0, 0),
        "InverseArea" => Quantity::of(-2, 0, 0, 0),
        "InverseVolume" => Quantity::of(-3, 0, 0, 0),
        "VolumeFlowRate" => Quantity::of(3, 0, 0, -1),
        "KinematicViscosity" => Quantity::of(2, 0, 0, -1),
        "DynamicViscosity" => Quantity::of(-1, 1, 0, -1),
        "SpecificEnergy" => Quantity::of(2, 0, 0, -2),
        "DissipationRate" => Quantity::of(2, 0, 0, -3),
        "HeatFlux" => Quantity::of(0, 1, 0, -3),
        "Stiffness" => Quantity::of(0, 1, 0, -2),
        "StiffnessDensity" => Quantity::of(-2, 1, 0, -2),
        "Float" | "FloatConstraint" | "Precision" | "Integer" | "IntegerConstraint" | "Percent" => {
            Quantity::REAL
        }
        _ => return None,
    })
}

/// The kind of a cell's result, and the value that a reference to the cell
/// reads.
#[derive(Debug, Clone)]
struct CellValue {
    kind: Kind,
    value: Value,
}

impl CellValue {
    fn of(value: Value) -> Self {
        Self {
            kind: value.kind(),
            value: value.into_stored(),
        }
    }
}

/// The values of the cells of each sheet, each formula after the cells it
/// references. A formula in a cycle has no kind the file determines.
fn cell_values<'a>(
    parameters: &BTreeMap<&'a ParameterId, &'a LegacyParameter>,
    formulas: &BTreeMap<&'a ParameterId, Result<formula::Node<'a>, Unparsed>>,
    catalog: &Catalog<'a, '_>,
    is_cell: &dyn Fn(&LegacyParameter) -> bool,
) -> HashMap<&'a ParameterId, CellValue> {
    let mut values = HashMap::new();
    for parameter in parameters.values().filter(|parameter| is_cell(parameter)) {
        let value = match cell_content::read(&parameter.expression) {
            CellContent::Text(text) => Value::text(text),
            CellContent::Value(value) => Value::numeric(
                if value.dimensions == Dimensions::REAL {
                    Value::number(value.freecad_number.unwrap_or(value.number))
                } else {
                    Class::Quantity(Some(value.dimensions))
                },
                value.freecad_number,
            ),
            CellContent::Error => Value::of(&[Class::Raises]),
            CellContent::Unmodeled => Value::any(),
            CellContent::Formula(_) => match formulas.get(&parameter.id) {
                Some(Err(Unparsed::Syntax)) => Value::text(parameter.expression.as_str()),
                Some(Ok(_) | Err(Unparsed::Limit | Unparsed::Unmodeled)) | None => continue,
            },
        };
        values.insert(&parameter.id, CellValue::of(value));
    }
    let pending = formulas
        .iter()
        .filter_map(|(id, node)| Some((*id, node.as_ref().ok()?)))
        .collect::<BTreeMap<_, _>>();
    let mut waiting = BTreeMap::new();
    let mut dependents = BTreeMap::<&ParameterId, Vec<&ParameterId>>::new();
    for (id, node) in &pending {
        let owner = parameters
            .get(id)
            .and_then(|parameter| parameter.owner.as_ref());
        let mut sources = BTreeSet::new();
        for reference in node.references() {
            match reference {
                Reference::Path(path) => {
                    if let Target::Parameter { id, .. } = catalog.resolve(owner, path) {
                        sources.insert(id);
                    }
                }
                Reference::Range(from, to) => {
                    sources.extend(catalog.range(owner, from, to).unwrap_or_default());
                }
            }
        }
        sources.retain(|source| pending.contains_key(source));
        for source in &sources {
            dependents.entry(*source).or_default().push(*id);
        }
        waiting.insert(*id, sources.len());
    }
    let mut ready = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    while let Some(id) = ready.pop() {
        let owner = parameters
            .get(id)
            .and_then(|parameter| parameter.owner.as_ref());
        let mut lookup = Lookup {
            catalog,
            values: &values,
            owner,
            projected: false,
        };
        let value = pending[id].value(&mut lookup);
        values.insert(id, CellValue::of(value));
        for dependent in dependents.get(id).into_iter().flatten() {
            if let Some(count) = waiting.get_mut(dependent) {
                *count -= 1;
                if *count == 0 {
                    ready.push(*dependent);
                }
            }
        }
    }
    for id in formulas.keys() {
        values.entry(*id).or_insert(CellValue {
            kind: Kind::Unknown,
            value: Value::any(),
        });
    }
    values
}

/// The values a formula's paths take: a parameter's from its cell's value
/// or its stored value, and a property's from its type.
struct Lookup<'c, 'a, 'data> {
    catalog: &'c Catalog<'a, 'data>,
    values: &'c HashMap<&'a ParameterId, CellValue>,
    owner: Option<&'c FeatureId>,
    /// A cell of no kind the file determines is the real it is projected
    /// as.
    projected: bool,
}

impl Lookup<'_, '_, '_> {
    /// The value of parameter `id`, whose property has the value `typed`
    /// where the property has a stated type: a cell's from its result, and
    /// another parameter's from its property's type and its stored value.
    fn parameter(&self, id: &ParameterId, typed: Option<Value>) -> Value {
        if let Some(cell) = self.values.get(id) {
            return if self.projected && cell.kind == Kind::Unknown {
                Value::of(&[Class::quantity(Quantity::REAL)])
            } else {
                cell.value.clone()
            };
        }
        let Some(parameter) = self.catalog.parameters.get(id) else {
            return typed.unwrap_or_else(Value::any);
        };
        let stored = match &parameter.value {
            Some(LegacyValue::Length(length)) => {
                Value::numeric(Class::quantity(Quantity::LENGTH), Some(length.get()))
            }
            Some(LegacyValue::Angle(angle)) => Value::numeric(
                Class::quantity(Quantity::ANGLE),
                Some(angle.get().to_degrees()),
            ),
            Some(LegacyValue::Real(_)) => Value::of(&[Class::Int, Class::Float]),
            Some(LegacyValue::Integer(value)) => {
                Value::numeric(Class::Int, i32::try_from(*value).ok().map(f64::from))
            }
            Some(LegacyValue::Boolean(value)) => {
                Value::numeric(Class::Boolean, Some(f64::from(u8::from(*value))))
            }
            Some(LegacyValue::String(text)) => Value::text(text.as_str()),
            None => Value::any(),
        };
        match (typed, stored.kind()) {
            (Some(typed), _) if typed.is_any() => stored,
            (Some(typed), Kind::Quantity(quantity))
                if typed.classes() == [Class::Quantity(None)] =>
            {
                Value::numeric(Class::quantity(quantity), stored.numeric_fact())
            }
            (Some(typed), Kind::Quantity(_)) => typed,
            (Some(typed), Kind::Boolean | Kind::Text | Kind::Unknown) => typed,
            (None, _) => stored,
        }
    }
}

impl Scope for Lookup<'_, '_, '_> {
    fn path(&mut self, path: &Path) -> Value {
        match self.catalog.resolve(self.owner, path) {
            Target::Parameter { id, typed, .. } => self.parameter(id, typed),
            Target::Property(value) => value,
            Target::Outside | Target::Unresolved => Value::any(),
        }
    }

    fn range(&mut self, from: &str, to: &str) -> Option<Vec<Value>> {
        let ids = self.catalog.range(self.owner, from, to)?;
        Some(ids.into_iter().map(|id| self.parameter(id, None)).collect())
    }
}

/// What the projection of a parameter's expression reads.
struct Context<'c, 'a, 'data> {
    catalog: &'c Catalog<'a, 'data>,
    values: &'c HashMap<&'a ParameterId, CellValue>,
    /// The parameters that have no model record, and so no stored
    /// dependencies: their references, and each reference to them, bind.
    minted: &'c BTreeSet<&'a ParameterId>,
}

fn property<'a>(parameter: &'a LegacyParameter, key: &str) -> Option<&'a str> {
    parameter
        .properties
        .iter()
        .find(|(name, _)| name.as_str() == key)
        .map(|(_, value)| value.as_str())
}

/// The source's name of the parameter: a named constraint's name, or the
/// property's name.
fn name(parameter: &LegacyParameter) -> Option<NonBlankString> {
    let name = property(parameter, "source_name").unwrap_or(&parameter.name);
    NonBlankString::new(name.to_owned())
}

/// A cell's kind, value, and expression (§5.12), read as `FreeCAD` reads
/// its content (`cell_content`). A value of a quantity whose units have
/// symbols is a neutral literal; another value is the source's text. A
/// formula that `FreeCAD` does not parse is text. A formula has the kind of
/// its result, and where the file does not determine that kind it is a real,
/// with a loss.
fn cell(
    parameter: &LegacyParameter,
    formula: Option<&Result<formula::Node<'_>, Unparsed>>,
    context: &Context<'_, '_, '_>,
    losses: &mut Vec<LossNote>,
) -> (ValueKind, Option<ParameterValue>, Expression) {
    let content = parameter.expression.as_str();
    let fallback = |text: &str, losses: &mut Vec<LossNote>| {
        assumed(parameter, losses);
        (
            ValueKind::Quantity(Quantity::REAL),
            None,
            source(text, Vec::new()),
        )
    };
    let text = match cell_content::read(content) {
        CellContent::Formula(text) => text,
        CellContent::Text(text) => return text_cell(text),
        CellContent::Error | CellContent::Unmodeled => return fallback(content, losses),
        CellContent::Value(value) => {
            let kind = match value.kind {
                CellKind::Quantity(quantity) => ValueKind::Quantity(quantity),
                CellKind::Native(unit) => ValueKind::Native(unit),
            };
            if !value.number.is_finite() {
                losses.push(FreecadLossCode::ParameterValueNonFinite.note(format!(
                    "FCStd cell {} holds a number that is not finite; the cell keeps its \
                     content and has no value",
                    parameter.id.as_str()
                )));
            }
            let expression = value
                .neutral
                .and_then(|text| expr::parse(&text, &[], &[]).ok())
                .map_or_else(
                    || source(content, Vec::new()),
                    |tree| Expression::Neutral(Neutral(tree)),
                );
            return (
                kind,
                FiniteReal::new(value.number).map(ParameterValue::Number),
                expression,
            );
        }
    };
    let node = match formula {
        Some(Ok(node)) => node,
        Some(Err(Unparsed::Limit | Unparsed::Unmodeled)) => return fallback(text, losses),
        Some(Err(Unparsed::Syntax)) | None => return text_cell(content),
    };
    let kind = context
        .values
        .get(&parameter.id)
        .map_or(Kind::Unknown, |value| value.kind);
    let expression = translated(parameter, text, node, context, kind, None, losses);
    match kind {
        Kind::Quantity(quantity) => {
            let (stored, value) = kind_and_value(parameter.value.as_ref());
            let kind = ValueKind::Quantity(quantity);
            let value = value.filter(|_| stored == kind);
            (kind, value, expression)
        }
        Kind::Boolean => {
            let (stored, value) = kind_and_value(parameter.value.as_ref());
            let value = value.filter(|_| stored == ValueKind::Boolean);
            (ValueKind::Boolean, value, expression)
        }
        Kind::Text => (ValueKind::Text, None, expression),
        Kind::Unknown => {
            assumed(parameter, losses);
            (ValueKind::Quantity(Quantity::REAL), None, expression)
        }
    }
}

fn text_cell(text: &str) -> (ValueKind, Option<ParameterValue>, Expression) {
    (
        ValueKind::Text,
        Some(ParameterValue::Text(text.to_owned())),
        Expression::Neutral(Neutral(Node::Text(text.to_owned()))),
    )
}

fn assumed(parameter: &LegacyParameter, losses: &mut Vec<LossNote>) {
    losses.push(FreecadLossCode::ParameterQuantityAssumed.note(format!(
        "FCStd cell {} has content whose kind the file does not determine; its kind is \
         a dimensionless real",
        parameter.id.as_str()
    )));
}

/// The unit of a cell's display unit, where it has the cell's quantity and
/// each of its words has a symbol (§5.12, Unit).
fn display_unit(parameter: &LegacyParameter, kind: &ValueKind) -> Option<Unit> {
    let ValueKind::Quantity(quantity) = kind else {
        return None;
    };
    let (found, unit) = cell_content::neutral_unit(property(parameter, DISPLAY_UNIT)?)?;
    (found == *quantity).then_some(unit)
}

/// A cell's display attributes, which only a `FreeCAD` writer writes (Rule N
/// case (b)), and its display unit where `display_unit` is true: a native
/// option of a number, which another writer charges, and a source setting
/// of a text. The first map holds the native options.
fn cell_settings(
    parameter: &LegacyParameter,
    kind: &ValueKind,
    display_unit: bool,
) -> (NativeOptions, NativeOptions) {
    let option = |attribute: &str| {
        let value = NonBlankString::new(property(parameter, attribute)?.to_owned())?;
        let key = CodecName::new(&cadmpeg_ir::codec_id!("fcstd"), attribute)?;
        Some((key, NativeOption::Text(Dimension::Literal(value))))
    };
    let mut settings = CELL_DISPLAY
        .iter()
        .filter_map(|attribute| option(attribute))
        .collect::<NativeOptions>();
    let mut native = NativeOptions::default();
    if let Some((key, value)) = display_unit.then(|| option(DISPLAY_UNIT)).flatten() {
        if *kind == ValueKind::Text {
            settings.insert(key, value);
        } else {
            native.insert(key, value);
        }
    }
    (native, settings)
}

fn kind_and_value(value: Option<&LegacyValue>) -> (ValueKind, Option<ParameterValue>) {
    let quantity = |quantity, number: f64| {
        (
            ValueKind::Quantity(quantity),
            FiniteReal::new(number).map(ParameterValue::Number),
        )
    };
    match value {
        Some(LegacyValue::Length(value)) => quantity(Quantity::LENGTH, value.get()),
        Some(LegacyValue::Angle(value)) => quantity(Quantity::ANGLE, value.get()),
        Some(LegacyValue::Real(value)) => quantity(Quantity::REAL, value.get()),
        Some(LegacyValue::Integer(value)) => {
            (ValueKind::Integer, Some(ParameterValue::Integer(*value)))
        }
        Some(LegacyValue::Boolean(value)) => {
            (ValueKind::Boolean, Some(ParameterValue::Boolean(*value)))
        }
        Some(LegacyValue::String(value)) => {
            (ValueKind::Text, Some(ParameterValue::Text(value.clone())))
        }
        None => (ValueKind::Quantity(Quantity::REAL), None),
    }
}

/// The parameter's expression. A stored value is a one-literal expression in
/// the property's unit; a bound property holds the expression `FreeCAD` binds
/// to it. A parameter whose expression text is empty is stated with its
/// stored value and charged a loss.
fn expression(
    parameter: &LegacyParameter,
    context: &Context<'_, '_, '_>,
    kind: &ValueKind,
    carrier: Option<&str>,
    losses: &mut Vec<LossNote>,
) -> Expression {
    let text = parameter.expression.as_str();
    if text.trim().is_empty() {
        return empty_expression(parameter, carrier, losses);
    }
    if parameter.dependencies.is_empty() {
        if let Some(literal) = literal(text, parameter.value.as_ref(), carrier) {
            return Expression::Neutral(Neutral(literal));
        }
    }
    let Ok(node) = formula::parse(text) else {
        return source(text, Vec::new());
    };
    let mut lookup = Lookup {
        catalog: context.catalog,
        values: context.values,
        owner: parameter.owner.as_ref(),
        projected: true,
    };
    let stated = node.kind(&mut lookup);
    translated(parameter, text, &node, context, stated, Some(kind), losses)
}

/// The expression of a parameter that stores no expression text: its stored
/// value as a one-literal expression where it is a quantity in a known
/// carrier unit, and otherwise a source expression with no token, the
/// spelling of a text the decoder did not read (interim, gap 20261119-X3-1).
fn empty_expression(
    parameter: &LegacyParameter,
    carrier: Option<&str>,
    losses: &mut Vec<LossNote>,
) -> Expression {
    let literal = parameter
        .value
        .as_ref()
        .and_then(|value| stored_literal(value, carrier));
    let stated = if literal.is_some() {
        "its stored value is stated as its expression"
    } else {
        "its expression is stated with no text"
    };
    losses.push(FreecadLossCode::ParameterExpressionEmpty.note(format!(
        "FCStd parameter {} has an empty expression; {stated}",
        parameter.id.as_str()
    )));
    literal.map_or_else(
        || Expression::Source {
            dialect: ExpressionDialect::FreeCad,
            tokens: Vec::new(),
        },
        |literal| Expression::Neutral(Neutral(literal)),
    )
}

/// The unit a carrier stores a value in (§5.12): millimetres for a length,
/// degrees for an angle property, radians for a sketch constraint's angle,
/// and no unit for a real. An angle whose carrier the decoder does not know,
/// and a value that is no quantity, has none.
struct CarrierUnit {
    symbol: &'static str,
    /// A number in the unit, in the canonical unit.
    to_canonical: fn(f64) -> f64,
    /// A number in the canonical unit, in the unit.
    from_canonical: fn(f64) -> f64,
}

fn carrier_unit(value: &LegacyValue, carrier: Option<&str>) -> Option<CarrierUnit> {
    let same = |value| value;
    match value {
        LegacyValue::Length(_) => Some(CarrierUnit {
            symbol: "mm",
            to_canonical: same,
            from_canonical: same,
        }),
        LegacyValue::Angle(_) if carrier == Some(CONSTRAINT_LIST) => Some(CarrierUnit {
            symbol: "rad",
            to_canonical: same,
            from_canonical: same,
        }),
        LegacyValue::Angle(_) if carrier.is_some_and(|carrier| carrier.contains("Angle")) => {
            Some(CarrierUnit {
                symbol: "deg",
                to_canonical: f64::to_radians,
                from_canonical: f64::to_degrees,
            })
        }
        LegacyValue::Real(_) => Some(CarrierUnit {
            symbol: "",
            to_canonical: same,
            from_canonical: same,
        }),
        LegacyValue::Angle(_)
        | LegacyValue::Integer(_)
        | LegacyValue::Boolean(_)
        | LegacyValue::String(_) => None,
    }
}

/// A stored value's text as a literal in the unit its carrier stores it in.
fn literal(
    text: &str,
    value: Option<&LegacyValue>,
    carrier: Option<&str>,
) -> Option<Node<ParameterRef>> {
    let stored = text
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())?;
    literal_in(stored, &carrier_unit(value?, carrier)?)
}

/// A decoded value as a literal in the unit its carrier stores it in.
fn stored_literal(value: &LegacyValue, carrier: Option<&str>) -> Option<Node<ParameterRef>> {
    let unit = carrier_unit(value, carrier)?;
    let canonical = match value {
        LegacyValue::Length(value) => value.get(),
        LegacyValue::Angle(value) => value.get(),
        LegacyValue::Real(value) => value.get(),
        LegacyValue::Integer(_) | LegacyValue::Boolean(_) | LegacyValue::String(_) => return None,
    };
    literal_in((unit.from_canonical)(canonical), &unit)
}

fn literal_in(stored: f64, unit: &CarrierUnit) -> Option<Node<ParameterRef>> {
    let number = decimal(stored, unit.to_canonical);
    let text = if unit.symbol.is_empty() {
        number
    } else {
        format!("{number}[{}]", unit.symbol)
    };
    expr::parse(&text, &[], &[]).ok()
}

/// The shortest decimal whose value in the canonical unit equals that of
/// `stored` to 15 significant digits (§5.12). `canonical` converts a number
/// in the literal's unit to the canonical unit.
fn decimal(stored: f64, canonical: fn(f64) -> f64) -> String {
    let fifteen = |value: f64| format!("{:.14e}", canonical(value));
    let target = fifteen(stored);
    (0..17)
        .filter_map(|places| format!("{stored:.places$e}").parse::<f64>().ok())
        .find(|candidate| fifteen(*candidate) == target)
        .map_or_else(|| format!("{stored}"), |candidate| format!("{candidate}"))
}

/// A `FreeCAD` formula of the kind `stated` as a neutral tree where every
/// construct translates and its quantity is `kind`, or any quantity where
/// `kind` is `None`, and as the source's tokens with each reference to a
/// parameter it depends on otherwise. A reference to a property that is no
/// parameter stays the source's text, with a loss.
fn translated(
    parameter: &LegacyParameter,
    text: &str,
    node: &formula::Node<'_>,
    context: &Context<'_, '_, '_>,
    stated: Kind,
    kind: Option<&ValueKind>,
    losses: &mut Vec<LossNote>,
) -> Expression {
    let mut spans = BTreeMap::new();
    let mut dropped = Vec::new();
    let mut indirect = Vec::new();
    for reference in node.references() {
        let Reference::Path(path) = reference else {
            continue;
        };
        match context.catalog.resolve(parameter.owner.as_ref(), path) {
            Target::Parameter {
                id,
                indirect: false,
                ..
            } if context.minted.contains(&parameter.id)
                || context.minted.contains(id)
                || parameter
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == id) =>
            {
                let reference = ParameterRef {
                    target: RefTarget::Parameter(id.clone()),
                    through: None,
                };
                spans.insert(path.span.start, (path.span.end, reference));
            }
            Target::Property(_) | Target::Outside => {
                dropped.push(text.get(path.span.clone()).unwrap_or(text));
            }
            Target::Parameter { indirect: true, .. } => {
                indirect.push(text.get(path.span.clone()).unwrap_or(text));
            }
            Target::Parameter {
                indirect: false, ..
            }
            | Target::Unresolved => {}
        }
    }
    if !dropped.is_empty() {
        losses.push(FreecadLossCode::ParameterDependencyDropped.note(format!(
            "FCStd parameter {} references {}, which is no parameter; the reference stays \
             the source's text",
            parameter.id.as_str(),
            dropped.join(", ")
        )));
    }
    if !indirect.is_empty() {
        losses.push(FreecadLossCode::ParameterDependencyDropped.note(format!(
            "FCStd parameter {} references {} through a link; the reference stays the \
             source's text",
            parameter.id.as_str(),
            indirect.join(", ")
        )));
    }
    let mut tokens = Vec::new();
    let mut at = 0;
    for (start, (end, reference)) in &spans {
        if *start > at {
            tokens.push(ExpressionToken::Text(text[at..*start].to_owned()));
        }
        tokens.push(ExpressionToken::Reference(reference.clone()));
        at = *end;
    }
    if at < text.len() {
        tokens.push(ExpressionToken::Text(text[at..].to_owned()));
    }
    let mut refs = Vec::new();
    let tree = neutral_text(node, &spans, &mut refs)
        .and_then(|neutral| Some((expr::parse(&neutral, &refs, &[]).ok()?, neutral)));
    if let (Some((tree, neutral)), Kind::Quantity(quantity)) = (tree, stated) {
        let Some(kind) = kind else {
            return Expression::Neutral(Neutral(tree));
        };
        if ValueKind::Quantity(quantity) == *kind {
            return Expression::Neutral(Neutral(tree));
        }
        // FreeCAD takes a dimensionless result in the property's internal
        // unit (§5.12, Kind).
        let internal = match kind {
            ValueKind::Quantity(Quantity::LENGTH) => Some("mm"),
            ValueKind::Quantity(Quantity::ANGLE) => Some("deg"),
            _ => None,
        };
        if let (Quantity::REAL, Some(unit)) = (quantity, internal) {
            if let Ok(tree) = expr::parse(&format!("({neutral}) * 1[{unit}]"), &refs, &[]) {
                return Expression::Neutral(Neutral(tree));
            }
        }
    }
    source(text, tokens)
}

/// The neutral text of a formula of numbers, units, the constants `pi` and
/// `e`, references at `spans`, signs, groups, and `+ - * / ^`. Each operation
/// is parenthesized, since the neutral syntax binds `^` tighter than a sign.
fn neutral_text(
    node: &formula::Node<'_>,
    spans: &BTreeMap<usize, (usize, ParameterRef)>,
    refs: &mut Vec<ParameterRef>,
) -> Option<String> {
    use formula::{Node as Formula, Operator};
    Some(match node {
        Formula::Number { text, unit } => {
            let number = text.replace(',', ".");
            let number = if number.starts_with('.') {
                format!("0{number}")
            } else {
                number
            };
            match unit {
                None => number,
                Some(unit) => format!("{number}[{}]", unit.spelling()?),
            }
        }
        Formula::Constant { name, .. } if matches!(*name, "pi" | "e") => (*name).to_owned(),
        Formula::Path(path) => {
            let (_, reference) = spans.get(&path.span.start)?;
            let index = refs
                .iter()
                .position(|known| known == reference)
                .unwrap_or_else(|| {
                    refs.push(reference.clone());
                    refs.len() - 1
                });
            format!("${index}")
        }
        Formula::Negate(inner) => format!("(-({}))", neutral_text(inner, spans, refs)?),
        Formula::Plus(inner) | Formula::Group(inner) => {
            format!("({})", neutral_text(inner, spans, refs)?)
        }
        Formula::Binary { op, left, right } => {
            let symbol = match op {
                Operator::Add => "+",
                Operator::Subtract => "-",
                Operator::Multiply => "*",
                Operator::Divide => "/",
                Operator::Power => "^",
                Operator::Remainder
                | Operator::Equal
                | Operator::NotEqual
                | Operator::Less
                | Operator::LessEqual
                | Operator::Greater
                | Operator::GreaterEqual => return None,
            };
            let left = neutral_text(left, spans, refs)?;
            let right = match right.as_ref() {
                Formula::Unit(unit) => format!("1[{}]", unit.spelling()?),
                right => neutral_text(right, spans, refs)?,
            };
            format!("({left} {symbol} {right})")
        }
        _ => return None,
    })
}

/// The source's tokens of `text`. A source token list holds a text token
/// (part 10): a lone reference stays the source's text.
fn source(text: &str, mut tokens: Vec<ExpressionToken<ParameterRef>>) -> Expression {
    if !text.is_empty()
        && !tokens
            .iter()
            .any(|token| matches!(token, ExpressionToken::Text(_)))
    {
        tokens = vec![ExpressionToken::Text(text.to_owned())];
    }
    Expression::Source {
        dialect: ExpressionDialect::FreeCad,
        tokens,
    }
}

#[cfg(test)]
mod tests;
