// SPDX-License-Identifier: Apache-2.0
//! The `PhysX` API schemas that the codec carries (spec §16.8).
//!
//! Each attribute has a value kind and a unit. A unit that depends on the
//! joint degree of freedom, on the mimic joint's axes, or on another
//! attribute of the schema resolves when the attribute is read.

mod table;

use crate::schema::{attribute, Issues};
use crate::stage::Prim;

/// The kind of prim that a schema applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holder {
    Scene,
    Body,
    Articulation,
    Collision,
    Material,
    Joint,
}

/// The value kind of an attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Value {
    Bool,
    Integer,
    Real,
    Reals,
    Token,
    Tokens,
    Targets,
}

/// The exponents of length, mass, and angle of a unit, and whether its
/// angle unit is the degree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dim {
    pub(crate) length: i8,
    pub(crate) mass: i8,
    pub(crate) angle: i8,
    pub(crate) degrees: bool,
}

impl Dim {
    pub(crate) const fn new(length: i8, mass: i8, angle: i8, degrees: bool) -> Self {
        Self {
            length,
            mass,
            angle,
            degrees,
        }
    }

    const NONE: Self = Self::new(0, 0, 0, false);

    /// The unit of a quotient of two units.
    const fn over(self, other: Self) -> Self {
        Self::new(
            self.length - other.length,
            self.mass - other.mass,
            self.angle - other.angle,
            self.degrees || other.degrees,
        )
    }

    /// The exponents as an IR dimension.
    pub(crate) fn dimension(self) -> cadmpeg_ir::physics::solver::Dimension {
        cadmpeg_ir::physics::solver::Dimension::new(self.length, self.mass, self.angle)
    }
}

/// The unit of an attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    /// Not a real value.
    None,
    Fixed(Dim),
    /// By the joint degree of freedom.
    Joint {
        linear: Dim,
        angular: Dim,
    },
    /// The unit of the mimic joint's degree of freedom over that of its
    /// reference axis.
    MimicGearing,
    /// By a boolean attribute of the schema.
    Switched {
        switch: &'static str,
        off: Dim,
        on: Dim,
    },
    /// By `physxForce:mode`.
    Mode {
        force: Dim,
        acceleration: Dim,
    },
}

impl Unit {
    /// The units this unit can resolve to.
    pub(crate) fn candidates(self) -> Vec<Dim> {
        match self {
            Self::None => Vec::new(),
            Self::Fixed(dim) => vec![dim],
            Self::Joint { linear, angular } => vec![linear, angular],
            Self::MimicGearing => vec![Dim::NONE, DEGREES.over(DISTANCE), DISTANCE.over(DEGREES)],
            Self::Switched { off, on, .. } => vec![off, on],
            Self::Mode {
                force,
                acceleration,
            } => vec![force, acceleration],
        }
    }
}

const DISTANCE: Dim = Dim::new(1, 0, 0, false);
const DEGREES: Dim = Dim::new(0, 0, 1, true);

/// One carried attribute.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Attribute {
    pub(crate) schema: &'static str,
    /// The name, with `{}` for the instance name of a multiple-apply schema.
    pub(crate) name: &'static str,
    pub(crate) usd_type: &'static str,
    pub(crate) uniform: bool,
    pub(crate) value: Value,
    pub(crate) unit: Unit,
}

impl Attribute {
    /// The name for instance `instance`.
    pub(crate) fn instance_name(&self, instance: Option<&str>) -> String {
        match instance {
            Some(instance) => self.name.replace("{}", instance),
            None => self.name.to_owned(),
        }
    }
}

/// The holder of carried schema `schema`, without its instance name.
pub(crate) fn holder(schema: &str) -> Option<Holder> {
    table::HOLDERS
        .iter()
        .find(|(name, _)| *name == schema)
        .map(|(_, holder)| *holder)
}

/// The attributes of carried schema `schema`.
pub(crate) fn attributes(schema: &str) -> impl Iterator<Item = &'static Attribute> + '_ {
    table::ATTRIBUTES
        .iter()
        .filter(move |attribute| attribute.schema == schema)
}

/// The attribute of schema `schema` whose name for `instance` is `name`.
pub(crate) fn find(schema: &str, instance: Option<&str>, name: &str) -> Option<&'static Attribute> {
    attributes(schema).find(|attribute| attribute.instance_name(instance) == name)
}

/// A schema name split into its base name and its instance name.
pub(crate) fn split(schema: &str) -> (&str, Option<&str>) {
    match schema.split_once(':') {
        Some((base, instance)) => (base, Some(instance)),
        None => (schema, None),
    }
}

/// The property-name prefix of carried schema `base` with instance name
/// `instance`, such as `physxLimit:angular:`.
pub(crate) fn namespace(base: &str, instance: Option<&str>) -> Option<String> {
    let namespace = attributes(base)
        .next()
        .and_then(|attribute| attribute.name.split(':').next())?;
    Some(match instance {
        Some(instance) => format!("{namespace}:{instance}:"),
        None => format!("{namespace}:"),
    })
}

/// The authored properties of `prim` in the namespace of applied schema
/// `schema` that the codec does not carry.
pub(crate) fn uncarried(prim: &Prim<'_>, schema: &str) -> Vec<String> {
    let (base, instance) = split(schema);
    let Some(prefix) = namespace(base, instance) else {
        return Vec::new();
    };
    prim.property_names()
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .filter(|name| {
            !attributes(base).any(|attribute| attribute.instance_name(instance) == *name)
        })
        .map(str::to_owned)
        .collect()
}

/// A joint degree of freedom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Freedom {
    Linear,
    Angular,
}

/// The degree of freedom an axis token names, such as `rotX` or `linear`.
pub(crate) fn axis_freedom(axis: &str) -> Option<Freedom> {
    if axis.starts_with("rot") || axis == "angular" {
        Some(Freedom::Angular)
    } else if axis.starts_with("trans") || matches!(axis, "linear" | "distance") {
        Some(Freedom::Linear)
    } else {
        None
    }
}

/// A value read from a layer, in scene units.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Read {
    Bool(bool),
    Integer(i64),
    Real(f64, Dim),
    Reals(Vec<f64>, Dim),
    Token(String),
    Tokens(Vec<String>),
    Targets(Vec<String>),
}

/// The carried attributes of applied schema `schema` on `prim`, with their
/// names. `freedom` is the degree of freedom of a joint that the schema
/// applies to.
pub(crate) fn read(
    prim: &Prim<'_>,
    schema: &str,
    freedom: Option<Freedom>,
    issues: &mut Issues,
) -> Vec<(String, Read)> {
    let (base, instance) = split(schema);
    let freedom = instance.and_then(axis_freedom).or(freedom);
    let mut out = Vec::new();
    for spec in attributes(base) {
        let name = spec.instance_name(instance);
        if spec.value == Value::Targets {
            let targets = prim.relationship_targets(&name);
            if !targets.is_empty() {
                out.push((name, Read::Targets(targets)));
            }
            continue;
        }
        if !prim.has_attribute(&name) {
            continue;
        }
        let Some(value) = attribute(prim, &name, issues) else {
            continue;
        };
        let unit = if matches!(spec.value, Value::Real | Value::Reals) {
            resolve(spec.unit, prim, instance, freedom, issues)
        } else {
            Some(Dim::NONE)
        };
        let read = match spec.value {
            Value::Bool => value
                .bools()
                .and_then(|values| values.first().copied())
                .map(Read::Bool),
            Value::Integer => value
                .integers()
                .and_then(|values| values.first().copied())
                .and_then(|value| i64::try_from(value).ok())
                .map(Read::Integer),
            Value::Real => value
                .numbers()
                .and_then(|values| values.first().copied())
                .and_then(|number| unit.map(|dim| Read::Real(number, dim))),
            Value::Reals => value
                .numbers()
                .and_then(|values| unit.map(|dim| Read::Reals(values, dim))),
            Value::Token => value
                .texts()
                .and_then(|values| values.first().cloned())
                .map(Read::Token),
            Value::Tokens => value.texts().map(|values| Read::Tokens(values.to_vec())),
            Value::Targets => None,
        };
        match read {
            Some(read) => out.push((name, read)),
            None => issues.malformed(prim, &name, "the value does not have its schema's kind"),
        }
    }
    out
}

/// The unit of an attribute on `prim`, or `None` when it does not resolve.
fn resolve(
    unit: Unit,
    prim: &Prim<'_>,
    instance: Option<&str>,
    freedom: Option<Freedom>,
    issues: &mut Issues,
) -> Option<Dim> {
    let of = |freedom: Freedom| match freedom {
        Freedom::Linear => DISTANCE,
        Freedom::Angular => DEGREES,
    };
    Some(match unit {
        Unit::None => Dim::NONE,
        Unit::Fixed(dim) => dim,
        Unit::Joint { linear, angular } => match freedom? {
            Freedom::Linear => linear,
            Freedom::Angular => angular,
        },
        Unit::MimicGearing => {
            let follower = freedom?;
            let axis_name = format!("physxMimicJoint:{}:referenceJointAxis", instance?);
            let reference = crate::schema::token(prim, &axis_name, issues)
                .as_deref()
                .and_then(axis_freedom)
                .unwrap_or(Freedom::Angular);
            of(follower).over(of(reference))
        }
        Unit::Switched { switch, off, on } => {
            if crate::schema::boolean(prim, switch, issues).unwrap_or(false) {
                on
            } else {
                off
            }
        }
        Unit::Mode {
            force,
            acceleration,
        } => match crate::schema::token(prim, "physxForce:mode", issues).as_deref() {
            Some("force") => force,
            _ => acceleration,
        },
    })
}

#[cfg(test)]
mod tests;
