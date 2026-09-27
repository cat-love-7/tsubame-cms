import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';

import { TypedFixture } from 'app/core/testing/fixture';

import { Header } from './header';
import { CapabilitiesService } from '../../core/capabilities/capabilities.service';

describe('Header', () => {
  let component: Header;
  let fixture: TypedFixture<Header>;
  let httpMock: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Header],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();

    httpMock = TestBed.inject(HttpTestingController);
    fixture = TestBed.createComponent(Header);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  /**
   * What the deployment reports about itself, once it answers.
   *
   * The root component is what asks (`CapabilitiesService.load`), so here the spec asks for it the
   * way the shell does: the header only reads the answer.
   */
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

  // A reader who has two of these open needs to know which one they are looking at, and the
  // deployment's name is the only thing that says so: the product's name is the same on both.
  it('names the deployment, with the product under it', async () => {
    await answer('サンプル管理画面(dev)');

    const brand = fixture.nativeElement.querySelector('.brand') as HTMLElement;
    expect(brand.querySelector('.site-name')?.textContent?.trim()).toBe('サンプル管理画面(dev)');
    // The product does not disappear when the name is set: the two lines are the whole heading.
    expect(brand.querySelector('.product')?.textContent?.trim()).toBe('Tsubame');
  });

  // A deployment that never named itself keeps the heading it always had, one line and no empty
  // second one.
  it('shows the product alone when the deployment did not name itself', async () => {
    await answer();

    const brand = fixture.nativeElement.querySelector('.brand') as HTMLElement;
    expect(brand.textContent?.trim()).toBe('Tsubame');
    expect(brand.querySelector('.site-name')).toBeNull();
  });
});
