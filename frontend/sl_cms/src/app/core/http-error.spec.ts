import { HttpErrorResponse } from '@angular/common/http';

import { errorKey, errorMessage } from './http-error';

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
    expect(errorMessage(new HttpErrorResponse({ status: 502, statusText: 'Bad Gateway' }))).toContain(
      '502',
    );
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
});
