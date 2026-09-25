import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { RelationRef } from 'app/models/values/fields';
import { TypedFixture } from 'app/core/testing/fixture';
import { RelationPicker } from './relation-picker';

/**
 * The chooser: what it offers for a collection target, and what it offers for a page.
 *
 * The forms themselves are stubbed out at the service seam by the caller, so this drives the real
 * HTTP services: what matters here is which requests a picker makes and what it makes of them.
 */
describe('RelationPicker', () => {
  let fixture: TypedFixture<RelationPicker>;
  let http: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [RelationPicker],
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();
    http = TestBed.inject(HttpTestingController);
    fixture = TestBed.createComponent(RelationPicker);
  });

  afterEach(() => http.verify());

  async function open(target: { kind: 'collection' | 'single_page'; name: string }): Promise<void> {
    fixture.componentRef.setInput('target', target);
    await fixture.whenStable();
    fixture.detectChanges();
  }

  function labels(): string[] {
    return Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('.candidate .label'),
      (label: HTMLElement) => label.textContent?.trim() ?? '',
    );
  }

  it('offers the items of a collection, named by their title', async () => {
    await open({ kind: 'collection', name: 'categories' });
    const items = http.expectOne(
      (request) => request.url === '/api/models/collections/categories/items',
    );
    expect(items.request.params.get('limit')).toBe('100');
    items.flush([
      [3, { name: '技術' }],
      [4, { name: 'ニュース' }],
    ]);
    // The items say which ones they are; the titles say what to call them.
    http
      .expectOne(
        (request) =>
          request.url === '/api/models/collections/categories/items/titles' &&
          request.params.get('ids') === '3,4',
      )
      .flush({ 3: '技術' });
    await fixture.whenStable();
    fixture.detectChanges();

    // An item the target cannot name is still offerable, as the reference itself.
    expect(labels()).toEqual(['技術', 'categories #4']);
  });

  it('offers the pages, named by their title', async () => {
    await open({ kind: 'single_page', name: 'home' });
    http
      .expectOne((request) => request.url === '/api/models/single_pages')
      .flush(['home', 'about']);
    http
      .expectOne((request) => request.url === '/api/models/single_pages/titles')
      .flush({ home: 'ホーム' });
    await fixture.whenStable();
    fixture.detectChanges();

    expect(labels()).toEqual(['ホーム', 'about']);
  });

  it('filters what it lists, and says which are already referenced', async () => {
    await open({ kind: 'collection', name: 'categories' });
    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items')
      .flush([
        [3, {}],
        [4, {}],
      ]);
    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items/titles')
      .flush({ 3: '技術', 4: 'ニュース' });
    await fixture.whenStable();

    fixture.componentRef.setInput('selected', [{ target: 'categories', item: 4 }]);
    await fixture.whenStable();
    fixture.detectChanges();
    const picked = fixture.nativeElement.querySelectorAll('.candidate.picked');
    expect(picked.length).toBe(1);
    expect(picked[0].textContent).toContain('ニュース');

    fixture.componentInstance.filter.set('技');
    fixture.detectChanges();
    expect(labels()).toEqual(['技術']);
  });

  it('answers a click with the reference it chose', async () => {
    const picked: RelationRef[] = [];
    fixture.componentInstance.toggled.subscribe((reference) => picked.push(reference));

    await open({ kind: 'collection', name: 'categories' });
    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items')
      .flush([[3, {}]]);
    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items/titles')
      .flush({});
    await fixture.whenStable();
    fixture.detectChanges();

    (fixture.nativeElement.querySelector('.candidate') as HTMLButtonElement).click();

    expect(picked).toEqual([{ target: 'categories', item: 3 }]);
  });

  it('closes when asked', async () => {
    let closed = 0;
    fixture.componentInstance.closed.subscribe(() => (closed += 1));

    await open({ kind: 'single_page', name: 'home' });
    http.expectOne((request) => request.url === '/api/models/single_pages').flush([]);
    http.expectOne((request) => request.url === '/api/models/single_pages/titles').flush({});
    await fixture.whenStable();
    fixture.detectChanges();

    (fixture.nativeElement.querySelector('.picker-actions button') as HTMLButtonElement).click();

    expect(closed).toBe(1);
  });

  it('asks nothing for a relation whose target is not chosen yet', async () => {
    await open({ kind: 'collection', name: '  ' });
    fixture.detectChanges();

    expect(fixture.componentInstance.loading()).toBe(false);
    expect(labels()).toEqual([]);
  });
});
