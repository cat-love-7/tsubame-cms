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
});
