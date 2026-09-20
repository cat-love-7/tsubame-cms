/**
 * One piece of content that points at another, as the index reports it.
 *
 * The same shape both directions use: `kind` and `name` say which collection or page, and `item`
 * is the id inside it (absent for a single page, which has none).
 */
export interface RelationReference {
  kind: 'collection_item' | 'single_page';
  name: string;
  item?: number | null;
}
