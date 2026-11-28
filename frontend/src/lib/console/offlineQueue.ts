import { useSyncExternalStore } from 'react';
import type { z } from 'zod';
import type { serverPowerAction } from '@/lib/schemas/server/server.ts';
import type { Websocket } from '@/plugins/websocket/Websocket.ts';
import { SocketRequest } from '@/plugins/websocket/useWebsocketEvent.ts';

/**
 * Client-side offline queue for console actions (Feature 10: Mobile console
 * + offline queue).
 *
 * When the server websocket drops, a power action (start/stop/restart) or a
 * console command the user typed is queued instead of being silently lost.
 * The queue is memory-only on purpose: it is never persisted to localStorage,
 * so a page reload can never replay stale actions against a server whose
 * state has since changed. It is capped, and it is cleared whenever the
 * viewed server changes or the server page unmounts.
 *
 * Power actions coalesce: only the latest pending power action is kept, so
 * tapping Start then Stop while offline replays just Stop. Commands are
 * kept in the order they were typed.
 */

export const OFFLINE_QUEUE_MAX_ITEMS = 10;

export type OfflineQueuePowerAction = z.infer<typeof serverPowerAction>;

export type OfflineQueuedItem =
  | { id: string; kind: 'power'; action: OfflineQueuePowerAction; queuedAt: number }
  | { id: string; kind: 'command'; command: string; queuedAt: number };

export type OfflineQueueNewItem =
  | { kind: 'power'; action: OfflineQueuePowerAction }
  | { kind: 'command'; command: string };

export type OfflineQueueSendResult = 'sent' | 'queued';

let nextQueueId = 0;

function newQueueId(): string {
  nextQueueId += 1;
  return `offline-${Date.now().toString(36)}-${nextQueueId}`;
}

export class OfflineActionQueue {
  private items: OfflineQueuedItem[] = [];
  private listeners = new Set<() => void>();
  private serverUuid: string | null = null;

  /** The server this queue belongs to. Switching server clears the queue. */
  bindServer(uuid: string | null): void {
    if (uuid === this.serverUuid) return;
    this.serverUuid = uuid;
    if (this.items.length > 0) {
      this.items = [];
      this.emit();
    }
  }

  get snapshot(): OfflineQueuedItem[] {
    return this.items;
  }

  get size(): number {
    return this.items.length;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getSnapshot = (): OfflineQueuedItem[] => this.items;

  private emit(): void {
    for (const listener of [...this.listeners]) {
      listener();
    }
  }

  /**
   * Adds an item. A pending power action is replaced in place by a newer
   * one (last power action wins). When the queue is full, the oldest item
   * is dropped and returned so the caller can tell the user.
   */
  enqueue(item: OfflineQueueNewItem): { item: OfflineQueuedItem; dropped: OfflineQueuedItem | null } {
    const queued: OfflineQueuedItem = { ...item, id: newQueueId(), queuedAt: Date.now() } as OfflineQueuedItem;

    let next = [...this.items];
    let dropped: OfflineQueuedItem | null = null;

    if (queued.kind === 'power') {
      const existingIndex = next.findIndex((entry) => entry.kind === 'power');
      if (existingIndex >= 0) {
        next[existingIndex] = queued;
        this.items = next;
        this.emit();
        return { item: queued, dropped };
      }
    }

    if (next.length >= OFFLINE_QUEUE_MAX_ITEMS) {
      dropped = next[0];
      next = next.slice(1);
    }
    next.push(queued);
    this.items = next;
    this.emit();
    return { item: queued, dropped };
  }

  remove(id: string): void {
    if (!this.items.some((entry) => entry.id === id)) return;
    this.items = this.items.filter((entry) => entry.id !== id);
    this.emit();
  }

  clear(): void {
    if (this.items.length === 0) return;
    this.items = [];
    this.emit();
  }

  /**
   * Sends every queued item, in order, through `send`, then clears the
   * queue. Returns the items that were replayed. A send that throws for
   * one item does not stop the rest.
   */
  replay(send: (item: OfflineQueuedItem) => void): OfflineQueuedItem[] {
    const items = [...this.items];
    this.items = [];
    if (items.length > 0) {
      this.emit();
    }
    for (const item of items) {
      try {
        send(item);
      } catch (error) {
        console.warn('Failed to replay queued console action:', item, error);
      }
    }
    return items;
  }
}

/** Module-level, memory-only queue shared by the console page components. */
export const offlineActionQueue = new OfflineActionQueue();

/** React subscription to the queue; re-renders when items are added/replayed/cleared. */
export function useOfflineQueue(): OfflineQueuedItem[] {
  return useSyncExternalStore(offlineActionQueue.subscribe, offlineActionQueue.getSnapshot);
}

export function describeQueuedItem(item: OfflineQueuedItem): string {
  return item.kind === 'power' ? item.action : item.command;
}

/**
 * Sends a power action now when the socket is up, otherwise queues it.
 * Note: `Websocket.send` silently drops frames when not connected, so the
 * connected check here is what makes queueing reliable.
 */
export function sendOrQueuePowerAction(
  socket: Websocket | null,
  socketConnected: boolean,
  action: OfflineQueuePowerAction,
): OfflineQueueSendResult {
  if (socketConnected && socket) {
    socket.send(SocketRequest.SET_STATE, action);
    return 'sent';
  }
  offlineActionQueue.enqueue({ kind: 'power', action });
  return 'queued';
}

/** Sends a console command now when the socket is up, otherwise queues it. */
export function sendOrQueueCommand(
  socket: Websocket | null,
  socketConnected: boolean,
  command: string,
): OfflineQueueSendResult {
  if (socketConnected && socket) {
    socket.send(SocketRequest.SEND_COMMAND, command);
    return 'sent';
  }
  offlineActionQueue.enqueue({ kind: 'command', command });
  return 'queued';
}

/**
 * Replays the queue over a (re)connected socket, in order. Call this from
 * the websocket `auth success` handler, after the connection state is set
 * to connected. Returns the replayed items (empty when nothing was queued).
 */
export function replayOfflineQueue(socket: Websocket | null): OfflineQueuedItem[] {
  if (!socket) return [];
  return offlineActionQueue.replay((item) => {
    if (item.kind === 'power') {
      socket.send(SocketRequest.SET_STATE, item.action);
    } else {
      socket.send(SocketRequest.SEND_COMMAND, item.command);
    }
  });
}
