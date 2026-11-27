let tail: Promise<unknown> = Promise.resolve();

/** Runs reply generations one at a time so a long queue never fires a burst of model calls. */
export function enqueueGeneration<T>(task: () => Promise<T>): Promise<T> {
  const run = tail.then(task);
  tail = run.catch(() => undefined);
  return run;
}
