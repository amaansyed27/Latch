import { ArrowUpRight, Download, Laptop, MonitorCheck } from "lucide-react";
import { PageHeading } from "../layouts/layouts";
import { DOWNLOAD, RELEASE, WINDOWS_VERSION } from "../lib/api";

export default function DownloadPage() {
  return (
    <>
      <PageHeading
        eyebrow="For your desktop"
        title="Latch for Windows."
        description="Install once. Pair once. Latch stays ready in your system tray."
      />
      <section className="surface download-surface">
        <div className="download-art">
          <Laptop size={88} strokeWidth={1} />
          <span className="version">{WINDOWS_VERSION}</span>
        </div>
        <div>
          <span className="eyebrow">Windows private beta · {WINDOWS_VERSION}</span>
          <h2>A real desktop app, not a background command.</h2>
          <p className="muted">
            Windows 10/11 · x64
            <br />
            Latch lives in your system tray, starts after sign-in, and reconnects automatically.
          </p>
          <a href={DOWNLOAD} className="button primary">
            <Download size={18} />
            Download Latch {WINDOWS_VERSION}
          </a>
          <a
            className="text-link release-link"
            href={RELEASE}
            target="_blank"
            rel="noreferrer"
          >
            View GitHub release
            <ArrowUpRight size={15} />
          </a>
          <small>
            Private beta. The installer is currently unsigned, so Windows SmartScreen may warn.
          </small>
        </div>
      </section>

      <ol className="download-steps">
        {[
          ["Download", "Run the per-user MSI. No Rust, Cargo, or administrator setup required."],
          ["Open Latch", "Launch Latch from Start. It will remain available from the system tray."],
          ["Pair", "In Devices, create a one-time code and paste it into the Latch setup window."],
        ].map(([title, copy], i) => (
          <li key={title}>
            <span className="step-number">0{i + 1}</span>
            <h3>{title}</h3>
            <p>{copy}</p>
          </li>
        ))}
      </ol>

      <section className="notice">
        <MonitorCheck />
        <div>
          <h2>After setup</h2>
          <p>
            Closing the Latch window does not disconnect your computer. It continues quietly in the tray until you choose Quit Latch or revoke the computer from your account.
          </p>
        </div>
      </section>

      <section className="help-section">
        <h2>Need a hand?</h2>
        <details>
          <summary>Windows SmartScreen warning</summary>
          <p>The private-beta installer is unsigned. Only run the MSI downloaded from this page or the linked GitHub release.</p>
        </details>
        <details>
          <summary>Latch is not in the tray</summary>
          <p>Open Latch from the Start menu. After pairing, it will start automatically when you sign in to Windows.</p>
        </details>
        <details>
          <summary>Pairing failed</summary>
          <p>Pairing codes work once and expire after 10 minutes. Generate a fresh code in Devices and paste it into the Latch app.</p>
        </details>
      </section>
    </>
  );
}
