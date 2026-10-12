use anyhow::{Context, Result, ensure};
use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

mod pages;
pub use pages::{PageOperation, edit_pages};
mod compress;
pub use compress::compress;
#[cfg(target_os = "macos")]
mod blank;
#[cfg(target_os = "macos")]
pub use blank::blank_pages;

pub struct Merger {
    document: Document,
    roots: Vec<Object>,
    pages: usize,
    root: ObjectId,
}

impl Merger {
    pub fn new() -> Self {
        let mut document = Document::with_version("1.7");
        let root = document.new_object_id();
        Self {
            document,
            root,
            roots: Vec::new(),
            pages: 0,
        }
    }

    pub fn append(&mut self, path: &Path, input_label: &Path) -> Result<()> {
        let mut source = Document::load(path).context("read PDF")?;
        ensure!(
            !source.is_encrypted() && !source.was_encrypted(),
            "encrypted PDFs are unsupported; export an unencrypted copy first"
        );
        let catalog = source.catalog().context("read PDF catalog")?;
        for key in [b"AcroForm".as_slice(), b"Perms", b"OCProperties"] {
            ensure!(
                !catalog.has(key),
                "PDF contains {}; export a flattened PDF first",
                String::from_utf8_lossy(key)
            );
        }
        let omitted: Vec<_> = [
            b"Outlines".as_slice(),
            b"StructTreeRoot",
            b"PageLabels",
            b"Names",
            b"OutputIntents",
        ]
        .into_iter()
        .filter(|key| catalog.has(key))
        .map(String::from_utf8_lossy)
        .collect();
        if !omitted.is_empty() {
            eprintln!(
                "warning: {}: PDF page operations omit document-level {}",
                input_label.display(),
                omitted.join(", ")
            );
        }
        let root = source.catalog()?.get(b"Pages")?.as_reference()?;
        let count = validate_tree(&source, root, None, &mut HashSet::new(), 0)?;
        ensure!(count > 0, "PDF has no readable pages");
        // Resolve against each input's own destination map before IDs move.
        // Two inputs may use the same destination name for different pages.
        let destinations = destinations(&source)?;
        for object in source.objects.values_mut() {
            rewrite_links(object, &destinations, 0)?;
        }
        if source.version.as_str() > self.document.version.as_str() {
            self.document.version = source.version.clone();
        }
        let start = self
            .document
            .max_id
            .checked_add(1)
            .context("PDF object ID limit exceeded")?;
        ensure!(
            source.objects.len() < (u32::MAX - start) as usize,
            "too many PDF objects"
        );
        source.renumber_objects_with(start);
        let root = source
            .catalog()?
            .get(b"Pages")?
            .as_reference()
            .context("PDF Pages must be an indirect reference")?;
        // Graft the whole page subtree: flattening it loses inherited Resources,
        // MediaBox, CropBox, and Rotate values used by many real PDFs.
        let tree = source.get_dictionary_mut(root)?;
        ensure!(
            tree.get(b"Type")?.as_name()? == b"Pages",
            "invalid PDF page tree root"
        );
        tree.set("Parent", self.root);
        tree.set("Count", count as i64);
        self.roots.push(root.into());
        self.pages = self
            .pages
            .checked_add(count)
            .context("page count overflow")?;
        self.document.max_id = source.max_id;
        self.document.objects.extend(source.objects);
        Ok(())
    }

    pub fn finish(mut self) -> Result<(Document, usize)> {
        ensure!(self.pages > 0, "no pages to merge");
        self.document.objects.insert(
            self.root,
            dictionary! {
                "Type" => "Pages", "Kids" => self.roots, "Count" => self.pages as i64
            }
            .into(),
        );
        let catalog = self
            .document
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => self.root });
        self.document.trailer.set("Root", catalog);
        let info = self.document.add_object(dictionary! { "Producer" => Object::string_literal(concat!("pdfcli ", env!("CARGO_PKG_VERSION"))) });
        self.document.trailer.set("Info", info);
        self.document.prune_objects();
        ensure!(
            self.document.get_pages().len() == self.pages,
            "merged page count differs from inputs"
        );
        Ok((self.document, self.pages))
    }
}

fn validate_tree(
    doc: &Document,
    id: ObjectId,
    parent: Option<ObjectId>,
    seen: &mut HashSet<ObjectId>,
    depth: usize,
) -> Result<usize> {
    ensure!(
        depth < 128 && seen.insert(id),
        "cyclic, duplicate, or excessively deep PDF page tree"
    );
    let node = doc
        .get_dictionary(id)
        .context("missing PDF page tree object")?;
    if let Some(parent) = parent {
        ensure!(
            node.get(b"Parent")?.as_reference()? == parent,
            "PDF page tree has an inconsistent Parent"
        );
    }
    match node.get(b"Type")?.as_name()? {
        b"Page" => Ok(1),
        b"Pages" => {
            let mut count = 0usize;
            let kids = doc.dereference(node.get(b"Kids")?)?.1.as_array()?;
            for kid in kids {
                count = count
                    .checked_add(validate_tree(
                        doc,
                        kid.as_reference()?,
                        Some(id),
                        seen,
                        depth + 1,
                    )?)
                    .context("page count overflow")?;
            }
            ensure!(
                node.get(b"Count")?.as_i64()? == count as i64,
                "PDF page tree Count does not match its pages"
            );
            Ok(count)
        }
        _ => anyhow::bail!("invalid PDF page tree node type"),
    }
}

struct Destinations {
    names: HashMap<Vec<u8>, Object>,
    indirect_names: HashMap<ObjectId, Vec<u8>>,
}

fn destinations(doc: &Document) -> Result<Destinations> {
    let catalog = doc.catalog()?;
    let mut found = HashMap::new();
    if let Ok(legacy) = catalog.get(b"Dests") {
        for (key, value) in doc.dereference(legacy)?.1.as_dict()? {
            found.insert(key.clone(), destination_array(doc, value)?);
        }
    }
    if let Ok(names) = catalog.get(b"Names") {
        let names = doc.dereference(names)?.1.as_dict()?;
        if let Ok(tree) = names.get(b"Dests") {
            read_name_tree(doc, tree, &mut found, &mut HashSet::new(), 0)?;
        }
    }
    let indirect_names = doc
        .objects
        .keys()
        .filter_map(|id| match doc.get_object(*id).ok()? {
            Object::Name(name) | Object::String(name, _) => Some((*id, name.clone())),
            _ => None,
        })
        .collect();
    Ok(Destinations {
        names: found,
        indirect_names,
    })
}

fn destination_array(doc: &Document, value: &Object) -> Result<Object> {
    let value = doc.dereference(value)?.1;
    let value = if let Ok(dict) = value.as_dict() {
        doc.dereference(dict.get(b"D")?)?.1
    } else {
        value
    };
    ensure!(
        value.as_array()?.len() >= 2,
        "invalid named PDF destination"
    );
    Ok(value.clone())
}

fn read_name_tree(
    doc: &Document,
    value: &Object,
    found: &mut HashMap<Vec<u8>, Object>,
    seen: &mut HashSet<ObjectId>,
    depth: usize,
) -> Result<()> {
    ensure!(depth < 128, "PDF destination name tree too deep");
    if let Object::Reference(id) = value {
        ensure!(
            seen.insert(*id),
            "cyclic or duplicate PDF destination name tree"
        );
    }
    let node = doc.dereference(value)?.1.as_dict()?;
    if let Ok(entries) = node.get(b"Names") {
        let entries = doc.dereference(entries)?.1.as_array()?;
        ensure!(entries.len() % 2 == 0, "invalid PDF destination name pairs");
        for pair in entries.chunks_exact(2) {
            found.insert(
                pair[0].as_str()?.to_vec(),
                destination_array(doc, &pair[1])?,
            );
        }
    }
    if let Ok(kids) = node.get(b"Kids") {
        for kid in doc.dereference(kids)?.1.as_array()? {
            read_name_tree(doc, kid, found, seen, depth + 1)?;
        }
    }
    Ok(())
}

fn rewrite_links(object: &mut Object, destinations: &Destinations, depth: usize) -> Result<()> {
    ensure!(depth < 128, "PDF object nesting too deep");
    match object {
        Object::Dictionary(dict) => rewrite_dictionary(dict, destinations, depth)?,
        Object::Stream(stream) => rewrite_dictionary(&mut stream.dict, destinations, depth)?,
        Object::Array(array) => {
            for child in array {
                rewrite_links(child, destinations, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn rewrite_dictionary(
    dict: &mut Dictionary,
    destinations: &Destinations,
    depth: usize,
) -> Result<()> {
    ensure!(
        dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Widget")
            && dict.get(b"FT").and_then(Object::as_name).ok() != Some(b"Sig"),
        "PDF contains a Widget or signature field; export a flattened PDF first"
    );
    let is_goto = dict.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo");
    for key in [b"Dest".as_slice(), b"D"] {
        if key == b"D" && !is_goto {
            continue;
        }
        if let Ok(value) = dict.get_mut(key) {
            let name = match &*value {
                Object::Name(name) | Object::String(name, _) => Some(name),
                Object::Reference(id) => destinations.indirect_names.get(id),
                _ => None,
            };
            if let Some(name) = name {
                *value = destinations
                    .names
                    .get(name)
                    .with_context(|| {
                        format!(
                            "unresolved PDF destination: {}",
                            String::from_utf8_lossy(name)
                        )
                    })?
                    .clone();
            }
        }
    }
    for (_, value) in dict.iter_mut() {
        rewrite_links(value, destinations, depth + 1)?;
    }
    Ok(())
}
