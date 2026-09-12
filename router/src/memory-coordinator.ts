import type { DispatchHandler, RelayCoordinator } from './coordinator.js';
import type {
  DevicePresence,
  DispatchMessage,
  RelayCompletion,
} from './types.js';

interface PendingResponse {
  resolve: (completion: RelayCompletion) => void;
  timer: NodeJS.Timeout;
}

export class MemoryCoordinator implements RelayCoordinator {
  readonly #devices = new Map<string, DevicePresence>();
  readonly #dispatch = new Map<string, DispatchHandler>();
  readonly #pending = new Map<string, PendingResponse>();

  async start(): Promise<void> {}

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
    this.#dispatch.clear();
    this.#devices.clear();
  }

  async registerDevice(device: DevicePresence): Promise<void> {
    this.#devices.set(device.device_id, device);
  }

  async refreshDevice(
    deviceId: string,
    instanceId: string,
    connectionId: string,
  ): Promise<void> {
    const device = this.#devices.get(deviceId);
    if (
      device?.instance_id === instanceId &&
      device.connection_id === connectionId
    ) {
      this.#devices.set(deviceId, device);
    }
  }

  async removeDevice(
    deviceId: string,
    instanceId: string,
    connectionId: string,
  ): Promise<void> {
    const device = this.#devices.get(deviceId);
    if (
      device?.instance_id === instanceId &&
      device.connection_id === connectionId
    ) {
      this.#devices.delete(deviceId);
    }
  }

  async getDevice(deviceId: string): Promise<DevicePresence | null> {
    return this.#devices.get(deviceId) ?? null;
  }

  async listDevices(): Promise<DevicePresence[]> {
    return [...this.#devices.values()].sort((left, right) =>
      left.device_name.localeCompare(right.device_name),
    );
  }

  async subscribeDispatch(
    instanceId: string,
    handler: DispatchHandler,
  ): Promise<() => Promise<void>> {
    this.#dispatch.set(instanceId, handler);
    return async () => {
      if (this.#dispatch.get(instanceId) === handler) {
        this.#dispatch.delete(instanceId);
      }
    };
  }

  async request(
    instanceId: string,
    message: DispatchMessage,
    timeoutMs: number,
  ): Promise<RelayCompletion> {
    return new Promise<RelayCompletion>((resolve) => {
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

      const handler = this.#dispatch.get(instanceId);
      if (handler === undefined) {
        this.#complete(message.request_id, {
          kind: 'error',
          error: {
            code: 'device_offline',
            message: 'the device connection is no longer available',
          },
        });
        return;
      }

      void handler(message).catch(() => {
        this.#complete(message.request_id, {
          kind: 'error',
          error: {
            code: 'relay_failed',
            message: 'the router could not deliver the request to the device',
          },
        });
      });
    });
  }

  async respond(requestId: string, completion: RelayCompletion): Promise<void> {
    this.#complete(requestId, completion);
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
