use crate::models::identity::UintId;

pub type ImageId = UintId<Image>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ImageResponse {
    pub id: ImageId,
    pub url: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Image {
    pub original_filename: String,
    pub url: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    /// When the image was moved to the trash, if it is there.
    ///
    /// Trashing is not deleting: the record and the bytes stay, so content that references the
    /// image keeps resolving and the operator can change their mind. Purging is what removes both.
    #[serde(default)]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One entry in the image library: the metadata the admin screen lists.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ImageEntry {
    pub id: ImageId,
    pub url: String,
    pub original_filename: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    /// When it was moved to the trash, which is how the two lists are told apart.
    #[serde(default)]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ImageEntry {
    /// Pair stored metadata with the id it is filed under.
    pub fn from_image(id: ImageId, image: Image) -> Self {
        ImageEntry {
            id,
            url: image.url,
            original_filename: image.original_filename,
            uploaded_at: image.uploaded_at,
            deleted_at: image.deleted_at,
        }
    }
}

/// Which content uses an image.
///
/// A collection item is named by its collection and its id; a single page is named by the page,
/// because it has exactly one item and the id would say nothing.
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct ImageOwner {
    pub kind: ImageOwnerKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
}

#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ImageOwnerKind {
    CollectionItem,
    SinglePage,
}

impl ImageOwner {
    pub fn collection_item(name: &str, item: u64) -> Self {
        ImageOwner {
            kind: ImageOwnerKind::CollectionItem,
            name: name.to_string(),
            item: Some(item),
        }
    }

    pub fn single_page(name: &str) -> Self {
        ImageOwner {
            kind: ImageOwnerKind::SinglePage,
            name: name.to_string(),
            item: None,
        }
    }

    /// The owner as one string, for a key in the index.
    ///
    /// The name is escaped (`:` becomes `%3A`), so a collection named `a:b` cannot produce the
    /// same key as one named `a` with an item called `b`.
    pub fn storage_key(&self) -> String {
        match self.kind {
            ImageOwnerKind::CollectionItem => format!(
                "collection:{}:{}",
                escape(&self.name),
                self.item.unwrap_or_default()
            ),
            ImageOwnerKind::SinglePage => format!("page:{}", escape(&self.name)),
        }
    }

    /// The owner back from [`ImageOwner::storage_key`].
    pub fn from_storage_key(key: &str) -> Option<Self> {
        let mut parts = key.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("collection"), Some(name), Some(item)) => item
                .parse()
                .ok()
                .map(|item| ImageOwner::collection_item(&unescape(name), item)),
            (Some("page"), Some(name), _) => Some(ImageOwner::single_page(&unescape(name))),
            _ => None,
        }
    }
}

fn escape(name: &str) -> String {
    name.replace('%', "%25").replace(':', "%3A")
}

fn unescape(name: &str) -> String {
    name.replace("%3A", ":").replace("%25", "%")
}

/// Every image a piece of content uses.
///
/// `Image` fields hold ids, so those are exact. Markdown is read for the durable id link
/// (`/images/by-id/<id>`) and nothing else: a hand-written URL to wherever the bytes are served
/// from is indistinguishable from an ordinary link, and would go stale the moment the image is
/// replaced.
pub fn referenced_images<T>(item: &crate::models::field::FieldValueMap<T>) -> Vec<ImageId> {
    let mut found = std::collections::BTreeSet::new();
    for value in item.0.values() {
        collect_images(value, &mut found);
    }
    found.into_iter().collect()
}

fn collect_images(
    value: &crate::models::field::FieldValue,
    found: &mut std::collections::BTreeSet<ImageId>,
) {
    use crate::models::field::FieldValue;
    match value {
        FieldValue::Image(Some(id)) => {
            found.insert(id.clone());
        }
        FieldValue::Array(items) => {
            for item in items {
                collect_images(item, found);
            }
        }
        FieldValue::CompositeField(Some(composite)) => {
            for value in composite.values.values() {
                collect_images(value, found);
            }
        }
        FieldValue::Markdown(text) => {
            for id in image_ids_in_markdown(text) {
                found.insert(id);
            }
        }
        _ => {}
    }
}

/// The images a Markdown body links to by their durable id.
pub fn image_ids_in_markdown(text: &str) -> Vec<ImageId> {
    const MARKER: &str = "/images/by-id/";
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(MARKER) {
        rest = &rest[at + MARKER.len()..];
        let digits: String = rest
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            // Not a link: keep looking from where the marker was left behind.
            continue;
        }
        if let Ok(id) = digits.parse::<u64>() {
            ids.push(ImageId::from_u64(id));
        }
    }
    ids
}

/// The reference index's half of this module: which content uses which image.
#[cfg(test)]
mod reference_tests {
    use super::*;
    use crate::models::field::{CompositeFieldValue, FieldValue, FieldValueMap};
    use std::collections::HashMap;

    fn values(
        entries: Vec<(&str, FieldValue)>,
    ) -> FieldValueMap<Vec<crate::models::schema::FieldSchema>> {
        FieldValueMap(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            std::marker::PhantomData,
        )
    }

    #[test]
    fn an_owner_survives_its_storage_key() {
        for owner in [
            ImageOwner::collection_item("blog", 7),
            ImageOwner::single_page("about"),
            // A name that could otherwise be read as two fields of a key.
            ImageOwner::collection_item("a:b", 3),
            ImageOwner::single_page("a:1"),
        ] {
            assert_eq!(
                ImageOwner::from_storage_key(&owner.storage_key()),
                Some(owner.clone()),
                "{}",
                owner.storage_key()
            );
        }
        assert_eq!(ImageOwner::from_storage_key("nonsense"), None);
    }

    #[test]
    fn image_fields_are_read_wherever_they_sit() {
        let mut composite_values = HashMap::new();
        composite_values.insert(
            "photo".to_string(),
            FieldValue::Image(Some(ImageId::from_u64(3))),
        );
        let item = values(vec![
            ("cover", FieldValue::Image(Some(ImageId::from_u64(1)))),
            ("empty", FieldValue::Image(None)),
            (
                "gallery",
                FieldValue::Array(vec![
                    FieldValue::Image(Some(ImageId::from_u64(2))),
                    FieldValue::Text("not an image".to_string()),
                ]),
            ),
            (
                "block",
                FieldValue::CompositeField(Some(CompositeFieldValue {
                    id: "comp".into(),
                    values: FieldValueMap(composite_values, std::marker::PhantomData),
                })),
            ),
        ]);

        let found: Vec<u64> = referenced_images(&item).into_iter().map(|id| *id).collect();
        assert_eq!(found, vec![1, 2, 3], "sorted and deduplicated");
    }

    #[test]
    fn markdown_is_read_for_the_durable_link_only() {
        let text = "See ![one](/images/by-id/4) and [two](https://cms.example.com/images/by-id/12).\n\
                    Not this: /images/3ec3df37.png or /images/by-id/ or /images/by-id/abc";
        let found: Vec<u64> = image_ids_in_markdown(text)
            .into_iter()
            .map(|id| *id)
            .collect();
        assert_eq!(found, vec![4, 12]);

        // The same link written twice is one image.
        assert_eq!(
            image_ids_in_markdown("/images/by-id/4 /images/by-id/4").len(),
            2
        );
        assert!(image_ids_in_markdown("no links here").is_empty());
    }
}

/// The longest display name an image may carry.
///
/// Long enough for the file names people actually have, short enough that the library stays
/// readable and a record stays small.
pub const MAX_IMAGE_NAME_LENGTH: usize = 255;

/// Changing an image's record: the name it is shown under, or the bytes it serves.
///
/// Both fields are optional so a caller can do one without the other; the screen does them one at
/// a time, and a replacement finishes with the file name the upload was given.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UpdateImageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// Asking to replace the bytes of an existing image.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ReplaceImageRequest {
    pub ext: String,
}

/// Where to put replacement bytes, and what they will be called once they are there.
///
/// Unlike [`NewImageInfo`] there is no id: the image already has one, and nothing about the
/// record changes until the bytes have arrived (see `ImageService::replace_image`).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ReplacementInfo {
    /// The file name the bytes were uploaded under; pass it back to apply the replacement.
    pub file_name: String,
    /// Where to PUT them. Short-lived, like [`NewImageInfo::upload_url`].
    pub upload_url: String,
}

/// Whether `name` is usable as the *display* name of an image.
///
/// The display name is never used to build a path - the stored file name is generated - so it
/// may be anything a reader recognises, including non-ASCII. What it may not be is empty (the
/// library would show a blank entry) or a path, and control characters would let a name break
/// the line it is printed on.
pub fn is_safe_display_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= MAX_IMAGE_NAME_LENGTH
        && !trimmed.contains(['/', '\\'])
        && !trimmed.chars().any(char::is_control)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct NewImageRequest {
    pub original_filename: String,
    pub ext: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct NewImageInfo {
    pub id: ImageId,
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
        for ext in [
            "../../etc/passwd",
            "a/b",
            "..\\..\\x",
            "p n g",
            "waytoolongextension",
        ] {
            assert!(!is_safe_image_ext(ext), "expected {ext:?} to be rejected");
        }
    }

    #[test]
    fn sanitize_ext_strips_separators_and_traversal() {
        assert_eq!(sanitize_ext("png").as_deref(), Some("png"));
        assert_eq!(sanitize_ext(".PNG").as_deref(), Some("png"));
        assert_eq!(sanitize_ext("jpeg").as_deref(), Some("jpeg"));
        // Path separators and dots cannot survive.
        assert_eq!(
            sanitize_ext("../../etc/passwd").as_deref(),
            Some("etcpasswd")
        );
        assert_eq!(sanitize_ext("a/b").as_deref(), Some("ab"));
        assert_eq!(sanitize_ext("..\\..\\x").as_deref(), Some("x"));
        // Nothing usable left -> no extension.
        assert_eq!(sanitize_ext(""), None);
        assert_eq!(sanitize_ext("..."), None);
        assert_eq!(sanitize_ext("../"), None);
    }
}
