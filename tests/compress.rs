use lopdf::{Document, Object, Stream, dictionary};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;

fn compress(input: &Path, output: &Path, force: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pdfcli"));
    command.arg("compress").arg(input).arg("-o").arg(output);
    if force {
        command.arg("--force");
    }
    command.output().unwrap()
}

fn assert_ok(result: &Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn fixture(path: &Path) {
    let mut doc = Document::with_version("1.4");
    let root = doc.new_object_id();
    let font = doc.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
    );
    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 100, "Height" => 100,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8
        },
        [30, 100, 180].repeat(10000),
    ));
    let mut kids = Vec::new();
    for text in ["FIRST", "SECOND"] {
        let mut content =
            format!("BT /F1 18 Tf 25 50 Td ({text}) Tj ET\nq 40 0 0 40 120 50 cm /Image Do Q\n")
                .into_bytes();
        content.extend(b"% compressible comment without paint\n".repeat(5000));
        let stream = doc.add_object(Stream::new(dictionary! {}, content));
        kids.push(
            doc.add_object(dictionary! {"Type" => "Page", "Parent" => root, "Contents" => stream}),
        );
    }
    doc.objects.insert(root, dictionary! {
        "Type" => "Pages", "Kids" => kids.iter().copied().map(Object::Reference).collect::<Vec<_>>(), "Count" => 2,
        "MediaBox" => vec![0.into(), 0.into(), 320.into(), 160.into()],
        "CropBox" => vec![10.into(), 10.into(), 310.into(), 150.into()], "Rotate" => 90,
        "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}, "XObject" => dictionary! {"Image" => image}}
    }.into());
    let outlines = doc.new_object_id();
    let bookmark = doc.add_object(dictionary! {"Title" => Object::string_literal("Bookmark"), "Parent" => outlines, "Dest" => vec![kids[0].into(), Object::Name(b"Fit".to_vec())]});
    doc.objects.insert(
        outlines,
        dictionary! {"Type" => "Outlines", "First" => bookmark, "Last" => bookmark, "Count" => 1}
            .into(),
    );
    let link = doc.add_object(dictionary! {"Type" => "Annot", "Subtype" => "Link", "Rect" => vec![20.into(), 40.into(), 70.into(), 65.into()], "A" => dictionary! {"S" => "URI", "URI" => Object::string_literal("https://example.com")} });
    doc.get_dictionary_mut(kids[0])
        .unwrap()
        .set("Annots", vec![Object::Reference(link)]);
    let field = doc.add_object(dictionary! {"FT" => "Tx", "T" => Object::string_literal("field"), "V" => Object::string_literal("Keep field value")});
    let metadata = doc.add_object(Stream::new(
        dictionary! {"Type" => "Metadata", "Subtype" => "XML"},
        b"<metadata>Keep metadata</metadata>".to_vec(),
    ));
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => root, "Outlines" => outlines, "Metadata" => metadata, "AcroForm" => dictionary! {"Fields" => vec![Object::Reference(field)]}});
    doc.trailer.set("Root", catalog);
    let info = doc.add_object(dictionary! {"Title" => Object::string_literal("Keep title")});
    doc.trailer.set("Info", info);
    doc.add_object(Stream::new(dictionary! {}, vec![0; 50000])); // Unreferenced data.
    doc.save(path).unwrap();
}

#[test]
fn compress_reduces_size_preserves_contents_catalog_and_source() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input Ω.PDF");
    let output = dir.path().join("small.pdf");
    fixture(&input);
    let original = fs::read(&input).unwrap();
    let result = compress(&input, &output, false);
    assert_ok(&result);
    assert!(fs::metadata(&output).unwrap().len() < original.len() as u64 / 4);
    let before = Document::load(&input).unwrap();
    let after = Document::load(&output).unwrap();
    assert_eq!(before.catalog().unwrap(), after.catalog().unwrap());
    assert_eq!(before.get_pages(), after.get_pages());
    for id in before.get_pages().values() {
        assert_eq!(before.get_page_content(*id), after.get_page_content(*id));
        assert_eq!(
            before.get_dictionary(*id).unwrap(),
            after.get_dictionary(*id).unwrap()
        );
    }
    let mut reachable = before.clone();
    reachable.prune_objects();
    for (&id, source) in &reachable.objects {
        let object = after
            .get_object(id)
            .expect("A referenced source object was lost");
        if let Ok(stream) = source.as_stream() {
            assert_eq!(
                stream.get_plain_content().unwrap(),
                object.as_stream().unwrap().get_plain_content().unwrap()
            );
        } else {
            assert_eq!(
                source, object,
                "Referenced document objects must survive unchanged"
            );
        }
    }
    assert!(String::from_utf8_lossy(&result.stdout).contains("bytes"));
    assert_eq!(fs::read(input).unwrap(), original);
    let second = dir.path().join("again.pdf");
    assert_ok(&compress(&output, &second, false));
    assert!(fs::metadata(second).unwrap().len() <= fs::metadata(output).unwrap().len());
}

#[test]
fn compress_copies_original_when_optimization_would_grow_it() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("tiny.pdf");
    let output = dir.path().join("copy.pdf");
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in [
        "<</Type/Catalog/Pages 2 0 R>>",
        "<</Type/Pages/Kids[3 0 R]/Count 1>>",
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]>>",
    ]
    .iter()
    .enumerate()
    {
        offsets.push(bytes.len());
        bytes.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).bytes());
    }
    let xref = bytes.len();
    bytes.extend(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    bytes.extend(format!("trailer\n<</Size 4/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").bytes());
    fs::write(&input, &bytes).unwrap();
    let result = compress(&input, &output, false);
    assert_ok(&result);
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert!(String::from_utf8_lossy(&result.stdout).contains("unchanged copy"));
}

#[test]
fn compress_protects_inputs_aliases_and_outputs() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    fixture(&input);
    let original = fs::read(&input).unwrap();
    let alias = dir.path().join("alias.pdf");
    fs::hard_link(&input, &alias).unwrap();
    for target in [&input, &alias] {
        let result = compress(&input, target, true);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("must not replace an input"));
    }
    let output = dir.path().join("existing.pdf");
    fs::write(&output, b"keep output").unwrap();
    let result = compress(&input, &output, false);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("already exists"));
    assert_eq!(fs::read(&output).unwrap(), b"keep output");
    assert_ok(&compress(&input, &output, true));
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[test]
fn compress_rejects_bad_inputs_without_replacing_output() {
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    let output = dir.path().join("out.pdf");
    for kind in [
        "malformed",
        "broken tree",
        "signature",
        "inline signature",
        "empty password",
        "password",
    ] {
        fixture(&input);
        let mut doc = Document::load(&input).unwrap();
        match kind {
            "broken tree" => {
                let id = doc
                    .catalog()
                    .unwrap()
                    .get(b"Pages")
                    .unwrap()
                    .as_reference()
                    .unwrap();
                doc.get_dictionary_mut(id).unwrap().set("Count", 5);
            }
            "signature" => {
                doc.catalog_mut().unwrap().set("Signature", dictionary! {"Type" => "Sig", "ByteRange" => vec![0.into(), 1.into(), 2.into(), 3.into()]});
            }
            "inline signature" => {
                doc.catalog_mut().unwrap().set(
                    "Signature",
                    Object::Array(vec![dictionary! {"FT" => "Sig"}.into()]),
                );
            }
            "empty password" | "password" => {
                doc.trailer.set(
                    "ID",
                    vec![
                        Object::string_literal("fixture-id-12345"),
                        Object::string_literal("fixture-id-12345"),
                    ],
                );
                let state = EncryptionState::try_from(EncryptionVersion::V1 {
                    document: &doc,
                    owner_password: "owner",
                    user_password: if kind == "password" { "secret" } else { "" },
                    permissions: Permissions::all(),
                })
                .unwrap();
                doc.encrypt(&state).unwrap();
            }
            _ => {}
        }
        doc.save(&input).unwrap();
        if kind == "malformed" {
            fs::write(&input, b"not a PDF").unwrap();
        }
        fs::write(&output, b"keep output").unwrap();
        let result = compress(&input, &output, true);
        assert!(!result.status.success(), "Accepted {kind}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("compress"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read(&output).unwrap(), b"keep output");
    }
    let wrong_type = dir.path().join("source.docx");
    fs::copy(input, &wrong_type).unwrap();
    let result = compress(&wrong_type, &output, true);
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires a PDF"));
    assert_eq!(fs::read(output).unwrap(), b"keep output");
}

#[test]
#[ignore = "requires Poppler pdftoppm"]
fn compress_renders_text_images_and_geometry_identically() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    let output = dir.path().join("compressed.pdf");
    fixture(&input);
    assert_ok(&compress(&input, &output, false));
    let render = |path: &Path, page: u32| {
        let prefix = dir.path().join("rendered");
        let result = Command::new("pdftoppm")
            .args(["-r", "100", "-singlefile", "-f", &page.to_string()])
            .arg(path)
            .arg(&prefix)
            .output()
            .unwrap();
        assert_ok(&result);
        fs::read(prefix.with_extension("ppm")).unwrap()
    };
    for page in 1..=2 {
        assert_eq!(render(&input, page), render(&output, page));
    }
}
