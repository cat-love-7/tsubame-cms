import { TestBed } from '@angular/core/testing';
import { TypedFixture } from 'app/core/testing/fixture';
import { NoticeToast } from './notice-toast';

describe('NoticeToast', () => {
  let fixture: TypedFixture<NoticeToast>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [NoticeToast],
    }).compileComponents();

    fixture = TestBed.createComponent(NoticeToast);
    fixture.detectChanges();
  });

  /** The message on screen, as the reader sees it. */
  function shown(): string {
    return (fixture.nativeElement.querySelector('.toast .text')?.textContent ?? '').trim();
  }

  it('says nothing until something happened', () => {
    expect(fixture.nativeElement.querySelector('.toast')).toBeNull();
  });

  it('shows what the screen confirmed', () => {
    fixture.componentRef.setInput('notice', { key: 'common.saved' });
    fixture.detectChanges();

    expect(shown()).toBe('Saved');
    // A confirmation is announced politely: it is not a failure to interrupt for.
    const toast = fixture.nativeElement.querySelector('.toast') as HTMLElement;
    expect(toast.getAttribute('role')).toBe('status');
    expect(toast.getAttribute('aria-hidden')).toBeNull();
  });

  // The point of the component: the message is readable from wherever the reader is looking, which
  // the screen's own copy of it is not on a long form.
  it('floats over the page', () => {
    fixture.componentRef.setInput('notice', { key: 'common.saved' });
    fixture.detectChanges();

    expect(getComputedStyle(fixture.nativeElement).position).toBe('fixed');
  });

  it('lets a confirmation fade on its own', () => {
    vi.useFakeTimers();
    try {
      fixture.componentRef.setInput('notice', { key: 'common.saved' });
      fixture.detectChanges();
      expect(shown()).toBe('Saved');

      vi.advanceTimersByTime(6000);
      fixture.detectChanges();
      expect(fixture.nativeElement.querySelector('.toast')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  // Setting the same wording again is a second act, and says so again rather than being swallowed
  // as "the message that is already up".
  it('starts the fade again when the same confirmation arrives', () => {
    vi.useFakeTimers();
    try {
      fixture.componentRef.setInput('notice', { key: 'common.saved' });
      fixture.detectChanges();
      vi.advanceTimersByTime(4000);
      fixture.componentRef.setInput('notice', { key: 'common.saved' });
      fixture.detectChanges();
      vi.advanceTimersByTime(4000);
      fixture.detectChanges();

      expect(shown()).toBe('Saved');
    } finally {
      vi.useRealTimers();
    }
  });

  // A failure stays until it is put away: the reader may be halfway down a form.
  it('keeps a failure up until it is dismissed', () => {
    vi.useFakeTimers();
    try {
      // A server sentence, which is shown as it came: the wording is not what this is about.
      fixture.componentRef.setInput('error', { text: 'the server said no' });
      fixture.detectChanges();
      expect(shown()).toBe('the server said no');

      vi.advanceTimersByTime(60_000);
      fixture.detectChanges();
      expect(shown()).toBe('the server said no');
    } finally {
      vi.useRealTimers();
    }
  });

  // The screen keeps its own error block, which is `role="alert"` and holds the retry button, so
  // this copy is a nudge rather than a second announcement.
  it('is hidden from assistive technology when it repeats a failure', () => {
    fixture.componentRef.setInput('error', { text: 'the server said no' });
    fixture.detectChanges();

    const toast = fixture.nativeElement.querySelector('.toast') as HTMLElement;
    expect(toast.getAttribute('aria-hidden')).toBe('true');
    expect(toast.getAttribute('role')).toBeNull();
  });

  it('goes when it is dismissed, and comes back for the next thing', () => {
    fixture.componentRef.setInput('notice', { key: 'common.saved' });
    fixture.detectChanges();

    const close = fixture.nativeElement.querySelector('.toast .close') as HTMLButtonElement;
    close.click();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('.toast')).toBeNull();

    fixture.componentRef.setInput('notice', { key: 'content.imageLinkCopied' });
    fixture.detectChanges();
    expect(shown()).not.toBe('');
  });

  // A failure outranks a confirmation: it is the one that needs acting on.
  it('shows the failure when there is also something that succeeded', () => {
    fixture.componentRef.setInput('notice', { key: 'common.saved' });
    fixture.componentRef.setInput('error', { text: 'the server said no' });
    fixture.detectChanges();

    expect(shown()).toBe('the server said no');
  });
});
