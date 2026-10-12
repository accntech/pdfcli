"""Retain a native Office merge and verify that an already-open source stays open.

Run after cargo build --release --locked, in an interactive macOS session.
Uses only synthetic fixtures and the standard Python library.
"""

import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
FIXTURES = ROOT / "tests/fixtures"


def applescript(source, *args):
    return subprocess.run(
        ["/usr/bin/osascript", "-e", source, *map(str, args)],
        capture_output=True, text=True, check=True, timeout=20,
    ).stdout.strip()


def office_state():
    return {
        app: applescript(
            f'tell application id "com.microsoft.{app}" to get '
            f'{{count of {documents}, automation security}}'
        )
        for app, documents in [
            ("Word", "documents"), ("Excel", "workbooks"),
            ("Powerpoint", "presentations"),
        ]
    }


before = office_state()
cache = Path.home() / "Library/Caches/pdfcli/office"
with tempfile.NamedTemporaryFile(prefix="ownership-test-", suffix=".docx", dir=cache) as temporary:
    source = Path(temporary.name)
    shutil.copyfile(FIXTURES / "sample.docx", source)
    original = source.read_bytes()
    applescript('''on run argv
        tell application id "com.microsoft.Word"
            open (POSIX file (item 1 of argv)) add to recent files false
        end tell
    end run''', source)
    try:
        result = subprocess.run([
            str(ROOT / "target/release/pdfcli"), "merge", "--force", "-o",
            str(HERE / "cli-native-office.pdf"), str(HERE / "rea-base.pdf"),
            str(source), str(FIXTURES / "sample.xlsx"),
            str(FIXTURES / "sample.pptx"), str(FIXTURES / "sample.docx"),
        ], capture_output=True, text=True, timeout=600, check=True)
        still_open = applescript('''on run argv
            tell application id "com.microsoft.Word"
                return (item 1 of argv) is in (get posix full name of every document)
            end tell
        end run''', source)
        assert still_open == "true", "CLI closed the already-open source document"
        assert source.read_bytes() == original, "CLI modified the source file"
    finally:
        applescript('''on run argv
            tell application id "com.microsoft.Word"
                repeat with candidate in (get documents)
                    if posix full name of candidate is item 1 of argv then
                        close (contents of candidate) saving no
                    end if
                end repeat
            end tell
        end run''', source)

after = office_state()
assert before == after, (before, after)
output = HERE / "cli-native-office.pdf"
record = {
    "source_remained_open": True,
    "source_bytes_unchanged": True,
    "office_state_before": before,
    "office_state_after": after,
    "output": output.name,
    "output_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
    "cli_stdout": result.stdout,
    "cli_stderr": result.stderr,
}
(HERE / "cli-native-office.json").write_text(json.dumps(record, indent=2) + "\n")
print(json.dumps(record, indent=2))
