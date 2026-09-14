import { HttpClient } from '@angular/common/http';
import { Injectable, computed, inject, signal } from '@angular/core';

import { apiUrl } from '../api-url';

/**
 * What the deployment the CMS is talking to can do.
 *
 * The same CMS runs where it verifies passwords itself and where an identity provider does, and
 * a client has to know which: the password screens make no sense in the second case, and the
 * image upload does not go through the API there either. `GET /auth/capabilities` is the
 * answer; asking is cheaper than finding out from a 501.
 */
export interface Capabilities {
  /** Whether the CMS verifies passwords, so whether `/auth/login` and friends exist at all. */
  password_login: boolean;
  /** Whether an administrator can mint a password-reset link to hand to someone. */
  password_reset_links: boolean;
  /** Who accepts the bytes of an uploaded image. */
  image_upload: 'proxied' | 'presigned';
  /** Where to sign in, when that is not here. Absent when the deployment does not know. */
  login_url?: string | null;
}

/**
 * What to assume before the deployment has answered.
 *
 * The password kind, because that is what this CMS was before capabilities existed: an older
 * server that does not know the endpoint, or one whose answer never arrived, behaves the way
 * the UI then shows.
 */
const ASSUMED: Capabilities = {
  password_login: true,
  password_reset_links: true,
  image_upload: 'proxied',
  login_url: null,
};

@Injectable({ providedIn: 'root' })
export class CapabilitiesService {
  private http = inject(HttpClient);
  private readonly known = signal<Capabilities>(ASSUMED);
  private requested: boolean = false;

  /** The deployment's answer, or what is assumed until it arrives. */
  readonly capabilities = this.known.asReadonly();

  /** Whether the CMS checks passwords itself. */
  readonly passwordLogin = computed(() => this.known().password_login);

  /** Whether an administrator can issue a reset link. */
  readonly passwordResetLinks = computed(() => this.known().password_reset_links);

  /** Whether image bytes go through this API or straight to object storage. */
  readonly imageUpload = computed(() => this.known().image_upload);

  /** Where to send someone to sign in, when the deployment named a page. */
  readonly loginUrl = computed(() => this.known().login_url ?? null);

  /**
   * Ask the deployment, once.
   *
   * Failure is not an error the user has to see: the CMS then behaves as it did before this
   * endpoint existed.
   */
  load(): void {
    if (this.requested) {
      return;
    }
    this.requested = true;
    this.http.get<Capabilities>(apiUrl('/auth/capabilities')).subscribe({
      next: (capabilities) => this.known.set(capabilities),
      error: () => this.known.set(ASSUMED),
    });
  }
}
