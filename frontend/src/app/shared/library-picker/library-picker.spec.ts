import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ApplicationRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatDialog } from '@angular/material/dialog';
import { throwError } from 'rxjs';

import { StubImagesService } from 'app/core/testing/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';
import { LibraryPicker, LibraryPickerResult } from './library-picker';

/**
 * The picker is a dialog, so these specs open it the way a field does and read what it closes with.
 *
 * What a field does with the answer is that field's own spec; this is about the picker itself: the
 * wall, the ticks, the upload it carries, and what it hands back.
 */
describe('LibraryPicker', () => {
  let images: StubImagesService;

  beforeEach(async () => {
    images = new StubImagesService();
    await TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: ImagesService, useValue: images },
      ],
    }).compileComponents();
  });

  afterEach(() => {
    TestBed.inject(MatDialog).closeAll();
  });

  /** Opens the picker the way a field does, and answers what it closes with. */
  function open(multi = false): Promise<LibraryPickerResult> {
    const ref = TestBed.inject(MatDialog).open<
      LibraryPicker,
      { multi: boolean },
      LibraryPickerResult
    >(LibraryPicker, { data: { multi } });
    const answered = new Promise<LibraryPickerResult>((resolve) =>
      ref.afterClosed().subscribe(resolve),
    );
    // The content is attached and rendered a tick after `open`; without this the wall is not in the
    // document yet.
    TestBed.inject(ApplicationRef).tick();
    return answered;
  }

  /** What the wall is showing, in the order it lists them. */
  function tiles(): HTMLButtonElement[] {
    return Array.from(document.querySelectorAll<HTMLButtonElement>('.thumb'));
  }

  /** The file control inside the dialog, which is the upload the picker carries. */
  function uploadInput(): HTMLInputElement {
    const input = document.querySelector<HTMLInputElement>('mat-dialog-container input[type=file]');
    if (!input) {
      throw new Error('the dialog has no file control');
    }
    return input;
  }

  /** Hand `uploadInput()` a file and let the change flow, as a browser would. */
  function chooseFile(name: string) {
    const input = uploadInput();
    Object.defineProperty(input, 'files', {
      value: [new File(['x'], name, { type: 'image/png' })],
      configurable: true,
    });
    input.dispatchEvent(new Event('change'));
    TestBed.inject(ApplicationRef).tick();
  }

  it('shows the library it read', () => {
    void open();

    expect(tiles().length).toBe(2);
  });

  it('answers with the image a tile chooses, and closes', async () => {
    const answered = open();
    tiles()[1].click();

    expect(((await answered) as ImageEntry).id).toBe(4);
    // Choosing is the whole gesture: the dialog goes rather than asking for a second press.
    expect(document.querySelector('.thumb')).toBeNull();
  });

  it('answers with nothing when it is cancelled', async () => {
    const answered = open();
    (document.querySelector('mat-dialog-actions button:last-of-type') as HTMLButtonElement).click();

    expect(await answered).toBeUndefined();
  });

  it('ticks several and answers them in the order the library lists them', async () => {
    const answered = open(true);
    // Ticked newest first on purpose: the answer follows the wall, not the order of the clicks.
    tiles()[1].click();
    tiles()[0].click();
    TestBed.inject(ApplicationRef).tick();
    expect(tiles().filter((tile) => tile.classList.contains('selected')).length).toBe(2);

    document.querySelector<HTMLButtonElement>('button.array-add-selected')?.click();

    expect(((await answered) as ImageEntry[]).map((image) => image.id)).toEqual([3, 4]);
  });

  // The Markdown box has no upload of its own, so this is the only way to an image that is not in
  // the library yet - the case the picker used to be a dead end for.
  it('uploads a file from the dialog and answers with it', async () => {
    const answered = open();
    chooseFile('new-cover.png');

    const image = (await answered) as ImageEntry;
    expect(images.uploaded).toEqual(['new-cover.png']);
    expect(image.id).toBe(101);
    // The name travels with it: it is the alt text the Markdown box writes.
    expect(image.original_filename).toBe('new-cover.png');
  });

  it('adds an upload to the wall, ticked, when several are being chosen', async () => {
    const answered = open(true);
    tiles()[0].click();
    chooseFile('extra.png');

    // The wall grows by the upload and it is already ticked: the pick carries on rather than ending
    // at the first file.
    expect(tiles().length).toBe(3);
    expect(tiles()[0].title).toBe('extra.png');
    expect(tiles()[0].classList.contains('selected')).toBe(true);

    document.querySelector<HTMLButtonElement>('button.array-add-selected')?.click();
    expect(((await answered) as ImageEntry[]).map((image) => image.id)).toEqual([101, 3]);
  });

  it('says in the dialog that the library could not be read', () => {
    images.listImages = () => throwError(() => new Error('offline'));

    void open();

    expect(document.querySelector('mat-dialog-content .error')?.textContent).toContain('offline');
    // A wall that failed to load is empty, and says what to do about it.
    expect(document.querySelector('mat-dialog-content .note')).toBeTruthy();
  });

  it('says an upload failed, and stays open', async () => {
    const answered = open();
    images.uploadImage = () => throwError(() => new Error('too large'));

    chooseFile('huge.png');

    expect(document.querySelector('mat-dialog-content .error')?.textContent).toContain('too large');
    expect(document.querySelector('.thumb')).toBeTruthy();

    TestBed.inject(MatDialog).closeAll();
    expect(await answered).toBeUndefined();
  });
});
