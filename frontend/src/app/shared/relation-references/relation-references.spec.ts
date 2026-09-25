import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { TypedFixture } from 'app/core/testing/fixture';
import { RelationReferences } from './relation-references';

/** One item's referrers, and the schema that names the relation they hold. */
function schemaWith(inverse_name: string | null) {
  return [
    {
      name: 'category',
      field_type: {
        Relation: {
          target: { kind: 'collection', name: 'categories' },
          has_many: false,
          ...(inverse_name === null ? {} : { inverse_name }),
        },
      },
      required: false,
      width: 12,
      height: 1,
    },
  ];
}

describe('RelationReferences', () => {
  let fixture: TypedFixture<RelationReferences>;
  let http: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [RelationReferences],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  function create(kind: 'collection_item' | 'single_page', name: string, item?: number) {
    fixture = TestBed.createComponent(RelationReferences);
    fixture.componentRef.setInput('kind', kind);
    fixture.componentRef.setInput('name', name);
    if (item !== undefined) {
      fixture.componentRef.setInput('item', item);
    }
    fixture.detectChanges();
    // The panel asks when it is opened, not when the screen is drawn.
    (fixture.nativeElement.querySelector('.references-toggle') as HTMLButtonElement).click();
    fixture.detectChanges();
    return fixture.componentInstance;
  }

  it('names a group by what the other schema calls the relation, and links each referrer', async () => {
    create('collection_item', 'categories', 1);

    http
      .expectOne('/api/models/collections/categories/items/1/references')
      .flush([{ kind: 'collection_item', name: 'articles', item: 7 }]);
    // The referrer's schema is what says the relation is called `articles` from this side.
    http.expectOne('/api/models/collections/articles/schema').flush(schemaWith('articles'));
    http
      .expectOne((request) => request.url === '/api/models/collections/articles/items/titles')
      .flush({ 7: 'Hello' });
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('articles');
    const link = fixture.nativeElement.querySelector('a') as HTMLAnchorElement;
    expect(link.textContent?.trim()).toBe('Hello');
    expect(link.getAttribute('href')).toBe('/collections/articles/edit/7');
  });

  it('falls back to the collection name when the other schema names nothing', async () => {
    create('collection_item', 'categories', 1);

    http
      .expectOne('/api/models/collections/categories/items/1/references')
      .flush([{ kind: 'collection_item', name: 'articles', item: 7 }]);
    http.expectOne('/api/models/collections/articles/schema').flush(schemaWith(null));
    http
      .expectOne((request) => request.url === '/api/models/collections/articles/items/titles')
      .flush({ 7: 'Hello' });
    await fixture.whenStable();
    fixture.detectChanges();

    const heading = fixture.nativeElement.querySelector('.reference-group h4') as HTMLElement;
    expect(heading.textContent?.trim()).toBe('articles');
  });

  it('names a page referrer by the page, and its heading by the page schema', async () => {
    create('collection_item', 'categories', 1);

    http
      .expectOne('/api/models/collections/categories/items/1/references')
      .flush([{ kind: 'single_page', name: 'home' }]);
    http.expectOne('/api/models/single_pages/home/schema').flush([
      {
        name: 'category',
        field_type: {
          Relation: {
            target: { kind: 'collection', name: 'categories' },
            has_many: false,
            inverse_name: 'pages',
          },
        },
        required: false,
        width: 12,
        height: 1,
      },
    ]);
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('pages');
    const link = fixture.nativeElement.querySelector('a') as HTMLAnchorElement;
    expect(link.textContent?.trim()).toBe('home');
    expect(link.getAttribute('href')).toBe('/single-pages/home');
  });

  it('says so when nothing points here', async () => {
    create('single_page', 'home');
    http.expectOne('/api/models/single_pages/home/references').flush([]);
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('Nothing references this yet');
  });

  it('says the list could not be read rather than looking empty', async () => {
    create('single_page', 'home');
    http
      .expectOne('/api/models/single_pages/home/references')
      .flush({ code: 'internal_error' }, { status: 500, statusText: 'Server Error' });
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('The references could not be loaded');
  });
});
