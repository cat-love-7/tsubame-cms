use crate::models::identity::UintId;


pub type ImageID = UintId<Image>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ImageResponse {
    pub id: ImageID,
    pub url: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Image {
    pub original_filename: String,
    pub url: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

/// One entry in the image library: the metadata the admin screen lists.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ImageEntry {
    pub id: ImageID,
    pub url: String,
    pub original_filename: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

impl ImageEntry {
    /// Pair stored metadata with the id it is filed under.
    pub fn from_image(id: ImageID, image: Image) -> Self {
        ImageEntry {
            id,
            url: image.url,
            original_filename: image.original_filename,
            uploaded_at: image.uploaded_at,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct NewImageRequest{
    pub original_filename: String,
    pub ext: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct NewImageInfo{
    pub id: ImageID,
    /// Where to PUT the bytes. On-premises this points at the CMS (with a one-shot token in
    /// the query), on AWS at S3 with a signature — either way it is **short-lived** and is not
    /// what belongs in content.
    pub upload_url: String,
    /// Where the image will be readable from once it is uploaded: the *stable* URL, which is
    /// what content should store. A presigned URL in a page would expire with the page, and the
    /// client cannot derive this one from `upload_url` on AWS (it names a signature, not the
    /// object's public address).
    pub url: String,
}

/// Whether `name` is safe to use as a bare file name (and, on S3, as an object key
/// suffix).
///
/// This matters because HTTP path parameters are percent-decoded *after* routing, so a
/// request such as `/images/..%2F..%2Fsecrets` arrives at the handler as `../../secrets`
/// while having matched a single `{file_name}` segment. Any separator, parent reference
/// or absolute path is therefore rejected. Both the HTTP layer and the on-premises
/// adapter apply this check (defence in depth).
pub fn is_safe_file_name(name: &str) -> bool {
    if name.is_empty() || name.contains(['/', '\\', '\0']) {
        return false;
    }
    let mut components = std::path::Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

/// Whether `ext` is acceptable as an image extension.
///
/// The extension is client-supplied and is embedded into the generated file name, so
/// anything other than 1..=10 alphanumeric characters (after an optional leading dot) is
/// rejected outright rather than silently rewritten. An empty extension is allowed, in
/// which case the file name has no extension.
pub fn is_safe_image_ext(ext: &str) -> bool {
    let trimmed = ext.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        return true;
    }
    trimmed.len() <= 10 && trimmed.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Normalise a caller-supplied extension into something safe to embed in a file name.
///
/// The extension arrives from the client, so anything that could introduce a path separator or
/// a traversal (`../../x`) must be stripped. Returns `None` when nothing usable remains, in
/// which case the file name simply has no extension.
///
/// Shared by both adapters so that a local upload and an S3 object key are cleaned the same way.
pub fn sanitize_ext(ext: &str) -> Option<String> {
    let cleaned: String = ext
        .trim()
        .trim_start_matches('.')
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(10)
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::{is_safe_file_name, is_safe_image_ext, sanitize_ext};

    #[test]
    fn accepts_plain_file_names() {
        assert!(is_safe_file_name("abc.png"));
        assert!(is_safe_file_name("a-b_c.1.jpg"));
        assert!(is_safe_file_name("0b8f3a2c-1.png"));
    }

    #[test]
    fn rejects_traversal_separators_and_absolute_paths() {
        let unsafe_names = [
            "",
            ".",
            "..",
            "../x",
            "../../etc/passwd",
            "a/b",
            "sub/../x",
            "/etc/passwd",
            "..\\x",
            "a\\b",
        ];
        for name in unsafe_names {
            assert!(!is_safe_file_name(name), "expected {name:?} to be rejected");
        }
    }

    #[test]
    fn accepts_plain_image_extensions() {
        assert!(is_safe_image_ext("png"));
        assert!(is_safe_image_ext(".PNG"));
        assert!(is_safe_image_ext("jpeg"));
        // An absent extension is allowed; the file name is then a bare UUID.
        assert!(is_safe_image_ext(""));
        assert!(is_safe_image_ext("..."));
    }

    #[test]
    fn rejects_extension_containing_path_characters() {
        for ext in ["../../etc/passwd", "a/b", "..\\..\\x", "p n g", "waytoolongextension"] {
            assert!(!is_safe_image_ext(ext), "expected {ext:?} to be rejected");
        }
    }

    #[test]
    fn sanitize_ext_strips_separators_and_traversal() {
        assert_eq!(sanitize_ext("png").as_deref(), Some("png"));
        assert_eq!(sanitize_ext(".PNG").as_deref(), Some("png"));
        assert_eq!(sanitize_ext("jpeg").as_deref(), Some("jpeg"));
        // Path separators and dots cannot survive.
        assert_eq!(sanitize_ext("../../etc/passwd").as_deref(), Some("etcpasswd"));
        assert_eq!(sanitize_ext("a/b").as_deref(), Some("ab"));
        assert_eq!(sanitize_ext("..\\..\\x").as_deref(), Some("x"));
        // Nothing usable left -> no extension.
        assert_eq!(sanitize_ext(""), None);
        assert_eq!(sanitize_ext("..."), None);
        assert_eq!(sanitize_ext("../"), None);
    }
}
