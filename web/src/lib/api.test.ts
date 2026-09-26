import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getFiles, getFolders, getLog, getShares, rescan, writeConfiguration } from './api';

/// The last request the page made, and a body of the caller's choosing in reply.
function answering(body: unknown, ok = true, text?: string) {
  const fetch = vi.fn(async (_path: string, _init?: RequestInit) => ({
    ok,
    status: ok ? 200 : 400,
    statusText: ok ? 'OK' : 'Bad Request',
    text: async () => text ?? JSON.stringify(body),
  }));
  vi.stubGlobal('fetch', fetch);
  return fetch;
}

function asked(fetch: ReturnType<typeof answering>) {
  const [path] = fetch.mock.calls.at(0) ?? ['', undefined];
  return path;
}

function sent(fetch: ReturnType<typeof answering>) {
  const [, init] = fetch.mock.calls.at(0) ?? ['', undefined];
  return init ?? {};
}

beforeEach(() => answering({}));
afterEach(() => vi.unstubAllGlobals());

describe('asking for a folder listing', () => {
  it('asks for the top of the library with no query at all', async () => {
    const fetch = answering({});
    await getFolders('', '', false);
    expect(asked(fetch)).toBe('/api/folders');
  });

  it('names the folder it is opening', async () => {
    const fetch = answering({});
    await getFolders('Blue Note', '', false);
    expect(asked(fetch)).toBe('/api/folders?under=Blue+Note');
  });

  it('carries the search and the changed filter', async () => {
    const fetch = answering({});
    await getFolders('', 'kremerata', true);
    expect(asked(fetch)).toBe('/api/folders?q=kremerata&changed=true');
  });

  it('carries every box ticked in one parameter', async () => {
    const fetch = answering({});
    await getFolders('', '', false, ['duplicate-tracks', 'no-genre']);
    expect(asked(fetch)).toBe('/api/folders?only=duplicate-tracks%2Cno-genre');
  });

  /// An empty `only=` would be a question about nothing.
  it('leaves the boxes out where none is ticked', async () => {
    const fetch = answering({});
    await getFolders('', '', false, []);
    expect(asked(fetch)).toBe('/api/folders');
  });

  it('narrows the files of a folder by the same checks as the folders', async () => {
    const fetch = answering({});
    await getFiles('Blue Note', ['no-artist']);
    expect(asked(fetch)).toBe('/api/files?folder=Blue+Note&only=no-artist');
  });

  it('escapes a folder name that would break the query', async () => {
    const fetch = answering({});
    await getFolders('Rock & Roll/AC?DC', '', false);
    expect(asked(fetch)).toBe('/api/folders?under=Rock+%26+Roll%2FAC%3FDC');
  });
});

describe('the other routes', () => {
  it('asks the picker for images only when it wants an icon', async () => {
    const fetch = answering({});
    await getShares('/volume1', true);
    expect(asked(fetch)).toBe('/api/shares?under=%2Fvolume1&images=true');
  });

  it('rescans everything where no folder is named', async () => {
    const fetch = answering({});
    await rescan();
    expect(asked(fetch)).toBe('/api/rescan');
  });

  it('escapes the folder it rescans', async () => {
    const fetch = answering({});
    await rescan('Blue Note/Sierra');
    expect(asked(fetch)).toBe('/api/rescan?folder=Blue%20Note%2FSierra');
  });

  it('sends a settings write as a JSON body', async () => {
    const fetch = answering({});
    await writeConfiguration({ 'scan.threads': 8 });
    expect(sent(fetch).method).toBe('PUT');
    expect(sent(fetch).body).toBe('{"scan.threads":8}');
  });
});

describe('when the server refuses', () => {
  /// The server writes its refusals to be read, so the page shows them rather than a status code.
  it('throws the server own words', async () => {
    answering(null, false, 'scan.cover_art is held by the environment');
    await expect(writeConfiguration({})).rejects.toThrow(
      'scan.cover_art is held by the environment',
    );
  });

  it('falls back to the status where the server said nothing', async () => {
    answering(null, false, '   ');
    await expect(writeConfiguration({})).rejects.toThrow('400 Bad Request');
  });
});

describe('asking for the end of the log', () => {
  it('asks for two hundred lines unless told otherwise', async () => {
    const fetch = answering({ path: '/var/kantele.log', lines: [] });
    await getLog();
    expect(asked(fetch)).toBe('/api/log?lines=200');
  });

  it('passes the server\'s own sentence on when there is no file', async () => {
    answering({}, false, 'no log file: this server writes to the terminal it was started from');
    await expect(getLog(50)).rejects.toThrow('no log file');
  });
});
