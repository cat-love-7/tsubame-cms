import { ComponentFixture, TestBed } from '@angular/core/testing';

import { ItemStatusBadge } from './item-status';

describe('ItemStatusBadge', () => {
  let fixture: ComponentFixture<ItemStatusBadge>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({ imports: [ItemStatusBadge] }).compileComponents();
    fixture = TestBed.createComponent(ItemStatusBadge);
  });

  it('defaults to Draft, so an unset status never looks published', () => {
    fixture.detectChanges();

    const badge = fixture.nativeElement.querySelector('.badge') as HTMLElement;
    expect(badge.textContent?.trim()).toBe('Draft');
    expect(badge.classList).not.toContain('published');
  });

  it('marks a published item', () => {
    fixture.componentRef.setInput('status', 'published');
    fixture.detectChanges();

    const badge = fixture.nativeElement.querySelector('.badge') as HTMLElement;
    expect(badge.textContent?.trim()).toBe('Published');
    expect(badge.classList).toContain('published');
  });
});
