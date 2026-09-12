import { randomUUID } from 'node:crypto';
import type { IncomingMessage, Server as HttpServer } from 'node:http';
import type { Duplex } from 'node:stream';

import WebSocket, { WebSocketServer, type RawData } from 'ws';

import { safeTokenEqual } from './auth.js';
import type { RelayCoordinator } from './coordinator.js';
import type { RouterConfig } from './config.js';
import type {
  DevicePresence,
  DispatchMessage,
  RelayCompletion,
  RouterMessage,
} from './types.js';
import { parseDeviceMessage } from './validation.js';

interface DeviceSession {
  ws: WebSocket;
  presence: DevicePresence;
  alive: boolean;
  heartbeat: NodeJS.Timeout;
}

interface PendingDeviceRequest {
  deviceId: string;
  connectionId: string;
  timer: NodeJS.Timeout;
}

export class LinkServer {
  readonly #wss = new WebSocketServer({ noServer: true });
  readonly #sessions = new Map<string, DeviceSession>();
  readonly #pending = new Map<string, PendingDeviceRequest>();
  readonly #ready: Promise<void>;
  #unsubscribeDispatch: (() => Promise<void>) | null = null;

  constructor(
    private readonly config: RouterConfig,
    private readonly coordinator: RelayCoordinator,
    private readonly instanceId: string = randomUUID(),
  ) {
    this.#wss.on('connection', (ws) => this.#accept(ws));
    this.#ready = this.#start();
  }

  get ready(): Promise<void> {
    return this.#ready;
  }

  attach(server: HttpServer): void {
    server.on('upgrade', (request, socket, head) => {
      this.#upgrade(request, socket, head);
    });
  }

  async close(): Promise<void> {
    for (const session of this.#sessions.values()) {
      clearInterval(session.heartbeat);
      session.ws.terminate();
    }
    const removals = [...this.#sessions.values()].map((session) =>
      this.coordinator.removeDevice(
        session.presence.device_id,
        this.instanceId,
        session.presence.connection_id,
      ),
    );
    this.#sessions.clear();
    await Promise.allSettled(removals);

    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
    }
    this.#pending.clear();

    if (this.#unsubscribeDispatch !== null) {
      await this.#unsubscribeDispatch();
      this.#unsubscribeDispatch = null;
    }
    await this.coordinator.stop();
    await new Promise<void>((resolve) => this.#wss.close(() => resolve()));
  }

  async #start(): Promise<void> {
    await this.coordinator.start();
    this.#unsubscribeDispatch = await this.coordinator.subscribeDispatch(
      this.instanceId,
      (message) => this.#dispatch(message),
    );
  }

  #upgrade(request: IncomingMessage, socket: Duplex, head: Buffer): void {
    this.#wss.handleUpgrade(request, socket, head, (ws) => {
      this.#wss.emit('connection', ws, request);
    });
  }

  #accept(ws: WebSocket): void {
    const authTimer = setTimeout(() => {
      sendError(ws, 'authentication_timeout', 'device authentication timed out');
      ws.close(1008, 'authentication required');
    }, 10_000);

    ws.once('message', (data, isBinary) => {
      clearTimeout(authTimer);
      void this.#authenticate(ws, data, isBinary);
    });
  }

  async #authenticate(
    ws: WebSocket,
    data: RawData,
    isBinary: boolean,
  ): Promise<void> {
    try {
      await this.#ready;
      if (isBinary) {
        sendError(ws, 'invalid_message', 'expected a JSON authentication message');
        ws.close(1008, 'invalid authentication');
        return;
      }

      const message = parseDeviceMessage(data.toString());
      if (message?.type !== 'hello') {
        sendError(ws, 'invalid_message', 'expected a hello authentication message');
        ws.close(1008, 'invalid authentication');
        return;
      }
      if (!safeTokenEqual(message.pairing_token, this.config.pairingToken)) {
        sendError(ws, 'unauthorized', 'device authentication failed');
        ws.close(1008, 'unauthorized');
        return;
      }

      const connectionId = randomUUID();
      const presence: DevicePresence = {
        device_id: message.device_id,
        device_name: message.device_name.trim(),
        status: 'online',
        connected_at: new Date().toISOString(),
        instance_id: this.instanceId,
        connection_id: connectionId,
      };

      const previous = this.#sessions.get(presence.device_id);
      if (previous !== undefined) {
        previous.ws.close(1012, 'device reconnected');
      }

      const session: DeviceSession = {
        ws,
        presence,
        alive: true,
        heartbeat: setInterval(() => {
          void this.#heartbeat(presence.device_id, connectionId);
        }, this.config.heartbeatMs),
      };
      this.#sessions.set(presence.device_id, session);
      await this.coordinator.registerDevice(
        presence,
        this.config.presenceTtlSeconds,
      );

      ws.on('pong', () => {
        const current = this.#sessions.get(presence.device_id);
        if (current?.presence.connection_id === connectionId) {
          current.alive = true;
        }
      });
      ws.on('message', (nextData, nextIsBinary) => {
        void this.#onAuthenticatedMessage(
          presence.device_id,
          connectionId,
          nextData,
          nextIsBinary,
        );
      });
      ws.on('close', () => {
        void this.#disconnect(presence.device_id, connectionId);
      });
      ws.on('error', () => {
        console.warn('device websocket error', { device_id: presence.device_id });
      });

      send(ws, { type: 'welcome', device_id: presence.device_id });
      console.info('device connected', {
        device_id: presence.device_id,
        device_name: presence.device_name,
      });
    } catch (error) {
      console.error('device registration failed', {
        error_name: error instanceof Error ? error.name : 'unknown',
      });
      sendError(ws, 'router_unavailable', 'router relay is unavailable');
      ws.close(1011, 'router unavailable');
    }
  }

  async #heartbeat(deviceId: string, connectionId: string): Promise<void> {
    const session = this.#sessions.get(deviceId);
    if (
      session === undefined ||
      session.presence.connection_id !== connectionId
    ) {
      return;
    }
    if (!session.alive) {
      session.ws.terminate();
      return;
    }

    session.alive = false;
    session.ws.ping();
    try {
      await this.coordinator.refreshDevice(
        deviceId,
        this.instanceId,
        connectionId,
        this.config.presenceTtlSeconds,
      );
    } catch {
      console.warn('device presence refresh failed', { device_id: deviceId });
    }
  }

  async #onAuthenticatedMessage(
    deviceId: string,
    connectionId: string,
    data: RawData,
    isBinary: boolean,
  ): Promise<void> {
    const session = this.#sessions.get(deviceId);
    if (
      session === undefined ||
      session.presence.connection_id !== connectionId
    ) {
      return;
    }
    if (isBinary) {
      sendError(session.ws, 'invalid_message', 'expected a JSON link message');
      return;
    }

    const message = parseDeviceMessage(data.toString());
    if (message?.type !== 'response') {
      sendError(session.ws, 'invalid_message', 'expected a response message');
      return;
    }

    const pending = this.#pending.get(message.request_id);
    if (
      pending === undefined ||
      pending.deviceId !== deviceId ||
      pending.connectionId !== connectionId
    ) {
      sendError(session.ws, 'unknown_request', 'request is no longer pending');
      return;
    }

    clearTimeout(pending.timer);
    this.#pending.delete(message.request_id);
    await this.coordinator.respond(message.request_id, {
      kind: 'response',
      response: message.response,
    });
  }

  async #dispatch(message: DispatchMessage): Promise<void> {
    const session = this.#sessions.get(message.device_id);
    if (session === undefined || session.ws.readyState !== WebSocket.OPEN) {
      await this.coordinator.respond(message.request_id, offlineCompletion());
      return;
    }

    const timer = setTimeout(() => {
      this.#pending.delete(message.request_id);
    }, this.config.requestTimeoutMs + 5_000);
    this.#pending.set(message.request_id, {
      deviceId: message.device_id,
      connectionId: session.presence.connection_id,
      timer,
    });

    try {
      await sendAsync(session.ws, {
        type: 'request',
        request_id: message.request_id,
        request: message.request,
      });
    } catch (error) {
      clearTimeout(timer);
      this.#pending.delete(message.request_id);
      console.warn('device request write failed', {
        device_id: message.device_id,
        error_name: error instanceof Error ? error.name : 'unknown',
        error_message: error instanceof Error ? error.message : 'unknown',
      });
      await this.coordinator.respond(message.request_id, {
        kind: 'error',
        error: {
          code: 'relay_failed',
          message: 'the router could not write the request to the device connection',
        },
      });
    }
  }

  async #disconnect(deviceId: string, connectionId: string): Promise<void> {
    const session = this.#sessions.get(deviceId);
    if (
      session === undefined ||
      session.presence.connection_id !== connectionId
    ) {
      return;
    }

    clearInterval(session.heartbeat);
    this.#sessions.delete(deviceId);
    try {
      await this.coordinator.removeDevice(
        deviceId,
        this.instanceId,
        connectionId,
      );
    } catch {
      console.warn('device presence removal failed', { device_id: deviceId });
    }

    const completions: Promise<void>[] = [];
    for (const [requestId, pending] of this.#pending) {
      if (
        pending.deviceId === deviceId &&
        pending.connectionId === connectionId
      ) {
        clearTimeout(pending.timer);
        this.#pending.delete(requestId);
        completions.push(
          this.coordinator.respond(requestId, {
            kind: 'error',
            error: {
              code: 'device_disconnected',
              message: 'the device disconnected while the request was pending',
            },
          }),
        );
      }
    }
    await Promise.allSettled(completions);
    console.info('device disconnected', { device_id: deviceId });
  }
}

function offlineCompletion(): RelayCompletion {
  return {
    kind: 'error',
    error: {
      code: 'device_offline',
      message: 'the device connection is no longer available',
    },
  };
}

function send(ws: WebSocket, message: RouterMessage): void {
  if (ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(message));
  }
}

function sendError(ws: WebSocket, code: string, message: string): void {
  send(ws, { type: 'error', code, message });
}

function sendAsync(ws: WebSocket, message: RouterMessage): Promise<void> {
  return new Promise((resolve, reject) => {
    ws.send(JSON.stringify(message), (error) => {
      if (error === undefined) {
        resolve();
      } else {
        reject(error);
      }
    });
  });
}
