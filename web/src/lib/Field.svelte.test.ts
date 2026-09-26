// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
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

describe('the note under a field', () => {
  afterEach(() => cleanup());

  const level: Setting = {
    key: 'log level',
    label: 'Log level',
    value: 'kantele=info',
    as_written: null,
    source: { layer: 'default' },
    apply: 'never',
    says: 'You cannot change this setting on this page.',
    tag: 'read only',
    writable: false,
    note: 'The server reads it from RUST_LOG when it starts.',
  };

  it('says what the server does even where the field cannot be changed', () => {
    render(Field, {
      props: { setting: level, shape: { kind: 'read' }, value: null, changed: false, tagged: false, onchange: () => {} },
    });
    expect(screen.getByText('The server reads it from RUST_LOG when it starts.')).toBeTruthy();
  });

  it('puts what the server does before how the control is used', () => {
    const exclude: Setting = {
      ...interval,
      key: 'scan.exclude',
      note: 'The server never scans @eaDir.',
    };
    render(Field, {
      props: {
        setting: exclude,
        shape: { kind: 'words', note: 'Separate the entries with commas.' },
        value: [],
        changed: false,
        tagged: false,
        onchange: () => {},
      },
    });
    expect(screen.getByText('The server never scans @eaDir. Separate the entries with commas.')).toBeTruthy();
  });
});
