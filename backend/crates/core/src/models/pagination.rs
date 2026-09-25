//! Offset pagination for list endpoints.
//!
//! Offset rather than cursor pagination: the delivery API is read by a site build that
//! walks the pages once, in order, and the CMS has no cheap cursor to hand out yet. A
//! DynamoDB adapter may replace this with `LastEvaluatedKey` later — that would change the
//! shape of the response, so it is a decision about the API, not about the storage.
//!
//! The window is handed to the storage either way: `Pagination::window` gives a backend the
//! `offset`/`limit` to read for itself, and `Pagination::wrap` builds the page from what came
//! back. `apply` (slicing a whole list in memory) is the fallback for a backend that cannot
//! read a window.

use serde::Deserialize;

use crate::models::error::HttpError;

/// Page size the public delivery API uses when the caller does not ask for one.
pub const DEFAULT_PAGE_LIMIT: usize = 50;

/// Largest accepted page size. A caller that wants everything walks the pages, so no
/// single request can pull an unbounded amount of content (or of work) with it.
pub const MAX_PAGE_LIMIT: usize = 200;

/// `?limit=&offset=` exactly as it arrives in a query string.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct PageQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// A validated window over a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    limit: Option<usize>,
    offset: usize,
}

/// One page of a longer list, plus what the caller needs to ask for the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Number of items in the whole list, before the window was applied.
    pub total: usize,
    /// Page size, or `None` when the list was returned whole.
    pub limit: Option<usize>,
    pub offset: usize,
    /// Offset that returns the next page; `None` once this is the last page.
    pub next_offset: Option<usize>,
}

impl Pagination {
    /// A window of at most `DEFAULT_PAGE_LIMIT` items unless the caller asks for another
    /// size. The public delivery API uses this.
    pub fn limited(query: PageQuery) -> Result<Self, HttpError> {
        Pagination::validate(query.limit, query.offset, Some(DEFAULT_PAGE_LIMIT))
    }

    /// A window only when the caller actually asked for one.
    ///
    /// The admin API uses this: its list is local, authenticated and currently rendered in
    /// full by the UI, so cutting it short by default would hide rows.
    pub fn optional(query: PageQuery) -> Result<Self, HttpError> {
        Pagination::validate(query.limit, query.offset, None)
    }

    fn validate(
        limit: Option<usize>,
        offset: Option<usize>,
        default_limit: Option<usize>,
    ) -> Result<Self, HttpError> {
        let limit = match limit {
            // Rejected rather than silently treated as "the default": a caller that sends
            // `limit=0` is asking for something that cannot be answered.
            Some(0) => return Err(HttpError::BadRequest("limit must be at least 1")),
            Some(limit) if limit > MAX_PAGE_LIMIT => {
                return Err(HttpError::BadRequest(&format!(
                    "limit must be at most {MAX_PAGE_LIMIT}"
                )));
            }
            Some(limit) => Some(limit),
            None => default_limit,
        };
        Ok(Pagination {
            limit,
            offset: offset.unwrap_or(0),
        })
    }

    /// The window itself, for a backend that can read one instead of the whole list.
    ///
    /// `(offset, limit)`: the first `offset` items are to be skipped and at most `limit`
    /// returned, `None` meaning "to the end".
    pub fn window(&self) -> (usize, Option<usize>) {
        (self.offset, self.limit)
    }

    /// Build a page from items a backend already cut to this window.
    ///
    /// [`Pagination::apply`] slices a whole list; this is the other half of the same contract,
    /// for a storage layer that returned exactly the window and reported how long the list is.
    /// Keeping `total` is what lets `X-Total-Count` and `next_offset` stay truthful without
    /// reading the list to the end.
    pub fn wrap<T>(&self, items: Vec<T>, total: usize) -> Page<T> {
        let consumed = self.offset + items.len();
        Page {
            next_offset: match self.limit {
                Some(_) if consumed < total => Some(consumed),
                _ => None,
            },
            items,
            total,
            limit: self.limit,
            offset: self.offset,
        }
    }

    /// Apply the window to `items`.
    ///
    /// An offset past the end is not an error: it returns an empty page, which is what a
    /// client walking pages until `next_offset` is `None` will eventually send.
    pub fn apply<T>(&self, items: Vec<T>) -> Page<T> {
        let total = items.len();
        let items: Vec<T> = match self.limit {
            Some(limit) => items.into_iter().skip(self.offset).take(limit).collect(),
            None => items.into_iter().skip(self.offset).collect(),
        };
        let consumed = self.offset + items.len();

        Page {
            next_offset: match self.limit {
                Some(_) if consumed < total => Some(consumed),
                _ => None,
            },
            items,
            total,
            limit: self.limit,
            offset: self.offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(limit: Option<usize>, offset: Option<usize>) -> PageQuery {
        PageQuery { limit, offset }
    }

    #[test]
    fn a_large_list_is_cut_into_pages() {
        let pagination = Pagination::limited(query(Some(2), None)).unwrap();
        let page = pagination.apply(vec![1, 2, 3, 4, 5]);

        assert_eq!(page.items, vec![1, 2]);
        assert_eq!(page.total, 5);
        assert_eq!(page.limit, Some(2));
        assert_eq!(page.offset, 0);
        assert_eq!(page.next_offset, Some(2));
    }

    #[test]
    fn the_last_page_has_no_next_offset() {
        let pagination = Pagination::limited(query(Some(2), Some(4))).unwrap();
        let page = pagination.apply(vec![1, 2, 3, 4, 5]);

        assert_eq!(page.items, vec![5]);
        assert_eq!(page.total, 5);
        assert_eq!(page.next_offset, None);
    }

    #[test]
    fn an_offset_past_the_end_is_an_empty_last_page() {
        let pagination = Pagination::limited(query(Some(2), Some(50))).unwrap();
        let page = pagination.apply(vec![1, 2, 3]);

        assert!(page.items.is_empty());
        assert_eq!(page.total, 3);
        assert_eq!(page.next_offset, None);
    }

    #[test]
    fn the_public_api_gets_a_default_page_size() {
        let page = Pagination::limited(query(None, None))
            .unwrap()
            .apply(vec![1, 2, 3]);

        assert_eq!(page.limit, Some(DEFAULT_PAGE_LIMIT));
        assert_eq!(page.offset, 0);
        assert_eq!(page.items, vec![1, 2, 3]);
    }

    #[test]
    fn the_admin_api_returns_everything_unless_a_page_is_asked_for() {
        let pagination = Pagination::optional(query(None, None)).unwrap();
        let page = pagination.apply(vec![1, 2, 3]);

        assert_eq!(page.items, vec![1, 2, 3]);
        assert_eq!(page.limit, None);
        assert_eq!(page.next_offset, None);

        // An offset without a limit still windows the list, and is then the last page.
        let offset_only = Pagination::optional(query(None, Some(1))).unwrap();
        let page = offset_only.apply(vec![1, 2, 3]);
        assert_eq!(page.items, vec![2, 3]);
        assert_eq!(page.next_offset, None);
    }

    #[test]
    fn a_page_size_outside_the_allowed_range_is_rejected() {
        assert_eq!(
            Pagination::limited(query(Some(0), None))
                .unwrap_err()
                .status_code,
            400
        );
        assert_eq!(
            Pagination::limited(query(Some(MAX_PAGE_LIMIT + 1), None))
                .unwrap_err()
                .status_code,
            400
        );
        // The largest allowed page is accepted.
        assert!(Pagination::limited(query(Some(MAX_PAGE_LIMIT), None)).is_ok());
        assert!(Pagination::optional(query(Some(0), None)).is_err());
    }
}
