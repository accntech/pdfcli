"""Generate synthetic ODF fixtures with LibreOffice; never overwrite fixtures.

The presentation uses a standard ODF text frame. LibreOffice's OOXML import
otherwise emits an ooxml-rect custom shape that PowerPoint for Mac omits.
"""

from pathlib import Path
import os
import subprocess
import tempfile
from xml.etree import ElementTree as ET
import zipfile

fixtures = Path(__file__).resolve().parent
soffice = os.environ.get("PDFCLI_SOFFICE", "soffice")
with tempfile.TemporaryDirectory(prefix="pdfcli-odf-fixtures-") as temporary:
    for source, extension in [
        ("sample.docx", "odt"), ("sample.xlsx", "ods"), ("sample.pptx", "odp")
    ]:
        target = fixtures / f"sample.{extension}"
        if target.exists():
            raise FileExistsError(target)
        profile = Path(temporary) / f"profile-{extension}"
        subprocess.run([
            soffice, f"-env:UserInstallation={profile.as_uri()}", "--headless",
            "--convert-to", extension, "--outdir", str(fixtures), str(fixtures / source)
        ], check=True, timeout=60)
        if extension == "odp":
            with zipfile.ZipFile(target) as archive:
                entries = [(info, archive.read(info)) for info in archive.infolist()]
            namespaces = {
                "draw": "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0",
                "text": "urn:oasis:names:tc:opendocument:xmlns:text:1.0",
            }
            with zipfile.ZipFile(target, "w") as archive:
                for info, data in entries:
                    if info.filename == "content.xml":
                        root = ET.fromstring(data)
                        page = root.find(".//draw:page", namespaces)
                        shape = page.find("draw:custom-shape", namespaces)
                        frame = ET.Element(f"{{{namespaces['draw']}}}frame", shape.attrib)
                        textbox = ET.SubElement(frame, f"{{{namespaces['draw']}}}text-box")
                        textbox.extend(shape.findall("text:p", namespaces))
                        page.remove(shape)
                        page.insert(0, frame)
                        data = ET.tostring(root, encoding="utf-8", xml_declaration=True)
                    archive.writestr(info, data)
