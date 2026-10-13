# pdfcli

A command-line tool to merge PDF and Office files, compress PDFs, and remove or
reorder pages. Original files stay unchanged.

## Install

Requires Rust and Cargo. From the project folder:

```sh
cargo install --path . --locked
pdfcli --version
```

After updating the source, reinstall the terminal command:

```sh
cargo install --path . --locked --force
```

## Requirements

PDF operations need no additional software. To convert Office files:

- **macOS / Windows:** install and activate Microsoft Office, or install LibreOffice.
- **Linux:** install LibreOffice.

On Windows and macOS, pdfcli chooses a converter for each input file, including
OpenDocument files:

| Input formats | Preferred converter | Fallback when the Microsoft app is absent |
| --- | --- | --- |
| DOC, DOCX, RTF, ODT | Microsoft Word | LibreOffice |
| XLS, XLSX, ODS | Microsoft Excel | LibreOffice |
| PPT, PPTX, ODP | Microsoft PowerPoint | LibreOffice |

Linux uses LibreOffice for all these formats. `--soffice PATH` or
`PDFCLI_SOFFICE` locates LibreOffice; it does not override an installed Microsoft
app. Errors from installed Office are reported, including activation and
permission failures, rather than retrying with LibreOffice.

On macOS, allow Automation access when prompted. Opening OpenDocument files in
Office for Mac can require an internet connection and Microsoft's conversion
permission prompt ([Microsoft's ODF guidance](https://support.microsoft.com/en-au/office/collab-files/view-opendocument-format-odf-files-in-office-for-mac)).
On Windows, pdfcli uses Windows PowerShell and Office COM automation in an
interactive desktop session. Windows Excel honors the saved workbook print layout.
Conversion uses private copies, restores app settings, and keeps recovery files
if cleanup fails or times out. Existing Office documents are not closed.

Native conversion has been tested on macOS. Windows compilation and worker
routing tests pass; real Windows Office automation still needs verification.
Layout follows the selected app; unsupported imported shapes can be omitted.

## Usage

### Merge or convert

```sh
# Merge files in the order listed.
pdfcli merge -o combined.pdf cover.pdf report.docx budget.xlsx slides.pptx

# Merge PDFs from a folder, sorted by filename.
pdfcli merge --folder ./reports -o combined.pdf

# Convert a single Office file to PDF.
pdfcli merge -o workbook.pdf workbook.xlsx

# Convert and merge OpenDocument files with the same converter preference.
pdfcli merge -o combined.pdf report.odt budget.ods slides.odp
```

Supported inputs: PDF, DOC/DOCX, XLS/XLSX, PPT/PPTX, RTF, and ODT/ODS/ODP.
Folder merges include only PDFs directly inside the folder.

If LibreOffice is not found automatically, specify its executable. For example,
on Windows:

```powershell
pdfcli merge --soffice "C:\Program Files\LibreOffice\program\soffice.com" -o report.pdf report.odt
```

With Microsoft Excel on macOS, tables automatically fit to the page width and
long tables continue onto more pages. To use the workbook's saved print layout:

```sh
pdfcli merge --excel-preserve-layout -o workbook.pdf workbook.xlsx
```

### Compress

```sh
pdfcli compress input.pdf -o smaller.pdf
```

Compression preserves image and text quality. Already-compressed files may not
get smaller.

### Remove pages

```sh
pdfcli remove input.pdf --pages 2,5-7 -o trimmed.pdf
```

Page numbers start at 1. Use commas for individual pages and hyphens for ranges.

### Reorder pages

```sh
# Move page 5 to the beginning.
pdfcli reorder input.pdf --pages 5 -o reordered.pdf

# Move page 5 to the end.
pdfcli reorder input.pdf --pages rest,5 -o reordered.pdf
```

Unlisted pages keep their original order. Every page is retained once.

## Notes

Quote paths containing spaces. The output folder must already exist. Add
`--force` to replace an existing output; input files cannot be overwritten.

Encrypted or signed PDFs are unsupported. Merge and page edits also reject
interactive forms and layers, and omit bookmarks, attachments, and accessibility
tags. Removing pages is not a redaction tool.

For all options, run `pdfcli --help` or `pdfcli <command> --help`.

## License

[MIT](LICENSE)
