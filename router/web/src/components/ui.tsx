import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import * as Dialog from "@radix-ui/react-dialog";
import * as Tooltip from "@radix-ui/react-tooltip";
import {
  Check,
  Copy,
  Info,
  LoaderCircle,
  Monitor,
  Moon,
  Sun,
  X,
} from "lucide-react";

export function Spinner() {
  return <LoaderCircle className="spin" aria-hidden="true" size={18} />;
}
export function Hint({
  text,
  children,
}: {
  text: string;
  children?: ReactNode;
}) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        {children || (
          <button type="button" className="icon-button help" aria-label={text}>
            <Info size={15} />
          </button>
        )}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content className="tooltip" sideOffset={8}>
          {text}
          <Tooltip.Arrow />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
export function Modal({
  open,
  onClose,
  title,
  description,
  children,
}: {
  open: boolean;
  onClose(): void;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={(value) => !value && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="overlay" />
        <Dialog.Content className="modal">
          <Dialog.Title>{title}</Dialog.Title>
          <Dialog.Description>{description}</Dialog.Description>
          <Dialog.Close className="icon-button close" aria-label="Close dialog">
            <X size={20} />
          </Dialog.Close>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
const ToastContext = createContext<(message: string) => void>(() => {});
export function Feedback({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState("");
  useEffect(() => {
    if (toast) {
      const t = setTimeout(() => setToast(""), 4500);
      return () => clearTimeout(t);
    }
  }, [toast]);
  return (
    <Tooltip.Provider delayDuration={250}>
      <ToastContext.Provider value={setToast}>
        {children}
        <div className={`toast ${toast ? "visible" : ""}`} role="status">
          {toast && (
            <>
              <Check size={18} />
              {toast}
              <button
                className="icon-button"
                onClick={() => setToast("")}
                aria-label="Dismiss notification"
              >
                <X size={16} />
              </button>
            </>
          )}
        </div>
      </ToastContext.Provider>
    </Tooltip.Provider>
  );
}
export const useToast = () => useContext(ToastContext);
export function CopyButton({
  value,
  label = "Copy",
}: {
  value: string;
  label?: string;
}) {
  const [copied, setCopied] = useState(false);
  const toast = useToast();
  return (
    <button
      className="button compact"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          toast(`${label} copied to clipboard`);
          setTimeout(() => setCopied(false), 2500);
        } catch {
          toast("Copy unavailable. Select the text and copy it manually.");
        }
      }}
    >
      {copied ? <Check size={16} /> : <Copy size={16} />}{" "}
      {copied ? "Copied" : label}
    </button>
  );
}
export function ThemePicker() {
  const [theme, setTheme] = useState(() => {
    try {
      return localStorage.getItem("latch-theme") || "system";
    } catch {
      return "system";
    }
  });
  useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.dataset.theme =
        theme === "system" ? (media.matches ? "dark" : "light") : theme;
    };
    apply();
    try {
      localStorage.setItem("latch-theme", theme);
    } catch {
      /* Preference remains usable for this visit. */
    }
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
  return (
    <div className="theme-picker" role="group" aria-label="Color theme">
      {[
        ["system", Monitor],
        ["light", Sun],
        ["dark", Moon],
      ].map(([value, Icon]) => {
        const I = Icon as typeof Monitor;
        return (
          <Hint key={String(value)} text={`${value} theme`}>
            <button
              className="icon-button"
              aria-label={`${value} theme`}
              aria-pressed={theme === value}
              onClick={() => setTheme(String(value))}
            >
              <I size={16} />
            </button>
          </Hint>
        );
      })}
    </div>
  );
}
export function Status({ online }: { online: boolean }) {
  return (
    <Hint
      text={
        online
          ? "This computer currently has an active encrypted connection to Latch."
          : "Latch is not currently connected. Start Latch on this computer to reconnect."
      }
    >
      <button className={`status ${online ? "online" : ""}`}>
        <span className="dot" />
        {online ? "Online" : "Offline"}
      </button>
    </Hint>
  );
}
export function Skeleton() {
  return (
    <div className="skeleton-stack" role="status" aria-label="Loading">
      <div className="skeleton" />
      <div className="skeleton" />
      <div className="skeleton" />
    </div>
  );
}
