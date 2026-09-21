import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { HttpTestingController } from '@angular/common/http/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';

import { CollectionSchemaEdit } from './schema';

describe('CollectionSchemaEdit', () => {
  let component: CollectionSchemaEdit;
  let fixture: ComponentFixture<CollectionSchemaEdit>;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    route = stubActivatedRoute({ name: 'blog' });
    await TestBed.configureTestingModule({
      imports: [CollectionSchemaEdit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(CollectionSchemaEdit);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  // A slow answer for the schema the reader left must not land on the one they are on now: what is
  // on screen is what `save` sends, so it would be written over the wrong collection.
  it('ignores a schema that arrives for the collection the reader left', async () => {
    const http = TestBed.inject(HttpTestingController);
    const left = http.expectOne('/api/models/collections/blog/schema');

    route.navigate({ name: 'pages' });
    const onScreen = http.expectOne('/api/models/collections/pages/schema');

    const field = (name: string) => ({
      name,
      field_type: 'Text',
      required: false,
      width: 12,
      height: 1,
    });
    onScreen.flush([field('pageOnly')]);
    await fixture.whenStable();
    expect(component.collectionSchema().map((f) => f.name)).toEqual(['pageOnly']);

    // The one asked for first answers last, and is not wanted any more.
    left.flush([field('blogOnly')]);
    await fixture.whenStable();
    expect(component.collectionSchema().map((f) => f.name)).toEqual(['pageOnly']);
  });

  // Leaving with a schema that has not been saved has to be asked about, the same as leaving an
  // item editor with unsaved values.
  it('answers the guard about unsaved edits', async () => {
    // The load has to land first: what is on screen when it does is what "unsaved" is measured
    // against.
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/collections/blog/schema').flush([]);
    http.expectOne('/api/models/collections/blog/settings').flush({ preview: false });
    await fixture.whenStable();
    expect(component.hasUnsavedChanges()).toBe(false);

    component.collectionSchema.set([
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
    ]);

    expect(component.hasUnsavedChanges()).toBe(true);
  });

  // The setting is saved beside the fields, from the one Save the screen has: an editor who turned
  // it on should not have to remember a second button.
  it('saves the preview setting with the fields', async () => {
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/collections/blog/schema').flush([]);
    http.expectOne('/api/models/collections/blog/settings').flush({ preview: true });
    await fixture.whenStable();
    expect(component.preview()).toBe(true);

    component.preview.set(false);
    component.save([]);
    const schema = http.expectOne(
      (request) => request.method === 'PUT' && request.url.endsWith('/blog/schema'),
    );
    const settings = http.expectOne(
      (request) => request.method === 'PUT' && request.url.endsWith('/blog/settings'),
    );
    expect(settings.request.body).toEqual({ preview: false });
    schema.flush(null);
    settings.flush(null);
    await fixture.whenStable();

    expect(component.hasUnsavedChanges()).toBe(false);
  });

  // Turning the setting on is an edit like any other: leaving without saving has to be asked about.
  it('counts the preview setting as an unsaved change', async () => {
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/collections/blog/schema').flush([]);
    http.expectOne('/api/models/collections/blog/settings').flush({ preview: false });
    await fixture.whenStable();
    expect(component.hasUnsavedChanges()).toBe(false);

    component.preview.set(true);

    expect(component.hasUnsavedChanges()).toBe(true);
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  // Switching collections keeps the same route and reuses the component, so the editor has to
  // follow the parameter or it would save one collection's schema over another's.
  it('asks for another collection\u2019s schema when the parameter changes', () => {
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/collections/blog/schema').flush([]);

    route.navigate({ name: 'pages' });

    expect(component.collectionName()).toBe('pages');
    http.expectOne('/api/models/collections/pages/schema').flush([]);
  });
});
