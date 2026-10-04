#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run a Cargo gate with CI's selected packages without shell interpolation."""

import json
import os
import subprocess
import sys


def command(arguments: list[str], packages: list[str]) -> list[str]:
    if not arguments or not packages:
        raise ValueError('Cargo gate requires a subcommand and selected packages')
    return ['cargo', arguments[0], '-q',
            *[argument for package in packages for argument in ('-p', package)],
            *arguments[1:]]


def main() -> int:
    return subprocess.run(command(sys.argv[1:], json.loads(os.environ['CI_PACKAGES']))).returncode


if __name__ == '__main__':
    raise SystemExit(main())
