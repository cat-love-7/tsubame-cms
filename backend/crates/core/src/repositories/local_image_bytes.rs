//! The part of image storage that only a *local* adapter has to provide.
//!
//! The shared contract ([`crate::repositories::image_repository::ImageRepository`]) is "hand
//! out a place to PUT the bytes, and keep the record of what was uploaded". A local adapter
//! answers that with an upload URL served by this application, which means it also has to
//! authorise that PUT (a single-use token) and read and write the bytes itself. An adapter
//! backed by object storage needs none of it: the object store authorises the upload, serves
//! the bytes, and the URL handed out is absolute.
//!
//! Keeping these apart is what lets the shared HTTP layer say what *every* backend must do,
//! instead of an adapter that never serves bytes having to implement three methods that must
//! never be called. The routes that use them are compiled only for the adapter that has them
//! (`src/http/images.rs`).

use std::future::Future;

use crate::repositories::image_repository::BoxError;

pub trait LocalImageBytes: Send + Sync {
    /// Consume the one-shot upload token previously handed out by
    /// [`crate::repositories::image_repository::ImageRepository::generate_image_upload_url`],
    /// returning the file name that the token authorises.
    ///
    /// Tokens are single-use: a successful call must also invalidate the token, so a leaked
    /// token cannot be replayed. Binding the token to a specific file name means a token for
    /// one upload cannot be used to overwrite a different file.
    fn take_upload_key(
        &self,
        key: &str,
    ) -> impl Future<Output = Result<Option<String>, BoxError>> + Send;

    /// Read the bytes previously stored under `file_name`.
    ///
    /// Implementations must reject unsafe file names rather than touching storage.
    fn read_image_bytes(
        &self,
        file_name: &str,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, BoxError>> + Send;

    /// Store `data` under `file_name`.
    ///
    /// Implementations must reject unsafe file names rather than touching storage.
    fn write_image_bytes(
        &self,
        file_name: &str,
        data: &[u8],
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}
