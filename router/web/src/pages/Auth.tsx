import { useState, type FormEvent } from "react";
import { Link, Navigate, useLocation } from "react-router-dom";
import {
  ArrowLeft,
  ArrowRight,
  CheckCircle2,
  Eye,
  EyeOff,
  LockKeyhole,
  ShieldCheck,
} from "lucide-react";
import { Brand } from "../layouts/layouts";
import { Spinner, ThemePicker } from "../components/ui";
import { ApiError, bootstrap, message, request, safeReturn } from "../lib/api";
import { useSession } from "../hooks/session";

function Password({
  name,
  label,
  confirm = false,
}: {
  name: string;
  label: string;
  confirm?: boolean;
}) {
  const [show, setShow] = useState(false);
  return (
    <label>
      {label}
      <span className="input-wrap">
        <input
          name={name}
          type={show ? "text" : "password"}
          autoComplete={confirm ? "new-password" : "current-password"}
          minLength={8}
          maxLength={128}
          required
          placeholder="At least 8 characters"
        />
        <button
          type="button"
          className="icon-button"
          aria-label={show ? `Hide ${label}` : `Show ${label}`}
          onClick={() => setShow(!show)}
        >
          {show ? <EyeOff size={18} /> : <Eye size={18} />}
        </button>
      </span>
    </label>
  );
}
export default function Auth() {
  const location = useLocation();
  const { user } = useSession();
  const mode = location.pathname.slice(1);
  const signup = mode === "signup";
  const forgot = mode === "forgot-password";
  const reset = mode === "reset-password";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState(false);
  const target = safeReturn(
    new URLSearchParams(location.search).get("return_to") ||
      bootstrap?.returnTo ||
      null,
  );
  if (user && !forgot && !reset) return <Navigate to={target} replace />;
  const title = signup
    ? "Make yourself at home."
    : forgot
      ? "Back to your account."
      : reset
        ? "A fresh start."
        : "Welcome back.";
  const action = signup
    ? "Create account"
    : forgot
      ? "Send reset link"
      : reset
        ? "Update password"
        : "Sign in";
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError("");
    const data = Object.fromEntries(new FormData(event.currentTarget));
    if ((signup || reset) && data.password !== data.confirm) {
      setError("Your passwords don’t match. Please try again.");
      return;
    }
    setBusy(true);
    try {
      if (forgot) {
        await request("/api/auth/request-password-reset", "POST", {
          email: data.email,
          redirectTo: `${window.location.origin}/reset-password`,
        });
        setSuccess(true);
      } else if (reset) {
        const token = new URLSearchParams(location.search).get("token");
        if (!token)
          throw new Error(
            "This reset link is missing or expired. Request a new link.",
          );
        await request("/api/auth/reset-password", "POST", {
          token,
          newPassword: data.password,
        });
        window.location.assign("/login");
      } else {
        await request(
          `/api/auth/${signup ? "sign-up/email" : "sign-in/email"}`,
          "POST",
          {
            email: data.email,
            password: data.password,
            ...(signup ? { name: String(data.email).split("@")[0] } : {}),
          },
        );
        window.location.assign(target);
      }
    } catch (err) {
      setError(
        !signup &&
          !forgot &&
          !reset &&
          err instanceof ApiError &&
          err.status === 401
          ? "That email and password don’t match. Please try again."
          : message(err),
      );
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="auth-page">
      <section className="auth-context">
        <Brand />
        <div>
          <span className="eyebrow">A little closer to your computer</span>
          <h2>
            Your tools.
            <br />
            Your files.
            <br />
            <span>Your machine.</span>
          </h2>
          <p>Bring the computer you already own into the conversation.</p>
          <div className="trust-line">
            <ShieldCheck size={19} /> Explicit access. Always revocable.
          </div>
        </div>
        <Link className="text-link" to="/">
          <ArrowLeft size={16} /> Back to Latch
        </Link>
      </section>
      <main id="main" className="auth-main">
        <div className="auth-toolbar">
          <ThemePicker />
        </div>
        <section className="auth-card surface">
          <span className="icon-tile">
            <LockKeyhole size={24} />
          </span>
          <h1>{title}</h1>
          <p className="muted">
            {signup
              ? "One account. All your computers."
              : forgot
                ? "We’ll send you a secure link to reset your password."
                : reset
                  ? "Choose a strong password for your Latch account."
                  : "Sign in to your Latch workspace."}
          </p>
          {success ? (
            <div className="success-panel" role="status">
              <CheckCircle2 />
              <h2>Check your email</h2>
              <p>
                If an account exists, you’ll receive a password reset link.
                Check your spam folder too.
              </p>
              <Link to="/login" className="button">
                Back to sign in
              </Link>
            </div>
          ) : (
            <form onSubmit={submit}>
              {!reset && (
                <label>
                  Email address
                  <input
                    name="email"
                    type="email"
                    autoComplete="email"
                    required
                    placeholder="you@example.com"
                  />
                </label>
              )}
              {!forgot && (
                <Password
                  name="password"
                  label="Password"
                  confirm={signup || reset}
                />
              )}
              {(signup || reset) && (
                <Password name="confirm" label="Confirm password" confirm />
              )}
              {!signup && !forgot && !reset && (
                <Link className="forgot-link" to="/forgot-password">
                  Forgot password?
                </Link>
              )}
              {error && (
                <p role="alert" className="error-message">
                  {error}
                </p>
              )}
              <button
                className="button primary full"
                disabled={busy || bootstrap?.authReady === false}
              >
                {busy ? (
                  <Spinner />
                ) : (
                  <>
                    {action}
                    <ArrowRight size={18} />
                  </>
                )}
              </button>
              {bootstrap?.authReady === false && (
                <p className="error-message">
                  Sign in is temporarily unavailable. Please try again later.
                </p>
              )}
            </form>
          )}
          <p className="auth-switch">
            {signup ? (
              <>
                Already have an account?{" "}
                <Link to={`/login?return_to=${encodeURIComponent(target)}`}>
                  Sign in
                </Link>
              </>
            ) : forgot || reset ? (
              <Link to="/login">Back to sign in</Link>
            ) : (
              <>
                New to Latch?{" "}
                <Link to={`/signup?return_to=${encodeURIComponent(target)}`}>
                  Create an account
                </Link>
              </>
            )}
          </p>
        </section>
        <p className="auth-legal">
          By continuing, you agree to our <Link to="/terms">Terms</Link> and{" "}
          <Link to="/privacy">Privacy policy</Link>.
        </p>
      </main>
    </div>
  );
}
