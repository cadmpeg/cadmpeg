#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Exercise the base build contract with real Git worktrees and a fake Cargo."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("base-build.sh")


class BaseBuildTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="cadmpeg-base-build-")
        self.folder = Path(self.temporary.name)
        self.repo = self.folder / "repo with spaces"
        self.repo.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "pcurve")
        self.git("config", "user.email", "301495646+pcurve@users.noreply.github.com")
        (self.repo / "scripts").mkdir()
        shutil.copy2(SCRIPT, self.repo / "scripts/base-build.sh")
        (self.repo / "Cargo.toml").write_text("[workspace]\nmembers=[]\n")
        (self.repo / ".gitignore").write_text("/target/\n")
        self.commit("common ancestor")
        self.base = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("checkout", "-q", "-b", "remote-main")
        (self.repo / "main-only").write_text("main side")
        self.commit("main advances")
        self.main = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("update-ref", "refs/remotes/origin/main", self.main)
        self.git("checkout", "-q", "-b", "feature", self.base)
        (self.repo / "feature-only").write_text("feature side")
        self.commit("feature advances")
        self.head = self.git("rev-parse", "HEAD").stdout.strip()
        self.fake_dir = self.folder / "fake-bin"
        self.fake_dir.mkdir()
        self.log = self.folder / "cargo.jsonl"
        self.github_env = self.folder / "github-env"
        fake = self.fake_dir / "cargo"
        fake.write_text("""#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys
sha = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
with open(os.environ['FAKE_CARGO_LOG'], 'a') as log:
    log.write(json.dumps({'cwd': os.getcwd(), 'sha': sha, 'argv': sys.argv[1:], 'target': os.environ['CARGO_TARGET_DIR']}) + '\\n')
if os.environ.get('FAKE_CARGO_FAILURE'):
    sys.exit(7)
out = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / 'release' / 'cadmpeg'
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text('#!/bin/sh\\nprintf "base:' + sha + '\\\\n"\\n')
out.chmod(0o755)
""")
        fake.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.fake_dir) + os.pathsep + os.environ["PATH"], FAKE_CARGO_LOG=str(self.log), GITHUB_ENV=str(self.github_env), CARGO_TARGET_DIR=str(self.folder / "head-target"))

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def git(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["git", *args], cwd=self.repo, text=True, capture_output=True, check=True)

    def commit(self, message: str) -> None:
        self.git("add", ".")
        self.git("commit", "-q", "-m", message)

    def run_build(self, *args: str, **extra_env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["bash", str(self.repo / "scripts/base-build.sh"), *args], cwd=self.folder, env=dict(self.env, **extra_env), text=True, capture_output=True, check=False)

    def records(self) -> list[dict[str, object]]:
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def test_builds_computed_merge_base_in_isolated_worktree_and_target(self) -> None:
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotEqual(self.base, self.main)
        self.assertNotEqual(self.base, self.head)
        self.assertEqual(self.records(), [{"cwd": str(self.repo / "target/base-src"), "sha": self.base, "argv": ["build", "-q", "--release", "-p", "cadmpeg", "--bin", "cadmpeg"], "target": str(self.repo / "target/base-build")}])
        binary = self.repo / "target/base/cadmpeg"
        self.assertEqual(subprocess.check_output([str(binary)], text=True), f"base:{self.base}\n")
        self.assertEqual((self.repo / "target/base/base.commit").read_text(), self.base + "\n")
        self.assertEqual(self.github_env.read_text(), f"CADMPEG_BASE_BIN={binary}\n")
        exported = subprocess.check_output(["bash", "-c", result.stdout + '\nprintf "%s" "$CADMPEG_BASE_BIN"'], text=True)
        self.assertEqual(exported, str(binary))
        self.assertFalse((self.folder / "head-target").exists())
        self.assertEqual(self.git("rev-parse", "HEAD").stdout.strip(), self.head)
        self.assertEqual(self.git("status", "--porcelain").stdout, "")

    def test_rejects_a_commit_argument_instead_of_overriding_base(self) -> None:
        result = self.run_build(self.main)
        self.assertEqual(result.returncode, 2)
        self.assertIn("base is git merge-base HEAD origin/main", result.stderr)
        self.assertEqual(self.records(), [])
        self.assertFalse((self.repo / "target").exists())

    def test_updates_a_clean_base_worktree_when_the_merge_base_changes(self) -> None:
        self.assertEqual(self.run_build().returncode, 0)
        self.git("merge", "-q", "--no-edit", self.main)
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([row["sha"] for row in self.records()], [self.base, self.main])
        self.assertEqual((self.repo / "target/base/base.commit").read_text(), self.main + "\n")

    def test_refuses_to_discard_changes_in_the_base_worktree(self) -> None:
        self.assertEqual(self.run_build().returncode, 0)
        manifest = self.repo / "target/base-src/Cargo.toml"
        manifest.write_text("do not discard this edit\n")
        result = self.run_build()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("has local changes", result.stderr)
        self.assertEqual(manifest.read_text(), "do not discard this edit\n")
        self.assertEqual(len(self.records()), 1)

    def test_failed_build_cannot_publish_or_leave_a_stale_base_binary(self) -> None:
        self.assertEqual(self.run_build().returncode, 0)
        self.github_env.unlink()
        result = self.run_build(FAKE_CARGO_FAILURE="1")
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertFalse((self.repo / "target/base/cadmpeg").exists())
        self.assertFalse((self.repo / "target/base/base.commit").exists())
        self.assertFalse(self.github_env.exists())
        self.assertEqual(result.stdout, "")

    def test_unregistered_source_directory_and_concurrent_build_are_refused(self) -> None:
        source = self.repo / "target/base-src"
        source.mkdir(parents=True)
        marker = source / "keep"
        marker.write_text("untouched")
        result = self.run_build()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not its own worktree", result.stderr)
        self.assertEqual(marker.read_text(), "untouched")
        source.rename(self.repo / "target/other")
        (self.repo / "target/base-build.lock").mkdir()
        result = self.run_build()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("do not build concurrently", result.stderr)
        self.assertEqual(self.records(), [])


    def test_retries_and_updates_a_real_base_worktree_through_a_symlinked_target(self) -> None:
        physical_target = self.folder / "physical target with spaces"
        physical_target.mkdir()
        (self.repo / "target").symlink_to(physical_target, target_is_directory=True)
        first = self.run_build()
        self.assertEqual(first.returncode, 0, first.stderr)
        retry = self.run_build()
        self.assertEqual(retry.returncode, 0, retry.stderr)
        self.git("merge", "-q", "--no-edit", self.main)
        update = self.run_build()
        self.assertEqual(update.returncode, 0, update.stderr)
        self.assertEqual([row["sha"] for row in self.records()], [self.base, self.base, self.main])
        self.assertEqual({row["cwd"] for row in self.records()}, {str(physical_target / "base-src")})
        self.assertEqual({row["target"] for row in self.records()}, {str(self.repo / "target/base-build")})
        binary = self.repo / "target/base/cadmpeg"
        self.assertEqual(subprocess.check_output([str(binary)], text=True), f"base:{self.main}\n")
        self.assertEqual((physical_target / "base/base.commit").read_text(), self.main + "\n")
        self.assertEqual(self.github_env.read_text().splitlines(), [f"CADMPEG_BASE_BIN={binary}"] * 3)
        source = physical_target / "base-src"
        self.assertEqual(subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True), "")
        (source / "Cargo.toml").write_text("keep this dirty symlinked source\n")
        dirty = self.run_build()
        self.assertNotEqual(dirty.returncode, 0)
        self.assertIn("has local changes", dirty.stderr)
        self.assertEqual((source / "Cargo.toml").read_text(), "keep this dirty symlinked source\n")
        self.assertEqual(len(self.records()), 3)
        self.assertFalse((physical_target / "base-build.lock").exists())

    def test_symlinked_target_still_refuses_a_worktree_from_a_foreign_repository(self) -> None:
        physical_target = self.folder / "foreign target with spaces"
        physical_target.mkdir()
        (self.repo / "target").symlink_to(physical_target, target_is_directory=True)
        foreign = self.folder / "foreign repository"
        foreign.mkdir()
        subprocess.run(["git", "init", "-q", str(foreign)], check=True)
        subprocess.run(["git", "-C", str(foreign), "config", "user.name", "pcurve"], check=True)
        subprocess.run(["git", "-C", str(foreign), "config", "user.email", "301495646+pcurve@users.noreply.github.com"], check=True)
        subprocess.run(["git", "-C", str(foreign), "commit", "-q", "--allow-empty", "-m", "foreign root"], check=True)
        source = physical_target / "base-src"
        subprocess.run(["git", "-C", str(foreign), "worktree", "add", "-q", "--detach", str(source), "HEAD"], check=True)
        before = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True)
        result = self.run_build()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("belongs to a different repository", result.stderr)
        self.assertEqual(subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True), before)
        self.assertEqual(self.records(), [])
        self.assertFalse((physical_target / "base-build.lock").exists())


if __name__ == "__main__":
    unittest.main()
