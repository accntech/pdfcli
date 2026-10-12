//! Layout-only table detection. Rust receives coordinates, never workbook text.
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    top: u32,
    left: u32,
    bottom: u32,
    right: u32,
}

impl Rect {
    fn contains(self, row: u32, col: u32) -> bool {
        (self.top..=self.bottom).contains(&row) && (self.left..=self.right).contains(&col)
    }
    fn union(self, other: Self) -> Self {
        Self {
            top: self.top.min(other.top),
            left: self.left.min(other.left),
            bottom: self.bottom.max(other.bottom),
            right: self.right.max(other.right),
        }
    }
    fn address(self) -> String {
        fn column(mut n: u32) -> String {
            let mut letters = Vec::new();
            while n > 0 {
                n -= 1;
                letters.push(b'A' + (n % 26) as u8);
                n /= 26;
            }
            letters.reverse();
            String::from_utf8(letters).unwrap()
        }
        format!(
            "{}{}:{}{}",
            column(self.left),
            self.top,
            column(self.right),
            self.bottom
        )
    }
}

#[derive(Default)]
struct Sheet {
    index: u32,
    cells: Vec<(u32, u32)>,
    merges: Vec<Rect>,
    graphics: Vec<Rect>,
    tables: Vec<Rect>,
    skipped: bool,
}

impl Sheet {
    fn bounds(&self, left: u32, right: u32) -> Option<Rect> {
        let cells = self
            .cells
            .iter()
            .filter(|(_, c)| (left..=right).contains(c))
            .map(|&(r, c)| Rect {
                top: r,
                bottom: r,
                left: c,
                right: c,
            });
        let occupied_merges = self
            .merges
            .iter()
            .filter(|m| self.cells.iter().any(|&(r, c)| m.contains(r, c)));
        cells
            .chain(
                occupied_merges
                    .chain(&self.graphics)
                    .chain(&self.tables)
                    .filter(|m| m.left >= left && m.right <= right)
                    .copied(),
            )
            .reduce(Rect::union)
    }

    fn sections(&self, bounds: Rect, sections: &mut Vec<Rect>) {
        let columns: BTreeSet<_> = self
            .cells
            .iter()
            .filter(|(_, c)| (bounds.left..=bounds.right).contains(c))
            .map(|&(_, c)| c)
            .chain(
                self.graphics
                    .iter()
                    .chain(&self.tables)
                    .filter(|m| m.left >= bounds.left && m.right <= bounds.right)
                    .flat_map(|m| m.left..=m.right),
            )
            .collect();
        let columns: Vec<_> = columns.into_iter().collect();
        let mut best = None;
        // At least two occupied columns on either side: a description and its
        // distant amount column are one table, even with blank columns between.
        for i in 2..columns.len().saturating_sub(1) {
            let cut = columns[i - 1];
            let next = columns[i];
            if self
                .merges
                .iter()
                .chain(&self.graphics)
                .chain(&self.tables)
                .any(|m| m.left <= cut && m.right > cut)
            {
                continue;
            }
            let left_headings: Vec<_> = self
                .merges
                .iter()
                .filter(|m| m.left == bounds.left && m.right == cut && m.top <= bounds.top + 8)
                .collect();
            let right_headings: Vec<_> = self
                .merges
                .iter()
                .filter(|m| m.left == next && m.right == bounds.right && m.top <= bounds.top + 8)
                .collect();
            let staggered = left_headings
                .iter()
                .any(|a| right_headings.iter().any(|b| a.top != b.top));
            let grouped = left_headings
                .iter()
                .any(|a| right_headings.iter().any(|b| a.top == b.top));
            let structured = self
                .tables
                .iter()
                .any(|t| t.left >= bounds.left && t.right == cut)
                && self
                    .tables
                    .iter()
                    .any(|t| t.left == next && t.right <= bounds.right);
            let gutter = next - cut - 1;
            let score = if structured {
                300
            } else if staggered && !grouped {
                200
            } else if gutter > 0 && !grouped {
                100 + gutter
            } else {
                continue;
            };
            if best.is_none_or(|(previous, _, _)| score > previous) {
                best = Some((score, cut, next));
            }
        }
        if let Some((_, cut, next)) = best {
            // These bounds include all occupied cells, merged content and graphics.
            self.sections(self.bounds(bounds.left, cut).unwrap(), sections);
            self.sections(self.bounds(next, bounds.right).unwrap(), sections);
        } else {
            sections.push(bounds);
        }
    }
}

/// Worksheet-indexed data literals, never executable code or user-provided text.
pub fn automatic_areas_script(metadata: &str) -> Result<String> {
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut lines = metadata.lines();
    ensure!(
        lines.next() == Some("V\t1"),
        "unsupported Excel layout metadata"
    );
    let mut ended = false;
    for line in lines {
        ensure!(!ended, "data after Excel layout terminator");
        let mut fields = line.split('\t');
        let kind = fields.next().unwrap_or_default();
        if kind == "E" {
            ensure!(fields.next().is_none(), "invalid layout terminator");
            ended = true;
            continue;
        }
        let values: Vec<u32> = fields
            .map(|v| v.parse().context("invalid Excel layout coordinate"))
            .collect::<Result<_>>()?;
        if kind == "S" {
            ensure!(
                values.len() == 1
                    && values[0] > 0
                    && sheets.last().is_none_or(|s| s.index < values[0]),
                "invalid Excel worksheet index"
            );
            sheets.push(Sheet {
                index: values[0],
                ..Sheet::default()
            });
            continue;
        }
        let sheet = sheets
            .last_mut()
            .context("Excel layout record without worksheet")?;
        ensure!(!sheet.skipped, "partial skipped worksheet metadata");
        if kind == "K" {
            ensure!(
                values.is_empty()
                    && sheet.cells.is_empty()
                    && sheet.merges.is_empty()
                    && sheet.graphics.is_empty()
                    && sheet.tables.is_empty(),
                "invalid skipped worksheet"
            );
            sheet.skipped = true;
            continue;
        }
        let rect = match (kind, values.as_slice()) {
            ("C", &[r, c]) => Rect {
                top: r,
                left: c,
                bottom: r,
                right: c,
            },
            ("M" | "G" | "T", &[top, left, bottom, right]) => Rect {
                top,
                left,
                bottom,
                right,
            },
            _ => bail!("invalid Excel layout record"),
        };
        ensure!(
            rect.top > 0
                && rect.left > 0
                && rect.bottom <= 1048576
                && rect.right <= 16384
                && rect.top <= rect.bottom
                && rect.left <= rect.right,
            "Excel layout coordinate out of bounds"
        );
        match kind {
            "C" => sheet.cells.push((rect.top, rect.left)),
            "M" => sheet.merges.push(rect),
            "G" => sheet.graphics.push(rect),
            "T" => sheet.tables.push(rect),
            _ => unreachable!(),
        }
    }
    ensure!(ended, "incomplete Excel layout metadata");
    let mut areas = Vec::new();
    for sheet in sheets {
        if sheet.skipped {
            eprintln!(
                "Excel: worksheet {} exceeds layout inspection limit; fitting its complete content width",
                sheet.index
            );
            continue;
        }
        if let Some(bounds) = sheet.bounds(1, 16384) {
            let mut sections = Vec::new();
            sheet.sections(bounds, &mut sections);
            for rect in sections {
                // The native preparation already expands a single-cell area
                // using the workbook's column limit (256 for legacy XLS).
                if rect.top != rect.bottom || rect.left != rect.right {
                    areas.push(format!("{{{}, \"{}\"}}", sheet.index, rect.address()));
                }
            }
        }
    }
    Ok(format!("{{{}}}", areas.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grid(right: u32) -> String {
        let mut data = String::from("V\t1\nS\t1\n");
        for r in 1..=90 {
            for c in [1, 4, right, right + 3] {
                data.push_str(&format!("C\t{r}\t{c}\n"));
            }
        }
        data
    }
    #[test]
    fn gutter_sections_keep_amount_columns_and_vertical_extent() {
        assert_eq!(
            automatic_areas_script(&(grid(6) + "E\n")).unwrap(),
            "{{1, \"A1:D90\"}, {1, \"F1:I90\"}}"
        );
    }
    #[test]
    fn adjacent_staggered_headings_and_structured_tables() {
        let data = grid(5);
        assert_eq!(
            automatic_areas_script(&(data.clone() + "M\t4\t1\t4\t4\nM\t1\t5\t1\t8\nE\n")).unwrap(),
            "{{1, \"A1:D90\"}, {1, \"E1:H90\"}}"
        );
        assert_eq!(
            automatic_areas_script(&(data + "T\t1\t1\t90\t4\nT\t1\t5\t90\t8\nE\n")).unwrap(),
            "{{1, \"A1:D90\"}, {1, \"E1:H90\"}}"
        );
    }
    #[test]
    fn shared_headings_graphics_and_grouped_headers_prevent_false_splits() {
        for span in [
            "M\t1\t1\t1\t9\n",
            "G\t30\t1\t40\t9\n",
            "M\t1\t1\t1\t4\nM\t1\t6\t1\t9\n",
            "T\t1\t1\t90\t9\n",
        ] {
            assert_eq!(
                automatic_areas_script(&(grid(6) + span + "E\n")).unwrap(),
                "{{1, \"A1:I90\"}}"
            );
        }
    }
    #[test]
    fn band_bounds_retain_notes_merges_and_graphics() {
        let data = grid(6) + "C\t110\t1\nM\t110\t1\t112\t4\nG\t100\t6\t120\t9\nE\n";
        assert_eq!(
            automatic_areas_script(&data).unwrap(),
            "{{1, \"A1:D112\"}, {1, \"F1:I120\"}}"
        );
    }

    #[test]
    fn a_spanning_heading_extends_the_print_area_past_body_cells() {
        let data = grid(6) + "M\t1\t1\t1\t12\nE\n";
        assert_eq!(automatic_areas_script(&data).unwrap(), "{{1, \"A1:L90\"}}");
    }
    #[test]
    fn metadata_requires_complete_valid_records_and_skips_whole_large_sheets() {
        for bad in [
            "V\t1\nS\t1",
            "V\t1\nC\t1\t2\nE",
            "V\t1\nS\t1\nM\t1\t4\t2\t1\nE",
            "V\t1\nS\t1\nC\t1\t16385\nE",
            "V\t1\nS\t1\nK\nC\t1\t1\nE",
            "V\t1\nS\t1\nS\t1\nE",
            "V\t1\nE\nS\t1",
        ] {
            assert!(automatic_areas_script(bad).is_err(), "Accepted {bad:?}");
        }
        assert_eq!(
            automatic_areas_script("V\t1\nS\t1\nK\nS\t3\nC\t1\t1\nE").unwrap(),
            "{}"
        );
    }
}
