import { useState, type ReactNode } from "react";
import { Link, NavLink, useLocation } from "react-router-dom";
import {
  ArrowUpRight,
  Cable,
  Download,
  LayoutDashboard,
  LogOut,
  Menu,
  Monitor,
  Shield,
  User,
  X,
} from "lucide-react";
import * as Dialog from "@radix-ui/react-dialog";
import { useSession } from "../hooks/session";
import { ThemePicker, useToast } from "../components/ui";
import { message } from "../lib/api";

export function Brand() {
  return (
    <Link className="brand" to="/" aria-label="Latch home">
      <span className="brand-mark">
        <Cable size={22} />
      </span>
      Latch<span className="beta">BETA</span>
    </Link>
  );
}
const routes = [
  ["/dashboard", "Overview", LayoutDashboard],
  ["/devices", "Devices", Monitor],
  ["/download", "Download", Download],
  ["/connect-chatgpt", "Connect ChatGPT", Cable],
  ["/security", "Security", Shield],
  ["/account", "Account", User],
] as const;
function AppNav({ close }: { close?(): void }) {
  return (
    <nav className="app-nav" aria-label="Application">
      {routes.map(([path, label, Icon]) => (
        <NavLink key={path} to={path} onClick={close}>
          <Icon size={19} />
          {label}
          {path === "/connect-chatgpt" && (
            <ArrowUpRight size={15} className="nav-end" />
          )}
        </NavLink>
      ))}
    </nav>
  );
}
export function SignOut() {
  const { signOut } = useSession();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  return (
    <button
      className="quiet-button"
      disabled={busy}
      onClick={async () => {
        setBusy(true);
        try {
          await signOut();
        } catch (error) {
          toast(message(error));
          setBusy(false);
        }
      }}
    >
      <LogOut size={17} />
      {busy ? "Signing out…" : "Sign out"}
    </button>
  );
}
export function AppLayout({ children }: { children: ReactNode }) {
  const { user } = useSession();
  const [open, setOpen] = useState(false);
  const location = useLocation();
  const title =
    routes.find(([path]) => path === location.pathname)?.[1] || "Latch";
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <Brand />
        <AppNav />
        <div className="sidebar-foot">
          <ThemePicker />
          <div className="profile">
            <span className="avatar">
              <User size={18} />
            </span>
            <div>
              <strong>Your workspace</strong>
              <small>{user?.email}</small>
            </div>
          </div>
          <SignOut />
        </div>
      </aside>
      <div className="app-body">
        <header className="context-header">
          <button
            className="icon-button mobile-only"
            aria-label="Open navigation"
            onClick={() => setOpen(true)}
          >
            <Menu size={21} />
          </button>
          <span>{title}</span>
          <span className="header-note">
            <Shield size={14} /> Your machine. Your access.
          </span>
        </header>
        <main id="main" className="workspace">
          {children}
        </main>
      </div>
      <Dialog.Root open={open} onOpenChange={setOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="overlay" />
          <Dialog.Content className="mobile-drawer">
            <Dialog.Title className="sr-only">Navigation</Dialog.Title>
            <Dialog.Description className="sr-only">
              Navigate your Latch account
            </Dialog.Description>
            <Brand />
            <Dialog.Close
              className="icon-button close"
              aria-label="Close navigation"
            >
              <X />
            </Dialog.Close>
            <AppNav close={() => setOpen(false)} />
            <ThemePicker />
            <SignOut />
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  );
}
export function PublicLayout({ children }: { children: ReactNode }) {
  const { user } = useSession();
  const [open, setOpen] = useState(false);
  return (
    <>
      <header className="public-header">
        <Brand />
        <nav
          className={open ? "public-nav expanded" : "public-nav"}
          aria-label="Main navigation"
        >
          <NavLink to="/security" onClick={() => setOpen(false)}>
            Security
          </NavLink>
          <NavLink to="/download" onClick={() => setOpen(false)}>
            Download
          </NavLink>
          {user ? (
            <Link className="button compact" to="/dashboard">
              Open dashboard <ArrowUpRight size={15} />
            </Link>
          ) : (
            <>
              <Link to="/login" onClick={() => setOpen(false)}>
                Sign in
              </Link>
              <Link
                className="button compact"
                to="/signup"
                onClick={() => setOpen(false)}
              >
                Create account
              </Link>
            </>
          )}
        </nav>
        <button
          className="icon-button mobile-only"
          aria-label="Toggle navigation"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          {open ? <X /> : <Menu />}
        </button>
      </header>
      <main id="main" className="public-main">
        {children}
      </main>
      <footer>
        <Brand />
        <span>Local compute. On your terms.</span>
        <nav aria-label="Footer">
          <Link to="/privacy">Privacy</Link>
          <Link to="/terms">Terms</Link>
          <Link to="/support">Support</Link>
        </nav>
        <ThemePicker />
      </footer>
    </>
  );
}
export function PageHeading({
  eyebrow,
  title,
  description,
  children,
}: {
  eyebrow: string;
  title: string;
  description: string;
  children?: ReactNode;
}) {
  return (
    <div className="page-heading">
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h1>{title}</h1>
        <p className="muted">{description}</p>
      </div>
      {children}
    </div>
  );
}
