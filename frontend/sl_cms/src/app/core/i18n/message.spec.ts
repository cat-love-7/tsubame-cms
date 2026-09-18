import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { TranslocoService } from '@jsverse/transloco';

import { isStatusError } from '../http-error';
import { TypedFixture } from 'app/core/testing/fixture';
import { Message, MessagePipe, apiMessage, failure, t } from './message';

/** What `HttpClient` hands an error callback: a status, and the body the server sent. */
function response(status: number, body: unknown): unknown {
  return { status, error: body, message: `Http failure response for /api: ${status}` };
}

/** Somewhere for the pipe to live: it needs a view, like it has in a screen. */
@Component({
  imports: [MessagePipe],
  template: `{{ message() | message }}`,
})
class Host {
  public readonly message = signal<Message | null>(null);
}

describe('message helpers', () => {
  it('words a situational code itself, because the code is the whole reason', () => {
    const refused = response(409, { code: 'username_taken', message: 'that id is taken' });

    expect(failure('content.saveFailed', refused)).toEqual({ key: 'errors.username_taken' });
    expect(apiMessage(refused)).toEqual({ key: 'errors.username_taken' });
  });

  it('keeps the server message behind the screen own context for a mere status code', () => {
    // The reason `bad_request` exists as a separate kind: it is answered for a dozen different
    // mistakes, and the sentence after it is the only part that says which one happened.
    const invalid = response(400, { code: 'bad_request', message: 'a title is required' });

    expect(failure('content.saveFailed', invalid)).toEqual({
      key: 'content.saveFailed',
      params: { message: 'a title is required' },
    });
    expect(isStatusError(invalid)).toBe(true);
  });

  it('names the field a refusal is about', () => {
    // The body carries `field` for a refusal that is about one input, so the wording can point
    // at it instead of leaving the reader to find which field was meant.
    const refused = response(409, {
      code: 'value_taken',
      message: "field 'slug': the value 'intro' is already used by item 1",
      field: 'slug',
    });

    expect(failure('content.saveFailed', refused)).toEqual({
      key: 'errors.value_taken',
      params: { field: 'slug' },
    });
  });

  it('shows a code it does not know as the server wrote it', () => {
    // A code added since this client was built: the message is what makes that survivable.
    const future = response(400, { code: 'invented_later', message: 'something new went wrong' });

    expect(apiMessage(future)).toEqual({ text: 'something new went wrong' });
    expect(failure('content.saveFailed', future)).toEqual({
      key: 'content.saveFailed',
      params: { message: 'something new went wrong' },
    });
  });

  it('builds a key with parameters', () => {
    expect(t('accounts.roleChanged', { user: 'a@example.com' })).toEqual({
      key: 'accounts.roleChanged',
      params: { user: 'a@example.com' },
    });
  });
});

describe('MessagePipe', () => {
  let fixture: TypedFixture<Host>;

  /** The catalogs are bundled but still arrive through an observable, hence `whenStable`. */
  async function render(message: Message | null): Promise<string> {
    fixture.componentInstance.message.set(message);
    await fixture.whenStable();
    fixture.detectChanges();
    return fixture.nativeElement.textContent?.trim() ?? '';
  }

  beforeEach(() => {
    fixture = TestBed.createComponent(Host);
  });

  it('renders a key in the active language', async () => {
    const transloco = TestBed.inject(TranslocoService);

    transloco.setActiveLang('en');
    expect(await render({ key: 'common.save' })).toBe('Save');

    transloco.setActiveLang('ja');
    expect(await render({ key: 'common.save' })).toBe('保存');
  });

  it('fills in the parameters of a message', async () => {
    TestBed.inject(TranslocoService).setActiveLang('en');

    expect(
      await render({ key: 'content.imageMeta', params: { id: 7, uploaded: 'today' } }),
    ).toContain('id 7');
  });

  it('shows text as it came, and nothing at all for no message', async () => {
    TestBed.inject(TranslocoService).setActiveLang('ja');

    expect(await render({ text: 'plain server text' })).toBe('plain server text');
    expect(await render(null)).toBe('');
  });
});
