#!/usr/bin/env python3
"""
Sync bundled WebRTC C++ headers to the exact revision used by the Android AAR.

Design:
- WebRTC headers are fetched from `webrtc-sdk/webrtc` at `webrtc_revision`.
- absl headers are fetched from Chromium third_party revision pinned in that
  revision's `DEPS` (`src/third_party@<sha>`), then expanded recursively for
  `#include "absl/..."` dependencies.
- Sync always verifies file hashes after writing. `--verify` validates only.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
import tomllib


REPO_ROOT = Path(__file__).resolve().parents[2]
LIBS_TOML = REPO_ROOT / "android" / "gradle" / "libs.versions.toml"
HEADER_ROOT = REPO_ROOT / "android" / "app" / "src" / "main" / "cpp" / "third_party" / "webrtc_m124"
WEBRTC_RAW_BASE = "https://raw.githubusercontent.com/webrtc-sdk/webrtc"
CHROMIUM_TP_ARCHIVE_BASE = "https://chromium.googlesource.com/chromium/src/third_party/+archive"
USER_AGENT = "remoterg-sync-webrtc-headers/2.0"

ABSL_INCLUDE_PATTERN = re.compile(r'^\s*#\s*include\s+"(absl/[^"]+)"', re.MULTILINE)
THIRD_PARTY_REV_PATTERN = re.compile(
    r"'src/third_party'\s*:\s*'https://chromium\.googlesource\.com/chromium/src/third_party@([0-9a-f]{40})'"
)


class SyncError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Sync local WebRTC headers to AAR revision.")
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Verify local headers against expected upstream revisions without writing files.",
    )
    return parser.parse_args()


def normalize_bytes(data: bytes) -> bytes:
    return data.replace(b"\r\n", b"\n")


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(normalize_bytes(data)).hexdigest()


def load_webrtc_coordinates(libs_toml: Path) -> tuple[str, str, str]:
    data = tomllib.loads(libs_toml.read_text(encoding="utf-8"))
    libraries = data.get("libraries", {})
    versions = data.get("versions", {})
    webrtc = libraries.get("webrtc")
    if not isinstance(webrtc, dict):
        raise SyncError("libraries.webrtc is missing in libs.versions.toml")

    group = webrtc.get("group")
    name = webrtc.get("name")
    if not isinstance(group, str) or not isinstance(name, str):
        raise SyncError("libraries.webrtc.group/name is invalid")

    version_value = webrtc.get("version")
    version: str | None = None
    if isinstance(version_value, str):
        version = version_value
    elif isinstance(version_value, dict):
        ref = version_value.get("ref")
        if isinstance(ref, str):
            resolved = versions.get(ref)
            if isinstance(resolved, str):
                version = resolved
    if not version:
        raise SyncError("Failed to resolve libraries.webrtc version")
    return group, name, version


def find_aar_in_gradle_cache(group: str, name: str, version: str) -> Path:
    cache_root = Path.home() / ".gradle" / "caches" / "modules-2" / "files-2.1"
    base = cache_root / group / name / version
    if not base.exists():
        raise SyncError(
            f"AAR cache not found: {base}\nRun an Android Gradle build once to download dependencies."
        )
    aars = sorted(base.glob("**/*.aar"))
    if not aars:
        raise SyncError(f"No AAR found under cache path: {base}")
    return aars[0]


def find_javap() -> str:
    java_home_raw = os.environ.get("JAVA_HOME")
    if java_home_raw:
        exe = "javap.exe" if sys.platform.startswith("win") else "javap"
        candidate = Path(java_home_raw) / "bin" / exe
        if candidate.exists():
            return str(candidate)
    which = shutil.which("javap")
    if which:
        return which
    raise SyncError("`javap` not found. Install JDK and set JAVA_HOME or PATH.")


def read_webrtc_build_version(aar_path: Path) -> tuple[str, str, str, str]:
    javap = find_javap()
    with tempfile.TemporaryDirectory(prefix="sync_webrtc_headers_") as tmpdir:
        classes_jar = Path(tmpdir) / "classes.jar"
        with zipfile.ZipFile(aar_path, "r") as zf:
            try:
                classes_jar.write_bytes(zf.read("classes.jar"))
            except KeyError as exc:
                raise SyncError(f"classes.jar not found in AAR: {aar_path}") from exc

        proc = subprocess.run(
            [javap, "-classpath", str(classes_jar), "-verbose", "org.webrtc.WebrtcBuildVersion"],
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode != 0:
            raise SyncError(f"javap failed:\n{proc.stderr.strip()}")
        output = proc.stdout.splitlines()

    mapping = {
        "webrtc_branch": None,
        "webrtc_commit": None,
        "webrtc_revision": None,
        "maint_version": None,
    }
    current_field: str | None = None
    for raw in output:
        line = raw.strip()
        for key in mapping:
            if line.startswith(f"public static final java.lang.String {key};"):
                current_field = key
                break
        else:
            if current_field and line.startswith("ConstantValue: String "):
                mapping[current_field] = line.removeprefix("ConstantValue: String ").strip()
                current_field = None

    if not all(isinstance(v, str) and v for v in mapping.values()):
        raise SyncError(f"Failed to parse WebrtcBuildVersion constants: {mapping}")

    return (
        mapping["webrtc_branch"],  # type: ignore[arg-type]
        mapping["webrtc_commit"],  # type: ignore[arg-type]
        mapping["webrtc_revision"],  # type: ignore[arg-type]
        mapping["maint_version"],  # type: ignore[arg-type]
    )


def collect_local_seed_headers(header_root: Path) -> list[str]:
    if not header_root.exists():
        raise SyncError(f"Header root not found: {header_root}")
    files = sorted(
        p.relative_to(header_root).as_posix()
        for p in header_root.rglob("*.h")
        if p.is_file()
    )
    if not files:
        raise SyncError(f"No local header files found under: {header_root}")
    return files


def fetch_url_bytes(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=90) as resp:
            return resp.read()
    except urllib.error.HTTPError as exc:
        raise SyncError(f"Failed to fetch ({exc.code}): {url}") from exc
    except urllib.error.URLError as exc:
        raise SyncError(f"Failed to fetch: {url}\n{exc}") from exc


def fetch_webrtc_file(revision: str, local_rel: str) -> bytes:
    remote_path = urllib.parse.quote(local_rel, safe="/")
    url = f"{WEBRTC_RAW_BASE}/{revision}/{remote_path}"
    return fetch_url_bytes(url)


def fetch_webrtc_deps_file(revision: str) -> str:
    url = f"{WEBRTC_RAW_BASE}/{revision}/DEPS"
    return fetch_url_bytes(url).decode("utf-8")


def extract_chromium_third_party_revision(deps_text: str) -> str:
    match = THIRD_PARTY_REV_PATTERN.search(deps_text)
    if not match:
        raise SyncError("Failed to parse Chromium third_party revision from WebRTC DEPS")
    return match.group(1)


def normalize_member_name(name: str) -> str:
    return name.lstrip("./").replace("\\", "/")


def fetch_archive_directory_members(
    base_url: str,
    revision: str,
    directory: str,
    cache: dict[tuple[str, str], dict[str, bytes]],
) -> dict[str, bytes]:
    key = (revision, directory)
    cached = cache.get(key)
    if cached is not None:
        return cached

    encoded_rev = urllib.parse.quote(revision, safe="")
    encoded_dir = urllib.parse.quote(directory, safe="/")
    url = f"{base_url}/{encoded_rev}/{encoded_dir}.tar.gz"
    archive = fetch_url_bytes(url)
    members: dict[str, bytes] = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tf:
        for member in tf.getmembers():
            if not member.isfile():
                continue
            extracted = tf.extractfile(member)
            if extracted is None:
                continue
            members[normalize_member_name(member.name)] = extracted.read()
    cache[key] = members
    return members


def fetch_absl_file(
    chromium_third_party_revision: str,
    absl_local_rel: str,
    cache: dict[tuple[str, str], dict[str, bytes]],
) -> bytes:
    if not absl_local_rel.startswith("absl/"):
        raise SyncError(f"Invalid absl local rel path: {absl_local_rel}")

    repo_rel = f"abseil-cpp/{absl_local_rel}"
    path = PurePosixPath(repo_rel)
    directory = str(path.parent)
    filename = path.name
    members = fetch_archive_directory_members(
        CHROMIUM_TP_ARCHIVE_BASE, chromium_third_party_revision, directory, cache
    )
    data = members.get(filename)
    if data is None:
        raise SyncError(
            "Failed to resolve absl header from Chromium third_party archive: "
            f"{repo_rel}@{chromium_third_party_revision}"
        )
    return data


def parse_absl_include_targets(header_data: bytes) -> set[str]:
    text = header_data.decode("utf-8", errors="ignore")
    return {m.group(1) for m in ABSL_INCLUDE_PATTERN.finditer(text)}


def resolve_expected_headers(
    *,
    webrtc_revision: str,
    chromium_third_party_revision: str,
    local_seed_headers: list[str],
) -> dict[str, bytes]:
    expected: dict[str, bytes] = {}

    for rel in local_seed_headers:
        if rel.startswith("absl/"):
            continue
        expected[rel] = fetch_webrtc_file(webrtc_revision, rel)

    archive_cache: dict[tuple[str, str], dict[str, bytes]] = {}
    visited_absl: set[str] = set()
    queue = [rel for rel in local_seed_headers if rel.startswith("absl/")]

    while queue:
        rel = queue.pop(0)
        if rel in visited_absl:
            continue
        visited_absl.add(rel)

        data = fetch_absl_file(chromium_third_party_revision, rel, archive_cache)
        expected[rel] = data

        for include_rel in sorted(parse_absl_include_targets(data)):
            if include_rel not in visited_absl:
                queue.append(include_rel)

    return expected


def verify_expected_headers(header_root: Path, expected: dict[str, bytes]) -> list[str]:
    mismatches: list[str] = []
    for rel, remote_data in sorted(expected.items()):
        local_path = header_root / rel
        if not local_path.exists():
            mismatches.append(f"missing: {rel}")
            continue
        local_data = local_path.read_bytes()
        if sha256_hex(local_data) != sha256_hex(remote_data):
            mismatches.append(f"content_mismatch: {rel}")
    return mismatches


def write_synced_headers(
    *,
    header_root: Path,
    expected: dict[str, bytes],
    group: str,
    artifact: str,
    version: str,
    webrtc_branch: str,
    webrtc_commit: str,
    webrtc_revision: str,
    maint_version: str,
    chromium_third_party_revision: str,
) -> None:
    temp_root = header_root.parent / f".{header_root.name}.sync_tmp"
    backup_root = header_root.parent / f".{header_root.name}.sync_backup"
    if temp_root.exists():
        shutil.rmtree(temp_root)
    if backup_root.exists():
        shutil.rmtree(backup_root)
    temp_root.mkdir(parents=True, exist_ok=True)

    for rel, data in sorted(expected.items()):
        out = temp_root / rel
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(data)

    stamp = datetime.now(timezone.utc).isoformat(timespec="seconds")
    metadata = "\n".join(
        [
            "Synced by android/scripts/sync_webrtc_headers.py",
            f"artifact={group}:{artifact}:{version}",
            f"webrtc_branch={webrtc_branch}",
            f"webrtc_commit={webrtc_commit}",
            f"webrtc_revision={webrtc_revision}",
            f"maint_version={maint_version}",
            "webrtc_source_repo=webrtc-sdk/webrtc",
            f"chromium_third_party_revision={chromium_third_party_revision}",
            f"synced_at_utc={stamp}",
            "",
        ]
    )
    (temp_root / "SYNC_INFO.txt").write_text(metadata, encoding="utf-8")

    try:
        if header_root.exists():
            header_root.rename(backup_root)
        temp_root.rename(header_root)
        if backup_root.exists():
            shutil.rmtree(backup_root)
    except Exception:
        if header_root.exists():
            shutil.rmtree(header_root, ignore_errors=True)
        if backup_root.exists():
            backup_root.rename(header_root)
        if temp_root.exists():
            shutil.rmtree(temp_root, ignore_errors=True)
        raise


def main() -> int:
    args = parse_args()

    group, artifact, version = load_webrtc_coordinates(LIBS_TOML)
    aar = find_aar_in_gradle_cache(group, artifact, version)
    webrtc_branch, webrtc_commit, webrtc_revision, maint_version = read_webrtc_build_version(aar)
    local_seed_headers = collect_local_seed_headers(HEADER_ROOT)

    deps_text = fetch_webrtc_deps_file(webrtc_revision)
    chromium_third_party_revision = extract_chromium_third_party_revision(deps_text)
    expected = resolve_expected_headers(
        webrtc_revision=webrtc_revision,
        chromium_third_party_revision=chromium_third_party_revision,
        local_seed_headers=local_seed_headers,
    )

    if args.verify:
        mismatches = verify_expected_headers(HEADER_ROOT, expected)
        if mismatches:
            detail = "\n".join(mismatches[:20])
            more = "" if len(mismatches) <= 20 else f"\n... and {len(mismatches) - 20} more"
            raise SyncError(
                "Header verification failed.\n"
                f"mismatch_count={len(mismatches)}\n{detail}{more}"
            )
        print(f"Verified {len(expected)} files")
        print(f"AAR: {group}:{artifact}:{version}")
        print(
            f"WebRTC revision: {webrtc_revision} (branch={webrtc_branch}, commit={webrtc_commit}, maint={maint_version})"
        )
        print(f"Chromium third_party revision: {chromium_third_party_revision}")
        print(f"Header root: {HEADER_ROOT}")
        return 0

    write_synced_headers(
        header_root=HEADER_ROOT,
        expected=expected,
        group=group,
        artifact=artifact,
        version=version,
        webrtc_branch=webrtc_branch,
        webrtc_commit=webrtc_commit,
        webrtc_revision=webrtc_revision,
        maint_version=maint_version,
        chromium_third_party_revision=chromium_third_party_revision,
    )

    mismatches = verify_expected_headers(HEADER_ROOT, expected)
    if mismatches:
        detail = "\n".join(mismatches[:20])
        more = "" if len(mismatches) <= 20 else f"\n... and {len(mismatches) - 20} more"
        raise SyncError(
            "Post-sync header verification failed.\n"
            f"mismatch_count={len(mismatches)}\n{detail}{more}"
        )

    print(f"Synced {len(expected)} files")
    print(f"AAR: {group}:{artifact}:{version}")
    print(
        f"WebRTC revision: {webrtc_revision} (branch={webrtc_branch}, commit={webrtc_commit}, maint={maint_version})"
    )
    print(f"Chromium third_party revision: {chromium_third_party_revision}")
    print(f"Header root: {HEADER_ROOT}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SyncError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1)
