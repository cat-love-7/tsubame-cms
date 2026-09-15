import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { HttpTestingController } from '@angular/common/http/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';

import { Schema } from './schema';

describe('Schema', () => {
  let component: Schema;
  let fixture: ComponentFixture<Schema>;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    route = stubActivatedRoute({ name: 'home' });
    await TestBed.configureTestingModule({
      imports: [Schema],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
      ],
    })
    .compileComponents();

    fixture = TestBed.createComponent(Schema);
    component = fixture.componentInstance;
    await fixture.whenStable();
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
