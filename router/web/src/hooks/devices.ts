import { useCallback, useEffect, useState } from "react";
import { ApiError, message, request, type Device } from "../lib/api";

export function useDevices() {
  const [devices, setDevices] = useState<Device[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const refresh = useCallback(async () => {
    try {
      const result = await request<{ devices: Device[] }>("/api/my/devices");
      setDevices(result.devices);
      setError("");
      return result.devices;
    } catch (e) {
      if (e instanceof ApiError && e.status === 401)
        window.location.assign(
          `/login?return_to=${encodeURIComponent(location.pathname)}`,
        );
      setError(message(e));
      return null;
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => {
    void refresh();
    const interval = setInterval(() => {
      if (!document.hidden) void refresh();
    }, 15000);
    return () => clearInterval(interval);
  }, [refresh]);
  return { devices, loading, error, refresh };
}
