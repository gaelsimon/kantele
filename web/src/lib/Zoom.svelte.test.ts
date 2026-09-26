// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import Zoom from './Zoom.svelte';

describe('a cover', () => {
  afterEach(cleanup);

  it('opens at its own size on a click, and closes on another or on Escape', async () => {
    render(Zoom, { props: { src: '/art/tr-1' } });
    expect(screen.queryByLabelText('Close the cover')).toBeNull();
    await fireEvent.click(screen.getByTitle('Show at its own size'));
    await fireEvent.click(screen.getByLabelText('Close the cover'));
    expect(screen.queryByLabelText('Close the cover')).toBeNull();

    await fireEvent.click(screen.getByTitle('Show at its own size'));
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByLabelText('Close the cover')).toBeNull();
  });
});
