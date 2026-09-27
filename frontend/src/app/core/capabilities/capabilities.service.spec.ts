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
    // Guessing a preview site would hand out a link that goes nowhere, so silence means none.
    expect(service.previewSiteUrl()).toBeNull();
    // A deployment that has not named itself is shown by the product's name, which is what every
    // deployment looked like before a name could be set: nothing to invent here either.
    expect(service.siteName()).toBeNull();
  });

  it('takes the deployment at its word about what it administers', () => {
    service.load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset: 'link',
      image_upload: 'proxied',
      site_name: '公式サイト管理画面(dev)',
    });

    // The operator's own wording, passed through exactly as it arrived.
    expect(service.siteName()).toBe('公式サイト管理画面(dev)');
  });

  it('takes the preview site from the deployment', () => {
    service.load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset: 'link',
      image_upload: 'proxied',
      preview_site_url: 'https://preview.example.com',
    });

    expect(service.previewSiteUrl()).toBe('https://preview.example.com');
  });

  it('takes the deployment at its word', () => {
    service.load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: false,
      password_reset: 'temporary',
      image_upload: 'presigned',
    });

    expect(service.passwordLogin()).toBe(false);
    // A provider holds the password, so a reset hands over a temporary one.
    expect(service.passwordReset()).toBe('temporary');
    expect(service.imageUpload()).toBe('presigned');
  });

  it('asks once, however many screens need the answer', () => {
    service.load();
    service.load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset: 'link',
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
