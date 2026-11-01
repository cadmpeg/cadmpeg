#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Generate the PhysX attribute table of the USD codec.

Reads the PhysxSchema and PhysxSchemaAddition ``generatedSchema.usda`` files
of a PhysX schema release that states attribute units, and writes
``crates/cadmpeg-codec-usd/src/schema/physx/table.rs``.

    python3 scripts/generate-usd-physx-table.py PHYSX_SCHEMA.usda ADDITION.usda
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "crates/cadmpeg-codec-usd/src/schema/physx/table.rs"

# The schemas that the codec carries, with the kind of prim each applies to.
HOLDERS = {
    "PhysxSceneAPI": "Scene",
    "PhysxSceneQuasistaticAPI": "Scene",
    "PhysxRigidBodyAPI": "Body",
    "PhysxContactReportAPI": "Body",
    "PhysxSurfaceVelocityAPI": "Body",
    "PhysxForceAPI": "Body",
    "PhysxArticulationAPI": "Articulation",
    "PhysxTendonAxisAPI": "Joint",
    "PhysxTendonAxisRootAPI": "Joint",
    "PhysxTendonAttachmentAPI": "Body",
    "PhysxTendonAttachmentRootAPI": "Body",
    "PhysxTendonAttachmentLeafAPI": "Body",
    "PhysxCollisionAPI": "Collision",
    "PhysxConvexHullCollisionAPI": "Collision",
    "PhysxConvexDecompositionCollisionAPI": "Collision",
    "PhysxSphereFillCollisionAPI": "Collision",
    "PhysxTriangleMeshSimplificationCollisionAPI": "Collision",
    "PhysxTriangleMeshCollisionAPI": "Collision",
    "PhysxSDFMeshCollisionAPI": "Collision",
    "PhysxMaterialAPI": "Material",
    "PhysxJointAPI": "Joint",
    "PhysxPhysicsDistanceJointAPI": "Joint",
    "PhysxLimitAPI": "Joint",
    "PhysicsJointStateAPI": "Joint",
    "PhysxMimicJointAPI": "Joint",
    "PhysxJointAxisAPI": "Joint",
    "PhysxDrivePerformanceEnvelopeAPI": "Joint",
}

# (length, mass, angle) exponents and whether the angle unit is degrees.
NONE = (0, 0, 0, False)
DISTANCE = (1, 0, 0, False)
MASS = (0, 1, 0, False)
FORCE = (1, 1, 0, False)
TORQUE = (2, 1, 0, False)
DEGREES = (0, 0, 1, True)

UNITS = {
    "distance": DISTANCE,
    "distance (local-frame)": DISTANCE,
    "dimensionless": NONE,
    "dimensionless.": NONE,
    "unitless": NONE,
    "distance / seconds": DISTANCE,
    "distance/second.": DISTANCE,
    "distance/second": DISTANCE,
    "mass * distance * distance / seconds / seconds": TORQUE,
    "force / distance": MASS,
    "torque * seconds": TORQUE,
    "distance * distance / seconds / seconds": (2, 0, 0, False),
    "seconds": NONE,
    "radians / seconds": (0, 0, 1, False),
    "mass * distance * distance": TORQUE,
    "force * seconds / distance": MASS,
    "force / distance * time": MASS,
    "force": FORCE,
    "s^-1": NONE,
    "mass * distance / seconds / seconds / radians": (1, 1, -1, False),
    "mass * distance / seconds / seconds": FORCE,
    "mass * distance / second / second": FORCE,
    "mass * distance * distance / second / second": TORQUE,
    "mass / distance / distance / distance": (-3, 1, 0, False),
    "mass": MASS,
    "force * seconds": FORCE,
    "energy": TORQUE,
    "degrees / seconds": DEGREES,
    "degrees/second": DEGREES,
    "degrees/second.": DEGREES,
    "degrees / distance": (-1, 0, 1, True),
    "1 / (distance * distance * distance)": (-3, 0, 0, False),
}

# Attributes whose unit depends on the joint degree of freedom:
# (linear, angular).
JOINT = {
    "physxJoint:armature": (MASS, TORQUE),
    "physxJoint:maxJointVelocity": (DISTANCE, DEGREES),
    "physxJointAxis:{}:armature": (MASS, TORQUE),
    "physxJointAxis:{}:maxJointVelocity": (DISTANCE, DEGREES),
    "physxJointAxis:{}:staticFrictionEffort": (FORCE, TORQUE),
    "physxJointAxis:{}:dynamicFrictionEffort": (FORCE, TORQUE),
    "physxJointAxis:{}:viscousFrictionCoefficient": (MASS, (2, 1, -1, True)),
    "physxDrivePerformanceEnvelope:{}:maxActuatorVelocity": (DISTANCE, DEGREES),
    "physxDrivePerformanceEnvelope:{}:speedEffortGradient": ((0, -1, 0, False), (-2, -1, 1, True)),
    "physxDrivePerformanceEnvelope:{}:velocityDependentResistance": (MASS, (2, 1, -1, True)),
    "physxLimit:{}:stiffness": (MASS, (2, 1, -1, True)),
    "physxLimit:{}:damping": (MASS, (2, 1, -1, True)),
    "physxLimit:{}:bounceThreshold": (DISTANCE, DEGREES),
    "physxMimicJoint:{}:offset": (DISTANCE, DEGREES),
    "state:{}:physics:position": (DISTANCE, DEGREES),
    "state:{}:physics:velocity": (DISTANCE, DEGREES),
}

# Attributes whose unit depends on another attribute of the schema:
# (unit when the switch is off, unit when it is on).
SWITCHED = {
    "physxMaterial:compliantContactStiffness": ("physxMaterial:compliantContactAccelerationSpring", MASS, NONE),
    "physxMaterial:compliantContactDamping": ("physxMaterial:compliantContactAccelerationSpring", MASS, NONE),
}

# Force-API vectors, by `physxForce:mode`: (force, acceleration).
MODE = {
    "physxForce:force": (FORCE, DISTANCE),
    "physxForce:torque": (TORQUE, DEGREES),
}

# Attributes that the schema states no unit for.
FIXED = {
    "physxConvexDecompositionCollision:errorPercentage": NONE,
    "physxTriangleMeshSimplificationCollision:metric": NONE,
    "physxTendon:{}:forceCoefficient": NONE,
    "physxTendon:{}:gearing": NONE,
    "physxLimit:{}:restitution": NONE,
    "physxMimicJoint:{}:dampingRatio": NONE,
}

TYPES = {
    "bool": "Bool",
    "int": "Integer",
    "uint": "Integer",
    "uint64": "Integer",
    "float": "Real",
    "double": "Real",
    "token": "Token",
    "token[]": "Tokens",
    "float[]": "Reals",
    "vector3f": "Reals",
    "float3": "Reals",
    "point3f": "Reals",
    "rel": "Targets",
}


def attributes(path):
    text = pathlib.Path(path).read_text()
    for match in re.finditer(r'^class (?:\w+ )?"(\w+)" \((.*?)\n\)\n\{(.*?)\n\}\n', text, re.S | re.M):
        schema, _meta, body = match.groups()
        if schema not in HOLDERS:
            continue
        lines = body.split("\n")
        index = 0
        while index < len(lines):
            line = lines[index]
            relationship = re.match(r"^    (?:uniform )?rel ([\w:]+)", line)
            attribute = re.match(r"^    (uniform )?(\S+) ([\w:]+)(?: = (.*?))?( \()?$", line)
            if relationship:
                yield schema, "rel", relationship.group(1), False, None
            elif attribute:
                uniform, kind, name, opened = attribute.group(1, 2, 3, 5)
                info = ""
                if opened:
                    end = index + 1
                    while end < len(lines) and not re.match(r"^    \)$", lines[end]):
                        end += 1
                    info = "\n".join(lines[index + 1 : end])
                    index = end
                units = re.search(r'Units: ([^\n"\\]*)', info)
                yield schema, kind, name, bool(uniform), units.group(1).strip() if units else None
            index += 1


def dimension(unit):
    length, mass, angle, degrees = unit
    return f"Dim::new({length}, {mass}, {angle}, {'true' if degrees else 'false'})"


def unit_text(name, kind, units):
    if TYPES[kind] not in ("Real", "Reals"):
        return "Unit::None"
    if name in JOINT:
        linear, angular = JOINT[name]
        return f"Unit::Joint {{ linear: {dimension(linear)}, angular: {dimension(angular)} }}"
    if name in SWITCHED:
        switch, off, on = SWITCHED[name]
        return f'Unit::Switched {{ switch: "{switch}", off: {dimension(off)}, on: {dimension(on)} }}'
    if name in MODE:
        force, acceleration = MODE[name]
        return f"Unit::Mode {{ force: {dimension(force)}, acceleration: {dimension(acceleration)} }}"
    if name == "physxMimicJoint:{}:gearing":
        return "Unit::MimicGearing"
    if name in FIXED:
        return f"Unit::Fixed({dimension(FIXED[name])})"
    units = (units or "").split(" = ")[0].strip()
    if units in UNITS:
        return f"Unit::Fixed({dimension(UNITS[units])})"
    sys.exit(f"no unit for {name}: {units!r}")


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    rows = []
    seen = set()
    for path in sys.argv[1:]:
        for schema, kind, name, uniform, units in attributes(path):
            name = name.replace("__INSTANCE_NAME__", "{}")
            if (schema, name) in seen or kind not in TYPES:
                continue
            if not name.startswith(("physx", "state:")):
                continue
            seen.add((schema, name))
            rows.append(
                f'    Attribute {{ schema: "{schema}", name: "{name}", usd_type: "{kind}", '
                f'uniform: {"true" if uniform else "false"}, value: Value::{TYPES[kind]}, '
                f"unit: {unit_text(name, kind, units)} }},"
            )
    missing = sorted(set(HOLDERS) - {row.split('"')[1] for row in rows})
    if missing:
        sys.exit(f"schemas with no attribute: {missing}")
    holders = "\n".join(
        f'    ("{schema}", Holder::{holder}),' for schema, holder in sorted(HOLDERS.items())
    )
    OUTPUT.write_text(
        "// SPDX-License-Identifier: Apache-2.0\n"
        "//! The `PhysX` attributes that the codec carries (spec §16.8). Generated by\n"
        "//! `scripts/generate-usd-physx-table.py`; do not edit.\n\n"
        "use super::{Attribute, Dim, Holder, Unit, Value};\n\n"
        "/// The carried schemas and the kind of prim that each applies to.\n"
        f"pub(super) const HOLDERS: &[(&str, Holder)] = &[\n{holders}\n];\n\n"
        "/// The attributes of the carried schemas. `{}` is the instance name of a\n"
        "/// schema that applies more than once.\n"
        "pub(super) const ATTRIBUTES: &[Attribute] = &[\n" + "\n".join(rows) + "\n];\n"
    )
    subprocess.run(["rustfmt", "--edition", "2021", str(OUTPUT)], check=True)
    print(f"wrote {OUTPUT.relative_to(ROOT)}: {len(rows)} attributes")


if __name__ == "__main__":
    main()
