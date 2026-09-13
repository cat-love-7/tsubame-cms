import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';

import { CapabilitiesService } from './capabilities.service';

describe('CapabilitiesService', () => {
  let service: CapabilitiesService;
  let httpMock: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    service = TestBed.inject(CapabilitiesService);
    httpMock = TestBed.inject(HttpTestingController);
  });

  afterEach(() => httpMock.verify());

  it('assumes the CMS checks passwords until the deployment says otherwise', () => {
    expect(service.passwordLogin()).toBe(true);
    expect(service.imageUpload()).toBe('proxied');
  });

  it('takes the deployment at its word', () => {
    service.load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: false,
      password_reset_links: false,
      image_upload: 'presigned',
    });

    expect(service.passwordLogin()).toBe(false);
    expect(service.passwordResetLinks()).toBe(false);
    expect(service.imageUpload()).toBe('presigned');
  });

  it('asks once, however many screens need the answer', () => {
    service.load();
    service.load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset_links: true,
      image_upload: 'proxied',
    });
    httpMock.expectNone('/api/auth/capabilities');
  });

  it('falls back to what it assumed when the deployment does not answer', () => {
    service.load();
    httpMock.expectOne('/api/auth/capabilities').error(new ProgressEvent('error'));

    expect(service.passwordLogin()).toBe(true);
  });
});
