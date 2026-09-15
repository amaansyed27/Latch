import { Link } from "react-router-dom";
import {
  ArrowRight,
  ArrowUpRight,
  Cable,
  Cpu,
  Download,
  LockKeyhole,
  MessageSquare,
  Monitor,
  ShieldCheck,
  Terminal,
  Wifi,
} from "lucide-react";
import { useSession } from "../hooks/session";

export function ConnectionDiagram() {
  return (
    <div
      className="connection-diagram"
      aria-label="ChatGPT routes approved requests through Latch to your computer"
    >
      <div className="connection-node">
        <MessageSquare />
        <span>ChatGPT</span>
      </div>
      <span className="connection-line" />
      <div className="connection-node latch-node">
        <Cable />
        <span>Latch</span>
      </div>
      <span className="connection-line" />
      <div className="connection-node">
        <Monitor />
        <span>Your PC</span>
      </div>
    </div>
  );
}
export default function Landing() {
  const { user } = useSession();
  return (
    <>
      <section className="hero">
        <div className="hero-copy">
          <div className="release-note">
            <span className="dot" /> Windows private beta{" "}
            <ArrowUpRight size={13} />
          </div>
          <h1>
            Your computer.
            <br />
            <span>In the conversation.</span>
          </h1>
          <p>
            Give ChatGPT controlled access to the machine you already own. Your
            tools and files, right where you left them.
          </p>
          <div className="actions">
            <Link
              className="button primary"
              to={user ? "/dashboard" : "/download"}
            >
              <Download size={18} />
              {user ? "Open your workspace" : "Get Latch for Windows"}
            </Link>
            <Link className="text-link" to={user ? "/devices" : "/login"}>
              {user ? "Manage computers" : "Sign in"}
              <ArrowRight size={17} />
            </Link>
          </div>
          <small className="hero-meta">
            Windows 10/11 · x64 · Free & open source
          </small>
        </div>
        <div className="hero-object">
          <div className="object-top">
            <span className="tiny-label">YOUR MACHINE, CONNECTED</span>
            <span className="indicator">
              <span className="dot" /> By invitation only
            </span>
          </div>
          <ConnectionDiagram />
          <div className="sample-command">
            <span>
              <Terminal size={16} /> A conversation, a real result.
            </span>
            <p>“Run node --version on my laptop.”</p>
            <div className="sample-foot">
              <ShieldCheck size={15} /> Execution happens on your computer
            </div>
          </div>
          <div className="object-bottom">
            <span>Local execution</span>
            <span>Encrypted connection</span>
          </div>
        </div>
      </section>
      <section className="principle-grid">
        {[
          [
            Cpu,
            "Local compute",
            "Your computer does the work. Use the tools and projects you already have.",
          ],
          [
            LockKeyhole,
            "Explicit access",
            "Pair each computer, review permissions, and revoke access whenever you choose.",
          ],
          [
            Wifi,
            "Outbound only",
            "Your computer starts the connection. No open ports or exposed localhost.",
          ],
        ].map(([Icon, title, copy]) => {
          const I = Icon as typeof Cpu;
          return (
            <article key={String(title)}>
              <span className="icon-tile small">
                <I size={21} />
              </span>
              <h2>{String(title)}</h2>
              <p>{String(copy)}</p>
            </article>
          );
        })}
      </section>
      <section className="how-section">
        <div>
          <p className="eyebrow">A simple connection</p>
          <h2>
            From installed
            <br />
            to in conversation.
          </h2>
          <p className="muted">A few deliberate steps. Then you’re ready.</p>
        </div>
        <ol className="setup-list">
          {[
            [
              "Install Latch",
              "Download the Windows app and install it for your account.",
            ],
            [
              "Pair your computer",
              "Sign in and follow a one-time pairing command.",
            ],
            [
              "Connect ChatGPT",
              "Add Latch privately and review the requested access.",
            ],
            [
              "Ask away",
              "Try “@Latch list my computers” in a connected conversation.",
            ],
          ].map(([title, copy], i) => (
            <li key={title}>
              <span className="step-number">0{i + 1}</span>
              <div>
                <h3>{title}</h3>
                <p>{copy}</p>
              </div>
            </li>
          ))}
        </ol>
      </section>
      <section className="security-strip">
        <ShieldCheck size={28} />
        <div>
          <h2>Know what you’re giving access to.</h2>
          <p>
            File tools stay inside a workspace. Commands run with your Windows
            user permissions and are not sandboxed.
          </p>
        </div>
        <Link className="button" to="/security">
          Understand the boundary <ArrowUpRight size={17} />
        </Link>
      </section>
      <section className="closing">
        <p className="eyebrow">Your next step</p>
        <h2>Make your computer part of it.</h2>
        <Link to="/signup" className="button primary">
          Create your Latch account
          <ArrowRight size={18} />
        </Link>
      </section>
    </>
  );
}
