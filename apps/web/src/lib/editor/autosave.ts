// SPDX-License-Identifier: AGPL-3.0-only
// Debounced autosave: bursts of edits become one save, saves never overlap,
// and edits made while a save is running are saved right after it.

export type SaveState = 'saved' | 'pending' | 'saving' | 'error';

export interface Autosave {
  /** Current state; read it after awaiting flush() to know whether everything is saved. */
  readonly state: SaveState;
  /** Mark the document as changed; a save runs `delayMs` after the last touch. */
  touch(): void;
  /** Save now (waiting for a running save first). Resolves when nothing unsaved is left or a save failed. */
  flush(): Promise<void>;
  /** Stop autosaving for good (e.g. after a conflict); later touches and flushes do nothing. */
  stop(): void;
}

/**
 * @param save  Saves the current document; resolves `false` (or throws) on failure.
 * @param delayMs  Quiet time after the last touch before saving.
 * @param onState  Called whenever `state` changes (for reactive UIs).
 */
export function createAutosave(
  save: () => Promise<boolean>,
  delayMs = 1500,
  onState?: (s: SaveState) => void,
): Autosave {
  let state: SaveState = 'saved';
  let dirty = false;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running: Promise<void> | null = null;

  function set(s: SaveState) {
    if (s === state) return;
    state = s;
    onState?.(s);
  }

  function clearTimer() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  async function drain() {
    while (dirty && !stopped) {
      dirty = false;
      set('saving');
      let ok = false;
      try {
        ok = await save();
      } catch {
        ok = false;
      }
      if (!ok) {
        // The changes are still unsaved; the next touch or flush retries.
        dirty = true;
        set('error');
        return;
      }
      // More edits arrived and their debounce is still running: let the timer start the next save.
      if (timer !== undefined) {
        set('pending');
        return;
      }
    }
    set(dirty ? 'pending' : 'saved');
  }

  function run(): Promise<void> {
    if (!running) running = drain().finally(() => (running = null));
    return running;
  }

  return {
    get state() {
      return state;
    },
    touch() {
      if (stopped) return;
      dirty = true;
      if (!running) set('pending');
      clearTimer();
      timer = setTimeout(() => {
        timer = undefined;
        void run();
      }, delayMs);
    },
    async flush() {
      if (stopped) return;
      clearTimer();
      while (running) await running;
      if (dirty && !stopped) await run();
    },
    stop() {
      stopped = true;
      clearTimer();
    },
  };
}
