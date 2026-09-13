import { HttpClient, provideHttpClient, withInterceptors } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { AuthService } from './auth.service';
import { authInterceptor } from './auth.interceptor';

/**
 * Where the token goes, and — more to the point — where it does not.
 *
 * Image bytes are uploaded straight to object storage on AWS with a presigned URL, and an
 * `Authorization` header there can only break the signature the URL carries. The token belongs
 * to this API and nowhere else.
 */
describe('authInterceptor', () => {
  let http: HttpClient;
  let httpMock: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideRouter([]),
        provideHttpClient(withInterceptors([authInterceptor])),
        provideHttpClientTesting(),
      ],
    });
    http = TestBed.inject(HttpClient);
    httpMock = TestBed.inject(HttpTestingController);
    // The token is read-only from outside, so the test signs in the way the application does.
    TestBed.inject(AuthService).replaceToken('a-token');
  });

  afterEach(() => httpMock.verify());

  it('signs requests to this API', () => {
    http.get('/api/models/collections').subscribe();

    const request = httpMock.expectOne('/api/models/collections');
    expect(request.request.headers.get('Authorization')).toBe('Bearer a-token');
    request.flush([]);
  });

  it('leaves a request to another host unsigned', () => {
    const presigned =
      'https://cms-images.s3.eu-west-1.amazonaws.com/9f3c.png?X-Amz-Signature=deadbeef';
    http.put(presigned, new Blob(['x'])).subscribe();

    const request = httpMock.expectOne(presigned);
    expect(request.request.headers.has('Authorization')).toBe(false);
    request.flush(null);
  });
});
