import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { HttpTestingController } from '@angular/common/http/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';

import { CompositeFieldSchema } from './schema';

describe('CompositeFieldSchema', () => {
  let component: CompositeFieldSchema;
  let fixture: ComponentFixture<CompositeFieldSchema>;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    route = stubActivatedRoute({ id: 'seo' });
    await TestBed.configureTestingModule({
      imports: [CompositeFieldSchema],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
      ],
    })
    .compileComponents();

    fixture = TestBed.createComponent(CompositeFieldSchema);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('answers the guard about unsaved edits', async () => {
    // The load has to land first: what is on screen when it does is what "unsaved" is measured
    // against.
    const http = TestBed.inject(HttpTestingController);
    http.expectOne((request) => request.method === 'GET').flush([]);
    await fixture.whenStable();
    expect(component.hasUnsavedChanges()).toBe(false);

    component.schema.set([
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
    ]);

    expect(component.hasUnsavedChanges()).toBe(true);
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('asks for another definition when the parameter changes', () => {
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/composite_fields/seo').flush([]);

    route.navigate({ id: 'gallery' });

    expect(component.compositeId()).toBe('gallery');
    http.expectOne('/api/models/composite_fields/gallery').flush([]);
  });
});
