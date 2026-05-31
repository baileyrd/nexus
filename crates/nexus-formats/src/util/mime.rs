//! Simple MIME type detection by file extension.

/// Return a MIME type string for the given file extension.
///
/// The extension should be provided **without** the leading dot (e.g. `"png"`).
/// Returns `"application/octet-stream"` for unknown extensions.
#[must_use]
pub fn detect_mime(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        // Text / markup
        "md" | "mdx" => "text/markdown",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",

        // JSON / structured
        "json" | "canvas" => "application/json",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "xml" => "application/xml",

        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "avif" => "image/avif",

        // Documents
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",

        // Audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",

        // Video
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",

        // Code / scripts
        "rs" => "text/x-rust",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "py" => "text/x-python",
        "sh" => "text/x-sh",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" => "text/x-c++",

        // Archives
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",

        _ => "application/octet-stream",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions() {
        assert_eq!(detect_mime("md"), "text/markdown");
        assert_eq!(detect_mime("png"), "image/png");
        assert_eq!(detect_mime("pdf"), "application/pdf");
        assert_eq!(detect_mime("json"), "application/json");
        assert_eq!(detect_mime("mp4"), "video/mp4");
        assert_eq!(detect_mime("mp3"), "audio/mpeg");
    }

    #[test]
    fn canvas_is_json() {
        assert_eq!(detect_mime("canvas"), "application/json");
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(detect_mime("PNG"), "image/png");
        assert_eq!(detect_mime("Pdf"), "application/pdf");
    }

    #[test]
    fn unknown_extension_fallback() {
        assert_eq!(detect_mime("xyz"), "application/octet-stream");
        assert_eq!(detect_mime(""), "application/octet-stream");
        assert_eq!(detect_mime("wasm"), "application/octet-stream");
    }
}
