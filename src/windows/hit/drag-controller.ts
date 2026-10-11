export interface Point { x: number; y: number }

interface DragApi {
  start(point: Point): Promise<number>;
  move(id: number): Promise<void>;
  end(id: number, point: Point | null, cancelled: boolean): Promise<void>;
  cursor(): Promise<Point>;
  frame(callback: () => void): number;
  cancelFrame(id: number): void;
  error(error: unknown): void;
}

interface Gesture {
  ready: Promise<number>;
  inFlight: Promise<void>;
  frame: number | null;
  busy: boolean;
  dirty: boolean;
  closing: boolean;
}

// Only one move can be in flight. Capture the release position immediately,
// then finish after the last move, even when start/move IPC is delayed.
export function createDragController(api: DragApi) {
  let current: Gesture | null = null;
  let tail = Promise.resolve();

  function schedule(g: Gesture) {
    if (g.closing || g.busy || g.frame !== null) return;
    g.frame = api.frame(() => {
      g.frame = null;
      g.dirty = false;
      g.busy = true;
      g.inFlight = (async () => {
        try {
          const id = await g.ready;
          if (!g.closing) await api.move(id);
        } catch (error) {
          api.error(error);
        } finally {
          g.busy = false;
          if (g.dirty) schedule(g);
        }
      })();
    });
  }

  function end(cancelled = false): Promise<void> {
    const g = current;
    if (!g || g.closing) return tail;
    g.closing = true;
    current = null;
    if (g.frame !== null) api.cancelFrame(g.frame);
    // Register the rejection handler immediately; a failed sample cancels snap.
    const point = cancelled ? Promise.resolve(null) : api.cursor().catch(error => {
      api.error(error);
      return null;
    });
    tail = (async () => {
      const id = await g.ready;
      const release = await point;
      await g.inFlight;
      await api.end(id, release, cancelled || release === null);
    })().catch(api.error);
    return tail;
  }

  return {
    start() {
      if (current) void end(true);
      // Sample now, before waiting for the previous gesture's IPC. Otherwise
      // a rapid move/release could become the new gesture's starting position.
      const point = api.cursor().catch(error => {
        api.error(error);
        return null;
      });
      const ready = tail.then(async () => {
        const start = await point;
        if (!start) throw new Error('Could not sample drag start position');
        return api.start(start);
      });
      // Prevent an unhandled rejection if start fails before the next event.
      void ready.catch(api.error);
      current = { ready, inFlight: Promise.resolve(), frame: null,
        busy: false, dirty: false, closing: false };
    },
    move() {
      if (!current || current.closing) return;
      current.dirty = true;
      schedule(current);
    },
    end,
  };
}
