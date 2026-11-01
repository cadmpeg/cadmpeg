#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run a Cargo gate with CI's selected packages without shell interpolation."""

import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path


def command(arguments: list[str], packages: list[str], *, full: bool = False,
            codecs: list[str] | None = None) -> list[str]:
    if not arguments or (not full and not packages):
        raise ValueError('Cargo gate requires a subcommand and selected packages')
    scope = ['--workspace'] if full else [argument for package in packages
                                         for argument in ('-p', package)]
    if codecs and not full:
        registry = Path(__file__).resolve().parent.parent / 'crates/cadmpeg-registry/Cargo.toml'
        features = tomllib.loads(registry.read_text())['features']
        codec_features = {
            dependency.removeprefix('dep:cadmpeg-codec-'): feature
            for feature, dependencies in features.items()
            for dependency in dependencies
            if dependency.startswith('dep:cadmpeg-codec-')
        }
        scope += ['--no-default-features', '--features', ','.join(
            f'cadmpeg/{codec_features[codec]}' for codec in codecs)]
    if arguments[0] == 'nextest':
        return ['cargo', 'nextest', *arguments[1:2], '--cargo-quiet', '--status-level', 'fail',
                *scope, *arguments[2:]]
    return ['cargo', arguments[0], '-q', *scope, *arguments[1:]]


def main() -> int:
    arguments = sys.argv[1:]
    packages = json.loads(os.environ['CI_PACKAGES'])
    full = os.environ.get('CI_FULL') == 'true'
    # The CLI has no library target. Cargo rejects a CLI-only doc gate, but
    # accepts it alongside libraries and preserves their CLI-enabled features.
    if not full and arguments[:1] == ['test'] and '--doc' in arguments and packages == ['cadmpeg']:
        print('No selected package has library documentation tests.')
        return 0
    codecs = json.loads(os.environ.get('CI_CODEC_FEATURES', '[]'))
    return subprocess.run(command(arguments, packages, full=full, codecs=codecs)).returncode


if __name__ == '__main__':
    raise SystemExit(main())
