// Where an image's bytes are fetched from, and what makes two URLs the same image.
//
// This is what makes a replaced image update on the next build while an unchanged one is not fetched
// again. The delivery API renders `{id, url}` from the image record, so the id stays 3 while the url
// becomes the new file name; and a deployment that serves presigned URLs signs every read, so the
// query changes while the object does not.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import images from '../src/images.js';

const { extensionOf, imageSource, reusableFiles, sourceKey } = images;

const client = {
  absoluteUrl: (path) => `https://cms.example.com${path}`,
  apiPathUrl: (path) => `https://cms.example.com/api${path}`,
};

describe('imageSource', () => {
  it('downloads the URL the API returned, not the stable id link', () => {
    assert.deepEqual(imageSource({ id: 3, url: '/api/images/logo-v2.png' }, client), {
      url: 'https://cms.example.com/api/images/logo-v2.png',
      key: 'https://cms.example.com/api/images/logo-v2.png',
    });
  });

  it('uses the id link only when the value has no URL at all, and never reuses it', () => {
    // The id link resolves to whatever the object is now, so its path survives a replacement and
    // reusing it would keep serving the picture that was replaced.
    assert.deepEqual(imageSource({ id: 3, url: '' }, client), {
      url: 'https://cms.example.com/api/images/by-id/3',
      key: null,
    });
    assert.equal(imageSource({ id: null, url: '' }, client), null);
  });
});

describe('sourceKey', () => {
  it('is the same for two signatures of one object', () => {
    const first = 'https://bucket.s3.eu-west-1.amazonaws.com/abc.png?X-Amz-Date=1&X-Amz-Signature=one';
    const second = 'https://bucket.s3.eu-west-1.amazonaws.com/abc.png?X-Amz-Date=2&X-Amz-Signature=two';
    assert.equal(sourceKey(first), sourceKey(second));
    assert.equal(sourceKey(first), 'https://bucket.s3.eu-west-1.amazonaws.com/abc.png');
  });

  it('changes when the object key changes, which is what a replacement does', () => {
    assert.notEqual(sourceKey('https://host/abc.png?sig=1'), sourceKey('https://host/def.png?sig=1'));
  });

  it('changes with the host, so another deployment is a different image', () => {
    assert.notEqual(sourceKey('https://a.example.com/x.png'), sourceKey('https://b.example.com/x.png'));
  });

  it('answers null for something that is not a URL', () => {
    assert.equal(sourceKey('/api/images/logo.png'), null);
    assert.equal(sourceKey(''), null);
  });
});

describe('reusableFiles', () => {
  it('keys the File nodes this plugin fetched by their URL without the query', () => {
    const files = [
      { id: 'file:remote', url: 'https://host/abc.png?X-Amz-Signature=old' },
      { id: 'file:local', absolutePath: '/tmp/local.png' },
      null,
      { id: 'file:remote-again', url: 'https://host/abc.png?X-Amz-Signature=older' },
    ];
    const byKey = reusableFiles(() => files);
    assert.equal(byKey.get('https://host/abc.png').id, 'file:remote');
    assert.equal(byKey.size, 1);
  });

  it('answers nothing without a store to look in', () => {
    assert.equal(reusableFiles(undefined).size, 0);
  });
});

describe('extensionOf', () => {
  it('keeps the extension the File media type is read from', () => {
    assert.equal(extensionOf('/api/images/logo-v2.png'), '.png');
    assert.equal(extensionOf('/api/images/photo.JPG?x=1'), '.jpg');
    assert.equal(extensionOf('https://bucket.s3.amazonaws.com/a.webp#frag'), '.webp');
  });

  it('answers null when there is no usable extension', () => {
    // The id link has none, which is the other reason it is not used for downloading.
    assert.equal(extensionOf('/api/images/by-id/3'), null);
    assert.equal(extensionOf(''), null);
    assert.equal(extensionOf(undefined), null);
  });
});
