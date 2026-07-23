#!/usr/bin/env python3

from __future__ import annotations

import html
import os
import re
import shutil
from pathlib import Path

try:
    from markdown import markdown
except ImportError as exc:  # pragma: no cover - surfaced in CI
    raise SystemExit(
        "Missing dependency: install the 'markdown' package before running scripts/build-pages.py"
    ) from exc


REPO_ROOT = Path(__file__).resolve().parents[1]
DOCS_ROOT = REPO_ROOT / "docs"
OUTPUT_ROOT = REPO_ROOT / "target" / "pages-site"
REPO_BLOB_BASE = "https://github.com/mupozg823/codelens-mcp-plugin/blob/main"
SITE_NAME = "CodeLens MCP"
SITE_TAGLINE = (
    "A live code index for coding agents — bounded context, verifiable structure, "
    "and safer edits."
)
SITE_URL = "https://mupozg823.github.io/codelens-mcp-plugin/"
SOCIAL_IMAGE_URL = f"{SITE_URL}assets/codelens-social-preview.jpg"
LINK_PATTERN = re.compile(r"(!?\[[^\]]*\])\(([^)]+)\)")
TITLE_PATTERN = re.compile(r"^#\s+(.+?)\s*$", re.MULTILINE)
FIRST_H1_PATTERN = re.compile(r"^\s*<h1\b[^>]*>.*?</h1>\s*", re.DOTALL)
HANGUL_PATTERN = re.compile(r"[가-힣]")
NAV_STRUCTURE = [
    (
        "Start here",
        [
            Path("index.md"),
            Path("platform-setup.md"),
            Path("quickstart-transcript.md"),
            Path("harness-modes.md"),
            Path("multi-agent-integration.md"),
        ],
    ),
    (
        "Concepts",
        [
            Path("architecture.md"),
            Path("host-adaptive-harness.md"),
            Path("harness-spec.md"),
            Path("comparison.md"),
            Path("benchmarks.md"),
        ],
    ),
    (
        "Operations",
        [
            Path("operations/http-daemon.md"),
            Path("operations/response-envelope.md"),
            Path("operations/runtime-knobs.md"),
            Path("operations/tool-routing-matrix.md"),
            Path("observability.md"),
        ],
    ),
    (
        "Trust & release",
        [
            Path("release-verification.md"),
            Path("release-distribution.md"),
            Path("product-readiness.md"),
        ],
    ),
    (
        "Architecture decisions",
        [
            Path("adr/README.md"),
            Path("adr/ADR-0009-mutation-trust-substrate.md"),
            Path("adr/ADR-0015-host-neutral-execution-contract.md"),
            Path("adr/ADR-0016-default-surface-twenty.md"),
            Path("adr/ADR-0017-single-writer-project-runtime.md"),
            Path("adr/ADR-0018-session-identity-and-coordination-hardening.md"),
        ],
    ),
    (
        "Reference",
        [
            Path("scip-guide.md"),
            Path("design/arg-validation-policy.md"),
            Path("design/refactor-backend-honesty.md"),
        ],
    ),
]


def iter_markdown_files() -> list[Path]:
    return sorted(path for path in DOCS_ROOT.rglob("*.md") if path.is_file())


def output_path_for(source: Path) -> Path:
    relative = source.relative_to(DOCS_ROOT)
    if relative == Path("index.md"):
        return OUTPUT_ROOT / "index.html"
    return OUTPUT_ROOT / relative.with_suffix(".html")


def extract_title(source: Path) -> str:
    match = TITLE_PATTERN.search(source.read_text(encoding="utf-8"))
    if match:
        return match.group(1).strip()
    return source.stem.replace("-", " ").replace("_", " ").title()


def split_fragment(target: str) -> tuple[str, str]:
    if "#" in target:
        path_text, fragment = target.split("#", 1)
        return path_text, f"#{fragment}"
    return target, ""


def to_posix_relative(target: Path, current: Path) -> str:
    return os.path.relpath(target, current.parent).replace(os.sep, "/")


def github_blob_url(path: Path, fragment: str = "") -> str:
    relative = path.relative_to(REPO_ROOT).as_posix()
    return f"{REPO_BLOB_BASE}/{relative}{fragment}"


def rewrite_target(source: Path, target: str) -> str:
    stripped = target.strip()
    if (
        not stripped
        or stripped.startswith(("#", "http://", "https://", "mailto:", "tel:"))
    ):
        return target

    path_text, fragment = split_fragment(stripped)
    if not path_text:
        return target

    resolved = (source.parent / path_text).resolve()
    current_output = output_path_for(source)

    try:
        docs_relative = resolved.relative_to(DOCS_ROOT.resolve())
    except ValueError:
        if resolved.exists() and resolved.is_file():
            return github_blob_url(resolved, fragment)
        return target

    if resolved.suffix.lower() == ".md" and resolved.is_file():
        destination = output_path_for(DOCS_ROOT / docs_relative)
    else:
        destination = OUTPUT_ROOT / docs_relative
    return f"{to_posix_relative(destination, current_output)}{fragment}"


def rewrite_markdown_links(source: Path, raw_markdown: str) -> str:
    def replace(match: re.Match[str]) -> str:
        label, target = match.groups()
        return f"{label}({rewrite_target(source, target)})"

    return LINK_PATTERN.sub(replace, raw_markdown)


def render_nav(source: Path, titles: dict[Path, str]) -> str:
    blocks: list[str] = []
    current_relative = source.relative_to(DOCS_ROOT)
    for section, configured_entries in NAV_STRUCTURE:
        entries = [relative for relative in configured_entries if relative in titles]
        if not entries:
            continue

        lines = [f'<section class="nav-group"><h2>{html.escape(section)}</h2><ul>']
        for relative in entries:
            target = output_path_for(DOCS_ROOT / relative)
            href = to_posix_relative(target, output_path_for(source))
            active = ' class="active"' if relative == current_relative else ""
            lines.append(
                f'<li><a{active} href="{html.escape(href)}">{html.escape(titles[relative])}</a></li>'
            )
        lines.append("</ul></section>")
        blocks.append("".join(lines))
    return "\n".join(blocks)


def render_page(source: Path, titles: dict[Path, str]) -> str:
    raw_markdown = source.read_text(encoding="utf-8")
    rewritten_markdown = rewrite_markdown_links(source, raw_markdown)
    rendered = markdown(
        rewritten_markdown,
        extensions=["extra", "toc", "sane_lists"],
        output_format="html5",
    )
    rendered = FIRST_H1_PATTERN.sub("", rendered, count=1)
    title = titles[source.relative_to(DOCS_ROOT)]
    css_href = to_posix_relative(OUTPUT_ROOT / "assets" / "site.css", output_path_for(source))
    icon_href = to_posix_relative(
        OUTPUT_ROOT / "assets" / "codelens-mark.svg", output_path_for(source)
    )
    home_href = to_posix_relative(OUTPUT_ROOT / "index.html", output_path_for(source))
    output_relative = output_path_for(source).relative_to(OUTPUT_ROOT).as_posix()
    canonical_url = SITE_URL if output_relative == "index.html" else f"{SITE_URL}{output_relative}"
    language = "ko" if len(HANGUL_PATTERN.findall(raw_markdown)) >= 20 else "en"
    repo_href = github_blob_url(source)
    nav = render_nav(source, titles)
    return f"""<!doctype html>
<html lang="{language}">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{html.escape(title)} | {SITE_NAME}</title>
    <meta name="description" content="{html.escape(SITE_TAGLINE)}">
    <meta name="theme-color" content="#07111d">
    <meta property="og:type" content="website">
    <meta property="og:title" content="{html.escape(title)} | {SITE_NAME}">
    <meta property="og:description" content="{html.escape(SITE_TAGLINE)}">
    <meta property="og:image" content="{html.escape(SOCIAL_IMAGE_URL)}">
    <meta property="og:url" content="{html.escape(canonical_url)}">
    <meta name="twitter:card" content="summary_large_image">
    <link rel="icon" href="{html.escape(icon_href)}" type="image/svg+xml">
    <link rel="canonical" href="{html.escape(canonical_url)}">
    <link rel="stylesheet" href="{html.escape(css_href)}">
  </head>
  <body>
    <div class="layout">
      <aside class="sidebar">
        <a class="brand" href="{html.escape(home_href)}">
          <img class="brand-mark" src="{html.escape(icon_href)}" width="42" height="42" alt="">
          <span>{SITE_NAME}</span>
        </a>
        <p class="tagline">{html.escape(SITE_TAGLINE)}</p>
        <nav>
{nav}
        </nav>
        <div class="sidebar-links">
          <a href="https://github.com/mupozg823/codelens-mcp-plugin">GitHub</a>
          <a href="https://github.com/mupozg823/codelens-mcp-plugin/releases/latest">Latest release</a>
        </div>
      </aside>
      <main class="content">
        <header class="page-header">
          <p class="eyebrow">Documentation</p>
          <h1>{html.escape(title)}</h1>
          <p><a href="{html.escape(repo_href)}">View source on GitHub</a></p>
        </header>
        <article class="markdown-body">
{rendered}
        </article>
      </main>
    </div>
  </body>
</html>
"""


def copy_static_assets() -> None:
    for source in DOCS_ROOT.rglob("*"):
        if not source.is_file() or source.suffix.lower() == ".md":
            continue
        destination = OUTPUT_ROOT / source.relative_to(DOCS_ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)


def build_site() -> None:
    markdown_files = iter_markdown_files()
    titles = {path.relative_to(DOCS_ROOT): extract_title(path) for path in markdown_files}

    if OUTPUT_ROOT.exists():
        shutil.rmtree(OUTPUT_ROOT)
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)

    copy_static_assets()

    for source in markdown_files:
        destination = output_path_for(source)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(render_page(source, titles), encoding="utf-8")


def main() -> None:
    build_site()
    print(f"Built GitHub Pages site into {OUTPUT_ROOT}")


if __name__ == "__main__":
    main()
