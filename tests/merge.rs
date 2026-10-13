use lopdf::{Document, Object, Stream, dictionary};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;

fn run(args: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pdfcli"))
        .args(args)
        .output()
        .unwrap()
}

fn merge(out: &Path, inputs: &[&Path]) -> Output {
    let mut args = vec![Path::new("merge"), Path::new("-o"), out];
    args.extend_from_slice(inputs);
    run(&args)
}

fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// The parent owns inherited geometry and font resources. Page object IDs are
// deliberately in the opposite order from Kids; ID sorting is not page order.
fn pdf(path: &Path, labels: &[&str]) {
    let mut doc = Document::with_version("1.7");
    let root = doc.new_object_id();
    let font = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let mut kids = Vec::new();
    for label in labels.iter().rev() {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 24 Tf 30 60 Td ({label}) Tj ET").into_bytes(),
        ));
        kids.insert(
            0,
            Object::Reference(doc.add_object(
                dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content },
            )),
        );
    }
    doc.objects.insert(
        root,
        dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => labels.len() as i64,
            "MediaBox" => vec![0.into(), 0.into(), 320.into(), 160.into()],
            "CropBox" => vec![10.into(), 10.into(), 310.into(), 150.into()],
            "Rotate" => 90,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
        }
        .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

fn labels(path: &Path) -> Vec<String> {
    let doc = Document::load(path).unwrap();
    doc.get_pages()
        .values()
        .map(|id| String::from_utf8(doc.get_page_content(*id)).unwrap())
        .collect()
}

fn edit_pages(command: &str, input: &Path, output: &Path, pages: &str, force: bool) -> Output {
    let mut args = vec![
        Path::new(command),
        input,
        Path::new("--pages"),
        Path::new(pages),
        Path::new("-o"),
        output,
    ];
    if force {
        args.push(Path::new("--force"));
    }
    run(&args)
}

#[test]
fn page_remove_keeps_remaining_order_and_original_bytes() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    let output = dir.path().join("removed.pdf");
    pdf(
        &input,
        &[
            "ONE",
            "REMOVE_TWO",
            "THREE",
            "REMOVE_FOUR",
            "REMOVE_FIVE",
            "SIX",
        ],
    );
    let original = fs::read(&input).unwrap();
    assert_ok(&edit_pages("remove", &input, &output, "2, 4-5", false));
    let actual = labels(&output);
    assert_eq!(actual.len(), 3);
    for (page, expected) in actual.iter().zip(["ONE", "THREE", "SIX"]) {
        assert!(page.contains(expected));
    }
    let written = fs::read(&output).unwrap();
    assert!(!String::from_utf8_lossy(&written).contains("REMOVE_"));
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[test]
fn page_reorder_accepts_mixed_and_reverse_ranges() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    let output = dir.path().join("reordered.pdf");
    pdf(&input, &["ONE", "TWO", "THREE", "FOUR", "FIVE"]);
    let original = fs::read(&input).unwrap();
    assert_ok(&edit_pages(
        "reorder",
        &input,
        &output,
        "5-4, 1, 3-2",
        false,
    ));
    let actual = labels(&output);
    assert_eq!(actual.len(), 5);
    for (page, expected) in actual.iter().zip(["FIVE", "FOUR", "ONE", "THREE", "TWO"]) {
        assert!(page.contains(expected));
    }
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[test]
fn page_reorder_partial_lists_keep_all_unlisted_pages_in_original_order() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("large.pdf");
    let markers: Vec<_> = (1..=200).map(|n| format!("PAGE_{n:03}")).collect();
    pdf(
        &input,
        &markers.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let original = fs::read(&input).unwrap();
    let source = labels(&input);
    for (index, (selection, first)) in [
        ("150", vec![150]),
        ("198-200,4-2", vec![198, 199, 200, 4, 3, 2]),
    ]
    .into_iter()
    .enumerate()
    {
        let output = dir.path().join(format!("reordered-{index}.pdf"));
        assert_ok(&edit_pages("reorder", &input, &output, selection, false));
        let order: Vec<_> = first
            .iter()
            .copied()
            .chain((1..=200).filter(|n| !first.contains(n)))
            .collect();
        let expected: Vec<_> = order.into_iter().map(|n| source[n - 1].clone()).collect();
        assert_eq!(labels(&output), expected);
    }
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn page_reorder_rest_places_unlisted_pages_at_the_requested_position() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    pdf(&input, &["ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX"]);
    let source = labels(&input);
    let original = fs::read(&input).unwrap();
    for (index, (selection, order)) in [
        ("rest,2,5", [1, 3, 4, 6, 2, 5]),
        ("4,rest,1", [4, 2, 3, 5, 6, 1]),
        ("rest", [1, 2, 3, 4, 5, 6]),
        ("ReSt,6-4", [1, 2, 3, 6, 5, 4]),
        ("6-4,rest,1", [6, 5, 4, 2, 3, 1]),
    ]
    .into_iter()
    .enumerate()
    {
        let output = dir.path().join(format!("reordered-{index}.pdf"));
        assert_ok(&edit_pages("reorder", &input, &output, selection, false));
        assert_eq!(labels(&output), order.map(|n| source[n - 1].clone()));
    }
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn page_reorder_rest_rejects_ambiguous_or_invalid_selections() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    let output = dir.path().join("output.pdf");
    pdf(&input, &["ONE", "TWO", "THREE"]);
    for (command, selection, message) in [
        ("remove", "rest", "only supported by reorder"),
        ("reorder", "rest,REST", "rest may appear only once"),
        ("reorder", "1,rest,1", "more than once"),
        ("reorder", "rest,4", "outside 1..=3"),
        ("reorder", "rest,", "empty page selection"),
        ("reorder", "1-rest", "invalid page number"),
    ] {
        fs::write(&output, b"keep output").unwrap();
        let result = edit_pages(command, &input, &output, selection, true);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read(&output).unwrap(), b"keep output");
    }
}

#[test]
fn page_edits_reject_invalid_selections_without_replacing_output() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    let output = dir.path().join("output.pdf");
    pdf(&input, &["ONE", "TWO", "THREE"]);
    for command in ["remove", "reorder"] {
        for (selection, message) in [
            ("", "empty"),
            ("0", "1..=3"),
            ("4", "1..=3"),
            ("1-4294967295", "1..=3"),
            ("-1", "invalid page"),
            ("1,", "empty"),
            ("1,,2", "empty"),
            ("one", "invalid page"),
            ("1-2-3", "invalid page"),
            ("1,1", "more than once"),
            ("1-2,2", "more than once"),
        ] {
            fs::write(&output, b"keep existing output").unwrap();
            let result = edit_pages(command, &input, &output, selection, true);
            assert!(!result.status.success(), "Accepted {command} {selection:?}");
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(message),
                "Wrong error for {command} {selection:?}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(fs::read(&output).unwrap(), b"keep existing output");
        }
    }
    let result = edit_pages("remove", &input, &output, "1-3", true);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("at least one page"));
    assert_eq!(fs::read(&output).unwrap(), b"keep existing output");
}

#[test]
fn page_edits_protect_inputs_aliases_and_existing_outputs() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    let output = dir.path().join("output.pdf");
    let alias = dir.path().join("alias.pdf");
    pdf(&input, &["ONE", "TWO"]);
    let original = fs::read(&input).unwrap();
    fs::hard_link(&input, &alias).unwrap();
    for (command, selection, count) in [("remove", "1", 1), ("reorder", "2,1", 2)] {
        for target in [&input, &alias] {
            let result = edit_pages(command, &input, target, selection, true);
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("must not replace an input"));
        }
        fs::write(&output, b"keep me").unwrap();
        let result = edit_pages(command, &input, &output, selection, false);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("already exists"));
        assert_eq!(fs::read(&output).unwrap(), b"keep me");
        assert_ok(&edit_pages(command, &input, &output, selection, true));
        assert_eq!(labels(&output).len(), count);
        assert_eq!(fs::read(&input).unwrap(), original);
    }
}

#[test]
fn page_edits_preserve_kept_links_and_disable_links_to_removed_pages() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("links.pdf");
    pdf(&input, &["ONE", "REMOVED_CONTENT", "THREE"]);
    let mut doc = Document::load(&input).unwrap();
    let pages = doc.get_pages();
    let gone = doc.add_object(vec![pages[&2].into(), Object::Name(b"Fit".to_vec())]);
    let action = doc.add_object(dictionary! { "S" => "GoTo", "D" => gone });
    let mut annots: Vec<Object> = [
        dictionary! { "Subtype" => "Link", "Dest" => gone },
        dictionary! { "Subtype" => "Link", "A" => action },
        dictionary! { "Subtype" => "Link", "A" => dictionary! { "S" => "GoTo", "D" => Object::string_literal("kept") } },
        dictionary! { "Subtype" => "Link", "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com") } },
    ].into_iter().map(|mut d| {
        d.set("Type", "Annot");
        d.set("Rect", vec![0.into(), 0.into(), 10.into(), 10.into()]);
        doc.add_object(d).into()
    }).collect();
    // Inline annotations must also lose destinations to deleted pages.
    annots.push(
        dictionary! {
            "Subtype" => "Link", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Dest" => gone
        }
        .into(),
    );
    annots.push(
        dictionary! {
            "Subtype" => "Link", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "A" => dictionary! { "S" => "GoTo", "D" => gone }
        }
        .into(),
    );
    doc.get_dictionary_mut(pages[&1])
        .unwrap()
        .set("Annots", annots);
    doc.catalog_mut().unwrap().set(
        "Dests",
        dictionary! {
            "kept" => vec![pages[&3].into(), Object::Name(b"Fit".to_vec())]
        },
    );
    doc.save(&input).unwrap();
    for (command, selection, first, target, gone_target) in [
        ("remove", "2", 1, 2, 0),
        ("reorder", "3,2,1", 3, 1, 2),
        ("reorder", "3", 2, 1, 3),
        ("reorder", "rest,2", 1, 2, 3),
    ] {
        let output = dir.path().join(format!("{command}-{selection}.pdf"));
        assert_ok(&edit_pages(command, &input, &output, selection, false));
        let doc = Document::load(&output).unwrap();
        let pages = doc.get_pages();
        let annotations = doc.get_page_annotations(pages[&first]).unwrap();
        let kept = annotations[2]
            .get(b"A")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"D")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(kept[0].as_reference().unwrap(), pages[&target]);
        assert_eq!(
            annotations[3]
                .get(b"A")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"S")
                .unwrap()
                .as_name()
                .unwrap(),
            b"URI"
        );
        if command == "remove" {
            assert!(!annotations[4].has(b"Dest"));
            assert!(!annotations[5].has(b"A"));
            assert!(!annotations[0].has(b"Dest"));
            assert!(!annotations[1].has(b"A"));
            assert!(
                !String::from_utf8_lossy(&fs::read(&output).unwrap()).contains("REMOVED_CONTENT")
            );
        } else {
            let destination = doc
                .dereference(annotations[0].get(b"Dest").unwrap())
                .unwrap()
                .1
                .as_array()
                .unwrap();
            assert_eq!(destination[0].as_reference().unwrap(), pages[&gone_target]);
        }
    }
}

#[test]
#[ignore = "requires Poppler pdftoppm"]
fn page_edits_render_identically_with_inherited_and_overridden_layout() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("layout.pdf");
    pdf(&input, &["ONE", "TWO", "THREE"]);
    let mut doc = Document::load(&input).unwrap();
    let second = doc.get_pages()[&2];
    let page = doc.get_dictionary_mut(second).unwrap();
    page.set("Rotate", 0);
    page.set("MediaBox", vec![0.into(), 0.into(), 400.into(), 240.into()]);
    page.set("CropBox", vec![0.into(), 0.into(), 380.into(), 220.into()]);
    doc.save(&input).unwrap();
    let render = |path: &Path, page: usize| {
        let prefix = dir.path().join("rendered");
        let result = Command::new("pdftoppm")
            .args(["-r", "72", "-singlefile", "-f", &page.to_string()])
            .arg(path)
            .arg(&prefix)
            .output()
            .unwrap();
        assert_ok(&result);
        fs::read(prefix.with_extension("ppm")).unwrap()
    };
    for (command, selection, expected) in [
        ("remove", "1", vec![2, 3]),
        ("reorder", "3,1,2", vec![3, 1, 2]),
        ("reorder", "3", vec![3, 1, 2]),
        ("reorder", "rest,1", vec![2, 3, 1]),
    ] {
        let output = dir.path().join(format!("{command}-{selection}.pdf"));
        assert_ok(&edit_pages(command, &input, &output, selection, false));
        for (index, source) in expected.into_iter().enumerate() {
            assert_eq!(render(&input, source), render(&output, index + 1));
        }
    }
}

#[test]
fn preserves_input_and_page_tree_order_including_duplicates() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.PDF");
    let out = dir.path().join("merged.pdf");
    pdf(&a, &["FIRST", "SECOND"]);
    pdf(&b, &["THIRD"]);
    assert_ok(&merge(&out, &[&a, &b, &a]));
    let actual = labels(&out);
    assert_eq!(actual.len(), 5);
    for (page, expected) in actual
        .iter()
        .zip(["FIRST", "SECOND", "THIRD", "FIRST", "SECOND"])
    {
        assert!(page.contains(expected));
    }
    let doc = Document::load(&out).unwrap();
    for page in doc.get_pages().values() {
        let parent = doc
            .get_dictionary(*page)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let inherited = doc.get_dictionary(parent).unwrap();
        // lopdf's font convenience lookup ignores directly embedded Resources
        // on ancestors, so inspect the inherited dictionary itself.
        let fonts = inherited
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap();
        let font = fonts.get(b"F1").unwrap().as_reference().unwrap();
        assert_eq!(
            doc.get_dictionary(font)
                .unwrap()
                .get(b"BaseFont")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Helvetica"
        );
        assert_eq!(inherited.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
        assert_eq!(
            inherited.get(b"MediaBox").unwrap().as_array().unwrap()[2]
                .as_i64()
                .unwrap(),
            320
        );
        assert!(inherited.has(b"CropBox"));
    }
}

#[test]
fn folder_merge_sorts_filenames_and_includes_only_direct_pdf_files() {
    let dir = tempdir().unwrap();
    let folder = dir.path().join("PDF inputs Ω");
    fs::create_dir(&folder).unwrap();
    let mut originals = Vec::new();
    for (name, marker) in [
        ("z.pdf", "LAST"),
        ("2.pdf", "TWO"),
        ("a appendix.pdf", "LOWERCASE"),
        ("10.PDF", "TEN"),
        ("A cover.pdf", "UPPERCASE"),
    ] {
        let path = folder.join(name);
        pdf(&path, &[marker]);
        originals.push((path.clone(), fs::read(path).unwrap()));
    }
    fs::write(folder.join("ignored.docx"), b"not an Office document").unwrap();
    fs::write(folder.join("ignored.pdf.txt"), b"not a PDF").unwrap();
    let nested = folder.join("nested.pdf");
    fs::create_dir(&nested).unwrap();
    pdf(&nested.join("nested.pdf"), &["NESTED"]);
    let out = dir.path().join("combined.pdf");
    assert_ok(&run(&[
        Path::new("merge"),
        Path::new("--folder"),
        &folder,
        Path::new("-o"),
        &out,
    ]));
    let pages = labels(&out);
    assert_eq!(pages.len(), 5);
    for (page, marker) in pages
        .iter()
        .zip(["TEN", "TWO", "UPPERCASE", "LOWERCASE", "LAST"])
    {
        assert!(page.contains(marker), "Incorrect filename order: {page}");
    }
    for (path, original) in originals {
        assert_eq!(fs::read(path).unwrap(), original);
    }
}

#[test]
fn folder_merge_excludes_existing_destination_and_its_aliases() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.pdf");
    pdf(&source, &["SOURCE ONE", "SOURCE TWO"]);
    let original = fs::read(&source).unwrap();
    let output = dir.path().join("./combined.pdf");
    fs::write(&output, b"previous output, not a PDF").unwrap();
    let alias = dir.path().join("old-output-link.pdf");
    fs::hard_link(&output, &alias).unwrap();
    let args = [
        Path::new("merge"),
        Path::new("--folder"),
        dir.path(),
        Path::new("--force"),
        Path::new("-o"),
        &output,
    ];
    assert_ok(&run(&args));
    assert_eq!(labels(&output).len(), 2);
    assert_eq!(fs::read(&alias).unwrap(), b"previous output, not a PDF");
    fs::remove_file(alias).unwrap();
    assert_ok(&run(&args));
    assert_eq!(
        labels(&output).len(),
        2,
        "Rerunning must not append the previous output"
    );
    assert_eq!(fs::read(source).unwrap(), original);
}

#[test]
fn folder_merge_rejects_missing_non_directory_and_empty_inputs() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("out.pdf");
    fs::write(&out, b"keep output").unwrap();
    for (folder, expected) in [
        (dir.path().join("missing"), "not a directory"),
        (out.clone(), "not a directory"),
        (dir.path().to_owned(), "no PDF input files"),
    ] {
        let result = run(&[
            Path::new("merge"),
            Path::new("--folder"),
            &folder,
            Path::new("--force"),
            Path::new("-o"),
            &out,
        ]);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read(&out).unwrap(), b"keep output");
    }
}

#[test]
fn folder_merge_cannot_be_combined_with_explicit_inputs() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("source.pdf");
    pdf(&input, &["SOURCE"]);
    let out = dir.path().join("out.pdf");
    let result = run(&[
        Path::new("merge"),
        Path::new("--folder"),
        dir.path(),
        Path::new("-o"),
        &out,
        &input,
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be used with"));
    assert!(!out.exists());
}

#[test]
fn folder_merge_bad_pdf_leaves_existing_output_untouched() {
    let dir = tempdir().unwrap();
    pdf(&dir.path().join("01-good.pdf"), &["GOOD"]);
    fs::write(dir.path().join("02-broken.pdf"), b"not a PDF").unwrap();
    let out = dir.path().join("out.pdf");
    fs::write(&out, b"keep output").unwrap();
    let result = run(&[
        Path::new("merge"),
        Path::new("--folder"),
        dir.path(),
        Path::new("--force"),
        Path::new("-o"),
        &out,
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("02-broken.pdf"));
    assert_eq!(fs::read(out).unwrap(), b"keep output");
}

#[test]
fn refuses_overwrite_and_input_alias_even_with_force() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let out = dir.path().join("output.pdf");
    pdf(&a, &["A"]);
    fs::write(&out, b"existing output").unwrap();
    assert!(!merge(&out, &[&a]).status.success());
    assert_eq!(fs::read(&out).unwrap(), b"existing output");
    assert_ok(&run(&[
        Path::new("merge"),
        Path::new("--force"),
        Path::new("-o"),
        &out,
        &a,
    ]));
    assert_eq!(labels(&out).len(), 1);
    let before = fs::read(&a).unwrap();
    assert!(
        !run(&[
            Path::new("merge"),
            Path::new("--force"),
            Path::new("-o"),
            &a,
            &a
        ])
        .status
        .success()
    );
    assert_eq!(fs::read(&a).unwrap(), before);
    let alias = dir.path().join("alias.pdf");
    fs::hard_link(&a, &alias).unwrap();
    assert!(
        !run(&[
            Path::new("merge"),
            Path::new("--force"),
            Path::new("-o"),
            &alias,
            &a
        ])
        .status
        .success()
    );
}

#[test]
fn invalid_input_never_replaces_existing_output() {
    let dir = tempdir().unwrap();
    let bad = dir.path().join("broken.pdf");
    let out = dir.path().join("out.pdf");
    fs::write(&bad, b"not a PDF").unwrap();
    fs::write(&out, b"keep me").unwrap();
    let result = run(&[
        Path::new("merge"),
        Path::new("--force"),
        Path::new("-o"),
        &out,
        &bad,
    ]);
    assert!(!result.status.success());
    assert_eq!(fs::read(&out).unwrap(), b"keep me");
}

#[test]
fn rejects_forms_instead_of_silently_breaking_fields() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("form.pdf");
    let out = dir.path().join("out.pdf");
    pdf(&input, &["FORM"]);
    let mut doc = Document::load(&input).unwrap();
    let field = doc.add_object(dictionary! { "FT" => "Tx", "T" => Object::string_literal("name"), "V" => Object::string_literal("Ada") });
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(root).unwrap().set(
        "AcroForm",
        dictionary! { "Fields" => vec![Object::Reference(field)] },
    );
    doc.save(&input).unwrap();
    let result = merge(&out, &[&input]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("AcroForm"));
    assert!(!out.exists());
}

#[test]
fn validates_cli_and_extensions() {
    assert!(!run(&[Path::new("merge")]).status.success());
    let dir = tempdir().unwrap();
    let input = dir.path().join("image.png");
    fs::write(&input, b"anything").unwrap();
    assert!(
        !merge(&dir.path().join("out.pdf"), &[&input])
            .status
            .success()
    );
    assert!(!dir.path().join("out.pdf").exists());
}

#[test]
fn rejects_broken_page_trees_instead_of_dropping_pages() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("broken-tree.pdf");
    let out = dir.path().join("out.pdf");
    pdf(&input, &["ONE", "TWO"]);
    let mut doc = Document::load(&input).unwrap();
    let root = doc
        .catalog()
        .unwrap()
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(root)
        .unwrap()
        .get_mut(b"Kids")
        .unwrap()
        .as_array_mut()
        .unwrap()[1] = Object::Reference((99999, 0));
    doc.save(&input).unwrap();
    assert!(!merge(&out, &[&input]).status.success());
    assert!(!out.exists());
}

#[test]
fn resolves_named_links_separately_for_each_input() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.pdf");
    let out = dir.path().join("out.pdf");
    for (input, legacy) in [(&a, false), (&b, true)] {
        pdf(input, &["ONE", "TWO"]);
        let mut doc = Document::load(input).unwrap();
        let pages = doc.get_pages();
        let destination = Object::Array(vec![pages[&2].into(), Object::Name(b"Fit".to_vec())]);
        let link_target = if legacy {
            doc.add_object(Object::string_literal("chapter")).into()
        } else {
            Object::string_literal("chapter")
        };
        let annotation = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Link", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "A" => dictionary! { "S" => "GoTo", "D" => link_target }
        });
        doc.get_dictionary_mut(pages[&1])
            .unwrap()
            .set("Annots", vec![annotation.into()]);
        if legacy {
            doc.catalog_mut().unwrap().set(
                "Dests",
                dictionary! { "chapter" => dictionary! { "D" => destination } },
            );
        } else {
            let leaf = doc.add_object(
                dictionary! { "Names" => vec![Object::string_literal("chapter"), destination] },
            );
            doc.catalog_mut().unwrap().set(
                "Names",
                dictionary! { "Dests" => dictionary! { "Kids" => vec![leaf.into()] } },
            );
        }
        doc.save(input).unwrap();
    }
    assert_ok(&merge(&out, &[&a, &b]));
    let doc = Document::load(&out).unwrap();
    let pages = doc.get_pages();
    for (first, target) in [(1, 2), (3, 4)] {
        let annotation = doc.get_page_annotations(pages[&first]).unwrap()[0];
        let dest = annotation
            .get(b"A")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"D")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(dest[0].as_reference().unwrap(), pages[&target]);
    }
}

#[test]
fn rejects_orphaned_widgets() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("widget.pdf");
    let out = dir.path().join("out.pdf");
    pdf(&input, &["ONE"]);
    let mut doc = Document::load(&input).unwrap();
    let page = doc.get_pages()[&1];
    let widget = doc.add_object(
        dictionary! { "Subtype" => "Widget", "FT" => "Tx", "T" => Object::string_literal("field") },
    );
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![widget.into()]);
    doc.save(&input).unwrap();
    assert!(!merge(&out, &[&input]).status.success());
}

#[test]
fn rejects_encryption_even_with_an_empty_open_password() {
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};
    let dir = tempdir().unwrap();
    let input = dir.path().join("encrypted.pdf");
    let out = dir.path().join("out.pdf");
    for password in ["", "secret"] {
        pdf(&input, &["ONE"]);
        let mut doc = Document::load(&input).unwrap();
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
            user_password: password,
            permissions: Permissions::all(),
        })
        .unwrap();
        doc.encrypt(&state).unwrap();
        doc.save(&input).unwrap();
        assert!(!merge(&out, &[&input]).status.success());
        assert!(!out.exists());
    }
}

#[test]
#[ignore = "requires Poppler pdftoppm"]
fn rendered_pages_match_their_sources() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.pdf");
    let out = dir.path().join("out.pdf");
    pdf(&a, &["FIRST", "SECOND"]);
    pdf(&b, &["THIRD"]);
    assert_ok(&merge(&out, &[&a, &b]));
    let render = |input: &Path, page: &str, name: &str| {
        let prefix = dir.path().join(name);
        let result = Command::new("pdftoppm")
            .args(["-r", "72", "-singlefile", "-f", page])
            .arg(input)
            .arg(&prefix)
            .output()
            .unwrap();
        assert_ok(&result);
        fs::read(prefix.with_extension("ppm")).unwrap()
    };
    for (input, source, merged) in [(&a, "1", "1"), (&a, "2", "2"), (&b, "1", "3")] {
        assert_eq!(
            render(input, source, "source"),
            render(&out, merged, "merged"),
            "render changed for merged page {merged}"
        );
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn office_missing_output_and_timeout_are_errors() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let input = dir.path().join("document.odt");
    let out = dir.path().join("out.pdf");
    let converter = dir.path().join("fake-soffice");
    fs::write(&input, b"fixture").unwrap();
    fs::write(&converter, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&converter, fs::Permissions::from_mode(0o755)).unwrap();
    let args = [
        Path::new("merge"),
        Path::new("--soffice"),
        &converter,
        Path::new("--timeout"),
        Path::new("1"),
        Path::new("-o"),
        &out,
        &input,
    ];
    let result = run(&args);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("produce"));
    fs::write(&converter, "#!/bin/sh\nsleep 30\n").unwrap();
    let start = std::time::Instant::now();
    let result = run(&args);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("timed out"));
    assert!(start.elapsed().as_secs() < 10);
    assert!(!out.exists());
}

#[test]
#[ignore = "requires Microsoft Office on macOS/Windows, LibreOffice on Linux"]
fn real_office_converts_docx_xlsx_pptx_and_same_named_inputs() {
    let dir = tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let originals: Vec<_> = ["sample.docx", "sample.xlsx", "sample.pptx"]
        .into_iter()
        .map(|name| (fixture.join(name), fs::read(fixture.join(name)).unwrap()))
        .collect();
    let a = dir.path().join("before.pdf");
    let out = dir.path().join("mixed.pdf");
    pdf(&a, &["PDF FIRST"]);
    let same = dir.path().join("same name");
    fs::create_dir(&same).unwrap();
    fs::copy(fixture.join("sample.docx"), same.join("sample.docx")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_pdfcli"));
    command.arg("merge").arg("-o").arg(&out).args([
        &a,
        &fixture.join("sample.docx"),
        &fixture.join("sample.xlsx"),
        &fixture.join("sample.pptx"),
        &same.join("sample.docx"),
    ]);
    #[cfg(target_os = "macos")]
    command.env("PATH", "/usr/bin:/bin");
    #[cfg(any(target_os = "macos", windows))]
    command.env("PDFCLI_SOFFICE", "/not-a-libreoffice-executable");
    assert_ok(&command.output().unwrap());
    for (path, before) in originals {
        assert_eq!(fs::read(path).unwrap(), before, "Office input was modified");
    }
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 5);
    assert!(labels(&out)[0].contains("PDF FIRST"));
    for (number, text) in [
        (2, "WORD MARKER"),
        (3, "SHEET MARKER"),
        (4, "SLIDE MARKER"),
        (5, "WORD MARKER"),
    ] {
        assert!(
            doc.extract_text(&[number]).unwrap().contains(text),
            "missing {text} on page {number}"
        );
    }
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn help_describes_microsoft_preference_and_libreoffice_fallback() {
    let result = run(&[Path::new("merge"), Path::new("--help")]);
    assert_ok(&result);
    let help = String::from_utf8_lossy(&result.stdout);
    assert!(help.contains("Microsoft"));
    assert!(help.contains("LibreOffice"));
    assert!(help.contains("--soffice"));
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn office_fallback_converts_all_opendocument_types_in_order() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let converter = dir.path().join("fake soffice");
    // Stand in only for the external renderer; exercise argument building,
    // per-input staging, renderer selection and the real PDF merge.
    fs::write(
        &converter,
        r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in
        pdf:*) filter="${arg#pdf:}" ;;
    esac
    input="$arg"
done
cp "$(dirname "$0")/$filter.pdf" "${input%.*}.pdf"
"#,
    )
    .unwrap();
    fs::set_permissions(&converter, fs::Permissions::from_mode(0o755)).unwrap();
    for (filter, label) in [
        ("writer_pdf_Export", "WRITER"),
        ("calc_pdf_Export", "CALC"),
        ("impress_pdf_Export", "IMPRESS"),
    ] {
        pdf(&dir.path().join(format!("{filter}.pdf")), &[label]);
    }
    let inputs: Vec<_> = ["odt", "ODS", "odp"]
        .into_iter()
        .map(|ext| {
            let input = dir.path().join(format!("same name.{ext}"));
            fs::write(&input, b"original document").unwrap();
            input
        })
        .collect();
    let out = dir.path().join("out.pdf");
    let result = Command::new(env!("CARGO_BIN_EXE_pdfcli"))
        .args(["merge", "-o"])
        .arg(&out)
        .args(&inputs)
        .env("PDFCLI_SOFFICE", &converter)
        .output()
        .unwrap();
    assert_ok(&result);
    let actual = labels(&out);
    assert_eq!(actual.len(), 3);
    for (page, expected) in actual.iter().zip(["WRITER", "CALC", "IMPRESS"]) {
        assert!(page.contains(expected));
    }
    for input in inputs {
        assert_eq!(fs::read(input).unwrap(), b"original document");
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn office_fallback_reports_missing_executable_without_replacing_output() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("document.odt");
    let out = dir.path().join("out.pdf");
    fs::write(&input, b"fixture").unwrap();
    fs::write(&out, b"keep existing output").unwrap();
    let result = run(&[
        Path::new("merge"),
        Path::new("--force"),
        Path::new("--soffice"),
        &dir.path().join("missing-soffice"),
        Path::new("-o"),
        &out,
        &input,
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("LibreOffice executable not found"));
    assert_eq!(fs::read(&out).unwrap(), b"keep existing output");
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "requires installed Microsoft Office apps and desktop automation"]
fn real_office_prefers_microsoft_for_opendocument_files() {
    let dir = tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let inputs: Vec<_> = ["sample.odt", "sample.ods", "sample.odp"]
        .iter()
        .map(|name| fixture.join(name))
        .collect();
    let originals: Vec<_> = inputs.iter().map(|p| fs::read(p).unwrap()).collect();
    let out = dir.path().join("out.pdf");
    let result = Command::new(env!("CARGO_BIN_EXE_pdfcli"))
        .args(["merge", "--timeout", "30", "-o"])
        .arg(&out)
        .args(&inputs)
        .env("PDFCLI_SOFFICE", dir.path().join("missing-soffice"))
        .output()
        .unwrap();
    assert_ok(&result);
    let doc = Document::load(out).unwrap();
    assert_eq!(doc.get_pages().len(), 3);
    for (index, marker) in ["WORD MARKER", "SHEET MARKER", "SLIDE MARKER"]
        .iter()
        .enumerate()
    {
        assert!(
            doc.extract_text(&[index as u32 + 1])
                .unwrap()
                .contains(marker)
        );
    }
    for (input, original) in inputs.iter().zip(originals) {
        assert_eq!(fs::read(input).unwrap(), original);
    }
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_exports_all_visible_sheets() {
    let dir = tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multiple-sheets.xlsx");
    let out = dir.path().join("sheets.pdf");
    assert_ok(&merge(&out, &[&input]));
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
    assert!(doc.extract_text(&[1]).unwrap().contains("FIRST SHEET"));
    assert!(doc.extract_text(&[2]).unwrap().contains("SECOND SHEET"));
    assert!(!doc.extract_text(&[1, 2]).unwrap().contains("HIDDEN SHEET"));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_fits_columns_and_exports_mixed_paper_without_blank_pages() {
    let dir = tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/excel-pagination.xlsx");
    let before = fs::read(&input).unwrap();
    let out = dir.path().join("pagination.pdf");
    assert_ok(&run(&[
        Path::new("merge"),
        Path::new("--excel-fit-width"),
        Path::new("-o"),
        &out,
        &input,
    ]));
    assert_eq!(fs::read(&input).unwrap(), before);
    let doc = Document::load(&out).unwrap();
    assert_eq!(
        doc.get_pages().len(),
        2,
        "Expected one page for each nonempty visible sheet"
    );
    for (page, markers) in [
        (1, ["WIDE LEFT", "WIDE RIGHT", "BOTTOM"]),
        (2, ["FOLIO LEFT", "FOLIO RIGHT", "FOLIO BOTTOM"]),
    ] {
        let text = doc.extract_text(&[page]).unwrap();
        for marker in markers {
            assert!(
                text.contains(marker),
                "Page {page} missing {marker}: {text:?}"
            );
        }
        assert!(!text.contains("HIDDEN MARKER"));
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_preserve_layout_keeps_print_preview_pagination() {
    let dir = tempdir().unwrap();
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/excel-print-preview.xlsx");
    let original = fs::read(&input).unwrap();
    let output = dir.path().join("preview.pdf");
    assert_ok(&run(&[
        Path::new("merge"),
        Path::new("--excel-preserve-layout"),
        Path::new("-o"),
        &output,
        &input,
    ]));
    let doc = Document::load(&output).unwrap();
    assert_eq!(
        doc.get_pages().len(),
        5,
        "Manual row and column breaks must produce four pages from the first sheet"
    );
    for (index, marker) in [
        "TOP LEFT",
        "BOTTOM LEFT",
        "TOP RIGHT",
        "BOTTOM RIGHT",
        "LANDSCAPE SHEET",
    ]
    .iter()
    .enumerate()
    {
        assert!(
            doc.extract_text(&[index as u32 + 1])
                .unwrap()
                .contains(marker),
            "Wrong print-preview page {}",
            index + 1
        );
    }
    let text = doc.extract_text(&[1, 2, 3, 4, 5]).unwrap();
    assert!(!text.contains("OUTSIDE PRINT AREA"));
    assert!(!text.contains("HIDDEN SHEET"));
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_preserves_each_sheets_paper_and_nonblank_print_pages() {
    let dir = tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/excel-mixed-paper.xlsx");
    let original = fs::read(&input).unwrap();
    let output = dir.path().join("mixed.pdf");
    assert_ok(&run(&[
        Path::new("merge"),
        Path::new("--excel-preserve-layout"),
        Path::new("-o"),
        &output,
        &input,
    ]));
    let doc = Document::load(&output).unwrap();
    let pages = doc.get_pages();
    let text = doc
        .extract_text(&pages.keys().copied().collect::<Vec<_>>())
        .unwrap();
    assert!(
        text.contains("LANDSCAPE VAT MARKER"),
        "Mixed paper sizes must not drop a visible sheet"
    );
    assert_eq!(
        pages.len(),
        5,
        "Keep two A4 pages, Letter, landscape Folio and an image-only page; drop blank overflow"
    );
    for (page, marker) in [
        (1, "FIRST PRINT PAGE"),
        (2, "SECOND PRINT PAGE"),
        (3, "LETTER TOP"),
        (5, "LANDSCAPE VAT MARKER"),
    ] {
        assert!(doc.extract_text(&[page]).unwrap().contains(marker));
    }
    assert!(!text.contains("HIDDEN MARKER"));
    for (number, expected) in [
        (1, (595.28, 841.89)),
        (3, (612.0, 792.0)),
        (5, (936.0, 612.0)),
    ] {
        let mut node = pages[&number];
        let bounds = loop {
            let dict = doc.get_dictionary(node).unwrap();
            if let Ok(bounds) = dict.get(b"MediaBox") {
                break bounds.as_array().unwrap();
            }
            node = dict.get(b"Parent").unwrap().as_reference().unwrap();
        };
        let number = |v: &Object| match v {
            Object::Integer(v) => *v as f64,
            Object::Real(v) => *v as f64,
            _ => panic!("invalid page box"),
        };
        assert!((number(&bounds[2]) - number(&bounds[0]) - expected.0).abs() < 0.1);
        assert!((number(&bounds[3]) - number(&bounds[1]) - expected.1).abs() < 0.1);
    }
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_table_sections_keep_columns_together_and_continue_downward() {
    let dir = tempdir().unwrap();
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/excel-table-sections.xlsx");
    let original = fs::read(&input).unwrap();
    let output = dir.path().join("tables.pdf");
    assert_ok(&run(&[
        Path::new("merge"),
        Path::new("--excel-print-area"),
        Path::new("Parallel tables=A1:D90"),
        Path::new("--excel-print-area"),
        Path::new("Parallel tables=F1:I90"),
        Path::new("-o"),
        &output,
        &input,
    ]));
    let doc = Document::load(&output).unwrap();
    let pages: Vec<_> = doc
        .get_pages()
        .keys()
        .map(|n| doc.extract_text(&[*n]).unwrap())
        .collect();
    for side in ["LEFT", "RIGHT"] {
        let section: Vec<_> = pages
            .iter()
            .enumerate()
            .filter(|(_, text)| text.contains(&format!("{side} ROW")))
            .collect();
        assert!(section.len() >= 2, "Long tables must continue vertically");
        for row in 1..=90 {
            let matching: Vec<_> = section
                .iter()
                .filter(|(_, text)| text.contains(&format!("{side} ROW {row:02}")))
                .collect();
            assert_eq!(matching.len(), 1, "Every row must appear once");
            assert!(
                matching[0].1.contains(&format!("{side} VALUE {row:02}")),
                "All table columns must stay on the same page"
            );
        }
    }
    let last_left = pages.iter().rposition(|t| t.contains("LEFT ROW")).unwrap();
    let first_right = pages.iter().position(|t| t.contains("RIGHT ROW")).unwrap();
    assert!(
        last_left < first_right,
        "Print sections follow the supplied order"
    );
    assert!(pages.last().unwrap().contains("SUMMARY MARKER"));
    assert!(!pages.join("\n").contains("OUTSIDE SECTIONS"));
    assert!(pages.iter().all(|t| !t.trim().is_empty()));
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_automatically_detects_tables_by_default() {
    let dir = tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/excel-auto-tables.xlsx");
    let original = fs::read(&input).unwrap();
    let output = dir.path().join("automatic.pdf");
    assert_ok(&merge(&output, &[&input]));
    let doc = Document::load(&output).unwrap();
    let pages: Vec<_> = doc
        .get_pages()
        .keys()
        .map(|n| doc.extract_text(&[*n]).unwrap())
        .collect();
    let adjacent = pages
        .iter()
        .position(|s| s.contains("LEFT TABLE HEADING"))
        .unwrap();
    let shared = pages
        .iter()
        .position(|s| s.contains("SHARED TABLE HEADING"))
        .unwrap();
    for section in [&pages[..adjacent], &pages[adjacent..shared]] {
        for side in ["LEFT", "RIGHT"] {
            let table: Vec<_> = section
                .iter()
                .filter(|s| s.contains(&format!("{side} ROW")))
                .collect();
            assert!(table.len() >= 2, "Long tables must flow vertically");
            for row in 1..=90 {
                let matching: Vec<_> = table
                    .iter()
                    .filter(|s| s.contains(&format!("{side} ROW {row:02}")))
                    .collect();
                assert_eq!(matching.len(), 1, "Every row must be retained exactly once");
                assert!(
                    matching[0].contains(&format!("{side} VALUE {row:02}")),
                    "Table columns split across pages"
                );
            }
        }
        assert!(
            section
                .iter()
                .rposition(|s| s.contains("LEFT ROW"))
                .unwrap()
                < section
                    .iter()
                    .position(|s| s.contains("RIGHT ROW"))
                    .unwrap(),
            "Independent tables must print separately in column order"
        );
    }
    for row in 1..=90 {
        let matching: Vec<_> = pages[shared..]
            .iter()
            .filter(|s| s.contains(&format!("LEFT ROW {row:02}")))
            .collect();
        assert_eq!(matching.len(), 1);
        for marker in ["LEFT VALUE", "RIGHT ROW", "RIGHT VALUE"] {
            assert!(
                matching[0].contains(&format!("{marker} {row:02}")),
                "Shared heading must prevent cuts through one wide table"
            );
        }
    }
    assert!(pages.iter().all(|s| !s.trim().is_empty()));
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_detects_native_tables_and_protects_spanning_images() {
    let dir = tempdir().unwrap();
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/excel-auto-objects.xlsx");
    let original = fs::read(&input).unwrap();
    let output = dir.path().join("objects.pdf");
    assert_ok(&merge(&output, &[&input]));
    let doc = Document::load(&output).unwrap();
    let pages: Vec<_> = doc
        .get_pages()
        .keys()
        .map(|n| doc.extract_text(&[*n]).unwrap())
        .collect();
    for side in ["LEFT", "RIGHT"] {
        assert!(
            pages
                .iter()
                .filter(|s| s.contains(&format!("{side} ROW")))
                .count()
                >= 2
        );
        for row in 1..=90 {
            let matching: Vec<_> = pages
                .iter()
                .filter(|s| s.contains(&format!("{side} ROW {row:02}")))
                .collect();
            assert_eq!(matching.len(), 1);
            assert!(matching[0].contains(&format!("{side} VALUE {row:02}")));
        }
    }
    assert!(
        pages.iter().rposition(|s| s.contains("LEFT ROW")).unwrap()
            < pages.iter().position(|s| s.contains("RIGHT ROW")).unwrap()
    );
    let graphic = pages.last().unwrap();
    for marker in ["GRAPHIC LEFT", "GRAPHIC RIGHT", "LEFT VALUE", "RIGHT VALUE"] {
        assert!(
            graphic.contains(marker),
            "A spanning graphic must protect the complete table width"
        );
    }
    let last_page = *doc.get_pages().values().last().unwrap();
    let content = lopdf::content::Content::decode(&doc.get_page_content(last_page)).unwrap();
    assert!(
        content.operations.iter().any(|op| op.operator == "Do"),
        "Image must remain painted"
    );
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[cfg(any(target_os = "macos", windows))]
fn assert_native_legacy_conversion(name: &str, expected: &[&str]) {
    let dir = tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let original = fs::read(&fixture).unwrap();
    // Check genuine binary Office input, not OOXML renamed to a legacy suffix.
    assert!(original.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]));
    let input_directory = dir.path().join("legacy inputs Ω");
    fs::create_dir(&input_directory).unwrap();
    let input = input_directory.join(name);
    fs::copy(&fixture, &input).unwrap();
    let out = dir.path().join("legacy.pdf");
    let mut command = Command::new(env!("CARGO_BIN_EXE_pdfcli"));
    command
        .args(["merge", "-o"])
        .arg(&out)
        .arg(&input)
        .env("PDFCLI_SOFFICE", "/not-a-libreoffice-executable");
    #[cfg(target_os = "macos")]
    command.env("PATH", "/usr/bin:/bin");
    let result = command.output().unwrap();
    assert_ok(&result);
    assert_eq!(fs::read(&input).unwrap(), original, "Legacy source changed");
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), expected.len());
    for (index, marker) in expected.iter().enumerate() {
        let text = doc.extract_text(&[index as u32 + 1]).unwrap();
        assert!(
            text.contains(marker),
            "page {} expected {marker:?}, got {text:?}",
            index + 1
        );
    }
    assert!(
        !doc.extract_text(&doc.get_pages().keys().copied().collect::<Vec<_>>())
            .unwrap()
            .contains("HIDDEN SHEET")
    );
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "requires installed Microsoft Word and Automation permission"]
fn real_office_legacy_doc() {
    assert_native_legacy_conversion("sample.doc", &["WORD MARKER"]);
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_legacy_xls_visible_sheets() {
    assert_native_legacy_conversion("multiple-sheets.xls", &["FIRST SHEET", "SECOND SHEET"]);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires installed Microsoft Excel and Automation permission"]
fn real_office_excel_single_cell_print_areas_do_not_prompt() {
    let dir = tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multiple-sheets.xls");
    let original = fs::read(&input).unwrap();
    let output = dir.path().join("single-cells.pdf");
    assert_ok(&run(&[
        Path::new("merge"),
        Path::new("--excel-fit-width"),
        Path::new("--timeout"),
        Path::new("15"),
        Path::new("-o"),
        &output,
        &input,
    ]));
    let doc = Document::load(&output).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
    assert!(doc.extract_text(&[1]).unwrap().contains("FIRST SHEET"));
    assert!(doc.extract_text(&[2]).unwrap().contains("SECOND SHEET"));
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "requires installed Microsoft PowerPoint and Automation permission"]
fn real_office_legacy_ppt() {
    assert_native_legacy_conversion("sample.ppt", &["SLIDE MARKER"]);
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "requires installed Microsoft Office apps and Automation permission"]
fn real_office_legacy_and_modern_inputs_preserve_order() {
    let dir = tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let originals: Vec<_> = [
        "sample.doc",
        "sample.xls",
        "sample.ppt",
        "sample.docx",
        "sample.xlsx",
        "sample.pptx",
    ]
    .into_iter()
    .map(|name| {
        let path = fixture.join(name);
        let bytes = fs::read(&path).unwrap();
        (path, bytes)
    })
    .collect();
    let first = dir.path().join("first.pdf");
    pdf(&first, &["PDF FIRST"]);
    let out = dir.path().join("mixed-legacy.pdf");
    let mut command = Command::new(env!("CARGO_BIN_EXE_pdfcli"));
    command
        .args(["merge", "-o"])
        .arg(&out)
        .arg(&first)
        .args(originals.iter().map(|(path, _)| path))
        .env("PDFCLI_SOFFICE", "/not-a-libreoffice-executable");
    #[cfg(target_os = "macos")]
    command.env("PATH", "/usr/bin:/bin");
    let result = command.output().unwrap();
    assert_ok(&result);
    for (path, bytes) in originals {
        assert_eq!(fs::read(path).unwrap(), bytes, "Office source changed");
    }
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 7);
    // This synthetic PDF inherits a directly embedded Resources dictionary;
    // lopdf's text extractor misses its font. Inspect its content stream, as
    // the existing mixed-format test does; Poppler separately checks rendering.
    assert!(labels(&out)[0].contains("PDF FIRST"));
    for (index, marker) in [
        "WORD MARKER",
        "SHEET MARKER",
        "SLIDE MARKER",
        "WORD MARKER",
        "SHEET MARKER",
        "SLIDE MARKER",
    ]
    .iter()
    .enumerate()
    {
        let text = doc.extract_text(&[index as u32 + 2]).unwrap();
        assert!(
            text.contains(marker),
            "page {} expected {marker:?}, got {text:?}",
            index + 2
        );
    }
}
