import type { DebugEvent } from "../types";

const MAX_EVENTS = 500;

let seq = 0;

class DebugStore {
  events = $state<(DebugEvent & { id: number })[]>([]);

  push(e: DebugEvent) {
    this.events = [{ ...e, id: seq++ }, ...this.events].slice(0, MAX_EVENTS);
  }

  clear() {
    this.events = [];
    seq = 0;
  }
}

export const debug = new DebugStore();