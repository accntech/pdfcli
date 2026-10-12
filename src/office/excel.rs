use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct ExcelPrintArea {
    sheet: String,
    range: String,
}

impl FromStr for ExcelPrintArea {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (sheet, range) = value.rsplit_once('=').ok_or("use SHEET=A1:D90")?;
        if sheet.is_empty() || sheet.chars().any(char::is_control) {
            return Err("worksheet name must be nonempty and contain no control characters".into());
        }
        let range = range.trim().replace('$', "").to_ascii_uppercase();
        let (first, last) = range
            .split_once(':')
            .ok_or("use a rectangular range such as A1:D90")?;
        let cell = |value: &str| -> Option<(u32, u32)> {
            let split = value.find(|c: char| !c.is_ascii_uppercase())?;
            let (letters, digits) = value.split_at(split);
            if letters.is_empty()
                || digits.is_empty()
                || !digits.bytes().all(|c| c.is_ascii_digit())
            {
                return None;
            }
            let column = letters.bytes().try_fold(0u32, |n, c| {
                n.checked_mul(26)?.checked_add(u32::from(c - b'A' + 1))
            })?;
            let row: u32 = digits.parse().ok()?;
            (column <= 16384 && (1..=1048576).contains(&row)).then_some((column, row))
        };
        let start = cell(first).ok_or("invalid first cell; use A1 through XFD1048576")?;
        let end = cell(last).ok_or("invalid last cell; use A1 through XFD1048576")?;
        if start.0 > end.0 || start.1 > end.1 || start == end {
            return Err(
                "range must run top-left to bottom-right and contain at least two cells".into(),
            );
        }
        Ok(Self {
            sheet: sheet.into(),
            range,
        })
    }
}

/// Values are data literals, never executable AppleScript supplied by a caller.
pub fn print_areas_script(areas: &[ExcelPrintArea]) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    format!(
        "{{{}}}",
        areas
            .iter()
            .map(|a| format!("{{{}, {}}}", quote(&a.sheet), quote(&a.range)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_table_ranges_and_quotes_sheet_names_as_data() {
        let area: ExcelPrintArea = "A=\"B\\C=$a$1:$D$90".parse().unwrap();
        assert_eq!(area.sheet, "A=\"B\\C");
        assert_eq!(area.range, "A1:D90");
        assert_eq!(
            print_areas_script(&[area]),
            "{{\"A=\\\"B\\\\C\", \"A1:D90\"}}"
        );
        assert!("Table=XFD1:XFD1048576".parse::<ExcelPrintArea>().is_ok());
        for invalid in [
            "No range",
            "=A1:D2",
            "Tab\nle=A1:D2",
            "Table=A1",
            "Table=A1:A1",
            "Table=D2:A1",
            "Table=A0:D2",
            "Table=A1:XFE2",
            "Table=A1:D1048577",
            "Table=A1:D2, F1:G2",
            "Table=A1:D2;quit",
        ] {
            assert!(
                invalid.parse::<ExcelPrintArea>().is_err(),
                "Accepted {invalid:?}"
            );
        }
    }
}
