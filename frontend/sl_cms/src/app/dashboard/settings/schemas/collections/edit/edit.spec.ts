import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { HttpTestingController } from '@angular/common/http/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';

import { CollectionSchemaEdit } from './edit';

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
    })
    .compileComponents();

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
