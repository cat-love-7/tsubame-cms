import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { MatDialog } from '@angular/material/dialog';
import { of, throwError } from 'rxjs';

import { apiUrl } from 'app/core/api-url';
import { Message } from 'app/core/i18n/message';
import { TypedFixture } from 'app/core/testing/fixture';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';
import { ImageField } from './image-field';

const LIBRARY: ImageEntry[] = [
  {
    id: 3,
    url: '/images/logo.png',
    original_filename: 'logo.png',
    uploaded_at: '2024-01-01T00:00:00Z',
  },
  {
    id: 4,
    url: '/images/photo.png',
    original_filename: 'photo.png',
    uploaded_at: '2024-01-02T00:00:00Z',
  },
];

/** Counts library reads, so the picker can be checked for loading lazily. */
class StubImagesService {
  public listCalls = 0;
  /** Names of the files handed to `uploadImage`, in order. */
  public uploaded: string[] = [];
  /** What `uploadImage` answers with, when a spec wants a failure instead. */
  public failUpload = false;
  listImages = () => {
    this.listCalls += 1;
    // The page shape the server answers with: the images, and how many there are altogether.
    return of({ images: LIBRARY, total: LIBRARY.length });
  };
  uploadImage = (file: File) => {
    if (this.failUpload) {
      return throwError(() => new Error('too large'));
    }
    const index = this.uploaded.push(file.name);
    return of({
      id: 100 + index,
      upload_url: `/images/${file.name}?key=k`,
      url: `/images/${file.name}`,
    });
  };
  deleteImage = () => of(void 0);
}

/** Close every dialog: they are attached to the document, not to the fixture. */
function closeDialogs() {
  TestBed.inject(MatDialog).closeAll();
}

/** A change event for a file input that was given one file. */
function fileChosen(name: string): Event {
  const input = { files: [new File(['x'], name, { type: 'image/png' })], value: '' };
  return { target: input } as unknown as Event;
}

describe('ImageField', () => {
  let fixture: TypedFixture<ImageField>;
  let images: StubImagesService;

  beforeEach(async () => {
    images = new StubImagesService();
    await TestBed.configureTestingModule({
      imports: [ImageField],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: ImagesService, useValue: images },
      ],
    }).compileComponents();
  });

  /** Uses `setInput` so the inputs are in place before the first change detection. */
  function create(value: unknown = null): ImageField {
    fixture = TestBed.createComponent(ImageField);
    fixture.componentRef.setInput('value', value);
    fixture.detectChanges();
    return fixture.componentInstance;
  }

  function query(selector: string): Element | null {
    return fixture.nativeElement.querySelector(selector);
  }

  it('shows the picture and the id of the value it was given', () => {
    const component = create({ id: 3, url: '/images/logo.png' });

    const preview = query('img.preview') as HTMLImageElement;
    // The url the value holds is relative to the API: on another origin it needs the prefix.
    expect(preview.src).toContain(apiUrl('/images/logo.png'));
    expect(query('.image-id')?.textContent?.trim()).toBe('id: 3');
    expect(component.imageId()).toBe(3);
  });

  it('has no picture to show, and says so, while the value is empty', () => {
    create(null);

    expect(query('img.preview')).toBeFalsy();
    expect(query('.image-id')?.textContent?.trim()).toBe('id: —');
  });

  it('uploads a file and keeps the URL the server reports', () => {
    // On AWS the upload URL is a presigned request to S3: it names a signature and an expiry,
    // and trimming its query string yields nothing that can be read later. The server answers
    // with the stable address as well, and that is what content keeps.
    const presigned =
      'https://cms-images.s3.eu-west-1.amazonaws.com/9f3c.png?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Signature=deadbeef';
    const stable = 'https://images.example.com/9f3c.png';
    images.uploadImage = () => of({ id: 7, upload_url: presigned, url: stable });

    const component = create();
    const chosen: unknown[] = [];
    component.valueChange.subscribe((value) => chosen.push(value));

    component.onFileSelected(fileChosen('cover.png'));
    fixture.detectChanges();

    expect(chosen).toEqual([{ id: 7, url: stable }]);
    expect(JSON.stringify(chosen)).not.toContain('X-Amz-Signature');
    expect(component.uploading()).toBe(false);
  });

  it('says so when the upload fails, and keeps the value it had', () => {
    images.failUpload = true;

    const component = create({ id: 3, url: '/images/logo.png' });
    const errors: (Message | null)[] = [];
    component.errorChange.subscribe((message) => errors.push(message));

    component.onFileSelected(fileChosen('huge.png'));
    fixture.detectChanges();

    // The first is the clear that every attempt starts with, so a stale message does not sit
    // over an upload the editor has already replaced.
    // The server's own sentence is kept alongside the site's wording, so the editor can see
    // why: `cat docs/i18n.md`.
    expect(errors).toEqual([
      null,
      { key: 'content.uploadFailed', params: { message: 'too large' } },
    ]);
    expect(component.value).toEqual({ id: 3, url: '/images/logo.png' });
    expect(component.uploading()).toBe(false);
  });

  // The picker is a dialog: nothing is read until it is opened, because nothing is on screen until
  // then.
  it('reads the library when the picker is opened, and not before', async () => {
    const component = create();
    expect(images.listCalls).toBe(0);

    component.openLibrary();
    await fixture.whenStable();
    expect(images.listCalls).toBe(1);
  });

  it('re-reads the library for each opening, not once for the screen', async () => {
    const component = create();
    component.openLibrary();
    await fixture.whenStable();
    closeDialogs();
    await fixture.whenStable();

    // A fresh read is the point of a shared library: an image somebody else uploaded a minute ago
    // should be there.
    component.openLibrary();
    await fixture.whenStable();
    expect(images.listCalls).toBe(2);
  });

  it('picks an already uploaded image instead of uploading a new one', async () => {
    const component = create();
    const chosen: unknown[] = [];
    component.valueChange.subscribe((value) => chosen.push(value));

    component.openLibrary();
    await fixture.whenStable();

    const thumbs = document.querySelectorAll<HTMLElement>('.thumb');
    expect(thumbs.length).toBe(2);

    thumbs[0].click();
    await fixture.whenStable();
    fixture.detectChanges();

    // The value keeps the shape an upload produces, so the server cannot tell them apart.
    expect(chosen).toEqual([{ id: 3, url: '/images/logo.png' }]);
    // Choosing is the whole gesture: the dialog closes itself rather than asking for a second press.
    expect(document.querySelector('.thumb')).toBeNull();
  });

  it('leaves the file control out where the value is only being shown', () => {
    fixture = TestBed.createComponent(ImageField);
    fixture.componentRef.setInput('value', { id: 3, url: '/images/logo.png' });
    fixture.componentRef.setInput('disabled', true);
    fixture.detectChanges();

    expect(query('input[type="file"]')).toBeFalsy();
    expect(query('button')?.textContent).toBeUndefined();
    expect(query('img.preview')).toBeTruthy();
  });

  // The HTTP client is provided because the library picker reads through it; nothing in these
  // specs talks to it directly, so anything outstanding would be a leak.
  afterEach(() => {
    // A dialog lives in the document rather than in the fixture, so one left open is a `.thumb`
    // the next spec finds.
    closeDialogs();
    TestBed.inject(HttpTestingController).verify();
  });
});
