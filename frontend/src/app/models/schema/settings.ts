/**
 * What a schema is told about itself, as opposed to what its fields say.
 *
 * Kept apart from the fields on the server as well (`tsubame_core::models::schema::SchemaSettings`),
 * so a settings save never rewrites the definition the values are checked against.
 */
export interface SchemaSettings {
  /**
   * Whether a preview link may be minted for this schema's working copies.
   *
   * Off until an administrator turns it on: a preview link shows unpublished content to whoever
   * holds the URL, so a deployment that renders previews at all still decides schema by schema.
   */
  preview: boolean;
}
