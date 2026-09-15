import { useEffect, useRef, useState } from "react";
import {
  AppWindow,
  ArrowRight,
  Check,
  CheckCircle2,
  Download,
  Monitor,
} from "lucide-react";
import { DOWNLOAD, message, request, type Device } from "../lib/api";
import { CopyButton, Modal, Spinner } from "./ui";

export default function Pairing({
  onClose,
  onPaired,
  existing,
}: {
  onClose(): void;
  onPaired(): void;
  existing: Device[];
}) {
  const [step, setStep] = useState(0);
  const [code, setCode] = useState("");
  const [expires, setExpires] = useState(0);
  const [seconds, setSeconds] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [connected, setConnected] = useState<Device | null>(null);
  const previous = useRef(new Set(existing.map((d) => d.deviceId)));

  async function generate() {
    setBusy(true);
    setError("");
    try {
      const result = await request<{
        pairing_code: string;
        expires_in: number;
      }>("/api/pairing/create", "POST");
      setCode(result.pairing_code);
      setExpires(Date.now() + result.expires_in * 1000);
      setSeconds(result.expires_in);
      setStep(2);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (!expires || connected) return;
    const timer = setInterval(
      () => setSeconds(Math.max(0, Math.ceil((expires - Date.now()) / 1000))),
      1000,
    );
    return () => clearInterval(timer);
  }, [expires, connected]);

  useEffect(() => {
    if (step !== 2 || !seconds) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const { devices } = await request<{ devices: Device[] }>(
          "/api/my/devices",
        );
        const added = devices.find(
          (d) => d.online && !previous.current.has(d.deviceId),
        );
        if (cancelled) return;
        if (added) {
          setConnected(added);
          setStep(3);
          onPaired();
          return;
        }
        setError("");
      } catch (e) {
        if (!cancelled) setError(message(e));
      }
      if (!cancelled) timer = setTimeout(poll, 4000);
    }
    timer = setTimeout(poll, 2000);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [step, Boolean(seconds)]);

  return (
    <Modal
      open
      onClose={onClose}
      title={step === 3 ? "Computer connected" : "Add a computer"}
      description={
        step === 3
          ? "Your computer is now available to Latch."
          : "Install Latch, pair once, and it stays available from your tray."
      }
    >
      <div
        className="pair-progress"
        aria-label={`Step ${Math.min(step + 1, 3)} of 3`}
      >
        {["Install", "Pair", "Connect"].map((name, i) => (
          <span className={i <= step ? "current" : ""} key={name}>
            <b>{i < step ? <Check size={13} /> : i + 1}</b>
            {name}
          </span>
        ))}
      </div>

      {step === 0 ? (
        <div className="pair-step">
          <span className="big-icon">
            <Monitor size={38} />
          </span>
          <h3>Install Latch for Windows.</h3>
          <p>
            The desktop app lives in your system tray, starts after sign-in, and
            reconnects automatically.
          </p>
          <a className="button primary full" href={DOWNLOAD}>
            <Download size={18} />
            Download for Windows
          </a>
          <button className="quiet-button full" onClick={() => setStep(1)}>
            I already installed Latch
            <ArrowRight size={17} />
          </button>
          <small>Windows 10/11 · x64 · Private beta</small>
        </div>
      ) : step === 1 ? (
        <div className="pair-step">
          <span className="big-icon">
            <AppWindow size={34} />
          </span>
          <h3>Pair through the Latch app.</h3>
          <p>
            We’ll create a one-time code. Open Latch from the Start menu or tray
            and paste it into the setup screen.
          </p>
          <button
            className="button primary full"
            onClick={generate}
            disabled={busy}
          >
            {busy ? <Spinner /> : "Generate pairing code"}
          </button>
        </div>
      ) : step === 2 ? (
        <div className="pair-step">
          <h3>Paste this code into Latch.</h3>
          <p>
            Open the Latch tray app on this computer, paste the code below, and
            choose <strong>Pair this computer</strong>.
          </p>
          <div className="code-box">
            <code>{code}</code>
            <CopyButton value={code} label="Pairing code" />
          </div>
          <span className="expiry">
            Expires in{" "}
            {Math.floor(seconds / 60)
              .toString()
              .padStart(2, "0")}
            :{(seconds % 60).toString().padStart(2, "0")}
          </span>
          {seconds ? (
            <p className="waiting" role="status">
              <Spinner />
              Waiting for the Latch app…
            </p>
          ) : (
            <>
              <p className="error-message">
                This code expired. Generate a new invitation.
              </p>
              <button className="button" onClick={generate} disabled={busy}>
                Generate new code
              </button>
            </>
          )}
          <details>
            <summary>Prefer the command line?</summary>
            <p>
              The desktop app is recommended. As a fallback, open Windows
              Terminal and run <code>latch pair {code}</code>.
            </p>
          </details>
        </div>
      ) : (
        <div className="pair-step">
          <span className="big-icon success">
            <CheckCircle2 size={40} />
          </span>
          <h3>{connected?.deviceName}</h3>
          <p>
            You’re connected. Latch can now stay in the tray while you authorize
            it in ChatGPT or another MCP client.
          </p>
          <button className="button primary full" onClick={onClose}>
            Done
            <Check size={18} />
          </button>
        </div>
      )}

      {error && (
        <p role="alert" className="error-message">
          {error}
        </p>
      )}
    </Modal>
  );
}
