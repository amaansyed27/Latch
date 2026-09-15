import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { bootstrap, request, type User } from "../lib/api";

const Context = createContext<{
  user: User | null;
  loading: boolean;
  error: string;
  refresh(): Promise<void>;
  signOut(): Promise<void>;
}>(null!);
export function SessionProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(bootstrap?.identity ?? null);
  const [loading, setLoading] = useState(!bootstrap);
  const [error, setError] = useState("");
  async function refresh() {
    try {
      const data = await request<{ user?: User } | null>(
        "/api/auth/get-session",
      );
      setUser(data?.user ?? null);
      setError("");
    } catch (e) {
      if (e instanceof Error && "status" in e && e.status === 401)
        setUser(null);
      else setError("Could not check your session. Please reload.");
    } finally {
      setLoading(false);
    }
  }
  async function signOut() {
    await request("/api/auth/sign-out", "POST", {});
    setUser(null);
    window.location.assign("/");
  }
  useEffect(() => {
    if (!bootstrap) void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);
  return (
    <Context.Provider value={{ user, loading, error, refresh, signOut }}>
      {children}
    </Context.Provider>
  );
}
export const useSession = () => useContext(Context);
