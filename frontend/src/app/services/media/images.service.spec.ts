import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { ImagesService } from './images.service';

describe('ImagesService', () => {
  let service: ImagesService;
  let capabilities: CapabilitiesService;
  let httpMock: HttpTestingController;

  /** The deployment's answer, as the root of the app asks for it (`App`'s constructor). */
  async function create(maxImageBytes?: number) {
    TestBed.resetTestingModule();
    await TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();
    httpMock = TestBed.inject(HttpTestingController);
    service = TestBed.inject(ImagesService);
    capabilities = TestBed.inject(CapabilitiesService);
    capabilities.load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset: 'link',
      image_upload: 'proxied',
      ...(maxImageBytes === undefined ? {} : { max_image_bytes: maxImageBytes }),
    });
  }

  function file(sizeInBytes: number, name = 'photograph.png'): File {
    return new File([new Uint8Array(sizeInBytes)], name, { type: 'image/png' });
  }

  afterEach(() => {
    // The small copy's stubs are per test: leaving a decoder behind would make the next one send a
    // copy it never asked for.
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  /**
   * Let the upload's own follow-up work finish.
   *
   * An upload is the bytes and then the small copy of them, which the browser makes
   * asynchronously (`image-thumbnail.ts`) even when it cannot make one at all - so the answer
   * arrives a microtask or two after the bytes are flushed.
   */
  function settle(): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, 0));
  }

  it('refuses a file over the deployment’s limit without asking where to put it', async () => {
    await create(1024 * 1024);
    const failures: unknown[] = [];

    service.uploadImage(file(2 * 1024 * 1024)).subscribe({ error: (e) => failures.push(e) });

    // Nothing was sent: the size was decided here, which is the whole point of the answer
    // carrying the limit. The message names both sizes, in the unit a reader thinks in.
    httpMock.expectNone('/api/models/images/get_upload_url');
    expect(failures).toEqual([{ key: 'content.imageTooLarge', params: { size: 2, max: 1 } }]);
  });

  it('uploads a file within the limit', async () => {
    await create(10 * 1024 * 1024);
    const uploaded: unknown[] = [];

    service.uploadImage(file(2048)).subscribe((info) => uploaded.push(info));

    // The size travels with the request: the server refuses an image over the limit with it, and
    // on a deployment that signs the upload it is part of the signature.
    const request = httpMock.expectOne('/api/models/images/get_upload_url');
    expect(request.request.body).toEqual({
      original_filename: 'photograph.png',
      ext: 'png',
      size: 2048,
    });
    request.flush({ id: 7, upload_url: '/images/one.png?key=k', url: '/images/one.png' });
    httpMock.expectOne('/api/images/one.png?key=k').flush(null);
    await settle();

    expect(uploaded).toEqual([
      { id: 7, upload_url: '/images/one.png?key=k', url: '/images/one.png' },
    ]);
  });

  /**
   * The browser pieces the small copy needs: a decoder and a canvas, neither of which jsdom has.
   *
   * Only the service's *use* of them is under test here - what they are asked for is
   * `image-thumbnail.spec.ts` - so this answers a fixed blob of the type a browser would make.
   */
  function stubThumbnail(): void {
    const original = document.createElement.bind(document);
    vi.spyOn(document, 'createElement').mockImplementation((tag: string) => {
      if (tag !== 'canvas') {
        return original(tag);
      }
      return {
        width: 0,
        height: 0,
        getContext: () => ({ drawImage: () => undefined }),
        toBlob: (callback: (value: Blob | null) => void) =>
          callback(new Blob(['small'], { type: 'image/webp' })),
      } as unknown as HTMLElement;
    });
    vi.stubGlobal(
      'createImageBitmap',
      vi.fn(() => Promise.resolve({ width: 800, height: 600, close: () => undefined })),
    );
  }

  // The picture is uploaded, and then the small copy of it - made here, so no adapter has to
  // decode an image.
  it('sends the small copy after the bytes it was made from', async () => {
    await create(1024 * 1024);
    stubThumbnail();
    const uploaded: unknown[] = [];

    service.uploadImage(file(2048)).subscribe((info) => uploaded.push(info));
    httpMock
      .expectOne('/api/models/images/get_upload_url')
      .flush({ id: 7, upload_url: '/images/one.png?key=k', url: '/images/one.png' });
    httpMock.expectOne('/api/images/one.png?key=k').flush(null);
    await settle();

    const thumbnail = httpMock.expectOne('/api/models/images/7/thumbnail?ext=webp');
    expect(thumbnail.request.method).toBe('PUT');
    expect(thumbnail.request.body).toBeInstanceOf(Blob);
    thumbnail.flush(null);
    await settle();

    expect(uploaded).toEqual([
      { id: 7, upload_url: '/images/one.png?key=k', url: '/images/one.png' },
    ]);
  });

  // A copy that cannot be stored is not an upload that failed: the picture is there and usable, and
  // only the tiles are heavier without it.
  it('keeps the upload when the small copy cannot be stored', async () => {
    await create(1024 * 1024);
    stubThumbnail();
    const uploaded: unknown[] = [];
    const failures: unknown[] = [];

    service.uploadImage(file(2048)).subscribe({
      next: (info) => uploaded.push(info),
      error: (e) => failures.push(e),
    });
    httpMock
      .expectOne('/api/models/images/get_upload_url')
      .flush({ id: 9, upload_url: '/images/two.png?key=k', url: '/images/two.png' });
    httpMock.expectOne('/api/images/two.png?key=k').flush(null);
    await settle();
    httpMock
      .expectOne('/api/models/images/9/thumbnail?ext=webp')
      .flush({ code: 'internal_error' }, { status: 500, statusText: 'Server Error' });
    await settle();

    expect(uploaded).toEqual([
      { id: 9, upload_url: '/images/two.png?key=k', url: '/images/two.png' },
    ]);
    expect(failures).toEqual([]);
  });

  it('lets the server decide when the deployment never said what its limit is', async () => {
    await create();
    const uploaded: unknown[] = [];

    service.uploadImage(file(3 * 1024 * 1024)).subscribe((info) => uploaded.push(info));

    // An older server, or an answer that has not arrived: sending it is how the CMS finds out,
    // and inventing a limit here would refuse files the deployment would have taken.
    httpMock
      .expectOne('/api/models/images/get_upload_url')
      .flush({ id: 1, upload_url: '/images/big.png?key=k', url: '/images/big.png' });
    httpMock.expectOne('/api/images/big.png?key=k').flush(null);
    await settle();
    expect(uploaded.length).toBe(1);
  });
});
