import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { CollectionItemPage } from 'app/models/values/collection';

import { CollectionRepository } from './collections.repository';

/**
 * The component tests stub the service out, so this is where the HTTP seam itself is
 * checked: the window has to travel as `?limit=&offset=` and the total has to be read from
 * the `X-Total-Count` header (it is not part of the body).
 */
describe('CollectionRepository', () => {
  let repository: CollectionRepository;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    repository = TestBed.inject(CollectionRepository);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('asks for one window and reads the total from the header', () => {
    let page: CollectionItemPage | undefined;
    repository.listCollectionItemsPage('blog', 25, 50).subscribe((value) => (page = value));

    const request = http.expectOne(
      (candidate) => candidate.url === '/api/models/collections/blog/items',
    );
    expect(request.request.method).toBe('GET');
    expect(request.request.params.get('limit')).toBe('25');
    expect(request.request.params.get('offset')).toBe('50');

    request.flush([[7, { title: 'Hello' }]], { headers: { 'X-Total-Count': '123' } });

    expect(page).toEqual({ items: [[7, { title: 'Hello' }]], total: 123 });
  });

  /** A response without the header must not turn into `NaN` in the pager. */
  it('falls back to an empty page when the header is missing', () => {
    let page: CollectionItemPage | undefined;
    repository.listCollectionItemsPage('blog', 25, 0).subscribe((value) => (page = value));

    http.expectOne((candidate) => candidate.url.endsWith('/items')).flush(null);

    expect(page).toEqual({ items: [], total: 0 });
  });
});
