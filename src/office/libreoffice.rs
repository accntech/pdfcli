use anyhow::{Context, Result, bail, ensure};
use std::{
    env,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

pub fn is_supported(extension: &str) -> bool {
    matches!(
        extension,
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" | "rtf"
    )
}

pub fn find_soffice(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return resolve_executable(path)
            .with_context(|| format!("LibreOffice executable not found: {}", path.display()));
    }
    for candidate in [
        "soffice",
        "libreoffice",
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        "C:\\Program Files\\LibreOffice\\program\\soffice.com",
        "C:\\Program Files\\LibreOffice\\program\\soffice.exe",
    ] {
        if let Some(path) = resolve_executable(Path::new(candidate)) {
            return Ok(path);
        }
    }
    bail!(
        "Office conversion requires LibreOffice. Install it and put soffice on PATH, or use --soffice /path/to/soffice (PDFCLI_SOFFICE)"
    )
}

fn resolve_executable(path: &Path) -> Option<PathBuf> {
    if path.components().count() > 1 || path.is_absolute() {
        return path.is_file().then(|| path.to_owned());
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(path))
        .find(|path| path.is_file())
}

pub fn convert(
    soffice: &Path,
    input: &Path,
    extension: &str,
    directory: &Path,
    timeout: Duration,
) -> Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let directory = fs::canonicalize(directory)?;
    let profile = directory.join("profile");
    fs::create_dir(&profile)?;
    // A separate profile avoids attaching to the user's running LibreOffice.
    // Disable document macros in this disposable profile.
    fs::create_dir(profile.join("user"))?;
    fs::write(
        profile.join("user/registrymodifications.xcu"),
        r#"<?xml version="1.0" encoding="UTF-8"?><oor:items xmlns:oor="http://openoffice.org/2001/registry"><item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop></item></oor:items>"#,
    )?;
    let profile_url = url::Url::from_directory_path(&profile)
        .map_err(|_| anyhow::anyhow!("cannot form LibreOffice profile URL"))?;
    // Use a private copy and fixed basename: identical names, non-UTF8 paths,
    // leading dashes, and different Office extensions cannot collide.
    let copy = directory.join(format!("input.{extension}"));
    fs::copy(input, &copy).with_context(|| format!("stage {}", input.display()))?;
    let filter = match extension {
        "xls" | "xlsx" | "ods" => "calc_pdf_Export",
        "ppt" | "pptx" | "odp" => "impress_pdf_Export",
        _ => "writer_pdf_Export",
    };
    let log_path = directory.join("conversion.log");
    let log = File::create(&log_path)?;
    let mut command = Command::new(soffice);
    command
        .arg(format!("-env:UserInstallation={profile_url}"))
        .args([
            "--headless",
            "--nologo",
            "--nodefault",
            "--norestore",
            "--convert-to",
        ])
        .arg(format!("pdf:{filter}"))
        .arg("--outdir")
        .arg(&directory)
        .arg(&copy)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("start LibreOffice: {}", soffice.display()))?;
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        other => {
            #[cfg(unix)]
            // SAFETY: the child is placed in a new process group above; the
            // negative ID targets only this conversion and its descendants.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            if let Err(error) = other {
                return Err(error).context("wait for LibreOffice");
            }
            bail!(
                "LibreOffice timed out after {} seconds converting {}",
                timeout.as_secs(),
                input.display()
            );
        }
    };
    let output = directory.join("input.pdf");
    ensure!(
        status.success() && output.is_file(),
        "LibreOffice did not produce a PDF for {} (exit {status}): {}",
        input.display(),
        log_tail(&log_path)?
    );
    ensure!(
        fs::metadata(&output)?.len() > 0,
        "LibreOffice produced an empty PDF for {}",
        input.display()
    );
    Ok(output)
}

fn log_tail(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let start = file.metadata()?.len().saturating_sub(8192);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}
