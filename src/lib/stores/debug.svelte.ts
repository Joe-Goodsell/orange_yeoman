import type { DebugEvent } from "../types";

const MAX_EVENTS = 500;

let seq = 0;

class DebugStore {
  events = $state<(DebugEvent & { id: number })[]>([]);
  // How many older events the 500-event cap has dropped. The console shows
  // this count so a full buffer is never mistaken for a quiet backend.
  dropped = $state(0);

  push(e: DebugEvent) {
    if (this.events.length >= MAX_EVENTS) this.dropped += 1;
    this.events = [{ ...e, id: seq++ }, ...this.events].slice(0, MAX_EVENTS);
  }

  clear() {
    this.events = [];
    this.dropped = 0;
    seq = 0;
  }
}

export const debug = new DebugStore();