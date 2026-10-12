mod office;
mod pdf;

use anyhow::{Context, Result, bail, ensure};
use clap::{Args, Parser, Subcommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "Merge PDF and Office files, compress PDFs, and remove or reorder PDF pages"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[cfg_attr(
        target_os = "macos",
        command(
            about = "Convert with Microsoft Office when installed, otherwise LibreOffice, then merge PDF pages"
        )
    )]
    #[cfg_attr(
        not(target_os = "macos"),
        command(about = "Convert Office files locally with LibreOffice, then merge PDF pages")
    )]
    Merge(MergeArgs),
    /// Remove selected pages from a PDF, keeping the remaining pages in order
    Remove(PageArgs),
    /// Put selected pages first; unlisted pages follow in their original order
    #[command(
        after_help = "Use 'rest' once in --pages to place all unlisted pages at that position.\nExamples: --pages 5 (move page 5 first), --pages rest,5 (move it last),\n          --pages 1-3,20 (move page 20 after page 3). All pages are retained."
    )]
    Reorder(PageArgs),
    /// Reduce PDF size without lowering image or text quality
    Compress(CompressArgs),
}

#[derive(Args)]
struct MergeArgs {
    /// Destination PDF (its parent directory must already exist)
    #[arg(short, long)]
    output: PathBuf,
    /// Replace an existing output after the entire merge succeeds
    #[arg(long)]
    force: bool,
    /// LibreOffice fallback executable; otherwise search PATH and standard install paths
    #[arg(long, env = "PDFCLI_SOFFICE", value_name = "PATH")]
    soffice: Option<PathBuf>,
    /// Maximum seconds for each Office conversion
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[cfg(target_os = "macos")]
    /// Fit detected Excel tables to one page wide (default with Microsoft Excel)
    #[arg(long, conflicts_with = "excel_preserve_layout")]
    excel_fit_width: bool,
    #[cfg(target_os = "macos")]
    /// Use Excel's saved print areas, scale and page breaks (requires Microsoft Excel)
    #[arg(long)]
    excel_preserve_layout: bool,
    #[cfg(target_os = "macos")]
    /// Override table detection with SHEET=A1:D90; repeat for sections (requires Microsoft Excel)
    #[arg(
        long = "excel-print-area",
        value_name = "SHEET=RANGE",
        conflicts_with = "excel_preserve_layout"
    )]
    excel_print_areas: Vec<office::ExcelPrintArea>,
    /// Merge PDFs directly in DIR, sorted by filename (case-sensitive; no subfolders)
    #[arg(long, value_name = "DIR", conflicts_with = "inputs")]
    folder: Option<PathBuf>,
    /// Files in output order: PDF, DOC/DOCX, XLS/XLSX, PPT/PPTX, RTF, ODT/ODS/ODP
    #[arg(required_unless_present = "folder", num_args = 1..)]
    inputs: Vec<PathBuf>,
}

#[derive(Args)]
struct PageArgs {
    /// Source PDF (always left unchanged)
    input: PathBuf,
    /// Page numbers starting at 1, separated by commas; ranges like 2-5 or 5-2
    #[arg(long, value_name = "LIST", allow_hyphen_values = true)]
    pages: String,
    /// Destination PDF (its parent directory must already exist)
    #[arg(short, long)]
    output: PathBuf,
    /// Replace an existing output only after the entire operation succeeds
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct CompressArgs {
    /// Source PDF (always left unchanged)
    input: PathBuf,
    /// Destination PDF (its parent directory must already exist)
    #[arg(short, long)]
    output: PathBuf,
    /// Replace an existing output only after compression succeeds
    #[arg(long)]
    force: bool,
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Commands::Merge(args) => merge(args),
        Commands::Remove(args) => edit_pages(args, pdf::PageOperation::Remove),
        Commands::Reorder(args) => edit_pages(args, pdf::PageOperation::Reorder),
        Commands::Compress(args) => compress_pdf(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn output_parent(output: &Path) -> &Path {
    output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn validate_output(output: &Path, force: bool) -> Result<()> {
    ensure!(
        extension(output) == "pdf",
        "output must have a .pdf extension"
    );
    let parent = output_parent(output);
    ensure!(
        parent.is_dir(),
        "output directory does not exist: {}",
        parent.display()
    );
    let output_exists = fs::symlink_metadata(output).is_ok();
    ensure!(
        !output_exists || force,
        "output already exists: {}; use --force to replace it",
        output.display()
    );
    if output_exists {
        ensure!(
            output.is_file(),
            "output is not a regular file: {}",
            output.display()
        );
    }
    Ok(())
}

fn validate_input(input: &Path, output: &Path) -> Result<PathBuf> {
    ensure!(
        input.is_file(),
        "input is not a regular file: {}",
        input.display()
    );
    if output.exists() && same_file::is_same_file(input, output)? {
        bail!(
            "output must not replace an input (including aliases): {}",
            input.display()
        );
    }
    fs::canonicalize(input).with_context(|| format!("resolve {}", input.display()))
}

fn folder_pdf_inputs(folder: &Path, output: &Path) -> Result<Vec<PathBuf>> {
    ensure!(
        folder.is_dir(),
        "folder is not a directory: {}",
        folder.display()
    );
    let mut inputs = Vec::new();
    let output_exists = output.exists();
    for entry in
        fs::read_dir(folder).with_context(|| format!("read PDF folder {}", folder.display()))?
    {
        let path = entry
            .with_context(|| format!("read entry in {}", folder.display()))?
            .path();
        if extension(&path) != "pdf" {
            continue;
        }
        if !fs::metadata(&path)
            .with_context(|| format!("inspect {}", path.display()))?
            .is_file()
        {
            continue;
        }
        // Rerunning into the same folder must not append the previous output,
        // including through symlinks or hard links to that output.
        if output_exists && same_file::is_same_file(&path, output)? {
            continue;
        }
        inputs.push(path);
    }
    inputs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    ensure!(
        !inputs.is_empty(),
        "no PDF input files in folder (excluding output): {}",
        folder.display()
    );
    Ok(inputs)
}

fn merge(mut args: MergeArgs) -> Result<()> {
    validate_output(&args.output, args.force)?;
    if let Some(folder) = &args.folder {
        args.inputs = folder_pdf_inputs(folder, &args.output)?;
    }
    let mut inputs = Vec::new();
    for input in &args.inputs {
        let canonical = validate_input(input, &args.output)?;
        let ext = extension(input);
        ensure!(
            ext == "pdf" || office::is_supported(&ext),
            "unsupported input type: {}",
            input.display()
        );
        inputs.push((canonical, ext));
    }
    let explicit_converter = args.soffice.as_deref();
    let mut converter = if inputs.iter().any(|(_, ext)| ext != "pdf") {
        Some(office::find_converter(explicit_converter)?)
    } else {
        None
    };
    let work = tempfile::Builder::new().prefix("pdfcli-").tempdir()?;
    let mut document = pdf::Merger::new();
    for (index, (input, ext)) in inputs.iter().enumerate() {
        let converted;
        let source = if let Some(converter) = converter.as_mut().filter(|_| ext != "pdf") {
            eprintln!("Converting {}", input.display());
            converted = office::convert(
                converter,
                input,
                ext,
                &work.path().join(index.to_string()),
                Duration::from_secs(args.timeout),
                #[cfg(target_os = "macos")]
                &office::ExcelOptions {
                    fit_width: args.excel_fit_width,
                    preserve_layout: args.excel_preserve_layout,
                    print_areas: &args.excel_print_areas,
                },
            )?;
            &converted
        } else {
            input
        };
        document
            .append(source, input)
            .with_context(|| format!("merge {}", input.display()))?;
    }
    let (document, pages) = document.finish()?;
    publish_pdf(document, &args.output, args.force)?;
    println!(
        "Merged {} files, {pages} pages -> {}",
        inputs.len(),
        args.output.display()
    );
    Ok(())
}

fn edit_pages(args: PageArgs, operation: pdf::PageOperation) -> Result<()> {
    validate_output(&args.output, args.force)?;
    let input = validate_input(&args.input, &args.output)?;
    ensure!(
        extension(&args.input) == "pdf",
        "page editing requires a PDF input"
    );
    let (document, before, after) = pdf::edit_pages(&input, &args.pages, operation)
        .with_context(|| format!("edit pages in {}", args.input.display()))?;
    publish_pdf(document, &args.output, args.force)?;
    match operation {
        pdf::PageOperation::Remove => println!(
            "Removed {} pages, {after} pages remaining -> {}",
            before - after,
            args.output.display()
        ),
        pdf::PageOperation::Reorder => {
            println!("Reordered {after} pages -> {}", args.output.display())
        }
    }
    Ok(())
}

fn compress_pdf(args: CompressArgs) -> Result<()> {
    validate_output(&args.output, args.force)?;
    let input = validate_input(&args.input, &args.output)?;
    ensure!(
        extension(&args.input) == "pdf",
        "compression requires a PDF input"
    );
    let mut pending = pending_pdf(&args.output)?;
    let result = pdf::compress(&input, pending.as_file_mut())
        .with_context(|| format!("compress {}", args.input.display()))?;
    publish_pending(pending, &args.output, args.force)?;
    if result.after < result.before {
        let percent = (result.before - result.after) as f64 / result.before as f64 * 100.0;
        println!(
            "Compressed {} pages: {} -> {} bytes ({percent:.1}% smaller) -> {}",
            result.pages,
            result.before,
            result.after,
            args.output.display()
        );
    } else {
        println!(
            "Lossless compression did not reduce size; wrote unchanged copy ({} bytes) -> {}",
            result.before,
            args.output.display()
        );
    }
    Ok(())
}

fn pending_pdf(output: &Path) -> Result<tempfile::NamedTempFile> {
    // Save beside the destination so the final rename stays on one filesystem.
    Ok(tempfile::Builder::new()
        .prefix(".pdfcli-")
        .suffix(".pdf")
        .tempfile_in(output_parent(output))?)
}

fn publish_pdf(mut document: lopdf::Document, output: &Path, force: bool) -> Result<()> {
    let mut pending = pending_pdf(output)?;
    document
        .save_to(pending.as_file_mut())
        .context("write output PDF")?;
    publish_pending(pending, output, force)
}

fn publish_pending(pending: tempfile::NamedTempFile, output: &Path, force: bool) -> Result<()> {
    // No existing output is touched until all processing succeeds.
    pending.as_file().sync_all().context("flush output PDF")?;
    if force {
        pending
            .persist(output)
            .with_context(|| format!("replace {}", output.display()))?;
    } else {
        pending
            .persist_noclobber(output)
            .with_context(|| format!("create {} without overwriting", output.display()))?;
    }
    Ok(())
}
