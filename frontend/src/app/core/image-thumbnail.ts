/**
 * The small copy of an image, made in the browser that has the file.
 *
 * The library and the pickers show tiles a couple of hundred pixels wide, and a photograph is a
 * megabyte: showing the original in every tile is what made a library of a few hundred pictures
 * download hundreds of megabytes (see `docs/content-api.md`). Making the small copy here rather
 * than on the server keeps every adapter free of image decoding - the bytes are opaque to them,
 * as the original's are - and costs the uploader a few milliseconds it already had the pixels for.
 */

/** Edge the small copy is fitted into, in pixels. */
export const THUMBNAIL_MAX_EDGE = 360;

/** What the small copy is encoded as. WebP is what every browser that can run this app can make. */
const THUMBNAIL_TYPE = 'image/webp';

/** Quality the small copy is encoded at: high enough that a tile looks like the picture. */
const THUMBNAIL_QUALITY = 0.8;

/**
 * Make the small copy of `file`, or `null` when this browser will not.
 *
 * A browser that cannot decode the file (an exotic format, a broken upload) answers `null` rather
 * than throwing: the picture can still be uploaded, and the library falls back to showing the
 * original - which is what an image uploaded through the API has anyway.
 */
export async function makeThumbnail(file: File): Promise<Blob | null> {
  if (typeof createImageBitmap !== 'function') {
    return null;
  }
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file);
  } catch {
    return null;
  }
  try {
    const scale = Math.min(1, THUMBNAIL_MAX_EDGE / Math.max(bitmap.width, bitmap.height));
    // Never zero: an image one pixel wide still has to produce a canvas with a pixel in it.
    const width = Math.max(1, Math.round(bitmap.width * scale));
    const height = Math.max(1, Math.round(bitmap.height * scale));
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) {
      return null;
    }
    context.drawImage(bitmap, 0, 0, width, height);
    return await new Promise<Blob | null>((resolve) => {
      canvas.toBlob((blob) => resolve(blob), THUMBNAIL_TYPE, THUMBNAIL_QUALITY);
    });
  } finally {
    bitmap.close();
  }
}

/** The extension the small copy is stored under, which is what says what its bytes are. */
export function thumbnailExtension(blob: Blob): string {
  const type = blob.type.split('/')[1] ?? '';
  // `image/webp` -> `webp`; anything unexpected falls back to the one type this makes.
  return /^[a-z0-9]{1,10}$/.test(type) ? type : 'webp';
}
