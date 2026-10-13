#[cfg(target_os = "macos")]
mod excel;
#[cfg(target_os = "macos")]
mod excel_layout;
mod libreoffice;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(windows, test))]
mod windows;

#[cfg(target_os = "macos")]
pub use excel::ExcelPrintArea;
pub use libreoffice::is_supported;
#[cfg(not(any(target_os = "macos", windows)))]
pub use libreoffice::{convert, find_soffice as find_converter};
#[cfg(target_os = "macos")]
pub use macos::{ExcelOptions, convert, find_converter};
#[cfg(windows)]
pub use windows::{convert, find_converter};

#[cfg(any(target_os = "macos", windows, test))]
fn microsoft_application(extension: &str) -> Option<&'static str> {
    match extension {
        "doc" | "docx" | "rtf" | "odt" => Some("Word"),
        "xls" | "xlsx" | "ods" => Some("Excel"),
        "ppt" | "pptx" | "odp" => Some("PowerPoint"),
        _ => None,
    }
}
