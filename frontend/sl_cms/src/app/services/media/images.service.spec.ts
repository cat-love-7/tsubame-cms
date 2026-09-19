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

    expect(uploaded).toEqual([
      { id: 7, upload_url: '/images/one.png?key=k', url: '/images/one.png' },
    ]);
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
    expect(uploaded.length).toBe(1);
  });
});
