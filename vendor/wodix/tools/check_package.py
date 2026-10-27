"""Verify the local .crate archive without publishing it. Requires Python 3.12+."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile


class Package:
    root = Path(__file__).resolve().parents[1]

    @classmethod
    def command(cls, arguments, cwd=None, env=None, capture=False):
        print("+ " + " ".join(str(part) for part in arguments), flush=True)
        return subprocess.run(arguments, cwd=cwd or cls.root, env=env, text=True, capture_output=capture, check=True)

    @classmethod
    def verify(cls, offline=False):
        flags = ["--locked"] + (["--offline"] if offline else [])
        manifest = cls.root / "crates" / "wodix" / "Cargo.toml"
        metadata = json.loads(cls.command(["cargo", "metadata", "--no-deps", "--format-version=1", *flags], capture=True).stdout)
        package = next(item for item in metadata["packages"] if Path(item["manifest_path"]).resolve() == manifest.resolve())
        if (cls.root / "LICENSE").read_bytes() != (manifest.parent / "LICENSE").read_bytes():
            raise RuntimeError("workspace and packaged license text differ")
        cls.command(["cargo", "package", "--manifest-path", str(manifest), "--allow-dirty", *flags])
        stem = f"{package['name']}-{package['version']}"
        archive = Path(metadata["target_directory"]) / "package" / f"{stem}.crate"
        required = {
            "Cargo.toml", "README.md", "CHANGELOG.md", "LICENSE", "NOTICE",
            "src/lib.rs", "tests/data/basic.mdx", "tests/data/basic.mdd",
            "tests/support/mod.rs", "examples/corpus.rs", "benches/parse.rs",
        }
        with tempfile.TemporaryDirectory(prefix="wodix-package-") as directory:
            destination = Path(directory)
            with tarfile.open(archive) as packed:
                members = packed.getmembers()
                names = {item.name.removeprefix(stem + "/") for item in members}
                missing = required - names
                if missing:
                    raise RuntimeError(f"package misses required files: {sorted(missing)}")
                if any(name.startswith("tests/fixtures/") for name in names):
                    raise RuntimeError("private fixtures entered the package")
                packed.extractall(destination, filter="data")
            unpacked = destination / stem
            environment = dict(os.environ)
            environment["CARGO_TARGET_DIR"] = str(destination / "target")
            environment["RUSTDOCFLAGS"] = "-D warnings"
            cls.command(["cargo", "test", "--all-targets", *flags], cwd=unpacked, env=environment)
            cls.command(["cargo", "test", "--doc", *flags], cwd=unpacked, env=environment)
            cls.command(["cargo", "doc", "--no-deps", *flags], cwd=unpacked, env=environment)
            cls.command(["cargo", "run", "--example", "corpus", *flags, "--", "check-fixtures"], cwd=unpacked, env=environment)
        print(json.dumps({"archive": str(archive), "standalone_verified": True, "published": False}), flush=True)

    @classmethod
    def main(cls):
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument("--offline", action="store_true", help="require Cargo dependencies to already be cached")
        arguments = parser.parse_args()
        cls.verify(arguments.offline)


if __name__ == "__main__":
    Package.main()
