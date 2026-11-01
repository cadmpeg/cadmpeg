#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Simulate the USD physics scenes before and after a cadmpeg round trip.

Each scene in ``crates/cadmpeg-codec-usd/tests/simulation`` states a physical
relation, such as a gear ratio. The check simulates the scene as authored and
as cadmpeg writes it, verifies the relation in both runs, and verifies that
the two runs agree. It is a manual check: it needs a simulator, not CI.

Run it with an interpreter that has the engine installed, on the CPU:

    pip install ovphysx physx-usd-schemas   # --engine physx
    pip install "newton[importers]"         # --engine newton

    python scripts/check-usd-simulation.py --engine physx
    python scripts/check-usd-simulation.py --engine newton --cadmpeg target/release/cadmpeg
"""

import argparse
import math
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCENES = ROOT / "crates/cadmpeg-codec-usd/tests/simulation"
STEP = 1.0 / 240.0
# Relative agreement of a simulated relation with its stated value.
RELATION_TOLERANCE = 0.01
# Agreement of the source run and the round-trip run, in metres and degrees.
RUN_TOLERANCE = 1e-3


def gear(result):
    turned = abs(result["/World/B"]["turned"]), abs(result["/World/A"]["turned"])
    return [] if close(turned[0], 2.0 * turned[1]) else [f"|B| {turned[0]:.4f} is not 2 |A| {turned[1]:.4f}"]


def rack(result):
    turned = abs(result["/World/Pinion"]["turned"])
    travel = abs(result["/World/Rack"]["position"][0])
    expected = 36.0 * travel
    return [] if close(turned, expected) else [f"pinion {turned:.4f} deg is not 36 deg/m x {travel:.5f} m"]


def fall(result, seconds=1.0, gravity=9.81):
    position = result["/World/Ball"]["position"]
    steps = round(seconds / STEP)
    # Semi-implicit Euler: the drop after n steps is g dt^2 n (n + 1) / 2.
    drop = gravity * STEP * STEP * steps * (steps + 1) / 2.0
    failures = []
    if not close(position[0], 0.1):
        failures.append(f"x {position[0]:.5f} m is not 0.1 m")
    if not close(1.0 - position[2], drop):
        failures.append(f"drop {1.0 - position[2]:.5f} m is not {drop:.5f} m")
    return failures


def mimic(result):
    follower, leader = result["/World/Follower"]["turned"], result["/World/Leader"]["turned"]
    return [] if close(follower, 2.0 * leader) else [f"follower {follower:.4f} is not 2 x leader {leader:.4f}"]


# (scene, seconds, bodies, relation, engines).
CASES = [
    ("gear.usda", 1.0, ["/World/A", "/World/B"], gear, {"physx"}),
    ("rack.usda", 1.0, ["/World/Pinion", "/World/Rack"], rack, {"physx"}),
    ("rack_cm.usda", 1.0, ["/World/Pinion", "/World/Rack"], rack, {"physx"}),
    ("fall_cm.usda", 1.0, ["/World/Ball"], fall, {"physx", "newton"}),
    ("mimic.usda", 1.0, ["/World/Leader", "/World/Follower"], mimic, {"physx", "newton"}),
]


def close(found, expected):
    return abs(found - expected) <= RELATION_TOLERANCE * max(abs(expected), 1e-6)


def meters_per_unit(path):
    match = re.search(r"metersPerUnit\s*=\s*([0-9.eE+-]+)", pathlib.Path(path).read_text())
    return float(match.group(1)) if match else 0.01


def z_angle(qz, qw):
    return math.degrees(2.0 * math.atan2(qz, qw))


class Tracker:
    """Accumulates the rotation about z of each body across steps."""

    def __init__(self, count):
        self.previous = [None] * count
        self.turned = [0.0] * count

    def update(self, index, angle):
        if self.previous[index] is not None:
            self.turned[index] += (angle - self.previous[index] + 180.0) % 360.0 - 180.0
        self.previous[index] = angle


def simulate_physx(path, seconds, bodies):
    import warnings

    import numpy as np
    import ovstage
    from ovphysx import PhysX
    from ovphysx.samples.python_samples.schema_support import physx_schema_paths
    from ovphysx.types import TensorType

    if not getattr(simulate_physx, "registered", False):
        ovstage.population.register_usd_schemas([str(p) for p in physx_schema_paths()])
        PhysX.set_cpu_mode(True)
        simulate_physx.registered = True
    physx = PhysX()
    stage = ovstage.Stage("check-usd-simulation")
    try:
        ovstage.population.open_usd(stage, str(path), ordinal=1, domains=ovstage.PopulationDomain.ALL)
        stage.advance_write_floor(ordinal=1).wait()
        physx.wait_task(physx.attach_ovstage(stage, read_ordinal=1))
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", DeprecationWarning)
            binding = physx.create_tensor_binding(prim_paths=bodies, tensor_type=TensorType.RIGID_BODY_POSE)
        poses = np.zeros(binding.shape, dtype=np.float32)
        tracker = Tracker(len(bodies))
        for _ in range(round(seconds / STEP)):
            physx.step_sync(STEP)
            binding.read(poses)
            for index, pose in enumerate(poses):
                tracker.update(index, z_angle(pose[5], pose[6]))
        order = list(binding.prim_paths)
        result = {
            name: {"position": [float(v) for v in poses[order.index(name)][:3]], "turned": tracker.turned[order.index(name)]}
            for name in bodies
        }
        binding.destroy()
        physx.wait_task(physx.detach_ovstage())
        return result
    finally:
        stage.destroy()
        physx.destroy()


def simulate_newton(path, seconds, bodies):
    import warp as wp

    wp.config.quiet = True
    import newton

    builder = newton.ModelBuilder()
    builder.add_usd(str(path))
    model = builder.finalize(device="cpu")
    solver = newton.solvers.SolverXPBD(model)
    state, next_state, control = model.state(), model.state(), model.control()
    labels = list(model.body_label)
    tracker = Tracker(len(labels))
    for _ in range(round(seconds / STEP)):
        state.clear_forces()
        solver.step(state, next_state, control, None, STEP)
        state, next_state = next_state, state
        for index, pose in enumerate(state.body_q.numpy()):
            tracker.update(index, z_angle(pose[5], pose[6]))
    poses = state.body_q.numpy()
    return {
        name: {"position": [float(v) for v in poses[labels.index(name)][:3]], "turned": tracker.turned[labels.index(name)]}
        for name in bodies
    }


def in_metres(result, path):
    scale = meters_per_unit(path)
    return {
        name: {"position": [value * scale for value in body["position"]], "turned": body["turned"]}
        for name, body in result.items()
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--engine", choices=["physx", "newton"], required=True)
    parser.add_argument("--cadmpeg", default=str(ROOT / "target/release/cadmpeg"))
    args = parser.parse_args()
    simulate = simulate_physx if args.engine == "physx" else simulate_newton
    failures = 0
    with tempfile.TemporaryDirectory() as scratch:
        for scene, seconds, bodies, relation, engines in CASES:
            if args.engine not in engines:
                continue
            source = SCENES / scene
            written = pathlib.Path(scratch) / scene
            subprocess.run(
                [args.cadmpeg, "convert", str(source), "--force", "--allow-empty", "-o", str(written)],
                check=True,
                capture_output=True,
            )
            runs = {"round trip": in_metres(simulate(written, seconds, bodies), written)}
            # Newton reads lengths as metres and does not scale other units.
            if args.engine == "physx" or meters_per_unit(source) == 1.0:
                runs["source"] = in_metres(simulate(source, seconds, bodies), source)
            problems = [f"{run}: {problem}" for run, result in runs.items() for problem in relation(result)]
            if len(runs) == 2:
                for name in bodies:
                    first, second = runs["source"][name], runs["round trip"][name]
                    deltas = [abs(a - b) for a, b in zip(first["position"], second["position"])]
                    deltas.append(abs(first["turned"] - second["turned"]))
                    if max(deltas) > RUN_TOLERANCE:
                        problems.append(f"{name}: the round trip differs from the source by {max(deltas):.2e}")
            status = "ok" if not problems else "FAILED"
            print(f"{args.engine} {scene}: {status} ({', '.join(sorted(runs))})")
            for problem in problems:
                print(f"  {problem}")
            failures += bool(problems)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
