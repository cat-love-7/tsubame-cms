import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { TypedFixture } from 'app/core/testing/fixture';
import { App } from './app';

describe('App', () => {
  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [App],
      // Transloco comes from the test environment (`src/test-providers.ts`), like every spec.
      providers: [provideRouter([])],
    }).compileComponents();
  });

  it('should create the app', () => {
    const fixture: TypedFixture<App> = TestBed.createComponent(App);
    const app = fixture.componentInstance;
    expect(app).toBeTruthy();
  });

  // `app.html` is only `<router-outlet>`, so there is no <h1> to assert on (the previous
  // version of this test checked for one and could never pass).
  it('should render the router outlet', async () => {
    const fixture: TypedFixture<App> = TestBed.createComponent(App);
    await fixture.whenStable();
    const compiled = fixture.nativeElement;
    expect(compiled.querySelector('router-outlet')).toBeTruthy();
  });
});
