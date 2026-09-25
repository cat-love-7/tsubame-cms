import { HttpErrorResponse } from '@angular/common/http';

import { errorCode, errorKey, errorMessage, isStatusError } from './http-error';

/** What `HttpClient` hands an error callback for a body it parsed as JSON. */
function response(status: number, body: unknown): HttpErrorResponse {
  return new HttpErrorResponse({ status, error: body, statusText: 'Bad Request' });
}

describe('errorMessage', () => {
  it('reads the message out of the JSON body', () => {
    expect(
      errorMessage(response(400, { code: 'bad_request', message: 'a username is required' })),
    ).toBe('a username is required');
  });

  it('still reads a plain-text body, from a deployment that has not been updated', () => {
    expect(errorMessage(response(401, 'invalid username or password'))).toBe(
      'invalid username or password',
    );
  });

  it('falls back to the status line when there is no body at all', () => {
    expect(
      errorMessage(new HttpErrorResponse({ status: 502, statusText: 'Bad Gateway' })),
    ).toContain('502');
  });
});

describe('errorKey', () => {
  it('names the code so a screen can show its own wording', () => {
    expect(errorKey(response(404, { code: 'not_found', message: 'no such item' }))).toBe(
      'errors.not_found',
    );
  });

  it('says nothing for an unknown code, so the English message is shown instead', () => {
    expect(errorKey(response(400, { code: 'invented_later', message: 'x' }))).toBeNull();
    expect(errorKey(response(400, 'plain text'))).toBeNull();
  });

  it('words a code that only stands for a status differently from one that names the reason', () => {
    // A screen decides with this whether its own wording can replace the message: for
    // `bad_request` the message *is* the reason, for `username_taken` the code is.
    expect(isStatusError(response(400, { code: 'bad_request', message: 'x' }))).toBe(true);
    expect(isStatusError(response(409, { code: 'username_taken', message: 'x' }))).toBe(false);
    expect(isStatusError(response(400, 'plain text'))).toBe(false);
  });

  it('reads the code out of the body, and nothing out of a body without one', () => {
    expect(errorCode(response(404, { code: 'not_found', message: 'x' }))).toBe('not_found');
    expect(errorCode(response(404, { message: 'x' }))).toBeNull();
    expect(errorCode(response(404, 'not found'))).toBeNull();
  });
});
