from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WEB = ROOT / "webui" / "app"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    required = [
        "state/app-state.js",
        "api/client.js",
        "pages/input.js",
        "pages/configure.js",
        "pages/check.js",
        "pages/run.js",
        "pages/results.js",
        "pages/export.js",
        "core/actions.js",
        "core/selectors.js",
        "components/common.js",
        "components/viewers.js",
        "styles/tokens.css",
        "styles/base.css",
        "styles/layout.css",
        "styles/components.css",
        "styles/pages.css",
    ]
    for rel in required:
        require((WEB / rel).is_file(), f"missing split web asset: {rel}")

    index = read(WEB / "index.html")
    for rel in required[:12]:
        require(f'src="/{rel}"' in index, f"index.html does not load {rel}")

    user_text = "\n".join(
        read(path)
        for path in [WEB / "index.html", *[WEB / rel for rel in required[:12]]]
    )
    forbidden = ["GitHub", "commit", "remote", "schema", "manifest", "hash", "raw JSON", "gate", "route"]
    for term in forbidden:
        require(term not in user_text, f"user web assets expose developer term: {term}")

    mojibake_terms = ["鐨", "绌", "瀹", "杈", "鏁", "鏈", "闇", "淇", "妫€", "娴"]
    for term in mojibake_terms:
        require(term not in user_text, f"user web assets contain mojibake text: {term}")

    require("示例项目" not in user_text, "first-run UI must not seed example projects")
    require("components/viewers.js" in index, "index.html must load the structure and MD visual components")
    print("static_webui_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
