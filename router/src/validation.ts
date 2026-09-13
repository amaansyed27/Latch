import type {
  DeviceMessage,
  DeviceResponseMessage,
  HelloMessage,
  LatchRequestEnvelope,
} from './types.js';

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function isUuid(value: string): boolean {
  return UUID_PATTERN.test(value);
}

export function parseDeviceMessage(raw: string): DeviceMessage | null {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!isRecord(value) || typeof value.type !== 'string') {
    return null;
  }

  if (value.type === 'hello') {
    if (
      typeof value.device_id !== 'string' ||
      !isUuid(value.device_id) ||
      typeof value.device_name !== 'string' ||
      value.device_name.trim().length === 0 ||
      value.device_name.length > 128 ||
      (typeof value.pairing_token !== 'string' && typeof value.device_credential !== 'string')
    ) {
      return null;
    }
    return value as unknown as HelloMessage;
  }

  if (value.type === 'response') {
    if (
      typeof value.request_id !== 'string' ||
      !isUuid(value.request_id) ||
      !isLatchResponseEnvelope(value.response)
    ) {
      return null;
    }
    return value as unknown as DeviceResponseMessage;
  }

  return null;
}

export function isLatchRequestEnvelope(
  value: unknown,
): value is LatchRequestEnvelope {
  if (!isRecord(value)) {
    return false;
  }
  return (
    typeof value.id === 'string' &&
    value.id.length > 0 &&
    value.id.length <= 128 &&
    Number.isInteger(value.version) &&
    typeof value.method === 'string' &&
    value.method.length > 0 &&
    isRecord(value.params)
  );
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function isLatchResponseEnvelope(value: unknown): boolean {
  if (!isRecord(value)) {
    return false;
  }
  if (value.id !== null && typeof value.id !== 'string') {
    return false;
  }
  if (!Number.isInteger(value.version) || (value.status !== 'ok' && value.status !== 'error')) {
    return false;
  }
  return value.status === 'ok' ? isRecord(value.result) : isRecord(value.error);
}
