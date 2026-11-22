// SPDX-License-Identifier: AGPL-3.0-only
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createAutosave } from './autosave';

/** A save() whose calls stay in flight until `resolve(ok)` is called. */
function controlledSave() {
  const pending: ((ok: boolean) => void)[] = [];
  let active = 0;
  let maxActive = 0;
  const save = vi.fn(
    () =>
      new Promise<boolean>((res) => {
        active++;
        maxActive = Math.max(maxActive, active);
        pending.push((ok) => {
          active--;
          res(ok);
        });
      }),
  );
  return {
    save,
    resolve: async (ok = true) => {
      pending.shift()!(ok);
      await vi.advanceTimersByTimeAsync(0);
    },
    get maxActive() {
      return maxActive;
    },
  };
}

describe('createAutosave', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('starts saved and debounces a burst of touches into one save', async () => {
    const save = vi.fn(async () => true);
    const a = createAutosave(save, 1500);
    expect(a.state).toBe('saved');
    a.touch();
    expect(a.state).toBe('pending');
    await vi.advanceTimersByTimeAsync(1000);
    a.touch();
    await vi.advanceTimersByTimeAsync(1000);
    a.touch();
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1500);
    expect(save).toHaveBeenCalledTimes(1);
    expect(a.state).toBe('saved');
  });

  it('uses a 1500 ms delay by default', async () => {
    const save = vi.fn(async () => true);
    const a = createAutosave(save);
    a.touch();
    await vi.advanceTimersByTimeAsync(1499);
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it('reports saving while a save is in flight', async () => {
    const c = controlledSave();
    const a = createAutosave(c.save, 100);
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    expect(a.state).toBe('saving');
    await c.resolve(true);
    expect(a.state).toBe('saved');
  });

  it('never runs two saves at once, and a touch during a save triggers exactly one more', async () => {
    const c = controlledSave();
    const a = createAutosave(c.save, 100);
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    expect(c.save).toHaveBeenCalledTimes(1);
    // Edits keep coming while the first save is still running.
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    a.touch();
    await vi.advanceTimersByTimeAsync(500);
    expect(c.save).toHaveBeenCalledTimes(1);
    await c.resolve(true);
    await vi.advanceTimersByTimeAsync(100);
    expect(c.save).toHaveBeenCalledTimes(2);
    expect(a.state).toBe('saving');
    await c.resolve(true);
    await vi.advanceTimersByTimeAsync(1000);
    expect(c.save).toHaveBeenCalledTimes(2);
    expect(c.maxActive).toBe(1);
    expect(a.state).toBe('saved');
  });

  it('flush saves immediately without waiting for the delay', async () => {
    const save = vi.fn(async () => true);
    const a = createAutosave(save, 1500);
    a.touch();
    await a.flush();
    expect(save).toHaveBeenCalledTimes(1);
    expect(a.state).toBe('saved');
    // The debounce timer was cancelled: no second save later.
    await vi.advanceTimersByTimeAsync(5000);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it('flush does nothing when there are no changes', async () => {
    const save = vi.fn(async () => true);
    const a = createAutosave(save, 1500);
    await a.flush();
    expect(save).not.toHaveBeenCalled();
    expect(a.state).toBe('saved');
  });

  it('flush during a save waits for it and then saves the newer changes', async () => {
    const c = controlledSave();
    const a = createAutosave(c.save, 100);
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    a.touch();
    let done = false;
    const f = a.flush().then(() => (done = true));
    await vi.advanceTimersByTimeAsync(0);
    expect(c.save).toHaveBeenCalledTimes(1);
    expect(done).toBe(false);
    await c.resolve(true);
    expect(c.save).toHaveBeenCalledTimes(2);
    expect(done).toBe(false);
    await c.resolve(true);
    await f;
    expect(done).toBe(true);
    expect(c.maxActive).toBe(1);
    expect(a.state).toBe('saved');
  });

  it('enters error after a failed save and recovers on the next successful save', async () => {
    let ok = false;
    const save = vi.fn(async () => ok);
    const a = createAutosave(save, 100);
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    expect(a.state).toBe('error');
    ok = true;
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    expect(save).toHaveBeenCalledTimes(2);
    expect(a.state).toBe('saved');
  });

  it('treats a thrown save as a failure and retries the unsaved changes on flush', async () => {
    let fail = true;
    const save = vi.fn(async () => {
      if (fail) throw new Error('network');
      return true;
    });
    const a = createAutosave(save, 100);
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    expect(a.state).toBe('error');
    fail = false;
    await a.flush();
    expect(save).toHaveBeenCalledTimes(2);
    expect(a.state).toBe('saved');
  });

  it('notifies state changes to the optional listener', async () => {
    const seen: string[] = [];
    const a = createAutosave(async () => true, 100, (s) => seen.push(s));
    a.touch();
    await vi.advanceTimersByTimeAsync(100);
    expect(seen).toEqual(['pending', 'saving', 'saved']);
  });

  it('stop() cancels a pending save and ignores later touches', async () => {
    const save = vi.fn(async () => true);
    const a = createAutosave(save, 100);
    a.touch();
    a.stop();
    a.touch();
    await vi.advanceTimersByTimeAsync(1000);
    await a.flush();
    expect(save).not.toHaveBeenCalled();
  });
});
