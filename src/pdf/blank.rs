use lopdf::{Document, Object, ObjectId, content::Content};

/// Only used on freshly generated Excel PDFs. Absence of extracted text is
/// insufficient: image, chart, vector and annotation pages must survive.
pub fn blank_pages(doc: &Document) -> Vec<u32> {
    doc.get_pages()
        .into_iter()
        .filter_map(|(number, id)| is_blank(doc, id).then_some(number))
        .collect()
}

fn is_blank(doc: &Document, id: ObjectId) -> bool {
    let Ok(page) = doc.get_dictionary(id) else {
        return false;
    };
    if let Ok(annotations) = page.get(b"Annots") {
        match doc.dereference(annotations).map(|(_, value)| value) {
            Ok(Object::Array(items)) if items.is_empty() => {}
            _ => return false,
        }
    }
    let Ok(contents) = page.get(b"Contents") else {
        return true;
    };
    let Ok((_, contents)) = doc.dereference(contents) else {
        return false;
    };
    let streams = match contents {
        Object::Stream(_) => vec![contents],
        Object::Array(items) => items.iter().collect(),
        _ => return false,
    };
    let mut bytes = Vec::new();
    for value in streams {
        let decoded = doc
            .dereference(value)
            .and_then(|(_, value)| value.as_stream())
            .and_then(|stream| stream.decompressed_content());
        let Ok(decoded) = decoded else {
            return false;
        };
        bytes.extend(decoded);
        bytes.push(b'\n');
    }
    // lopdf accepts a successfully decoded prefix of malformed content. An
    // appended terminator proves the parser reached the end instead of
    // silently discarding a trailing string, number or broken operator.
    const END: &str = "PdfcliEndOfContent";
    if bytes.windows(END.len()).any(|part| part == END.as_bytes()) {
        return false;
    }
    bytes.extend_from_slice(format!("\n{END}\n").as_bytes());
    let Ok(mut content) = Content::decode(&bytes) else {
        return false;
    };
    if !content
        .operations
        .pop()
        .is_some_and(|op| op.operator == END && op.operands.is_empty())
    {
        return false;
    }
    // Accept only known operators that cannot paint. Painting, XObjects,
    // inline images, shadings, unknown operators and undecodable streams are
    // retained conservatively, even if their rendered appearance is white.
    content.operations.iter().all(|op| {
        matches!(
            op.operator.as_str(),
            "q" | "Q"
                | "cm"
                | "w"
                | "J"
                | "j"
                | "M"
                | "d"
                | "ri"
                | "i"
                | "gs"
                | "m"
                | "l"
                | "c"
                | "v"
                | "y"
                | "h"
                | "re"
                | "W"
                | "W*"
                | "n"
                | "CS"
                | "cs"
                | "SC"
                | "SCN"
                | "sc"
                | "scn"
                | "G"
                | "g"
                | "RG"
                | "rg"
                | "K"
                | "k"
                | "BT"
                | "ET"
                | "Tc"
                | "Tw"
                | "Tz"
                | "TL"
                | "Tf"
                | "Tr"
                | "Ts"
                | "Td"
                | "TD"
                | "Tm"
                | "T*"
                | "MP"
                | "DP"
                | "BMC"
                | "BDC"
                | "EMC"
                | "BX"
                | "EX"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    fn page(doc: &mut Document, bytes: &[u8]) -> ObjectId {
        let content = doc.add_object(Stream::new(dictionary! {}, bytes.to_vec()));
        doc.add_object(dictionary! { "Type" => "Page", "Contents" => content })
    }

    #[test]
    fn removes_only_content_that_cannot_paint() {
        let mut doc = Document::with_version("1.7");
        for bytes in [
            b"".as_slice(),
            b"q Q",
            b"0 0 100 100 re W n q Q",
            b"BT /F1 12 Tf 10 20 Td ET",
        ] {
            let id = page(&mut doc, bytes);
            assert!(is_blank(&doc, id));
        }
        for bytes in [
            b"BT (Text) Tj ET".as_slice(),
            b"/Image1 Do",
            b"0 0 50 50 re f",
            b"0 0 m 1 1 l S",
            b"/Shade sh",
            b"unknownOperator",
            b"(invalid",
            b"q Q (invalid",
            b"q Q 123",
            b"q Q PdfcliEndOfContent (invalid",
        ] {
            let id = page(&mut doc, bytes);
            assert!(!is_blank(&doc, id), "Must keep {bytes:?}");
        }
    }

    #[test]
    fn keeps_annotations_even_without_page_content() {
        let mut doc = Document::with_version("1.7");
        let id = page(&mut doc, b"q Q");
        let annotation = doc.add_object(dictionary! { "Subtype" => "Text" });
        let annotations = doc.add_object(vec![Object::Reference(annotation)]);
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("Annots", annotations);
        assert!(!is_blank(&doc, id));
    }
}
