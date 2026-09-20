#!/usr/bin/env python3
"""Export maintained English Markdown guides for the user documentation site."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "docs/user-docs.json"
REPOSITORY = "https://github.com/aicubetechnology/qilbeeDB/blob/main/"
LINK = re.compile(r"(?<!!)\[([^\]]+)\]\(([^\s)]+)\)")


def render_pages(status):
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    pages = manifest["pages"]
    mapping = {}
    for page in pages:
        source = (ROOT / page["source"]).resolve()
        target = Path(page["file"])
        if not source.is_relative_to(ROOT / "docs") or not source.is_file():
            raise ValueError("Source must be an existing repository documentation file")
        if target.name != page["file"] or target.suffix != ".md" or source in mapping:
            raise ValueError("Export names must be unique Markdown basenames")
        mapping[source] = target.name
    if len(set(mapping.values())) != len(pages):
        raise ValueError("Duplicate output file")
    rendered = {}
    for page in pages:
        source = (ROOT / page["source"]).resolve()
        original = source.read_text(encoding="utf-8")

        def link(match):
            label, destination = match.groups()
            if re.match(
                r"^[a-zA-Z][a-zA-Z0-9+.-]*:", destination
            ) or destination.startswith(("#", "/")):
                return match.group(0)
            filename, marker, anchor = destination.partition("#")
            target = (source.parent / filename).resolve()
            if not target.is_relative_to(ROOT) or not target.is_file():
                raise ValueError(
                    f'Unresolved documentation link in {page["source"]}: {destination}'
                )
            url = mapping.get(target, REPOSITORY + target.relative_to(ROOT).as_posix())
            return f"[{label}]({url}{marker}{anchor})"

        lines = []
        fence = None
        for line in original.splitlines(keepends=True):
            stripped = line.lstrip()
            if stripped.startswith(("```", "~~~")):
                marker = stripped[:3]
                fence = marker if fence is None else None if fence == marker else fence
                lines.append(line)
            else:
                lines.append(line if fence else LINK.sub(link, line))
        metadata = dict(
            title=page["title"],
            description=page["description"],
            slug=(
                "qilbeedb"
                if page["file"] == "index.md"
                else "qilbeedb/" + Path(page["file"]).stem
            ),
            section="QilbeeDB",
            order=page["order"],
            language="en",
            release=manifest["release"],
            status=status,
            source=page["source"],
            source_sha256=hashlib.sha256(original.encode()).hexdigest(),
        )
        header = (
            "---\n"
            + "".join(
                key + ": " + json.dumps(value, ensure_ascii=False) + "\n"
                for key, value in metadata.items()
            )
            + "---\n\n"
        )
        rendered[page["file"]] = header + "".join(lines)
    return rendered


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Managed QilbeeDB subdirectory under the site app/src/doc",
    )
    parser.add_argument(
        "--status", choices=["unreleased", "released"], default="unreleased"
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Report missing or stale managed files without changing them",
    )
    args = parser.parse_args()
    rendered = render_pages(args.status)
    if args.output.is_symlink():
        raise ValueError("Output directory cannot be a symlink")
    for filename in rendered:
        if (args.output / filename).is_symlink():
            raise ValueError("Refusing to replace a symlink: " + filename)
    stale = [
        name
        for name, body in rendered.items()
        if not (args.output / name).is_file()
        or (args.output / name).read_text(encoding="utf-8") != body
    ]
    if args.check:
        if stale:
            print(
                "User documentation needs export: " + ", ".join(stale), file=sys.stderr
            )
            return 1
        print(f"Verified {len(rendered)} Markdown user guides; no drift.")
        return 0
    args.output.mkdir(parents=True, exist_ok=True)
    for name in stale:
        destination = args.output / name
        destination.write_text(rendered[name], encoding="utf-8")
    print(
        f"Exported {len(stale)} updated Markdown guides; {len(rendered)} managed pages. Unrelated files preserved."
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError) as error:
        print("Documentation export failed: " + str(error), file=sys.stderr)
        sys.exit(1)
