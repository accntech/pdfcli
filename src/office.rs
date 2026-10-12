#[cfg(target_os = "macos")]
mod excel;
#[cfg(target_os = "macos")]
mod excel_layout;
mod libreoffice;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use excel::ExcelPrintArea;
pub use libreoffice::is_supported;
#[cfg(not(target_os = "macos"))]
pub use libreoffice::{convert, find_soffice as find_converter};
#[cfg(target_os = "macos")]
pub use macos::{ExcelOptions, convert, find_converter};
