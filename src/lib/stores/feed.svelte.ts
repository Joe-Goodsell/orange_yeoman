import type { BackendEvent, BackendEventType } from "../types";

const MAX_EVENTS = 1000;

class FeedStore {
  // Newest first. Events with the same id replace the existing card in place.
  events = $state<BackendEvent[]>([]);
  // How many events the 1000-event cap has dropped. The feed shows this count
  // so a full buffer is never mistaken for a quiet backend.
  dropped = $state(0);
  expandedIds = $state<Set<string>>(new Set());
  // Empty set means "all types shown".
  activeTypes = $state<Set<BackendEventType>>(new Set());

  visible = $derived(
    this.activeTypes.size === 0
      ? this.events
      : this.events.filter((e) => this.activeTypes.has(e.eventType)),
  );

  // Add a new event card or update an existing one (same id). New cards are
  // prepended; the oldest card (the tail) is dropped when the cap is exceeded.
  upsert(e: BackendEvent) {
    const idx = this.events.findIndex((ev) => ev.id === e.id);
    if (idx >= 0) {
      const next = [...this.events];
      next[idx] = e;
      this.events = next;
      return;
    }
    const next = [e, ...this.events];
    if (next.length > MAX_EVENTS) {
      this.dropped += next.length - MAX_EVENTS;
      this.events = next.slice(0, MAX_EVENTS);
    } else {
      this.events = next;
    }
  }

  clear() {
    this.events = [];
    this.dropped = 0;
    this.expandedIds = new Set();
  }

  toggleExpanded(id: string) {
    const next = new Set(this.expandedIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    this.expandedIds = next;
  }

  collapseAll() {
    this.expandedIds = new Set();
  }

  toggleTypeFilter(t: BackendEventType) {
    const next = new Set(this.activeTypes);
    if (next.has(t)) {
      next.delete(t);
    } else {
      next.add(t);
    }
    this.activeTypes = next;
  }

  showAllTypes() {
    this.activeTypes = new Set();
  }

  typeCount(t: BackendEventType): number {
    let n = 0;
    for (const e of this.events) {
      if (e.eventType === t) n += 1;
    }
    return n;
  }
}

export const feed = new FeedStore();