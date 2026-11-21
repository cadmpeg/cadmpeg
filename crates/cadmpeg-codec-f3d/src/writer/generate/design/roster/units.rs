// SPDX-License-Identifier: Apache-2.0
//! Records 47 to 160 of a Base Feature design: the six unit systems and
//! their members (f3d.md §3.1, "Unit systems").

use cadmpeg_core::CodecError;

use super::super::frames::DesignRecords;
use super::fixed::UNIT_SYSTEMS;
use super::{count, Context, UNIT_ROOT};

/// One unit system: name, description, modelling length and mass units,
/// and its unit types.
struct UnitGroup {
    name: &'static str,
    description: &'static str,
    length: &'static str,
    mass: &'static str,
    types: &'static [(u32, &'static str, &'static str)],
}

/// The unit types of a metric group: code, type name and unit.
const METRIC: [(u32, &str, &str); 17] = [
    (0, "Length", "meter"),
    (1, "Mass", "kilogram"),
    (2, "Time", "second"),
    (3, "Temperature", "celsius"),
    (10, "Speed", "MetersPerSecond"),
    (12, "Volume", "liter"),
    (13, "Pressure", "pascal"),
    (14, "Force", "newton"),
    (15, "Power", "watt"),
    (16, "Energy", "joule"),
    (4, "Current", "ampere"),
    (5, "Substance", "Mole"),
    (6, "Luminosity", "candela"),
    (7, "Angle", "degree"),
    (8, "Currency", "dollar"),
    (31, "Percentage", "percent"),
    (33, "Pieces", "pieces"),
];

/// The unit types of an imperial group.
const IMPERIAL: [(u32, &str, &str); 14] = [
    (0, "Length", "foot"),
    (1, "Mass", "lbmass"),
    (2, "Time", "second"),
    (3, "Temperature", "farenheit"),
    (10, "Speed", "FeetPerSecond"),
    (13, "Pressure", "PoundsPerSquareInch"),
    (14, "Force", "lbforce"),
    (4, "Current", "ampere"),
    (5, "Substance", "Mole"),
    (6, "Luminosity", "candela"),
    (7, "Angle", "degree"),
    (8, "Currency", "dollar"),
    (31, "Percentage", "percent"),
    (33, "Pieces", "pieces"),
];

/// The groups in record order.
const GROUPS: [UnitGroup; 6] = [
    UnitGroup {
        name: "CmMKS",
        description: "cm modeling length with MKS (m, kg, second, Celsius) units",
        length: "centimeter",
        mass: "gram",
        types: &METRIC,
    },
    UnitGroup {
        name: "MmMKS",
        description: "mm modeling length with MKS (m, kg, second, Celsius) units",
        length: "millimeter",
        mass: "gram",
        types: &METRIC,
    },
    UnitGroup {
        name: "MMKS",
        description: "standard MKS (m, kg, second, Celsius) units",
        length: "meter",
        mass: "kilogram",
        types: &METRIC,
    },
    UnitGroup {
        name: "InchImperial",
        description: "inch modeling length with imperial (foot, lbmass, second, Fahrenheit) units",
        length: "inch",
        mass: "ouncemass",
        types: &IMPERIAL,
    },
    UnitGroup {
        name: "Imperial",
        description: "Imperial (foot, lbmass, second, Fahrenheit) units",
        length: "foot",
        mass: "lbmass",
        types: &IMPERIAL,
    },
    UnitGroup {
        name: "Custom",
        description: "Custom units",
        length: "millimeter",
        mass: "gram",
        types: &METRIC,
    },
];

/// The common data namespace of the member records.
const NAMESPACE: &str = "NsCommonData";

pub(super) fn write(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    for (group, system) in GROUPS.iter().zip(UNIT_SYSTEMS) {
        let members = (system + 1..)
            .take(2 + group.types.len())
            .collect::<Vec<_>>();
        out.record(&ctx.class_of(system)?, system)
            .zeros(10)
            .lp_ascii(group.name)?
            .lp_ascii(group.description)?
            .byte(1)
            .lp_ascii(&format!("{}UnitSystemName", group.name))?
            .lp_ascii("NaFusion")?
            .u32(0)
            .u32(count(members.len())?);
        for member in &members {
            out.reference(*member);
        }
        out.reference(UNIT_ROOT);
        for (member, (name, label, key, unit)) in members.iter().zip([
            (
                "ModelingLength",
                "Modeling Length",
                "modelingLengthName",
                group.length,
            ),
            (
                "ModelingMass",
                "Modeling Mass",
                "modelingMassName",
                group.mass,
            ),
        ]) {
            out.record(&ctx.class_of(*member)?, *member)
                .zeros(10)
                .lp_ascii(name)?
                .lp_ascii(label)?
                .byte(1)
                .lp_ascii(key)?
                .lp_ascii(NAMESPACE)?
                .u32(0)
                .lp_utf16(unit)?
                .reference(system);
        }
        for (member, (code, name, unit)) in members.iter().skip(2).zip(group.types) {
            out.record(&ctx.class_of(*member)?, *member)
                .zeros(10)
                .u32(*code)
                .lp_ascii(name)?
                .byte(1)
                .lp_ascii(&format!("UnitType{name}"))?
                .lp_ascii(NAMESPACE)?
                .u32(0)
                .lp_utf16(unit)?
                .reference(system);
        }
    }
    Ok(())
}
