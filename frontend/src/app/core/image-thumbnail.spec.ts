import { describe, expect, it, vi } from 'vitest';

import { THUMBNAIL_MAX_EDGE, makeThumbnail, thumbnailExtension } from './image-thumbnail';

/** What a canvas is asked for, recorded so a check can see the size and the encoding. */
interface Recorded {
  width: number;
  height: number;
  drawn: unknown[];
  type: string | undefined;
  quality: number | undefined;
}

/**
 * The browser pieces jsdom does not have: a decoder and a canvas.
 *
 * `makeThumbnail` runs on exactly these two, so a fake of each is what makes the arithmetic and the
 * encoding checkable at all - the alternative is a browser, which is what the end-to-end suite is
 * for.
 */
function stubBrowser(
  bitmap: { width: number; height: number } = { width: 800, height: 400 },
  blob: Blob | null = new Blob(['small'], { type: 'image/webp' }),
): Recorded {
  const recorded: Recorded = {
    width: 0,
    height: 0,
    drawn: [],
    type: undefined,
    quality: undefined,
  };
  const context = {
    drawImage: (...args: unknown[]) => recorded.drawn.push(args),
  };
  const original = document.createElement.bind(document);
  vi.spyOn(document, 'createElement').mockImplementation((tag: string) => {
    if (tag !== 'canvas') {
      return original(tag);
    }
    return {
      get width() {
        return recorded.width;
      },
      set width(value: number) {
        recorded.width = value;
      },
      get height() {
        return recorded.height;
      },
      set height(value: number) {
        recorded.height = value;
      },
      getContext: () => context,
      toBlob: (callback: (value: Blob | null) => void, type?: string, quality?: number) => {
        recorded.type = type;
        recorded.quality = quality;
        callback(blob);
      },
    } as unknown as HTMLElement;
  });
  vi.stubGlobal(
    'createImageBitmap',
    vi.fn(() => Promise.resolve({ ...bitmap, close: () => undefined })),
  );
  return recorded;
}

describe('makeThumbnail', () => {
  it('fits the copy inside the edge it is allowed', async () => {
    const recorded = stubBrowser({ width: 1600, height: 900 });

    const thumbnail = await makeThumbnail(new File(['big'], 'photo.png', { type: 'image/png' }));

    // 1600x900 scaled to a 360 edge: 360x203, never larger than the original.
    expect(recorded.width).toBe(THUMBNAIL_MAX_EDGE);
    expect(recorded.height).toBe(203);
    expect(recorded.drawn).toEqual([[expect.anything(), 0, 0, 360, 203]]);
    expect(thumbnail?.type).toBe('image/webp');
  });

  it('never grows a picture that is already small', async () => {
    const recorded = stubBrowser({ width: 100, height: 50 });

    await makeThumbnail(new File(['small'], 'icon.png', { type: 'image/png' }));

    expect([recorded.width, recorded.height]).toEqual([100, 50]);
  });

  it('keeps a single pixel of a picture one pixel wide', async () => {
    const recorded = stubBrowser({ width: 1, height: 4000 });

    await makeThumbnail(new File(['tall'], 'tall.png', { type: 'image/png' }));

    // 4000 scaled to 360 gives 0.09 pixels of width, which is not a canvas.
    expect(recorded.width).toBe(1);
    expect(recorded.height).toBe(360);
  });

  // A browser that cannot decode the file answers nothing rather than throwing: the picture still
  // uploads, and the library shows the original - which is what an API upload gets anyway.
  it('answers nothing when the browser cannot decode the file', async () => {
    vi.stubGlobal(
      'createImageBitmap',
      vi.fn(async () => Promise.reject(new Error('no decoder'))),
    );

    expect(await makeThumbnail(new File(['?'], 'odd.tiff'))).toBeNull();
  });

  it('answers nothing where there is no decoder at all', async () => {
    vi.stubGlobal('createImageBitmap', undefined);

    expect(await makeThumbnail(new File(['?'], 'photo.png'))).toBeNull();
  });

  it('names the copy after what it is', () => {
    expect(thumbnailExtension(new Blob(['x'], { type: 'image/webp' }))).toBe('webp');
    expect(thumbnailExtension(new Blob(['x'], { type: 'image/jpeg' }))).toBe('jpeg');
    // Anything unexpected falls back to what this code makes, rather than to no extension at all.
    expect(thumbnailExtension(new Blob(['x'], { type: '' }))).toBe('webp');
  });
});
