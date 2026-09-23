// @vitest-environment jsdom
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import Field from './Field.svelte';
import type { Setting } from './api';

const interval: Setting = {
  key: 'scan.sweep_minutes',
  label: 'Scan interval',
  value: '15',
  as_written: 15,
  source: { layer: 'file' },
  apply: 'next-pass',
  says: 'used at the next check',
  tag: 'next check',
  writable: true,
};

describe('a number field', () => {
  afterEach(() => cleanup());

  it('leaves the value alone while the box is empty, since zero turns the scan off', async () => {
    const onchange = vi.fn();
    const { container } = render(Field, {
      props: {
        setting: interval,
        shape: { kind: 'number' },
        value: 15,
        changed: false,
        tagged: false,
        onchange,
      },
    });
    const box = container.querySelector('input') as HTMLInputElement;
    await fireEvent.input(box, { target: { value: '' } });
    expect(onchange).not.toHaveBeenCalled();
    await fireEvent.input(box, { target: { value: '30' } });
    expect(onchange).toHaveBeenLastCalledWith(30);
  });
});
