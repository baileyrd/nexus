//! File utility helpers: slug generation, filename validation,
//! attachment naming, SHA-256 hashing, and MIME detection.

mod attachment;
mod filename;
mod mime;
mod slug;

pub use attachment::{attachment_name, sha256_hex};
pub use filename::{validate_filename, validate_path, MAX_FILENAME_BYTES, MAX_PATH_BYTES};
pub use mime::detect_mime;
pub use slug::slugify;
