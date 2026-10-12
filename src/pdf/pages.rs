use super::{Merger, validate_tree};
use anyhow::{Context, Result, ensure};
use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Copy)]
pub enum PageOperation {
    Remove,
    Reorder,
}

pub fn edit_pages(
    input: &Path,
    specification: &str,
    operation: PageOperation,
) -> Result<(Document, usize, usize)> {
    // Share merge's validation, destination resolution and document-feature policy.
    let mut merger = Merger::new();
    merger.append(input, input)?;
    let (mut doc, before) = merger.finish()?;
    let (requested, rest_position) = parse_pages(
        specification,
        before,
        matches!(operation, PageOperation::Reorder),
    )?;
    let pages = doc.get_pages();
    let selected: Vec<ObjectId> = match operation {
        PageOperation::Remove => {
            ensure!(
                requested.len() < before,
                "removal must leave at least one page"
            );
            let removed: HashSet<_> = requested.into_iter().collect();
            pages
                .iter()
                .filter(|(number, _)| !removed.contains(number))
                .map(|(_, id)| *id)
                .collect()
        }
        PageOperation::Reorder => {
            // Every page is retained exactly once. A partial list moves the
            // specified pages first; `rest` can place the remaining block elsewhere.
            let listed: HashSet<_> = requested.iter().copied().collect();
            let position = rest_position.unwrap_or(requested.len());
            requested[..position]
                .iter()
                .chain(pages.keys().filter(|number| !listed.contains(*number)))
                .chain(requested[position..].iter())
                .map(|number| pages[number])
                .collect()
        }
    };
    let root = doc.catalog()?.get(b"Pages")?.as_reference()?;
    // Materialize inherited values before replacing the original hierarchy.
    // Keeping the page IDs preserves links to pages as they move.
    let mut replacements = Vec::with_capacity(selected.len());
    for id in &selected {
        let mut page = doc.get_dictionary(*id)?.clone();
        for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
            let mut ancestor = *id;
            loop {
                let node = doc.get_dictionary(ancestor)?;
                if let Ok(value) = node.get(key)
                    && !matches!(value, Object::Null)
                {
                    page.set(key, value.clone());
                    break;
                }
                match node.get(b"Parent") {
                    Ok(parent) => ancestor = parent.as_reference()?,
                    Err(_) => break,
                }
            }
        }
        page.set("Parent", root);
        replacements.push((*id, page));
    }
    let kept: HashSet<_> = selected.iter().copied().collect();
    let removed: HashSet<_> = pages
        .values()
        .filter(|id| !kept.contains(id))
        .copied()
        .collect();
    for (id, page) in replacements {
        doc.objects.insert(id, page.into());
    }
    disable_removed_links(&mut doc, &removed)?;
    doc.objects.insert(root, dictionary! {
        "Type" => "Pages", "Kids" => selected.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
        "Count" => selected.len() as i64
    }.into());
    // Remove old tree nodes and deleted pages even if nonstandard references
    // point at them. Unreachable content streams/resources are pruned below.
    doc.objects.retain(|id, object| {
        !removed.contains(id)
            && (*id == root
                || object
                    .as_dict()
                    .ok()
                    .and_then(|dict| dict.get(b"Type").ok())
                    .and_then(|kind| kind.as_name().ok())
                    != Some(b"Pages"))
    });
    doc.prune_objects();
    let after = validate_tree(&doc, root, None, &mut HashSet::new(), 0)?;
    ensure!(
        doc.get_pages().values().copied().collect::<Vec<_>>() == selected,
        "output page order differs from request"
    );
    Ok((doc, before, after))
}

fn parse_pages(
    specification: &str,
    total: usize,
    allow_rest: bool,
) -> Result<(Vec<u32>, Option<usize>)> {
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    let mut rest_position = None;
    let parse_number = |text: &str| -> Result<u32> {
        let text = text.trim();
        ensure!(
            !text.is_empty() && text.bytes().all(|c| c.is_ascii_digit()),
            "invalid page number: {text:?}"
        );
        let number = text
            .parse::<u32>()
            .with_context(|| format!("invalid page number: {text:?}"))?;
        ensure!(
            number > 0 && number as usize <= total,
            "page {number} is outside 1..={total}"
        );
        Ok(number)
    };
    for item in specification.split(',') {
        let item = item.trim();
        if item.eq_ignore_ascii_case("rest") {
            ensure!(allow_rest, "rest is only supported by reorder");
            ensure!(rest_position.is_none(), "rest may appear only once");
            rest_position = Some(pages.len());
            continue;
        }
        ensure!(
            !item.is_empty(),
            "empty page selection; use numbers and ranges such as 1,3-5"
        );
        let (first, last) = match item.split_once('-') {
            Some((first, last)) => (parse_number(first)?, parse_number(last)?),
            None => {
                let number = parse_number(item)?;
                (number, number)
            }
        };
        // Check endpoints before expanding; huge ranges cannot allocate beyond
        // the document's page count. Duplicate/overlapping ranges are errors.
        for offset in 0..=first.abs_diff(last) {
            let number = if first <= last {
                first + offset
            } else {
                first - offset
            };
            ensure!(
                seen.insert(number),
                "page {number} is listed more than once"
            );
            pages.push(number);
        }
    }
    Ok((pages, rest_position))
}

fn targets_removed(
    value: &Object,
    removed: &HashSet<ObjectId>,
    destinations: &HashSet<ObjectId>,
) -> bool {
    match value {
        Object::Reference(id) => destinations.contains(id),
        Object::Array(array) => array
            .first()
            .and_then(|first| first.as_reference().ok())
            .is_some_and(|id| removed.contains(&id)),
        _ => false,
    }
}

fn is_removed_action(
    dict: &Dictionary,
    removed: &HashSet<ObjectId>,
    destinations: &HashSet<ObjectId>,
) -> bool {
    dict.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo")
        && dict
            .get(b"D")
            .is_ok_and(|value| targets_removed(value, removed, destinations))
}

fn disable_removed_links(doc: &mut Document, removed: &HashSet<ObjectId>) -> Result<()> {
    if removed.is_empty() {
        return Ok(());
    }
    let destinations: HashSet<_> = doc
        .objects
        .keys()
        .copied()
        .filter(|id| {
            doc.get_object(*id)
                .is_ok_and(|value| targets_removed(value, removed, &HashSet::new()))
        })
        .collect();
    let actions: HashSet<_> = doc
        .objects
        .keys()
        .copied()
        .filter(|id| {
            doc.get_dictionary(*id)
                .is_ok_and(|dict| is_removed_action(dict, removed, &destinations))
        })
        .collect();
    for object in doc.objects.values_mut() {
        clean_links(object, removed, &destinations, &actions, 0)?;
    }
    Ok(())
}

fn clean_links(
    object: &mut Object,
    removed: &HashSet<ObjectId>,
    destinations: &HashSet<ObjectId>,
    actions: &HashSet<ObjectId>,
    depth: usize,
) -> Result<()> {
    ensure!(depth < 128, "PDF object nesting too deep");
    match object {
        Object::Reference(id) if actions.contains(id) => *object = Object::Null,
        Object::Dictionary(dict) if is_removed_action(dict, removed, destinations) => {
            *object = Object::Null
        }
        Object::Dictionary(dict) => {
            if dict
                .get(b"Dest")
                .is_ok_and(|value| targets_removed(value, removed, destinations))
            {
                dict.remove(b"Dest");
            }
            for (_, child) in dict.iter_mut() {
                clean_links(child, removed, destinations, actions, depth + 1)?;
            }
            // An annotation with no destination/action stays inert. Other
            // annotations and external URI actions are preserved unchanged.
            if matches!(dict.get(b"A"), Ok(Object::Null)) {
                dict.remove(b"A");
            }
        }
        Object::Array(array) => {
            for child in array {
                clean_links(child, removed, destinations, actions, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
