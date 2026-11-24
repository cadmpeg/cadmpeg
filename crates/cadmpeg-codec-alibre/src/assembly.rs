// SPDX-License-Identifier: Apache-2.0
//! The assembly records of the `STEP_DATA` stream of an `.AD_ASM` file.
//!
//! The stream is Part 21 text in the Alibre schema. The `DESIGNASSEMBLY` entity
//! lists the `DESIGNINCLUSION` entities, one per placed occurrence, and the
//! `DESIGNASSEMBLYCONFIGURATION` entities, whose lists of suppressed inclusions
//! and hidden members state the display state of each occurrence. An inclusion
//! names its component file by a path without extension, references the
//! `REPOSITORYVERSIONREFERENCE` that holds the document GUID, and places the
//! component with a `CARTESIAN_TRANSFORMATION_OPERATOR_3D`. Lengths are in
//! centimetres, the API unit of Alibre.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use crate::part21::{Instance, Part21, Value};

/// Millimetres per length unit of the stream.
pub(crate) const MM_PER_UNIT: f64 = 10.0;

/// The name of the configuration of a component that Alibre gives a new
/// document, which every specimen inclusion selects.
pub(crate) const DEFAULT_CONFIGURATION: &str = "Config<1>";

/// The attribute indices of the `DESIGNASSEMBLY` entity that the reader uses.
const ASSEMBLY_PRODUCT: usize = 18;
const ASSEMBLY_INCLUSIONS: usize = 20;
const ASSEMBLY_RELATION_LISTS: [usize; 3] = [21, 22, 23];
const ASSEMBLY_CONFIGURATIONS: usize = 25;
const ASSEMBLY_ACTIVE: usize = 26;

/// The attribute count of the `DESIGNINCLUSION` entity that the reader maps.
/// Attributes past it are reported as unmapped.
const INCLUSION_ARITY: usize = 15;
/// Attributes whose non-default value the occurrence does not carry, with the
/// value each holds in every specimen.
const INCLUSION_UNMAPPED: [(usize, Usual); 8] = [
    (0, Usual::EmptyList),
    (4, Usual::Null),
    (6, Usual::Null),
    (8, Usual::Null),
    (10, Usual::False),
    (11, Usual::Reference),
    (12, Usual::Zero),
    (13, Usual::Null),
];

/// The default value of an attribute that the reader does not map.
#[derive(Debug, Clone, Copy)]
enum Usual {
    EmptyList,
    Null,
    False,
    Zero,
    /// A reference, which names a profile record without content here.
    Reference,
}

impl Usual {
    fn holds(self, value: &Value) -> bool {
        match self {
            Self::EmptyList => value.as_list().is_some_and(<[Value]>::is_empty),
            Self::Null => *value == Value::Null,
            Self::False => value.as_logical() == Some(false),
            Self::Zero => value.as_int() == Some(0),
            Self::Reference => value.as_ref().is_some(),
        }
    }
}

/// The entity names of the specimens: the assembly structure, the document
/// profile, the origin datums, and the approval records. An entity of another
/// name is reported as unread.
const KNOWN_ENTITIES: &[&str] = &[
    "ANGLEUV",
    "APPLICATION_CONTEXT",
    "APPLICATION_PROTOCOL_DEFINITION",
    "APPROVAL",
    "APPROVAL_DATE_TIME",
    "APPROVAL_PERSON_ORGANIZATION",
    "APPROVAL_ROLE",
    "APPROVAL_STATUS",
    "AXIS1_PLACEMENT",
    "AXIS2_PLACEMENT_3D",
    "BOOLEANVALUEPAIR",
    "CALENDAR_DATE",
    "CARTESIAN_POINT",
    "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
    "CC_DESIGN_APPROVAL",
    "CC_DESIGN_DATE_AND_TIME_ASSIGNMENT",
    "CC_DESIGN_PERSON_AND_ORGANIZATION_ASSIGNMENT",
    "CC_DESIGN_SECURITY_CLASSIFICATION",
    "COORDINATED_UNIVERSAL_TIME_OFFSET",
    "DATE_AND_TIME",
    "DATE_TIME_ROLE",
    "DESIGNASSEMBLY",
    "DESIGNASSEMBLYCONFIGURATION",
    "DESIGNASSEMBLYSTATECONTEXT",
    "DESIGNAXISOPERATION",
    "DESIGNINCLUSION",
    "DESIGNMEMBER",
    "DESIGNPLANEOPERATION",
    "DESIGNPOINTOPERATION",
    "DESIGNPRIMARYAXIS",
    "DESIGNPRIMARYPLANE",
    "DESIGNPRIMARYPOINT",
    "DESIGNSTATE",
    "DESIGNWORKSPACE",
    "DESIGN_CONTEXT",
    "DIRECTION",
    "FONTDESCRIPTION",
    "FONTDESCRIPTIONPAIR",
    "HATCHPATTERN",
    "HATCHPATTERNPAIR",
    "HATCHSEGMENT",
    "INTEGERVALUEPAIR",
    "LINEPATTERN",
    "LOCAL_TIME",
    "MEASUREMENTFORMAT",
    "MEASUREMENTFORMATPAIR",
    "MEASUREMENTSEGMENTFORMAT",
    "MECHANICAL_CONTEXT",
    "ORGANIZATION",
    "PARAMETERTABLE",
    "PERSON",
    "PERSON_AND_ORGANIZATION",
    "PERSON_AND_ORGANIZATION_ROLE",
    "PLANE",
    "PRODUCT",
    "PRODUCTVERSION",
    "PRODUCT_DEFINITION",
    "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE",
    "PRODUCT_RELATED_PRODUCT_CATEGORY",
    "PROFILE",
    "REALVALUEPAIR",
    "REPOSITORYVERSIONREFERENCE",
    "SCHEMAVERSION",
    "SECURITY_CLASSIFICATION",
    "SECURITY_CLASSIFICATION_LEVEL",
    "STRINGVALUEPAIR",
    "VECTOR",
];

/// The placement of one inclusion: a transformation operator.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placement {
    /// The local origin, in millimetres.
    pub(crate) origin: [f64; 3],
    /// The images of the x, y, and z axes.
    pub(crate) axes: [[f64; 3]; 3],
    /// The uniform scale.
    pub(crate) scale: f64,
}

/// One `DESIGNMEMBER` of an inclusion.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Member {
    pub(crate) id: u32,
    /// The member's own hidden flag.
    pub(crate) hidden: bool,
    /// Whether the member names a component of a subassembly and not the
    /// inclusion itself.
    pub(crate) nested: bool,
}

/// One `DESIGNINCLUSION`: an occurrence of a component file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Inclusion {
    pub(crate) id: u32,
    /// The component path as stored, without extension.
    pub(crate) path: String,
    /// The GUID of the component document.
    pub(crate) document_id: Option<String>,
    pub(crate) placement: Option<Placement>,
    /// Whether the occurrence is anchored.
    pub(crate) anchored: bool,
    /// The configuration of the component that the occurrence uses.
    pub(crate) configuration: Option<String>,
    pub(crate) members: Vec<Member>,
    /// The members of the list that do not read as a `DESIGNMEMBER`.
    pub(crate) unreadable_members: usize,
    /// The indices of the attributes that state a non-default value that the
    /// occurrence does not carry, and of the attributes past the schema.
    pub(crate) unmapped: Vec<usize>,
}

/// One `DESIGNASSEMBLYCONFIGURATION`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Configuration {
    pub(crate) name: String,
    /// The suppressed inclusions.
    pub(crate) suppressed: BTreeSet<u32>,
    /// The hidden members.
    pub(crate) hidden: BTreeSet<u32>,
}

/// The assembly records of one `STEP_DATA` stream.
#[derive(Debug, Default)]
pub(crate) struct Assembly {
    /// The product name.
    pub(crate) name: Option<String>,
    /// The Alibre product version of the stream.
    pub(crate) product_version: Option<String>,
    pub(crate) inclusions: Vec<Inclusion>,
    pub(crate) configurations: Vec<Configuration>,
    /// The index of the active configuration.
    pub(crate) active: Option<usize>,
    /// The instances of each entity name that the reader does not know.
    pub(crate) unread: BTreeMap<String, usize>,
    /// The references in the relation lists of the assembly entity.
    pub(crate) relation_references: usize,
    /// The records that the reader needs and could not read, one text each.
    pub(crate) unreadable: Vec<String>,
}

impl Assembly {
    /// The configuration whose state the base occurrence states.
    pub(crate) fn active_configuration(&self) -> Option<&Configuration> {
        self.configurations.get(self.active?)
    }
}

fn vector(part: &Part21, id: u32, entity: &str) -> Option<[f64; 3]> {
    let values = part
        .of(id, entity)?
        .arg(1)?
        .as_list()?
        .iter()
        .map(Value::as_real)
        .collect::<Option<Vec<f64>>>()?;
    match values.as_slice() {
        [x, y, z] => Some([*x, *y, *z]),
        _ => None,
    }
}

fn placement(part: &Part21, id: u32) -> Option<Placement> {
    let operator = part.of(id, "CARTESIAN_TRANSFORMATION_OPERATOR_3D")?;
    // The name and the description come first, then the two axes, the local
    // origin, the scale, and the third axis.
    let axis = |index: usize| vector(part, operator.arg(index)?.as_ref()?, "DIRECTION");
    let origin = vector(part, operator.arg(5)?.as_ref()?, "CARTESIAN_POINT")?;
    let scale = operator.arg(6)?.as_real()?;
    let placement = Placement {
        origin: origin.map(|value| value * MM_PER_UNIT),
        axes: [axis(3)?, axis(4)?, axis(7)?],
        scale,
    };
    let finite = placement
        .origin
        .iter()
        .chain(placement.axes.iter().flatten())
        .chain(std::iter::once(&placement.scale))
        .all(|value| value.is_finite());
    finite.then_some(placement)
}

fn member(part: &Part21, id: u32) -> Option<Member> {
    let record = part.of(id, "DESIGNMEMBER")?;
    Some(Member {
        id,
        hidden: record.arg(3)?.as_logical()?,
        nested: !matches!(record.arg(6)?, Value::Null),
    })
}

fn inclusion(part: &Part21, record: &Instance) -> Option<Inclusion> {
    if record.args.len() < INCLUSION_ARITY {
        return None;
    }
    let reference = record
        .arg(3)
        .and_then(Value::as_ref)
        .and_then(|id| part.of(id, "REPOSITORYVERSIONREFERENCE"));
    let member_ids = record.arg(1)?.refs()?;
    let members = member_ids
        .iter()
        .filter_map(|id| member(part, *id))
        .collect::<Vec<_>>();
    let unreadable_members = member_ids.len() - members.len();
    let unmapped = INCLUSION_UNMAPPED
        .iter()
        .filter(|(index, default)| {
            record
                .arg(*index)
                .is_some_and(|value| !default.holds(value))
        })
        .map(|(index, _)| *index)
        .chain(INCLUSION_ARITY..record.args.len())
        .collect();
    Some(Inclusion {
        id: record.id,
        path: record.arg(2)?.as_str()?.to_owned(),
        document_id: reference
            .and_then(|reference| reference.arg(3))
            .and_then(Value::as_str)
            .filter(|guid| !guid.is_empty())
            .map(str::to_owned),
        placement: record
            .arg(5)
            .and_then(Value::as_ref)
            .and_then(|id| placement(part, id)),
        anchored: record.arg(7)?.as_logical()?,
        configuration: record
            .arg(14)
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        members,
        unreadable_members,
        unmapped,
    })
}

fn configuration(part: &Part21, id: u32) -> Option<Configuration> {
    let record = part.of(id, "DESIGNASSEMBLYCONFIGURATION")?;
    let set = |index: usize| -> Option<BTreeSet<u32>> {
        Some(record.arg(index)?.refs()?.into_iter().collect())
    };
    Some(Configuration {
        name: record.arg(0)?.as_str()?.to_owned(),
        suppressed: set(8)?,
        hidden: set(9)?,
    })
}

/// Reads the assembly records of a parsed `STEP_DATA` stream.
pub(crate) fn read(part: &Part21) -> Result<Assembly, CodecError> {
    let mut assemblies = part
        .instances
        .values()
        .filter(|instance| instance.name == "DESIGNASSEMBLY");
    let (Some(root), None) = (assemblies.next(), assemblies.next()) else {
        return Err(CodecError::malformed(
            "STEP_DATA holds no single DESIGNASSEMBLY entity",
        ));
    };
    let list = |index: usize| -> Result<Vec<u32>, CodecError> {
        root.arg(index)
            .and_then(Value::refs)
            .ok_or_else(|| CodecError::malformed("a DESIGNASSEMBLY list holds a non-reference"))
    };
    let mut assembly = Assembly {
        name: root
            .arg(ASSEMBLY_PRODUCT)
            .and_then(Value::as_ref)
            .and_then(|id| part.of(id, "PRODUCT"))
            .and_then(|product| product.arg(1))
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        product_version: part
            .instances
            .values()
            .find(|instance| instance.name == "SCHEMAVERSION")
            .and_then(|version| version.arg(0))
            .and_then(Value::as_str)
            .map(str::to_owned),
        ..Assembly::default()
    };
    for id in list(ASSEMBLY_INCLUSIONS)? {
        match part
            .of(id, "DESIGNINCLUSION")
            .and_then(|record| inclusion(part, record))
        {
            Some(inclusion) => assembly.inclusions.push(inclusion),
            None => assembly.unreadable.push(format!(
                "#{id} is not a DESIGNINCLUSION of the Alibre schema; its occurrence is left out"
            )),
        }
    }
    let mut read_ids = Vec::new();
    match root.arg(ASSEMBLY_CONFIGURATIONS).and_then(Value::refs) {
        Some(ids) => {
            for id in ids {
                match configuration(part, id) {
                    Some(configuration) => {
                        read_ids.push(id);
                        assembly.configurations.push(configuration);
                    }
                    None => assembly.unreadable.push(format!(
                        "#{id} is not a DESIGNASSEMBLYCONFIGURATION of the Alibre schema"
                    )),
                }
            }
        }
        None => assembly
            .unreadable
            .push("the DESIGNASSEMBLY entity has no list of configurations".to_owned()),
    }
    assembly.active = root
        .arg(ASSEMBLY_ACTIVE)
        .and_then(Value::as_ref)
        .and_then(|active| read_ids.iter().position(|id| *id == active));
    if assembly.active.is_none() {
        assembly.unreadable.push(
            "the active configuration of the assembly is not one of its readable configurations; \
             the occurrences carry no suppressed state"
                .to_owned(),
        );
    }
    for inclusion in &assembly.inclusions {
        if inclusion.unreadable_members > 0 {
            assembly.unreadable.push(format!(
                "inclusion #{} lists {} member(s) that are not a DESIGNMEMBER of the Alibre schema",
                inclusion.id, inclusion.unreadable_members
            ));
        }
    }
    for index in ASSEMBLY_RELATION_LISTS {
        assembly.relation_references += root
            .arg(index)
            .and_then(Value::as_list)
            .map_or(0, <[Value]>::len);
    }
    for instance in part.instances.values() {
        if KNOWN_ENTITIES
            .binary_search(&instance.name.as_str())
            .is_err()
        {
            *assembly.unread.entry(instance.name.clone()).or_default() += 1;
        }
    }
    Ok(assembly)
}

#[cfg(test)]
mod tests {
    use super::KNOWN_ENTITIES;

    #[test]
    fn the_known_entity_names_are_sorted_for_the_search() {
        assert!(KNOWN_ENTITIES.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
