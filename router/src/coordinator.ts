import type {
  DevicePresence,
  DispatchMessage,
  RelayCompletion,
} from './types.js';

export type DispatchHandler = (message: DispatchMessage) => Promise<void>;

export interface RelayCoordinator {
  start(): Promise<void>;
  stop(): Promise<void>;
  registerDevice(device: DevicePresence, ttlSeconds: number): Promise<void>;
  refreshDevice(
    deviceId: string,
    instanceId: string,
    connectionId: string,
    ttlSeconds: number,
  ): Promise<void>;
  removeDevice(
    deviceId: string,
    instanceId: string,
    connectionId: string,
  ): Promise<void>;
  getDevice(deviceId: string): Promise<DevicePresence | null>;
  listDevices(): Promise<DevicePresence[]>;
  subscribeDispatch(
    instanceId: string,
    handler: DispatchHandler,
  ): Promise<() => Promise<void>>;
  request(
    instanceId: string,
    message: DispatchMessage,
    timeoutMs: number,
  ): Promise<RelayCompletion>;
  respond(requestId: string, completion: RelayCompletion): Promise<void>;
}
