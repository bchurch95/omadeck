use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Office toolkit error: {0}")]
    OfficeToolkit(#[from] office_toolkit::Error),

    #[error("OPC package error: {0}")]
    Opc(#[from] opc_ooxml::Error),

    #[error("PowerPoint error: {0}")]
    Powerpoint(#[from] office_toolkit::powerpoint::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Slide index {0} out of range")]
    OutOfRange(usize),

    #[error("No shape with id {0} on the slide")]
    ShapeNotFound(u32),

    #[error("PDF export error: {0}")]
    Pdf(#[from] printpdf::Error),

    #[error("Image decode error: {0}")]
    Image(String),

    #[error("Invalid theme color: {0}")]
    InvalidColor(String),
}
