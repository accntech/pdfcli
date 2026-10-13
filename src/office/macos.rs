use anyhow::{Context, Result, bail, ensure};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use wait_timeout::ChildExt;

pub struct Converter {
    // Acquire only when a Microsoft app is used, then hold until the merge ends
    // because Office's automation security is global.
    lock: Option<File>,
    soffice: Option<PathBuf>,
    application_directories: Vec<PathBuf>,
}

#[derive(Default)]
pub struct ExcelOptions<'a> {
    pub fit_width: bool,
    pub preserve_layout: bool,
    pub print_areas: &'a [super::ExcelPrintArea],
}

fn installed_application(extension: &str, directories: &[PathBuf]) -> Option<&'static str> {
    let app = super::microsoft_application(extension)?;
    directories
        .iter()
        .any(|directory| directory.join(format!("Microsoft {app}.app")).is_dir())
        .then_some(app)
}

pub fn find_converter(explicit: Option<&Path>) -> Result<Converter> {
    let mut application_directories = vec![PathBuf::from("/Applications")];
    if let Some(home) = env::var_os("HOME") {
        application_directories.push(PathBuf::from(home).join("Applications"));
    }
    Ok(Converter {
        lock: None,
        soffice: explicit.map(Path::to_owned),
        application_directories,
    })
}

fn lock_office() -> Result<File> {
    let cache = PathBuf::from(env::var_os("HOME").context("HOME is not set")?)
        .join("Library/Caches/pdfcli");
    fs::create_dir_all(&cache)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(cache.join("office.lock"))?;
    lock.try_lock()
        .context("another pdfcli Office conversion is running")?;
    Ok(lock)
}

pub fn convert(
    converter: &mut Converter,
    input: &Path,
    extension: &str,
    result_directory: &Path,
    timeout: Duration,
    excel_options: &ExcelOptions<'_>,
) -> Result<PathBuf> {
    let Some(app) = installed_application(extension, &converter.application_directories) else {
        ensure!(
            !matches!(extension, "xls" | "xlsx" | "ods")
                || (!excel_options.fit_width
                    && !excel_options.preserve_layout
                    && excel_options.print_areas.is_empty()),
            "Excel options require Microsoft Excel export of .xls, .xlsx or .ods files; install Microsoft Excel or omit the Excel options to use LibreOffice"
        );
        let soffice =
            super::libreoffice::find_soffice(converter.soffice.as_deref()).with_context(|| {
                format!("no installed Microsoft app supports .{extension}; LibreOffice is required")
            })?;
        eprintln!("Using LibreOffice for {}", input.display());
        return super::libreoffice::convert(&soffice, input, extension, result_directory, timeout);
    };
    if converter.lock.is_none() {
        converter.lock = Some(lock_office()?);
    }
    eprintln!("Using Microsoft {app} for {}", input.display());
    let excel_fit_width = !excel_options.preserve_layout;
    let excel_print_areas = excel_options.print_areas;
    let (id, documents, open, save) = match app {
        "Excel" => (
            "com.microsoft.Excel",
            "workbooks",
            "open workbook workbook file name inputFile update links do not update links read only true",
            include_str!("excel.applescript"),
        ),
        "PowerPoint" => (
            "com.microsoft.Powerpoint",
            "presentations",
            "open inputFile",
            "save ownedDoc in (POSIX file outputPath) as save as PDF",
        ),
        _ => (
            "com.microsoft.Word",
            "documents",
            "open inputFile confirm conversions false add to recent files false",
            "save as ownedDoc file name outputPath file format format PDF add to recent files false",
        ),
    };
    fs::create_dir_all(result_directory)?;
    let app_cache = PathBuf::from(env::var_os("HOME").context("HOME is not set")?)
        .join("Library/Caches/pdfcli/office");
    fs::create_dir_all(&app_cache).context("create Office staging folder")?;
    fs::set_permissions(&app_cache, fs::Permissions::from_mode(0o700))?;
    let job = tempfile::Builder::new()
        .prefix("job-")
        .tempdir_in(&app_cache)?;
    let directory = fs::canonicalize(job.path())?;
    // Unique basename also avoids colliding with an already-open Office document.
    let staged = tempfile::Builder::new()
        .prefix("pdfcli-")
        .suffix(&format!(".{extension}"))
        .tempfile_in(&app_cache)?;
    fs::copy(input, staged.path()).with_context(|| format!("stage {}", input.display()))?;
    let output = if app == "Excel" {
        staged.path().with_extension("sheets")
    } else {
        staged.path().with_extension("pdf")
    };
    // Keep Office-created PDFs directly inside the already-approved staging
    // folder: new subfolders can trigger a separate sandbox access prompt.
    let mut excel_exports = (app == "Excel").then(|| ExcelExports {
        prefix: output.clone(),
        preserve: false,
    });
    let state = directory.join("automation-security.txt");
    let script = directory.join("convert.applescript");
    let template = |save: &str| {
        include_str!("macos.applescript")
            .replace("__APP_ID__", id)
            .replace("__DOCUMENTS__", documents)
            .replace(
                "__ALLOW_MISSING_SECURITY__",
                if app == "PowerPoint" { "true" } else { "false" },
            )
            .replace("__OPEN__", open)
            .replace("__SAVE__", save)
            .replace(
                "__EXCEL_FIT_WIDTH__",
                if excel_fit_width { "true" } else { "false" },
            )
            .replace(
                "__EXCEL_PRINT_AREAS__",
                &super::excel::print_areas_script(excel_print_areas),
            )
    };
    let export_template = template(save);
    fs::write(
        &script,
        export_template.replace("__EXCEL_AUTO_AREAS__", "{}"),
    )?;
    let args = [staged.path(), output.as_path(), state.as_path()];
    let log = directory.join("conversion.log");
    let result = (|| -> Result<bool> {
        let started = Instant::now();
        if app == "Excel" && excel_fit_width {
            let inspection = directory.join("inspect.applescript");
            let metadata = directory.join("layout.tsv");
            fs::write(
                &inspection,
                template(include_str!("excel-layout.applescript")),
            )?;
            if !run_script(
                &inspection,
                &[staged.path(), &metadata, &state],
                timeout,
                "convert",
                &log,
            )? {
                return Ok(false);
            }
            let areas =
                super::excel_layout::automatic_areas_script(&fs::read_to_string(metadata)?)?;
            fs::write(
                &script,
                export_template.replace("__EXCEL_AUTO_AREAS__", &areas),
            )?;
        }
        let remaining = timeout
            .checked_sub(started.elapsed())
            .context("Excel layout inspection exceeded conversion timeout")?;
        run_script(&script, &args, remaining, "convert", &log)
    })();
    let valid = result.as_ref().is_ok_and(|ok| *ok) && output.is_file();
    if !valid {
        let message = log_tail(&log).unwrap_or_default();
        let cleanup_log = directory.join("cleanup.log");
        let cleaned = run_script(
            &script,
            &args,
            Duration::from_secs(5),
            "cleanup",
            &cleanup_log,
        )
        .is_ok_and(|ok| ok);
        let reason = result.err().map(|e| e.to_string()).unwrap_or(message);
        if !cleaned {
            if let Some(exports) = excel_exports.as_mut() {
                exports.preserve = true;
            }
            // Keep our private job if Office was unable to restore/close it.
            let staged_path = staged.keep().context("preserve Office recovery input")?.1;
            let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
            let recovery = format!(
                "/usr/bin/osascript {} {} {} {} 5 cleanup {}\n",
                quote(&script),
                quote(&staged_path),
                quote(&output),
                quote(&state),
                quote(Path::new(staged_path.file_name().unwrap()))
            );
            fs::write(directory.join("recovery.txt"), recovery)?;
            let _retained = job.keep();
            bail!(
                "Microsoft {app} conversion failed: {reason}. Cleanup also failed: {}. Recovery command: {}",
                log_tail(&cleanup_log).unwrap_or_default(),
                directory.join("recovery.txt").display()
            );
        }
        let _ = fs::remove_file(&output);
        bail!(
            "Microsoft {app} could not convert {}: {reason}. Check Office activation and macOS Automation permission for your terminal/Codex",
            input.display()
        );
    }
    let result = result_directory.join("converted.pdf");
    if app == "Excel" {
        collect_excel_sheets(&output, &result, input)?;
        return Ok(result);
    }
    let copied = (|| -> Result<()> {
        ensure!(
            fs::metadata(&output)?.len() > 0,
            "Microsoft {app} produced an empty PDF"
        );
        fs::copy(&output, &result).context("collect Office PDF")?;
        Ok(())
    })();
    let _ = fs::remove_file(&output);
    copied?;
    Ok(result)
}

fn excel_sheet_path(prefix: &Path, index: usize) -> PathBuf {
    let mut path = prefix.as_os_str().to_owned();
    path.push(format!(".{index}.pdf"));
    path.into()
}

struct ExcelExports {
    prefix: PathBuf,
    preserve: bool,
}

impl Drop for ExcelExports {
    fn drop(&mut self) {
        if self.preserve {
            return;
        }
        let _ = fs::remove_file(&self.prefix);
        let Some(parent) = self.prefix.parent() else {
            return;
        };
        let Some(prefix) = self.prefix.file_name().and_then(|s| s.to_str()) else {
            return;
        };
        let Ok(entries) = fs::read_dir(parent) else {
            return;
        };
        // Also clean partial exports when Excel fails before writing its count.
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(suffix) = name.to_str().and_then(|s| s.strip_prefix(prefix)) else {
                continue;
            };
            if suffix
                .strip_prefix('.')
                .and_then(|s| s.strip_suffix(".pdf"))
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn collect_excel_sheets(prefix: &Path, output: &Path, input: &Path) -> Result<()> {
    let count: usize = fs::read_to_string(prefix)?
        .trim()
        .parse()
        .context("read exported Excel worksheet count")?;
    ensure!(count > 0, "Excel exported no worksheets");
    let mut merger = crate::pdf::Merger::new();
    let mut removed = 0;
    for index in 1..=count {
        let path = excel_sheet_path(prefix, index);
        let doc = lopdf::Document::load(&path)
            .with_context(|| format!("read Excel worksheet {index} PDF"))?;
        let blank = crate::pdf::blank_pages(&doc);
        removed += blank.len();
        if blank.len() == doc.get_pages().len() {
            continue;
        }
        if !blank.is_empty() {
            let selection = blank
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let (mut filtered, _, _) =
                crate::pdf::edit_pages(&path, &selection, crate::pdf::PageOperation::Remove)?;
            filtered.save(&path)?;
        }
        merger.append(&path, input)?;
    }
    let (mut doc, pages) = merger
        .finish()
        .context("Excel produced no nonblank pages")?;
    doc.save(output)?;
    eprintln!("Excel: {count} print sections, {pages} pages, {removed} blank pages removed");
    Ok(())
}

fn run_script(
    script: &Path,
    paths: &[&Path; 3],
    timeout: Duration,
    mode: &str,
    log: &Path,
) -> Result<bool> {
    let log_file = File::create(log)?;
    let mut child = Command::new("/usr/bin/osascript")
        .arg(script)
        .args(paths)
        .arg(timeout.as_secs().to_string())
        .arg(mode)
        .arg(paths[0].file_name().context("missing staged filename")?)
        .stdin(Stdio::null())
        .stdout(log_file.try_clone()?)
        .stderr(log_file)
        .spawn()
        .context("start macOS Office automation")?;
    // Give AppleScript's timeout a chance to return its error before killing it.
    match child.wait_timeout(timeout + Duration::from_secs(2)) {
        Ok(Some(status)) => Ok(status.success()),
        result => {
            let _ = child.kill();
            let _ = child.wait();
            if let Err(error) = result {
                return Err(error).context("wait for Office automation");
            }
            bail!("timed out after {} seconds", timeout.as_secs())
        }
    }
}

fn log_tail(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(file.metadata()?.len().saturating_sub(8192)))?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_matching_app_in_system_or_user_applications() {
        let dir = tempfile::tempdir().unwrap();
        let directories = vec![dir.path().join("system"), dir.path().join("user")];
        fs::create_dir_all(directories[0].join("Microsoft Word.app")).unwrap();
        fs::create_dir_all(&directories[1]).unwrap();
        // A file with the bundle's name is not an installed application.
        fs::write(directories[0].join("Microsoft Excel.app"), b"not an app").unwrap();
        for extension in ["doc", "docx", "rtf", "odt"] {
            assert_eq!(installed_application(extension, &directories), Some("Word"));
        }
        for extension in ["xls", "xlsx", "ppt", "pptx", "ods", "odp"] {
            assert_eq!(installed_application(extension, &directories), None);
        }
        fs::create_dir(directories[1].join("Microsoft Excel.app")).unwrap();
        fs::create_dir(directories[1].join("Microsoft PowerPoint.app")).unwrap();
        for extension in ["xls", "xlsx", "ods"] {
            assert_eq!(
                installed_application(extension, &directories),
                Some("Excel")
            );
        }
        for extension in ["ppt", "pptx", "odp"] {
            assert_eq!(
                installed_application(extension, &directories),
                Some("PowerPoint")
            );
        }
        for extension in ["pdf", "unknown"] {
            assert_eq!(installed_application(extension, &directories), None);
        }
    }

    #[test]
    fn missing_microsoft_apps_route_every_supported_format_to_libreoffice() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("soffice");
        fs::write(
            &executable,
            r#"#!/bin/sh
for input do :; done
printf 'exported by LibreOffice' > "${input%.*}.pdf"
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let mut converter = find_converter(Some(&executable)).unwrap();
        converter.application_directories = vec![dir.path().join("no apps installed")];
        for extension in [
            "doc", "docx", "rtf", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp",
        ] {
            let input = dir.path().join(format!("original.{extension}"));
            fs::write(&input, b"original").unwrap();
            let result = convert(
                &mut converter,
                &input,
                extension,
                &dir.path().join(extension),
                Duration::from_secs(5),
                &ExcelOptions::default(),
            )
            .unwrap();
            assert_eq!(fs::read(result).unwrap(), b"exported by LibreOffice");
            assert_eq!(fs::read(input).unwrap(), b"original");
            assert!(
                converter.lock.is_none(),
                "fallback must not lock Microsoft Office"
            );
        }
    }

    #[test]
    fn fallback_errors_and_timeouts_are_reported_without_locking_office() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("soffice");
        let input = dir.path().join("input.odt");
        fs::write(&input, b"original").unwrap();
        let mut converter = find_converter(Some(&executable)).unwrap();
        converter.application_directories.clear();
        let run = |converter: &mut Converter, name: &str| {
            convert(
                converter,
                &input,
                "odt",
                &dir.path().join(name),
                Duration::from_secs(1),
                &ExcelOptions::default(),
            )
        };
        assert!(
            format!("{:#}", run(&mut converter, "missing").unwrap_err())
                .contains("LibreOffice executable not found")
        );
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            run(&mut converter, "empty")
                .unwrap_err()
                .to_string()
                .contains("produce a PDF")
        );
        fs::write(&executable, "#!/bin/sh\nexec sleep 30\n").unwrap();
        let start = Instant::now();
        assert!(
            run(&mut converter, "timeout")
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(converter.lock.is_none());
        assert_eq!(fs::read(input).unwrap(), b"original");
    }

    #[test]
    fn fallback_rejects_explicit_excel_options_for_ods_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut converter = find_converter(Some(&dir.path().join("missing-soffice"))).unwrap();
        converter.application_directories.clear();
        let areas = ["Sheet1=A1:D20".parse().unwrap()];
        for options in [
            ExcelOptions {
                fit_width: true,
                ..Default::default()
            },
            ExcelOptions {
                preserve_layout: true,
                ..Default::default()
            },
            ExcelOptions {
                print_areas: &areas,
                ..Default::default()
            },
        ] {
            let result = convert(
                &mut converter,
                &dir.path().join("input.ods"),
                "ods",
                dir.path(),
                Duration::from_secs(1),
                &options,
            );
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("Excel options require Microsoft Excel export")
            );
        }
    }

    #[test]
    #[ignore = "requires LibreOffice"]
    fn real_libreoffice_fallback_converts_modern_and_legacy_office_files() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let explicit = env::var_os("PDFCLI_SOFFICE").map(PathBuf::from);
        let mut converter = find_converter(explicit.as_deref()).unwrap();
        // Simulate a Mac without Microsoft apps while using the real renderer.
        converter.application_directories.clear();
        for (extension, marker) in [
            ("doc", "WORD MARKER"),
            ("docx", "WORD MARKER"),
            ("xls", "SHEET MARKER"),
            ("xlsx", "SHEET MARKER"),
            ("ppt", "SLIDE MARKER"),
            ("pptx", "SLIDE MARKER"),
            ("odt", "WORD MARKER"),
            ("ods", "SHEET MARKER"),
            ("odp", "SLIDE MARKER"),
        ] {
            let input = fixture.join(format!("sample.{extension}"));
            let original = fs::read(&input).unwrap();
            let output = convert(
                &mut converter,
                &input,
                extension,
                &dir.path().join(extension),
                Duration::from_secs(120),
                &ExcelOptions::default(),
            )
            .unwrap();
            let doc = lopdf::Document::load(output).unwrap();
            assert_eq!(doc.get_pages().len(), 1, "{extension}");
            assert!(
                doc.extract_text(&[1]).unwrap().contains(marker),
                "{extension}"
            );
            assert_eq!(fs::read(input).unwrap(), original);
        }
    }
}
