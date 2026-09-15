import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { of, throwError } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { t } from 'app/core/i18n/message';
import { ImageEntry, NewImageInfo } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';

import { List } from './list';

/** A fake library, so uploading and deleting can be observed without HTTP. */
class StubImagesService {
  public library: ImageEntry[] = [
    {
      id: 1,
      url: '/images/first.png',
      original_filename: 'first.png',
      uploaded_at: '2024-01-01T00:00:00Z',
    },
  ];
  public uploaded: string[] = [];
  public deleted: number[] = [];
  /** The renames the screen asked for, in order. */
  public renamed: { id: number; name: string }[] = [];

  listImages = () => of(this.library);

  uploadImage = (file: File) => {
    this.uploaded.push(file.name);
    return of<NewImageInfo>({ id: 2, upload_url: '/images/second.png', url: '/images/second.png' });
  };

  /** The replacements asked for, and the bytes they applied. */
  public replacements: { id: number; file: string }[] = [];
  public applied: { id: number; fileName: string }[] = [];

  replaceImage = (id: number, file: File) => {
    this.replacements.push({ id, file: file.name });
    this.applied.push({ id, fileName: `replacement-${id}` });
    this.library = this.library.map((image) =>
      image.id === id ? { ...image, url: `/images/replacement-${id}` } : image,
    );
    return of({ file_name: `replacement-${id}`, upload_url: `/images/replacement-${id}?key=k` });
  };

  imageLink = (id: number) => `/images/by-id/${id}`;

  renameImage = (id: number, name: string) => {
    this.renamed.push({ id, name });
    this.library = this.library.map((image) =>
      image.id === id ? { ...image, original_filename: name } : image,
    );
    return of(void 0);
  };

  deleteImage = (id: number) => {
    this.deleted.push(id);
    this.library = this.library.filter((image) => image.id !== id);
    return of(void 0);
  };
}

/** A change event for a file input, as the template would produce. */
function fileChosen(name: string): Event {
  const input = { files: [new File(['x'], name, { type: 'image/png' })], value: name };
  return { target: input } as unknown as Event;
}

/** Permissions are the server's business; the screens are only told what to offer. */
function stubAuth(canEdit = true, canPublish = true, isAdmin = true) {
  return {
    provide: AuthService,
    useValue: {
      user: () => null,
      canEdit: () => canEdit,
      canPublish: () => canPublish,
      // Uploading is open to anyone who may edit content; the rest of the library is not.
      canUploadImages: () => canEdit,
      // The screens ask about the resource they are showing; these stubs answer the same way
      // everywhere.
      canEditIn: () => canEdit,
      canPublishIn: () => canPublish,
      isAdmin: () => isAdmin,
    },
  };
}

describe('Image library', () => {
  let fixture: ComponentFixture<List>;
  let stub: StubImagesService;

  beforeEach(async () => {
    stub = new StubImagesService();
    await TestBed.configureTestingModule({
      imports: [List],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: ImagesService, useValue: stub },
        stubAuth(),
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(List);
    await fixture.whenStable();
  });

  const images = () => fixture.nativeElement.querySelectorAll('.library .image');

  it('lists what has been uploaded', () => {
    fixture.detectChanges();

    expect(images().length).toBe(1);
    expect(fixture.nativeElement.textContent).toContain('first.png');
    expect(fixture.componentInstance.library()[0].id).toBe(1);
  });

  it('says so when the library is empty', () => {
    stub.library = [];
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    // Query the fresh fixture, not the one from the shared setup.
    expect(fresh.nativeElement.querySelectorAll('.library .image').length).toBe(0);
    expect(fresh.nativeElement.querySelector('.note')?.textContent).toContain('No images yet');
  });

  it('deletes an image after confirming', () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    fixture.detectChanges();

    (fixture.nativeElement.querySelector('.remove') as HTMLButtonElement).click();
    fixture.detectChanges();

    expect(stub.deleted).toEqual([1]);
    // The list is reloaded, so the deleted image is gone from the grid.
    expect(images().length).toBe(0);
    expect(fixture.componentInstance.error()).toBeNull();
  });

  it('leaves the image alone when the confirmation is declined', () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    fixture.detectChanges();

    (fixture.nativeElement.querySelector('.remove') as HTMLButtonElement).click();
    fixture.detectChanges();

    expect(stub.deleted).toEqual([]);
    expect(images().length).toBe(1);
  });

  it('uploads the chosen file and shows it afterwards', () => {
    stub.uploadImage = (file: File) => {
      stub.uploaded.push(file.name);
      stub.library = [
        {
          id: 2,
          url: '/images/second.png',
          original_filename: file.name,
          uploaded_at: '2024-01-02T00:00:00Z',
        },
        ...stub.library,
      ];
      return of<NewImageInfo>({ id: 2, upload_url: '/images/second.png', url: '/images/second.png' });
    };
    fixture.detectChanges();

    fixture.componentInstance.onFileSelected(fileChosen('second.png'));
    fixture.detectChanges();

    expect(stub.uploaded).toEqual(['second.png']);
    expect(images().length).toBe(2);
    expect(fixture.componentInstance.uploading()).toBe(false);
  });

  it('reports a failed upload instead of dropping it silently', () => {
    stub.uploadImage = () => throwError(() => new Error('upload failed'));
    fixture.detectChanges();

    fixture.componentInstance.onFileSelected(fileChosen('broken.png'));
    fixture.detectChanges();

    expect(fixture.componentInstance.uploading()).toBe(false);
    // Our own sentence around the server's, in the reader's language.
    expect(fixture.componentInstance.error()).toEqual({
      key: 'content.uploadFailed',
      params: { message: 'upload failed' },
    });
  });
  // The name is a label, and the id is what content references: renaming must not disturb
  // anything else, which is what the inline editor is for.
  it('renames an image in place, without touching anything else', () => {
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.startRename(stub.library[0]);
    fresh.componentInstance.draftName.set('  表紙.png  ');
    fresh.componentInstance.confirmRename(stub.library[0]);
    fresh.detectChanges();

    expect(stub.renamed).toEqual([{ id: 1, name: '表紙.png' }]);
    expect(fresh.componentInstance.renaming()).toBeNull();
    expect(fresh.nativeElement.textContent).toContain('表紙.png');
  });

  it('refuses a name that is empty or a path, without asking the server', () => {
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    for (const bad of ['   ', 'a/b.png', 'a\\b.png']) {
      fresh.componentInstance.startRename(stub.library[0]);
      fresh.componentInstance.draftName.set(bad);
      fresh.componentInstance.confirmRename(stub.library[0]);
    }

    expect(stub.renamed).toEqual([]);
    expect(fresh.componentInstance.error()).toEqual(t('content.invalidImageName'));
    // The editor stays open on the name that was refused.
    expect(fresh.componentInstance.renaming()).toBe(1);
  });

  it('leaves the name alone when the edit is cancelled', () => {
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.startRename(stub.library[0]);
    fresh.componentInstance.draftName.set('something else');
    fresh.componentInstance.cancelRename();
    fresh.detectChanges();

    expect(stub.renamed).toEqual([]);
    expect(fresh.componentInstance.renaming()).toBeNull();
    expect(fresh.nativeElement.querySelector('input[name=imageName]')).toBeNull();
  });

  // Replacing is not renaming and not deleting: the image keeps its id and its name, and content
  // that references it shows the new picture without being touched.
  it('replaces what an image shows, keeping its id', () => {
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.onReplacementSelected(stub.library[0], fileChosen('new.png'));
    fresh.detectChanges();

    expect(stub.replacements).toEqual([{ id: 1, file: 'new.png' }]);
    expect(stub.applied).toEqual([{ id: 1, fileName: 'replacement-1' }]);
    expect(stub.library[0].original_filename).toBe('first.png');
    expect(fresh.componentInstance.notice()).toEqual(t('content.imageReplaced'));
    expect(fresh.componentInstance.error()).toBeNull();
  });

  it('reports a replacement that failed, and keeps the image', () => {
    stub.replaceImage = () => throwError(() => new Error('upload failed'));
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.onReplacementSelected(stub.library[0], fileChosen('new.png'));
    fresh.detectChanges();

    expect(fresh.componentInstance.error()).toEqual({
      key: 'content.replaceFailed',
      params: { message: 'upload failed' },
    });
    expect(stub.library[0].url).toBe('/images/first.png');
  });

  // The link that is worth writing into a Markdown body: it names the image by id, so it points
  // at whatever the image shows when the page is read.
  it('offers the durable link, and says so when it cannot copy it', async () => {
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    await fresh.componentInstance.copyLink(stub.library[0]);

    expect(fresh.componentInstance.notice()).toEqual(
      t('content.imageLinkNotCopied', { url: `${location.origin}/api/images/by-id/1` }),
    );
    expect(fresh.componentInstance.error()).toBeNull();
  });

  // An editor with a grant for one collection needs the images that collection uses, so the
  // library offers the upload button - and not the buttons that change what is already there.
  it('offers uploading to an editor of one collection, but not changing the library', async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      imports: [List],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: ImagesService, useValue: stub },
        {
          provide: AuthService,
          useValue: {
            user: () => null,
            canEdit: () => false,
            canPublish: () => false,
            canEditIn: () => false,
            canPublishIn: () => false,
            isAdmin: () => false,
            canUploadImages: () => true,
          },
        },
      ],
    });
    const fresh = TestBed.createComponent(List);
    await fresh.whenStable();
    fresh.detectChanges();

    expect(fresh.nativeElement.textContent).toContain('Upload image');
    expect(fresh.nativeElement.querySelector('.remove')).toBeNull();
  });
});
