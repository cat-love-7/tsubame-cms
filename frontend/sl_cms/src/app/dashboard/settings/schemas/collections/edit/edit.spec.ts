import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { HttpTestingController } from '@angular/common/http/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';

import { Edit } from './edit';

describe('Edit', () => {
  let component: Edit;
  let fixture: ComponentFixture<Edit>;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    route = stubActivatedRoute({ name: 'blog' });
    await TestBed.configureTestingModule({
      imports: [Edit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
      ],
    })
    .compileComponents();

    fixture = TestBed.createComponent(Edit);
    component = fixture.componentInstance;
    await fixture.whenStable();
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
