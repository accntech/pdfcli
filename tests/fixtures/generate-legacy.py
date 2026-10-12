"""Create genuine binary Office fixtures using installed macOS Microsoft apps.

Run from an interactive login session after granting Office access to
~/Library/Caches/pdfcli/office. Existing fixture files are never overwritten.
Only synthetic OOXML input copies are opened; no user documents are touched.
"""

import hashlib
import json
import plistlib
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CACHE = Path.home() / "Library/Caches/pdfcli/office"
TEMPLATE = (ROOT / "src/office/macos.applescript").read_text()
CASES = [
    ("Word", "com.microsoft.Word", "documents", "sample.docx", "sample.doc",
     "open inputFile confirm conversions false add to recent files false",
     "save as ownedDoc file name outputPath file format format document97 add to recent files false"),
    ("Excel", "com.microsoft.Excel", "workbooks", "sample.xlsx", "sample.xls",
     "open workbook workbook file name inputFile update links do not update links",
     "save workbook as ownedDoc filename outputPath file format Excel98to2004 file format"),
    ("PowerPoint", "com.microsoft.Powerpoint", "presentations", "sample.pptx", "sample.ppt",
     "open inputFile", "save ownedDoc in (POSIX file outputPath) as save as presentation"),
    ("Excel", "com.microsoft.Excel", "workbooks", "multiple-sheets.xlsx", "multiple-sheets.xls",
     "open workbook workbook file name inputFile update links do not update links",
     "save workbook as ownedDoc filename outputPath file format Excel98to2004 file format"),
]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


CACHE.mkdir(parents=True, exist_ok=True)
records = []
for app, app_id, documents, source_name, output_name, open_command, save_command in CASES:
    destination = HERE / output_name
    if destination.exists():
        raise FileExistsError(f"Refusing to overwrite fixture {destination}")
    source = HERE / source_name
    original_hash = digest(source)
    with tempfile.NamedTemporaryFile(prefix="legacy-fixture-", suffix=source.suffix, dir=CACHE) as f:
        staged = Path(f.name)
        shutil.copyfile(source, staged)
        output = staged.with_suffix(destination.suffix)
        with tempfile.TemporaryDirectory(prefix="legacy-generation-", dir=CACHE) as job:
            script = Path(job) / "convert.applescript"
            state = Path(job) / "automation-security.txt"
            # Native Save As changes a document's name. Reacquire the owned
            # document by its output path before the template closes it.
            save_command += f'''
                    repeat with candidate in (get {documents})
                        set savedPath to full name of candidate
                        if savedPath does not start with "/" then set savedPath to POSIX path of (savedPath as alias)
                        if savedPath is outputPath then
                            set ownedDoc to contents of candidate
                            exit repeat
                        end if
                    end repeat'''
            rendered = TEMPLATE
            for key, value in {
                "__APP_ID__": app_id, "__DOCUMENTS__": documents,
                "__ALLOW_MISSING_SECURITY__": "true" if app == "PowerPoint" else "false",
                "__OPEN__": open_command, "__SAVE__": save_command,
            }.items():
                rendered = rendered.replace(key, value)
            script.write_text(rendered)
            args = [str(script), str(staged), str(output), str(state), "120", "convert", staged.name]
            print(f"Creating {output_name} with Microsoft {app}", flush=True)
            try:
                subprocess.run(["/usr/bin/osascript", *args], check=True, timeout=125)
                assert output.read_bytes().startswith(bytes.fromhex("d0cf11e0a1b11ae1")), "Not a binary compound Office file"
                assert digest(source) == original_hash, "Original OOXML fixture changed"
                shutil.copyfile(output, destination)
            finally:
                args[4:6] = ["5", "cleanup"]
                subprocess.run(["/usr/bin/osascript", *args], check=True, timeout=10)
                output.unlink(missing_ok=True)
    with open(f"/Applications/Microsoft {app}.app/Contents/Info.plist", "rb") as info:
        version = plistlib.load(info)["CFBundleShortVersionString"]
    records.append({
        "file": output_name, "sha256": digest(destination), "source": source_name,
        "source_sha256": original_hash, "application": app, "version": version,
        "save_command": save_command.splitlines()[0], "binary_compound_header_verified": True,
    })

(HERE / "legacy-provenance.json").write_text(json.dumps({
    "generated_at": datetime.now(timezone.utc).isoformat(), "fixtures": records,
}, indent=2) + "\n")
print("Created all four legacy fixtures.")
