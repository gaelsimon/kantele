// Walking a tree one column at a time: what is open, what has been read, and what would not read.
// The component draws this; nothing here knows what fills a column or what a row means.

import { opened, shown, trailUnder, type Level } from './columns';

export type Fill<T> = (path: string, first: boolean) => Promise<Level<T>>;

export class Walk<T> {
  /// The folders opened, outermost first. The column before them is the top of the listing.
  trail = $state<string[]>([]);
  levels = $state<Record<string, Level<T>>>({});
  /// Why a column could not be read, by its folder.
  broke = $state<Record<string, string>>({});
  waiting = $state<string[]>([]);
  /// The latest read asked for per folder, so an answer that arrives late is dropped.
  #asked: Record<string, number> = {};
  #fill: Fill<T>;

  constructor(fill: Fill<T>) {
    this.#fill = fill;
  }

  get columns(): string[] {
    return shown(this.trail);
  }

  entries(path: string) {
    return this.levels[path]?.entries ?? [];
  }

  /// Reads one column, whether or not it has been read before.
  async read(path: string): Promise<void> {
    const ticket = (this.#asked[path] ?? 0) + 1;
    this.#asked[path] = ticket;
    this.waiting = [...this.waiting, path];
    try {
      const level = await this.#fill(path, path === '');
      if (this.#asked[path] !== ticket) return;
      this.levels = { ...this.levels, [path]: level };
      const { [path]: _read, ...rest } = this.broke;
      this.broke = rest;
    } catch (error) {
      if (this.#asked[path] !== ticket) return;
      this.broke = {
        ...this.broke,
        [path]: error instanceof Error ? error.message : String(error),
      };
    } finally {
      this.waiting = this.waiting.filter((one) => one !== path);
    }
  }

  /// Opens a folder in the column at `depth`, closing whatever was open below it.
  open(depth: number, path: string): void {
    this.trail = opened(this.trail, depth, path);
    if (!this.levels[path]) void this.read(path);
  }

  /// Closes every column past the one at `depth`, which is what picking a file does.
  close(depth: number): void {
    this.trail = this.trail.slice(0, depth);
  }

  /// Empties everything and reads the top of the listing again.
  async restart(): Promise<void> {
    this.trail = [];
    this.levels = {};
    this.broke = {};
    await this.read('');
  }

  /// Reads every open column again, keeping the trail as far as it still leads.
  async refresh(): Promise<void> {
    const trail = this.trail;
    await this.read('');
    const kept: string[] = [];
    for (const path of trail) {
      if (!this.entries(kept.at(-1) ?? '').some((entry) => entry.path === path)) break;
      await this.read(path);
      kept.push(path);
    }
    this.trail = kept;
    this.levels = Object.fromEntries(
      Object.entries(this.levels).filter(([path]) => path === '' || kept.includes(path)),
    );
  }

  /// Opens down to a path the page arrived with, so an owner sees where they already are.
  async revealTo(start: string): Promise<void> {
    const under = this.entries('').find(
      (entry) => start === entry.path || start.startsWith(`${entry.path}/`),
    );
    if (!under) return;
    const wanted = trailUnder(under.path, start);
    for (const path of wanted) {
      if (!this.levels[path]) await this.read(path);
    }
    this.trail = wanted;
  }
}
