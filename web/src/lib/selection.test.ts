import { describe, expect, it } from 'vitest';
import { leaf } from './selection';

describe('what the pane is showing', () => {
  it('heads a folder with its last part, and the library with none', () => {
    expect(leaf('Blue Note/Sierra Maestra')).toBe('Sierra Maestra');
    expect(leaf('Autechre')).toBe('Autechre');
    expect(leaf('a/b/')).toBe('b');
    expect(leaf('')).toBe('');
  });
});
