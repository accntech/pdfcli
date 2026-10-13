use anyhow::{Context, Result, bail, ensure};
use std::{
    env,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use wait_timeout::ChildExt;

pub struct Converter {
    powershell: PathBuf,
    soffice: Option<PathBuf>,
}

pub fn find_converter(explicit: Option<&Path>) -> Result<Converter> {
    let system_root = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    Ok(Converter {
        powershell: system_root.join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        soffice: explicit.map(Path::to_owned),
    })
}

pub fn convert(
    converter: &mut Converter,
    input: &Path,
    extension: &str,
    directory: &Path,
    timeout: Duration,
) -> Result<PathBuf> {
    let app = super::microsoft_application(extension).context("unsupported Office input")?;
    let started = Instant::now();
    let job = tempfile::Builder::new()
        .prefix("pdfcli-office-")
        .tempdir()?;
    let staged = job.path().join(format!("input.{extension}"));
    fs::copy(input, &staged).with_context(|| format!("stage {}", input.display()))?;
    let output = job.path().join("converted.pdf");
    let script = job.path().join("convert.ps1");
    let recovery = job.path().join("recovery.json");
    let log_path = job.path().join("conversion.log");
    fs::write(&script, include_str!("windows.ps1"))?;
    let log = File::create(&log_path)?;
    let mut child = Command::new(&converter.powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-STA",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        // Paths are data, never interpolated into PowerShell source or commands.
        .env("PDFCLI_OFFICE_APP", app)
        .env("PDFCLI_OFFICE_INPUT", &staged)
        .env("PDFCLI_OFFICE_OUTPUT", &output)
        .env("PDFCLI_OFFICE_RECOVERY", &recovery)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("start Windows PowerShell for Microsoft Office conversion")?;
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        result => {
            // Never terminate an Office process that may contain user documents.
            let _ = child.kill();
            let _ = child.wait();
            let retained = job.keep();
            let reason = match result {
                Err(error) => format!("failed waiting for PowerShell: {error}"),
                _ => format!("timed out after {} seconds", timeout.as_secs()),
            };
            bail!(
                "Microsoft {app} {reason}. Office may still have the staged copy open; conversion and recovery files retained at {}",
                retained.display()
            );
        }
    };
    // Only an unregistered COM application triggers fallback. Launch, export,
    // cleanup, activation and permission errors from installed Office are errors.
    if status.code() != Some(42) {
        let result = (|| -> Result<PathBuf> {
            ensure!(
                status.success(),
                "Microsoft {app} could not convert {}: {}",
                input.display(),
                log_tail(&log_path)?
            );
            ensure!(
                output.is_file() && fs::metadata(&output)?.len() > 0,
                "Microsoft {app} did not produce a nonempty PDF for {}",
                input.display()
            );
            fs::create_dir_all(directory)?;
            let collected = directory.join("converted.pdf");
            fs::copy(&output, &collected).context("collect Microsoft Office PDF")?;
            eprintln!("Using Microsoft {app} for {}", input.display());
            Ok(collected)
        })();
        if result.is_err() && recovery.exists() {
            let retained = job.keep();
            return result.with_context(|| {
                format!(
                    "Office cleanup was incomplete; recovery files retained at {}",
                    retained.display()
                )
            });
        }
        return result;
    }
    let soffice = super::libreoffice::find_soffice(converter.soffice.as_deref())?;
    let remaining = timeout
        .checked_sub(started.elapsed())
        .filter(|duration| !duration.is_zero())
        .context("Office detection exceeded conversion timeout")?;
    eprintln!(
        "Microsoft {app} is not installed; using LibreOffice for {}",
        input.display()
    );
    super::libreoffice::convert(&soffice, input, extension, directory, remaining)
}

fn log_tail(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(file.metadata()?.len().saturating_sub(8192)))?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    fn executable(path: &Path, source: &str) {
        fs::write(path, format!("#!/bin/sh\n{source}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn native_windows_export_is_preferred_for_office_and_opendocument() {
        let dir = tempfile::tempdir().unwrap();
        let runner = dir.path().join("powershell");
        executable(
            &runner,
            "printf '%s' \"$PDFCLI_OFFICE_APP\" > \"$PDFCLI_OFFICE_OUTPUT\"",
        );
        let mut converter = find_converter(Some(&dir.path().join("missing-soffice"))).unwrap();
        converter.powershell = runner;
        for (extension, app) in [
            ("doc", "Word"),
            ("docx", "Word"),
            ("rtf", "Word"),
            ("odt", "Word"),
            ("xls", "Excel"),
            ("xlsx", "Excel"),
            ("ods", "Excel"),
            ("ppt", "PowerPoint"),
            ("pptx", "PowerPoint"),
            ("odp", "PowerPoint"),
        ] {
            let input = dir.path().join(format!("input ' $ Ω.{extension}"));
            fs::write(&input, b"original").unwrap();
            let result = convert(
                &mut converter,
                &input,
                extension,
                &dir.path().join(extension),
                Duration::from_secs(5),
            )
            .unwrap();
            assert_eq!(fs::read_to_string(result).unwrap(), app);
            assert_eq!(fs::read(input).unwrap(), b"original");
        }
    }

    #[test]
    fn windows_native_failures_do_not_silently_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let runner = dir.path().join("powershell");
        let fallback = dir.path().join("soffice");
        executable(&runner, "echo 'Office activation failed' >&2; exit 1");
        executable(
            &fallback,
            "for input do :; done; printf 'fallback' > \"${input%.*}.pdf\"",
        );
        let mut converter = find_converter(Some(&fallback)).unwrap();
        converter.powershell = runner;
        let input = dir.path().join("document.docx");
        fs::write(&input, b"original").unwrap();
        let result = convert(
            &mut converter,
            &input,
            "docx",
            &dir.path().join("out"),
            Duration::from_secs(5),
        );
        assert!(
            result.is_err(),
            "an installed Office app's failure must be reported"
        );
        assert!(format!("{:#}", result.unwrap_err()).contains("Office activation failed"));
    }

    #[test]
    fn unregistered_windows_apps_use_libreoffice_for_every_format() {
        let dir = tempfile::tempdir().unwrap();
        let runner = dir.path().join("powershell");
        let fallback = dir.path().join("soffice");
        executable(&runner, "exit 42");
        executable(
            &fallback,
            "for input do :; done; printf 'fallback' > \"${input%.*}.pdf\"",
        );
        let mut converter = find_converter(Some(&fallback)).unwrap();
        converter.powershell = runner;
        for extension in [
            "doc", "docx", "rtf", "odt", "xls", "xlsx", "ods", "ppt", "pptx", "odp",
        ] {
            let input = dir.path().join(format!("document.{extension}"));
            fs::write(&input, b"original").unwrap();
            let result = convert(
                &mut converter,
                &input,
                extension,
                &dir.path().join(extension),
                Duration::from_secs(5),
            )
            .unwrap();
            assert_eq!(fs::read(result).unwrap(), b"fallback");
            assert_eq!(fs::read(input).unwrap(), b"original");
        }
    }

    #[test]
    fn windows_requires_nonempty_output_and_retains_incomplete_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let runner = dir.path().join("powershell");
        let mut converter = find_converter(None).unwrap();
        converter.powershell = runner.clone();
        let input = dir.path().join("document.docx");
        fs::write(&input, b"original").unwrap();
        for script in ["exit 0", ": > \"$PDFCLI_OFFICE_OUTPUT\""] {
            executable(&runner, script);
            let result = convert(
                &mut converter,
                &input,
                "docx",
                &dir.path().join("out"),
                Duration::from_secs(5),
            );
            assert!(result.unwrap_err().to_string().contains("nonempty PDF"));
        }
        executable(
            &runner,
            "echo '{}' > \"$PDFCLI_OFFICE_RECOVERY\"; echo \"$PDFCLI_OFFICE_RECOVERY\" > \"$0.record\"; exit 1",
        );
        let result = convert(
            &mut converter,
            &input,
            "docx",
            &dir.path().join("out"),
            Duration::from_secs(5),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cleanup was incomplete")
        );
        let recovery = fs::read_to_string(runner.with_extension("record")).unwrap();
        let recovery = Path::new(recovery.trim());
        assert!(recovery.is_file());
        assert!(recovery.parent().unwrap().join("input.docx").is_file());
        fs::remove_dir_all(recovery.parent().unwrap()).unwrap();
    }

    #[test]
    fn windows_timeout_is_bounded_and_preserves_recovery_files() {
        let dir = tempfile::tempdir().unwrap();
        let runner = dir.path().join("powershell");
        executable(&runner, "exec sleep 30");
        let mut converter = find_converter(None).unwrap();
        converter.powershell = runner;
        let input = dir.path().join("document.docx");
        fs::write(&input, b"original").unwrap();
        let started = Instant::now();
        let result = convert(
            &mut converter,
            &input,
            "docx",
            &dir.path().join("out"),
            Duration::from_secs(1),
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains("timed out after 1 seconds"));
        assert!(started.elapsed() < Duration::from_secs(5));
        let retained = Path::new(message.split_once("retained at ").unwrap().1);
        assert!(retained.join("input.docx").is_file());
        fs::remove_dir_all(retained).unwrap();
    }
}
