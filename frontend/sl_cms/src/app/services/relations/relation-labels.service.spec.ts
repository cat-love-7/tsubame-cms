import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { RelationLabelsService } from './relation-labels.service';

/**
 * The screens ask what their references are called; this is the seam that decides what to ask for -
 * one request per target collection and one for the pages, not one per row.
 */
describe('RelationLabelsService', () => {
  let labels: RelationLabelsService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    labels = TestBed.inject(RelationLabelsService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('asks once per collection, and reads the answers as rendered names', () => {
    let answer: Map<string, string> | undefined;
    labels
      .labelsFor([
        { target: 'categories', item: 3 },
        { target: 'categories', item: 4 },
        { target: 'categories', item: 3 },
        { target: 'authors', item: 7 },
      ])
      .subscribe((value) => (answer = value));

    const categories = http.expectOne(
      (request) => request.url === '/api/models/collections/categories/items/titles',
    );
    // Each id once, however often it was referenced.
    expect(categories.request.params.get('ids')).toBe('3,4');
    categories.flush({ 3: '技術', 4: 12 });

    const authors = http.expectOne(
      (request) => request.url === '/api/models/collections/authors/items/titles',
    );
    expect(authors.request.params.get('ids')).toBe('7');
    authors.flush({});

    expect([...(answer ?? new Map())]).toEqual([
      ['collection:categories:3', '技術'],
      // A number is rendered the way a table renders it, not as JSON.
      ['collection:categories:4', '12'],
    ]);
  });

  it('asks for the pages when a reference names one', () => {
    let answer: Map<string, string> | undefined;
    labels.labelsFor([{ target: 'home' }]).subscribe((value) => (answer = value));

    http
      .expectOne((request) => request.url === '/api/models/single_pages/titles')
      .flush({ home: 'ホーム' });

    expect(answer?.get('page:home')).toBe('ホーム');
  });

  // An array of composites holds one relation field per element, and every one of them asks about
  // the same target in the same moment: that is one request, not one per element.
  it('shares a request another part of the screen is already waiting on', () => {
    const first: Map<string, string>[] = [];
    const second: Map<string, string>[] = [];
    labels.labelsFor([{ target: 'categories', item: 3 }]).subscribe((value) => first.push(value));
    labels.labelsFor([{ target: 'categories', item: 3 }]).subscribe((value) => second.push(value));

    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items/titles')
      .flush({ 3: '技術' });

    expect(first).toHaveLength(1);
    expect(second).toHaveLength(1);
    expect(second[0].get('collection:categories:3')).toBe('技術');
  });

  // Shared only while it is on the way: a title may have been renamed since, so the next question
  // is asked again rather than answered from what the last one said.
  it('asks again once the answer has arrived', () => {
    labels.labelsFor([{ target: 'categories', item: 3 }]).subscribe();
    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items/titles')
      .flush({ 3: '技術' });

    let answer: Map<string, string> | undefined;
    labels.labelsFor([{ target: 'categories', item: 3 }]).subscribe((value) => (answer = value));
    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items/titles')
      .flush({ 3: 'もっと新しい' });

    expect(answer?.get('collection:categories:3')).toBe('もっと新しい');
  });

  it('asks nothing when there is nothing to name', () => {
    let answer: Map<string, string> | undefined;
    labels.labelsFor([]).subscribe((value) => (answer = value));

    expect(answer?.size).toBe(0);
  });
});
