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
  /**
   * What an administrator gets to hand over after a password reset, or null where this deployment
   * cannot reset one at all: a link the account's owner completes, or a temporary password the
   * identity provider already set, which has to be changed at the next sign-in.
   */
  password_reset: 'link' | 'temporary' | null;
  /** Who accepts the bytes of an uploaded image. */
  image_upload: 'proxied' | 'presigned';
  /** Where to sign in, when that is not here. Absent when the deployment does not know. */
  login_url?: string | null;
  /**
   * Where a shared preview link should be opened: the origin of the site that renders
   * unpublished content, when the deployment has one.
   *
   * The API's own preview answer is JSON, which is not something to hand a reviewer, so a
   * deployment that has no preview site says nothing here and the screen offers nothing.
   */
  preview_site_url?: string | null;
  /**
   * The largest image the deployment accepts, in bytes. Absent from a server that predates the
   * answer, and absent until the answer arrives: the client then lets the upload go and shows
   * what the server says, rather than refusing a file on a number it invented.
   */
  max_image_bytes?: number;
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
  password_reset: 'link',
  image_upload: 'proxied',
  login_url: null,
  // A server that does not answer cannot be assumed to have a preview site: guessing one would
  // hand out a link that goes nowhere.
  preview_site_url: null,
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

  /** What an administrator can hand over after a reset, or null where none is possible. */
  readonly passwordReset = computed(() => this.known().password_reset ?? null);

  /** Whether image bytes go through this API or straight to object storage. */
  readonly imageUpload = computed(() => this.known().image_upload);

  /** Where to send someone to sign in, when the deployment named a page. */
  readonly loginUrl = computed(() => this.known().login_url ?? null);

  /** The preview site's origin, or null where the deployment has none. */
  readonly previewSiteUrl = computed(() => this.known().preview_site_url ?? null);

  /** How large an image may be, or null while that is not known. */
  readonly maxImageBytes = computed(() => this.known().max_image_bytes ?? null);

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
