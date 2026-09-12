import { createClient } from '@redis/client';

import type { DispatchHandler, RelayCoordinator } from './coordinator.js';
import type {
  DevicePresence,
  DispatchMessage,
  RelayCompletion,
} from './types.js';

type RedisClient = ReturnType<typeof createClient>;

interface PendingResponse {
  resolve: (completion: RelayCompletion) => void;
  timer: NodeJS.Timeout;
}

const PREFIX = 'latch:v02:';

export class RedisCoordinator implements RelayCoordinator {
  readonly #command: RedisClient;
  readonly #dispatchSubscriber: RedisClient;
  readonly #responseSubscriber: RedisClient;
  readonly #pending = new Map<string, PendingResponse>();
  #startPromise: Promise<void> | null = null;

  constructor(redisUrl: string) {
    this.#command = createClient({ url: redisUrl });
    this.#dispatchSubscriber = this.#command.duplicate();
    this.#responseSubscriber = this.#command.duplicate();
  }

  async start(): Promise<void> {
    this.#startPromise ??= this.#start();
    await this.#startPromise;
  }

  async stop(): Promise<void> {
    for (const requestId of [...this.#pending.keys()]) {
      this.#complete(requestId, {
        kind: 'error',
        error: {
          code: 'coordinator_stopped',
          message: 'router coordination stopped before the request completed',
        },
      });
    }
    await Promise.all([
      closeClient(this.#responseSubscriber),
      closeClient(this.#dispatchSubscriber),
      closeClient(this.#command),
    ]);
    this.#startPromise = null;
  }

  async registerDevice(
    device: DevicePresence,
    ttlSeconds: number,
  ): Promise<void> {
    await this.start();
    await this.#command.set(deviceKey(device.device_id), JSON.stringify(device), {
      EX: ttlSeconds,
    });
  }

  async refreshDevice(
    deviceId: string,
    instanceId: string,
    connectionId: string,
    ttlSeconds: number,
  ): Promise<void> {
    await this.start();
    const key = deviceKey(deviceId);
    const raw = await this.#command.get(key);
    const device = parsePresence(raw);
    if (
      device?.instance_id === instanceId &&
      device.connection_id === connectionId
    ) {
      await this.#command.set(key, JSON.stringify(device), { EX: ttlSeconds });
    }
  }

  async removeDevice(
    deviceId: string,
    instanceId: string,
    connectionId: string,
  ): Promise<void> {
    await this.start();
    const key = deviceKey(deviceId);
    const device = parsePresence(await this.#command.get(key));
    if (
      device?.instance_id === instanceId &&
      device.connection_id === connectionId
    ) {
      await this.#command.del(key);
    }
  }

  async getDevice(deviceId: string): Promise<DevicePresence | null> {
    await this.start();
    return parsePresence(await this.#command.get(deviceKey(deviceId)));
  }

  async listDevices(): Promise<DevicePresence[]> {
    await this.start();
    const keys = await this.#command.keys(`${PREFIX}device:*`);
    if (keys.length === 0) {
      return [];
    }
    const values = await this.#command.mGet(keys);
    return values
      .map(parsePresence)
      .filter((device): device is DevicePresence => device !== null)
      .sort((left, right) => left.device_name.localeCompare(right.device_name));
  }

  async subscribeDispatch(
    instanceId: string,
    handler: DispatchHandler,
  ): Promise<() => Promise<void>> {
    await this.start();
    const channel = dispatchChannel(instanceId);
    await this.#dispatchSubscriber.subscribe(channel, (raw) => {
      const message = parseDispatch(raw);
      if (message !== null) {
        void handler(message).catch(() => undefined);
      }
    });
    return async () => {
      if (this.#dispatchSubscriber.isOpen) {
        await this.#dispatchSubscriber.unsubscribe(channel);
      }
    };
  }

  async request(
    instanceId: string,
    message: DispatchMessage,
    timeoutMs: number,
  ): Promise<RelayCompletion> {
    await this.start();
    const response = new Promise<RelayCompletion>((resolve) => {
      const timer = setTimeout(() => {
        this.#complete(message.request_id, {
          kind: 'error',
          error: {
            code: 'request_timeout',
            message: 'the connected device did not answer before the request timeout',
          },
        });
      }, timeoutMs);
      this.#pending.set(message.request_id, { resolve, timer });
    });

    try {
      const subscribers = await this.#command.publish(
        dispatchChannel(instanceId),
        JSON.stringify(message),
      );
      if (subscribers === 0) {
        this.#complete(message.request_id, {
          kind: 'error',
          error: {
            code: 'device_offline',
            message: 'the device connection is no longer available',
          },
        });
      }
    } catch (error) {
      this.#complete(message.request_id, {
        kind: 'error',
        error: {
          code: 'relay_failed',
          message: 'the router could not deliver the request to the device',
        },
      });
      console.warn('redis dispatch publish failed', {
        error_name: error instanceof Error ? error.name : 'unknown',
      });
    }

    return response;
  }

  async respond(requestId: string, completion: RelayCompletion): Promise<void> {
    await this.start();
    await this.#command.publish(responseChannel(requestId), JSON.stringify(completion));
  }

  async #start(): Promise<void> {
    await Promise.all([
      this.#command.connect(),
      this.#dispatchSubscriber.connect(),
      this.#responseSubscriber.connect(),
    ]);
    await this.#responseSubscriber.pSubscribe(
      `${PREFIX}response:*`,
      (raw, channel) => {
        const requestId = channel.slice(`${PREFIX}response:`.length);
        const completion = parseCompletion(raw);
        if (completion !== null) {
          this.#complete(requestId, completion);
        }
      },
    );
  }

  #complete(requestId: string, completion: RelayCompletion): void {
    const pending = this.#pending.get(requestId);
    if (pending === undefined) {
      return;
    }
    clearTimeout(pending.timer);
    this.#pending.delete(requestId);
    pending.resolve(completion);
  }
}

async function closeClient(client: RedisClient): Promise<void> {
  if (client.isOpen) {
    await client.quit();
  }
}

function deviceKey(deviceId: string): string {
  return `${PREFIX}device:${deviceId}`;
}

function dispatchChannel(instanceId: string): string {
  return `${PREFIX}dispatch:${instanceId}`;
}

function responseChannel(requestId: string): string {
  return `${PREFIX}response:${requestId}`;
}

function parsePresence(raw: string | null): DevicePresence | null {
  if (raw === null) {
    return null;
  }
  try {
    const value: unknown = JSON.parse(raw);
    if (!isRecord(value)) {
      return null;
    }
    if (
      typeof value.device_id !== 'string' ||
      typeof value.device_name !== 'string' ||
      value.status !== 'online' ||
      typeof value.connected_at !== 'string' ||
      typeof value.instance_id !== 'string' ||
      typeof value.connection_id !== 'string'
    ) {
      return null;
    }
    return value as unknown as DevicePresence;
  } catch {
    return null;
  }
}

function parseDispatch(raw: string): DispatchMessage | null {
  try {
    const value: unknown = JSON.parse(raw);
    if (!isRecord(value)) {
      return null;
    }
    if (
      typeof value.request_id !== 'string' ||
      typeof value.device_id !== 'string' ||
      !isRecord(value.request)
    ) {
      return null;
    }
    return value as unknown as DispatchMessage;
  } catch {
    return null;
  }
}

function parseCompletion(raw: string): RelayCompletion | null {
  try {
    const value: unknown = JSON.parse(raw);
    if (!isRecord(value) || (value.kind !== 'response' && value.kind !== 'error')) {
      return null;
    }
    return value as unknown as RelayCompletion;
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
