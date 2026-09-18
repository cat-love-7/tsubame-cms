import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { HttpTestingController } from '@angular/common/http/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';

import { SinglePageSchema } from './schema';

describe('SinglePageSchema', () => {
  let component: SinglePageSchema;
  let fixture: ComponentFixture<SinglePageSchema>;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    route = stubActivatedRoute({ name: 'home' });
    await TestBed.configureTestingModule({
      imports: [SinglePageSchema],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
      ],
    })
    .compileComponents();

    fixture = TestBed.createComponent(SinglePageSchema);
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

  // Switching pages reuses this component: the schema of the previous page would otherwise stay
  // in the editor, and saving would write it to the page just opened.
  it('asks for another page\u2019s schema when the parameter changes', () => {
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/single_pages/home/schema').flush([]);

    route.navigate({ name: 'about' });

    expect(component.pageName()).toBe('about');
    http.expectOne('/api/models/single_pages/about/schema').flush([]);
  });
});
