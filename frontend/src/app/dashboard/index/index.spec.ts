import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { TypedFixture } from 'app/core/testing/fixture';

import { Index } from './index';

describe('Index', () => {
  let component: Index;
  let fixture: TypedFixture<Index>;
  let httpMock: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Index],
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();

    httpMock = TestBed.inject(HttpTestingController);
    fixture = TestBed.createComponent(Index);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  /** What the deployment reports about itself, once it answers. */
  async function answer(siteName?: string): Promise<void> {
    TestBed.inject(CapabilitiesService).load();
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset: 'link',
      image_upload: 'proxied',
      site_name: siteName ?? null,
    });
    await fixture.whenStable();
    fixture.detectChanges();
  }

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  // The first screen after signing in says what is being administered, then what administers it -
  // the same two lines as the app bar, because "which site is this?" is the question a reader with
  // two deployments open has, and the biggest text on the screen is a good place to answer it.
  it('heads the screen with the deployment, and the product under it', async () => {
    await answer('公式サイト管理画面(dev)');

    const welcome = fixture.nativeElement.querySelector('.welcome') as HTMLElement;
    expect(welcome.querySelector('h3')?.textContent?.trim()).toBe('公式サイト管理画面(dev)');
    expect(welcome.querySelector('.product')?.textContent?.trim()).toBe('Tsubame');
    // The instruction it has always given is still there, under both names.
    expect(welcome.querySelector('.landing')?.textContent?.trim()).toContain(
      'Choose a collection or a single page',
    );
  });

  // A deployment that never named itself keeps the heading it always had, and gets no empty
  // second line.
  it('heads the screen with the product when the deployment did not name itself', async () => {
    await answer();

    const welcome = fixture.nativeElement.querySelector('.welcome') as HTMLElement;
    expect(welcome.querySelector('h3')?.textContent?.trim()).toBe('Tsubame');
    expect(welcome.querySelector('.product')).toBeNull();
  });
});
