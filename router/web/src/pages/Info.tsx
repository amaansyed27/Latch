import { Link } from "react-router-dom";
import {
  ArrowRight,
  ArrowUpRight,
  Check,
  Download,
  Eye,
  FileText,
  Folder,
  Laptop,
  LockKeyhole,
  Monitor,
  MousePointer2,
  Plug,
  ShieldCheck,
  Terminal,
} from "lucide-react";
import { CopyButton, Hint } from "../components/ui";
import { PageHeading, SignOut } from "../layouts/layouts";
import { useSession } from "../hooks/session";
import { bootstrap, DOWNLOAD, MCP, RELEASE } from "../lib/api";
import { ConnectionDiagram } from "./Landing";

export function DownloadPage() {
  return (
    <>
      <PageHeading
        eyebrow="For your desktop"
        title="Latch for Windows."
        description="Install once. Connect on your terms."
      />
      <section className="surface download-surface">
        <div className="download-art">
          <Laptop size={88} strokeWidth={1} />
          <span className="version">v0.5.0-beta.1</span>
        </div>
        <div>
          <span className="eyebrow">Windows private beta</span>
          <h2>Your computer, ready to connect.</h2>
          <p className="muted">
            Windows 10/11 · x64
            <br />
            Installs for your Windows account. Runs quietly after login.
          </p>
          <a href={DOWNLOAD} className="button primary">
            <Download size={18} />
            Download MSI
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
            The beta installer is unsigned. Windows SmartScreen may warn.
          </small>
        </div>
      </section>
      <ol className="download-steps">
        {[
          ["Download", "Save the MSI to your computer."],
          ["Install", "Run the installer, then open Latch from the Start menu."],
          ["Pair", "Sign in to Latch and select Add computer."],
        ].map(([title, copy], i) => (
          <li key={title}>
            <span className="step-number">0{i + 1}</span>
            <h3>{title}</h3>
            <p>{copy}</p>
          </li>
        ))}
      </ol>
      <Help
        topics={[
          "SmartScreen warning",
          "Latch command not found",
          "Pairing failed",
        ]}
      />
    </>
  );
}
const help: Record<string, string> = {
  "Computer offline":
    "Make sure your computer is awake and connected to the internet. Open Terminal and run latch status. Use latch start to reconnect, or latch restart if a connection is stuck.",
  "Pairing failed":
    "Pairing codes work once and expire after 10 minutes. Generate a fresh code in Devices, then open Latch from the Start menu or tray and paste the code into Latch Desktop. The CLI remains available as a fallback.",
  "Latch command not found":
    "Close and reopen Terminal after installing Latch so it can pick up your updated PATH. If it is still unavailable, reinstall Latch for your current Windows account.",
  "SmartScreen warning":
    "This private beta installer is unsigned. Only run an installer downloaded from the Latch website or the linked GitHub release. On the warning, review the source before choosing More info → Run anyway.",
  "OAuth failed":
    "Sign in to Latch again, then restart the connection from your MCP client. Check that the server URL ends in /mcp and that authentication is set to OAuth.",
  "ChatGPT plugin not connected":
    "Follow Connect ChatGPT to add the private MCP connection. Developer mode availability depends on your account and workspace policy. Latch is not yet published in the public directory.",
};
export function Help({ topics = Object.keys(help) }: { topics?: string[] }) {
  return (
    <section className="help-section">
      <h2>Need a hand?</h2>
      {topics.map((topic) => (
        <details key={topic}>
          <summary>{topic}</summary>
          <p>{help[topic]}</p>
        </details>
      ))}
    </section>
  );
}
export function ConnectPage() {
  return (
    <>
      <PageHeading
        eyebrow="The final connection"
        title="Bring Latch into ChatGPT."
        description="Your computer is ready. Give your conversation a way to reach it."
      />
      <section className="surface guide">
        <div className="guide-heading">
          <span className="icon-tile">
            <LockKeyhole />
          </span>
          <div>
            <h2>Private beta setup</h2>
            <p>
              This creates a private connection. Latch is not in the public
              directory yet.
            </p>
          </div>
        </div>
        <ol className="setup-list">
          <li>
            <span className="step-number">01</span>
            <div>
              <h3>Enable developer mode</h3>
              <p>
                In ChatGPT, open Settings → Security and login → Developer mode.
                Availability depends on your account and workspace policy.
              </p>
            </div>
          </li>
          <li>
            <span className="step-number">02</span>
            <div>
              <h3>Add Latch in Plugins</h3>
              <p>
                Open Plugins, select the plus button, and name the connection
                Latch. Enter this MCP server URL:
              </p>
              <div className="code-box">
                <code>{MCP}</code>
                <CopyButton value={MCP} label="URL" />
              </div>
            </div>
          </li>
          <li>
            <span className="step-number">03</span>
            <div>
              <h3>Choose OAuth and authorize</h3>
              <p>
                Sign in to your Latch account. Review the permissions, including
                command execution, then allow access.
              </p>
            </div>
          </li>
          <li>
            <span className="step-number">04</span>
            <div>
              <h3>Start a new conversation</h3>
              <p>Select Latch from the tools menu, then try:</p>
              <div className="code-box">
                <code>@Latch list my computers</code>
                <CopyButton value="@Latch list my computers" label="Prompt" />
              </div>
            </div>
          </li>
        </ol>
        <a
          className="text-link"
          href="https://developers.openai.com/plugins/deploy/connect-chatgpt"
          target="_blank"
          rel="noreferrer"
        >
          Current OpenAI setup instructions
          <ArrowUpRight size={15} />
        </a>
      </section>
      <div className="notice">
        <ShieldCheck size={20} />
        <p>
          Latch cannot see whether your ChatGPT connection is active. Confirm it
          by listing your computers from ChatGPT. The same MCP URL works with
          other clients that support OAuth.
        </p>
      </div>
    </>
  );
}
export function AccountPage() {
  const { user } = useSession();
  return (
    <>
      <PageHeading
        eyebrow="Your account"
        title="A space of your own."
        description="Your profile, session, and sign-in settings."
      />
      <section className="surface account-section">
        <h2>Profile</h2>
        <dl className="details-list">
          <div>
            <dt>Email</dt>
            <dd>{user?.email || "Not provided"}</dd>
          </div>
          <div>
            <dt>Account status</dt>
            <dd>
              <Check size={16} />
              Active
            </dd>
          </div>
        </dl>
        <h2>Sign-in & security</h2>
        <div className="account-action">
          <div>
            <h3>Password</h3>
            <p>Send a secure reset link to your email.</p>
          </div>
          <Link className="button" to="/forgot-password">
            Reset password
            <ArrowRight size={16} />
          </Link>
        </div>
        <div className="account-action">
          <div>
            <h3>Current session</h3>
            <p>Sign out of Latch in this browser.</p>
          </div>
          <SignOut />
        </div>
      </section>
    </>
  );
}
export function SecurityPage() {
  return (
    <>
      <PageHeading
        eyebrow="Trust, explained"
        title="Your computer. Your control."
        description="A clear view of what Latch can access, and where the boundaries are."
      />
      <section className="surface security-connect">
        <h2>Every connection starts with you.</h2>
        <p>
          Your computer connects outward over TLS. You do not open a port or
          expose localhost.
        </p>
        <ConnectionDiagram />
      </section>
      <div className="boundary-grid">
        <section className="surface">
          <Folder size={26} />
          <h2>Files stay in their workspace.</h2>
          <p>
            File tools accept relative paths inside the workspace you open.
            Traversal and symlink escapes are rejected.
          </p>
          <span className="boundary-label">Workspace confined</span>
        </section>
        <section className="surface command-boundary">
          <Terminal size={26} />
          <h2>Commands have real power.</h2>
          <p>
            Commands run with the permissions of the Windows user running Latch.
            They are <strong>not sandboxed</strong> and can affect files outside
            the workspace.
          </p>
          <span className="boundary-label">
            Your Windows account permissions
          </span>
        </section>
      </div>
      <section className="article">
        <h2>Access you can take back</h2>
        <p>
          Each computer is explicitly paired with a one-time code and receives
          its own credential. Revoke a computer to block new commands
          immediately and invalidate that credential. Commands already executing
          may have side effects before their process is stopped.
        </p>
        <h2>Credentials stay protected</h2>
        <p>
          Device credentials are stored in Windows Credential Manager. ChatGPT
          signs in through OAuth with scoped access and rotating tokens. Your
          control-plane credentials are never given to the model.
        </p>
        <h2>What is stored</h2>
        <p>
          Account identity, device metadata, OAuth grants, and hashed tokens are
          persisted. Redis keeps short-lived routing presence and rate-limit
          counters.
        </p>
        <h2>What is not stored by the relay</h2>
        <p>
          Project files and command output are not durably stored by Latch. A
          tool result intentionally sent back is visible to the connected client
          and governed by that client’s data policies.
        </p>
        <Link className="text-link" to="/privacy">
          Read the privacy details
          <ArrowRight size={16} />
        </Link>
      </section>
    </>
  );
}
export function DocumentPage({ path }: { path: string }) {
  const support = path === "/support";
  const privacy = path === "/privacy";
  return (
    <>
      <PageHeading
        eyebrow={support ? "We’re here to help" : "In plain language"}
        title={
          support
            ? "A little help, when you need it."
            : privacy
              ? "Privacy."
              : "Terms."
        }
        description={
          support
            ? "Start with the common fixes below."
            : "For the Latch personal beta."
        }
      />
      {support ? (
        <>
          <Help />
          <section className="notice">
            <FileText />
            <div>
              <h2>Still stuck?</h2>
              <p>
                <a href="https://github.com/amaansyed27/Latch/issues">
                  Open a GitHub issue
                </a>{" "}
                with what you expected, what happened, and steps to reproduce.
                Never share passwords, tokens, pairing codes, or connection
                strings.
              </p>
            </div>
          </section>
        </>
      ) : (
        <article className="article">
          {privacy ? (
            <>
              <h2>Account and authorization data</h2>
              <p>
                Latch stores your account identity, device metadata, pairing
                records, OAuth grants, and hashed device credentials and tokens.
                Managed authentication stores the information needed to sign you
                in securely.
              </p>
              <h2>Files and command results</h2>
              <p>
                Latch does not durably store source code, project contents,
                command stdout/stderr, or conversations. Tool requests and
                results pass through the relay to your selected client. That
                client may retain returned content under its own policies.
              </p>
              <h2>Infrastructure and logs</h2>
              <p>
                Neon stores durable authorization metadata. Redis holds
                ephemeral device presence, request/response messages, and
                rate-limit counters. Operational providers may retain request
                metadata and diagnostic logs. Avoid placing secrets in tool
                inputs or outputs.
              </p>
              <h2>Your choices</h2>
              <p>
                You can revoke paired computers from Devices and sign out from
                Account. For account data questions, contact the project through
                GitHub without posting private data.
              </p>
            </>
          ) : (
            <>
              <h2>Experimental software</h2>
              <p>
                Latch is an open-source private beta, provided without warranty
                to the extent permitted by law. Availability and behavior can
                change during testing.
              </p>
              <h2>Your responsibility</h2>
              <p>
                You are responsible for the computers you pair and the commands
                you authorize. Commands execute with your local OS user’s
                permissions and are not sandboxed.
              </p>
              <h2>Acceptable use</h2>
              <p>
                Only access computers and data you are authorized to use. Do not
                use Latch to harm others, steal data, or bypass access controls.
              </p>
              <h2>Open-source license</h2>
              <p>
                The repository’s license governs your use, modification, and
                distribution of the source code. These beta terms do not replace
                that license.
              </p>
            </>
          )}
        </article>
      )}
    </>
  );
}
export function ConsentPage() {
  const auth = bootstrap?.authorization;
  const { user } = useSession();
  const permissionDetails: Record<string, [typeof Monitor, string, string, string]> = {
    "latch:devices:read": [Monitor, "Computers", "See connected computers", "List computers paired to your Latch account."],
    "latch:roots:read": [Folder, "Folders", "See approved folder names", "See only the names and opaque IDs of folders approved locally."],
    "latch:workspace:open": [Folder, "Folders", "Open approved folders", "Open an approved folder or one of its relative subfolders."],
    "latch:files:read": [FileText, "Files", "Read files", "Read and search files inside an opened approved workspace."],
    "latch:files:write": [FileText, "Files", "Edit files", "Create, patch, move, and delete files inside an opened approved workspace."],
    "latch:exec:run": [Terminal, "Terminal", "Run and manage commands", "Run programs as your signed-in Windows user and manage long-running jobs."],
    "latch:computer:read": [Eye, "Screen", "View screen", "Capture a screen only when explicitly requested and discover visible windows."],
    "latch:computer:control": [MousePointer2, "Computer control", "Control keyboard and mouse", "Focus windows and send mouse, keyboard, typing, and scroll input."],
    "latch:mcp:read": [Plug, "Local integrations", "Discover local MCP servers", "See local integrations that you explicitly allow through ChatGPT and list their tools."],
    "latch:mcp:call": [Plug, "Local integrations", "Use local MCP tools", "Invoke tools on locally configured integrations such as Blender or a browser MCP."],
  };
  if (!auth)
    return (
      <div className="surface">
        <h1>Authorization link unavailable</h1>
        <p>Start the connection again from your MCP client.</p>
      </div>
    );
  const groups = Array.from(new Set(auth.scopes.map((scope) => permissionDetails[scope]?.[1] || "Other")));
  return (
    <section className="surface consent-card">
      <span className="icon-tile"><LockKeyhole size={25} /></span>
      <p className="eyebrow">Review access</p>
      <h1>Connect a client to Latch.</h1>
      <p className="muted">Continue only if you started this connection in a client you trust. Local permission switches can deny any capability even after OAuth approval.</p>
      <div className="connected-account"><span>Connected account</span><strong>{user?.email}</strong></div>
      <h2>Requested permissions</h2>
      <div className="permission-groups">
        {groups.map((group) => (
          <section key={group} className="permission-group">
            <h3>{group}</h3>
            <ul className="permission-list">
              {auth.scopes.filter((scope) => (permissionDetails[scope]?.[1] || "Other") === group).map((scope) => {
                const [Icon, , label, description] = permissionDetails[scope] || [ShieldCheck, "Other", scope, "Requested access"];
                const powerful = ["latch:files:write", "latch:exec:run", "latch:computer:control", "latch:mcp:call"].includes(scope);
                return <li key={scope} className={powerful ? "permission-command" : ""}><Icon size={22} /><span>{label}</span><Hint text={description} /></li>;
              })}
            </ul>
          </section>
        ))}
      </div>
      {(auth.scopes.includes("latch:exec:run") || auth.scopes.includes("latch:computer:control") || auth.scopes.includes("latch:mcp:call")) && (
        <div className="notice warning"><Terminal size={20} /><p>These permissions can cause real changes on your computer. Commands run with your Windows account permissions and <strong>are not sandboxed.</strong></p></div>
      )}
      <form action="/oauth/authorize" method="post">
        {Object.entries(auth.params).map(([name, value]) => <input key={name} type="hidden" name={name} value={value} />)}
        <input type="hidden" name="csrf" value={auth.csrf} />
        <div className="actions">
          <button className="button primary" name="approve" value="yes">Allow access<ArrowRight size={17} /></button>
          <button className="button" name="approve" value="no">Cancel</button>
        </div>
      </form>
    </section>
  );
}
