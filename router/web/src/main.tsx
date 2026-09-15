import { Component, Suspense, lazy, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import {
  BrowserRouter,
  Link,
  Navigate,
  Route,
  Routes,
  useLocation,
} from "react-router-dom";
import { SessionProvider, useSession } from "./hooks/session";
import { Feedback, Skeleton } from "./components/ui";
import { AppLayout, PublicLayout } from "./layouts/layouts";
import { bootstrap } from "./lib/api";
import "./styles/app.css";

const Landing = lazy(() => import("./pages/Landing"));
const Auth = lazy(() => import("./pages/Auth"));
const Devices = lazy(() => import("./pages/Devices"));
const DownloadPage = lazy(() => import("./pages/Download"));
const Info = lazy(() =>
  import("./pages/Info").then((m) => ({
    default: function InfoRoute() {
      const path = useLocation().pathname;
      return path === "/connect-chatgpt" ? (
        <m.ConnectPage />
      ) : path === "/account" ? (
        <m.AccountPage />
      ) : path === "/security" ? (
        <m.SecurityPage />
      ) : path === "/oauth/authorize" ? (
        <m.ConsentPage />
      ) : (
        <m.DocumentPage path={path} />
      );
    },
  })),
);

class ErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    return this.state.failed ? (
      <main className="error-page">
        <h1>Something didn’t load.</h1>
        <p>Please reload to reconnect to Latch.</p>
        <button className="button" onClick={() => location.reload()}>
          Reload
        </button>
      </main>
    ) : (
      this.props.children
    );
  }
}

function Frame({
  children,
  protect = false,
}: {
  children: ReactNode;
  protect?: boolean;
}) {
  const { user, loading, error } = useSession();
  const location = useLocation();
  if (loading)
    return (
      <main className="workspace">
        <Skeleton />
      </main>
    );
  if (error)
    return (
      <main className="error-page">
        <h1>Connection interrupted</h1>
        <p>{error}</p>
        <button className="button" onClick={() => window.location.reload()}>
          Reload
        </button>
      </main>
    );
  if (protect && !user)
    return (
      <Navigate
        to={`/login?return_to=${encodeURIComponent(location.pathname + location.search)}`}
        replace
      />
    );
  return user &&
    location.pathname !== "/" &&
    location.pathname !== "/oauth/authorize" ? (
    <AppLayout>{children}</AppLayout>
  ) : (
    <PublicLayout>{children}</PublicLayout>
  );
}

function App() {
  const location = useLocation();
  if (bootstrap?.error)
    return (
      <PublicLayout>
        <section className="error-page">
          <h1>{bootstrap.error.title}</h1>
          <p>{bootstrap.error.message}</p>
          <Link to="/" className="button">
            Return home
          </Link>
        </section>
      </PublicLayout>
    );
  return (
    <>
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <Suspense
        fallback={
          <main className="workspace">
            <Skeleton />
          </main>
        }
      >
        <Routes>
          <Route
            path="/"
            element={
              <Frame>
                <Landing />
              </Frame>
            }
          />
          {["login", "signup", "forgot-password", "reset-password"].map(
            (path) => (
              <Route
                key={path}
                path={`/${path}`}
                element={<Auth key={location.pathname} />}
              />
            ),
          )}
          <Route
            path="/dashboard"
            element={
              <Frame protect>
                <Devices overview />
              </Frame>
            }
          />
          <Route
            path="/devices"
            element={
              <Frame protect>
                <Devices />
              </Frame>
            }
          />
          <Route
            path="/download"
            element={
              <Frame>
                <DownloadPage />
              </Frame>
            }
          />
          <Route
            path="/account"
            element={
              <Frame protect>
                <Info />
              </Frame>
            }
          />
          {[
            "connect-chatgpt",
            "security",
            "privacy",
            "terms",
            "support",
            "oauth/authorize",
          ].map((path) => (
            <Route
              key={path}
              path={`/${path}`}
              element={
                <Frame>
                  <Info />
                </Frame>
              }
            />
          ))}
          <Route
            path="*"
            element={
              <Frame>
                <section className="error-page">
                  <p className="eyebrow">404</p>
                  <h1>This page isn’t here.</h1>
                  <Link className="button" to="/">
                    Return home
                  </Link>
                </section>
              </Frame>
            }
          />
        </Routes>
      </Suspense>
    </>
  );
}

createRoot(document.getElementById("root")!).render(
  <ErrorBoundary>
    <BrowserRouter>
      <SessionProvider>
        <Feedback>
          <App />
        </Feedback>
      </SessionProvider>
    </BrowserRouter>
  </ErrorBoundary>,
);
