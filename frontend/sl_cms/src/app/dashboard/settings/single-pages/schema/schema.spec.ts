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
    }).compileComponents();

    fixture = TestBed.createComponent(SinglePageSchema);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('answers the guard about unsaved edits', async () => {
    // The load has to land first: what is on screen when it does is what "unsaved" is measured
    // against.
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/single_pages/home/schema').flush([]);
    http.expectOne('/api/models/single_pages/home/settings').flush({ preview: false });
    await fixture.whenStable();
    expect(component.hasUnsavedChanges()).toBe(false);

    component.schema.set([
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
    ]);

    expect(component.hasUnsavedChanges()).toBe(true);
  });

  // The same as the collection schema editor: one Save writes the fields and the setting.
  it('saves the preview setting with the fields', async () => {
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/api/models/single_pages/home/schema').flush([]);
    http.expectOne('/api/models/single_pages/home/settings').flush({ preview: false });
    await fixture.whenStable();
    expect(component.preview()).toBe(false);

    component.preview.set(true);
    component.save([]);
    const schema = http.expectOne(
      (request) => request.method === 'PUT' && request.url.endsWith('/home/schema'),
    );
    const settings = http.expectOne(
      (request) => request.method === 'PUT' && request.url.endsWith('/home/settings'),
    );
    expect(settings.request.body).toEqual({ preview: true });
    schema.flush(null);
    settings.flush(null);
    await fixture.whenStable();

    expect(component.hasUnsavedChanges()).toBe(false);
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
