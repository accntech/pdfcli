use anyhow::{Context, Result, ensure};
use lopdf::{Document, Object};
use std::{
    collections::HashSet,
    fs::File,
    io::{self, Seek},
    path::Path,
};

pub struct CompressionResult {
    pub before: u64,
    pub after: u64,
    pub pages: usize,
}

/// Optimize a whole document without rebuilding its catalog or page tree.
pub fn compress(input: &Path, output: &mut File) -> Result<CompressionResult> {
    let mut source = File::open(input).context("open PDF")?;
    let before = source.metadata()?.len();
    let mut document = Document::load_from(&mut source).context("read PDF")?;
    ensure!(
        !document.is_encrypted() && !document.was_encrypted(),
        "encrypted PDFs are unsupported; export an unencrypted copy first"
    );
    let catalog = document.catalog().context("read PDF catalog")?;
    ensure!(
        !catalog.has(b"Perms"),
        "PDF certification permissions cannot survive compression; use an unsigned copy"
    );
    let root = catalog.get(b"Pages")?.as_reference()?;
    let pages = super::validate_tree(&document, root, None, &mut HashSet::new(), 0)?;
    ensure!(pages > 0, "PDF has no readable pages");
    for object in document.objects.values() {
        reject_signatures(object, 0)?;
    }
    // Only unreachable objects are removed. Keep document-level features such
    // as outlines, metadata, attachments, unsigned forms and accessibility data.
    document.prune_objects();
    document.compress();
    document
        .save_modern(output)
        .context("write compressed PDF")?;
    let mut after = output.metadata()?.len();
    if after >= before {
        // Keep the exact original bytes when optimization offers no saving.
        output.set_len(0)?;
        output.rewind()?;
        source.rewind()?;
        after = io::copy(&mut source, output).context("write unchanged PDF copy")?;
        ensure!(after == before, "source PDF changed during compression");
    }
    Ok(CompressionResult {
        before,
        after,
        pages,
    })
}

fn reject_signatures(object: &Object, depth: usize) -> Result<()> {
    ensure!(depth < 128, "excessively nested PDF object");
    let dictionary = match object {
        Object::Dictionary(dictionary) => Some(dictionary),
        Object::Stream(stream) => Some(&stream.dict),
        Object::Array(values) => {
            for value in values {
                reject_signatures(value, depth + 1)?;
            }
            None
        }
        _ => None,
    };
    if let Some(dictionary) = dictionary {
        let name = |key| dictionary.get(key).and_then(Object::as_name).ok();
        ensure!(
            name(b"Type") != Some(b"Sig")
                && name(b"Type") != Some(b"DocTimeStamp")
                && name(b"FT") != Some(b"Sig")
                && !(dictionary.has(b"ByteRange") && dictionary.has(b"Contents")),
            "PDF contains a signature or signature field; compression would invalidate signatures; use an unsigned copy"
        );
        for (_, value) in dictionary {
            reject_signatures(value, depth + 1)?;
        }
    }
    Ok(())
}
