export type DeviceStatus = 'online';

export interface DevicePresence {
  device_id: string;
  device_name: string;
  status: DeviceStatus;
  connected_at: string;
  instance_id: string;
  connection_id: string;
  owner_user_id?: string;
}

export interface PublicDevice {
  device_id: string;
  device_name: string;
  status: DeviceStatus;
  connected_at: string;
}

export interface LatchRequestEnvelope {
  id: string;
  version: number;
  method: string;
  params: unknown;
}

export interface DispatchMessage {
  request_id: string;
  device_id: string;
  request: LatchRequestEnvelope;
}

export interface RelayError {
  code: string;
  message: string;
}

export type RelayCompletion =
  | { kind: 'response'; response: unknown }
  | { kind: 'error'; error: RelayError };

export interface HelloMessage {
  type: 'hello';
  device_id: string;
  device_name: string;
  pairing_token?: string;
  device_credential?: string;
}

export interface DeviceResponseMessage {
  type: 'response';
  request_id: string;
  response: unknown;
}

export type DeviceMessage = HelloMessage | DeviceResponseMessage;

export type RouterMessage =
  | { type: 'welcome'; device_id: string }
  | { type: 'request'; request_id: string; request: LatchRequestEnvelope }
  | { type: 'error'; code: string; message: string };
