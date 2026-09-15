import { useState } from "react";
import { Link } from "react-router-dom";
import {
  ArrowRight,
  Cable,
  Download,
  Monitor,
  Plus,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { useDevices } from "../hooks/devices";
import {
  CopyButton,
  Hint,
  Modal,
  Skeleton,
  Spinner,
  Status,
  useToast,
} from "../components/ui";
import Pairing from "../components/Pairing";
import { PageHeading } from "../layouts/layouts";
import { message, request, type Device } from "../lib/api";

export default function Devices({ overview = false }: { overview?: boolean }) {
  const { devices, loading, error, refresh } = useDevices();
  const [pairing, setPairing] = useState(false);
  const [detail, setDetail] = useState<Device | null>(null);
  const [revoke, setRevoke] = useState<Device | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState("");
  const toast = useToast();
  const online = devices.filter((d) => d.online).length;
  async function remove() {
    if (!revoke) return;
    setBusy(true);
    setActionError("");
    try {
      await request("/api/my/devices", "DELETE", {
        device_id: revoke.deviceId,
      });
      setRevoke(null);
      setDetail(null);
      await refresh();
      toast("Computer revoked. Its credential is no longer valid.");
    } catch (e) {
      setActionError(message(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <PageHeading
        eyebrow={overview ? "Your workspace" : "Your computers"}
        title={
          overview
            ? devices.length
              ? "Everything, within reach."
              : "Welcome to Latch."
            : "Devices"
        }
        description={
          overview
            ? "A direct line to the computers you own."
            : "Choose what’s connected. Keep control of every computer."
        }
      >
        <button className="button primary" onClick={() => setPairing(true)}>
          <Plus size={18} />
          Add computer
        </button>
      </PageHeading>
      {loading ? (
        <Skeleton />
      ) : (
        <>
          {error && (
            <div className="error-message" role="alert">
              {error}
              <button className="quiet-button" onClick={() => void refresh()}>
                Try again
              </button>
            </div>
          )}
          {overview && (
            <div className="summary-grid">
              <div className="surface summary">
                <span className="icon-tile">
                  <Monitor />
                </span>
                <div>
                  <strong>
                    {online}
                    <span> / {devices.length}</span>
                  </strong>
                  <p>computers online</p>
                </div>
              </div>
              <Link to="/connect-chatgpt" className="surface next-step">
                <Cable size={22} />
                <div>
                  <strong>Connect your conversation</strong>
                  <p>Set up ChatGPT or another MCP client</p>
                </div>
                <ArrowRight size={19} />
              </Link>
            </div>
          )}
          {devices.length === 0 ? (
            <section className="surface onboarding">
              <div className="onboarding-copy">
                <span className="eyebrow">
                  {overview ? "Let’s get you connected" : "No computers yet"}
                </span>
                <h2>
                  Your first computer.
                  <br />A few steps away.
                </h2>
                <p>
                  Pair a computer to give ChatGPT access to your local tools and
                  workspaces.
                </p>
                <button
                  className="button primary"
                  onClick={() => setPairing(true)}
                >
                  Connect my first computer
                  <ArrowRight size={18} />
                </button>
                <div className="trust-line">
                  <ShieldCheck size={16} /> Every connection is yours to revoke.
                </div>
              </div>
              <ol className="onboarding-steps">
                <li>
                  <span className="step-number">01</span>
                  <div>
                    <h3>Download & install</h3>
                    <p>Get the Latch app for Windows.</p>
                    <Link to="/download" className="text-link">
                      Download Latch
                      <Download size={14} />
                    </Link>
                  </div>
                </li>
                <li>
                  <span className="step-number">02</span>
                  <div>
                    <h3>Pair your computer</h3>
                    <p>Create a one-time code here, then paste it into Latch Desktop.</p>
                  </div>
                </li>
                <li>
                  <span className="step-number">03</span>
                  <div>
                    <h3>Connect ChatGPT</h3>
                    <p>Review permissions, then start a conversation.</p>
                  </div>
                </li>
              </ol>
            </section>
          ) : (
            <section className="surface device-section">
              <div className="section-title">
                <h2>
                  {overview ? "Your computers" : "Paired computers"}
                  <span className="count">{devices.length}</span>
                </h2>
                <button
                  className="icon-button"
                  aria-label="Refresh computers"
                  onClick={() => void refresh()}
                >
                  <RefreshCw size={18} />
                </button>
              </div>
              <div className="device-list">
                {devices.map((device) => (
                  <article className="device-row" key={device.deviceId}>
                    <span className="device-icon">
                      <Monitor size={26} />
                    </span>
                    <div className="device-main">
                      <button
                        className="device-name"
                        onClick={() => setDetail(device)}
                      >
                        {device.deviceName}
                      </button>
                      <div className="device-meta">
                        <code>
                          {device.deviceId.slice(0, 8)}…
                          {device.deviceId.slice(-4)}
                        </code>
                        <Hint text="Unique identifier for this computer." />
                        {device.pairedAt && (
                          <span>
                            Paired{" "}
                            {new Date(device.pairedAt).toLocaleDateString()}
                          </span>
                        )}
                      </div>
                    </div>
                    <Status online={device.online} />
                    <button
                      className="button compact"
                      onClick={() => setDetail(device)}
                    >
                      Details
                      <ArrowRight size={15} />
                    </button>
                  </article>
                ))}
              </div>
            </section>
          )}
          {overview && (
            <div className="workspace-foot">
              <ShieldCheck size={19} />
              <p>
                File tools stay inside a workspace. Commands use your Windows
                account permissions.
              </p>
              <Link to="/security">
                How access works
                <ArrowRight size={15} />
              </Link>
            </div>
          )}
        </>
      )}
      {pairing && (
        <Pairing
          onClose={() => setPairing(false)}
          existing={devices}
          onPaired={() => void refresh()}
        />
      )}
      {detail && (
        <Modal
          open
          onClose={() => setDetail(null)}
          title={detail.deviceName}
          description="Connection details and access controls."
        >
          <dl className="details-list">
            <div>
              <dt>Connection</dt>
              <dd>
                <Status
                  online={
                    devices.find((d) => d.deviceId === detail.deviceId)
                      ?.online ?? false
                  }
                />
              </dd>
            </div>
            <div>
              <dt>
                Device ID <Hint text="Unique identifier for this computer." />
              </dt>
              <dd>
                <code>{detail.deviceId}</code>
                <CopyButton value={detail.deviceId} label="Device ID" />
              </dd>
            </div>
            {detail.pairedAt && (
              <div>
                <dt>Paired</dt>
                <dd>{new Date(detail.pairedAt).toLocaleString()}</dd>
              </div>
            )}
          </dl>
          <div className="danger-zone">
            <h3>Remove this computer’s access</h3>
            <p>
              New requests will be blocked and its device credential
              invalidated.
            </p>
            <button
              className="button"
              onClick={() => {
                setRevoke(detail);
                setDetail(null);
              }}
            >
              Revoke computer
            </button>
          </div>
        </Modal>
      )}
      {revoke && (
        <Modal
          open
          onClose={() => !busy && setRevoke(null)}
          title={`Revoke ${revoke.deviceName}?`}
          description="This immediately prevents ChatGPT from sending new Latch commands to this computer."
        >
          <p>You can reconnect it later with a new pairing code.</p>
          {actionError && (
            <p role="alert" className="error-message">
              {actionError}
            </p>
          )}
          <div className="actions end">
            <button
              className="button"
              disabled={busy}
              onClick={() => setRevoke(null)}
            >
              Cancel
            </button>
            <button className="button danger" disabled={busy} onClick={remove}>
              {busy ? <Spinner /> : "Revoke computer"}
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}
