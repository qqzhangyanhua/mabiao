import { AuthProvider, useAuth } from "./auth";
import { Layout } from "./components/Layout";
import { ExportProvider } from "./export-action";
import { useHashRoute } from "./hooks";
import { allowedRoute, parseRoute } from "./lib/route";
import { LoginPage } from "./pages/LoginPage";
import { MemberPage } from "./pages/MemberPage";
import { MembersPage } from "./pages/MembersPage";
import { OverviewPage } from "./pages/OverviewPage";
import { PricingPage } from "./pages/PricingPage";
import { ProjectPage } from "./pages/ProjectPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import { SessionPage } from "./pages/SessionPage";
import { WorkTimelinePage } from "./pages/WorkTimelinePage";
import { ThemeProvider } from "./theme";
import { ViewProvider } from "./view";

function Routed() {
  const { profile } = useAuth();
  const hash = useHashRoute();
  const route = allowedRoute(parseRoute(hash), profile.role, profile.id);
  return (
    <ViewProvider route={route} hash={hash}>
      <ExportProvider>
        <Layout route={route}>
          {route.page === "overview" && <OverviewPage />}
          {route.page === "members" && <MembersPage />}
          {route.page === "member" && <MemberPage key={route.accountId} accountId={route.accountId} />}
          {route.page === "projects" && <ProjectsPage />}
          {route.page === "project" && <ProjectPage key={route.projectId} projectId={route.projectId} />}
          {route.page === "session" && <SessionPage key={route.sessionId} sessionId={route.sessionId} />}
          {route.page === "timeline" && <WorkTimelinePage />}
          {route.page === "pricing" && <PricingPage />}
        </Layout>
      </ExportProvider>
    </ViewProvider>
  );
}

export function App() {
  return (
    <ThemeProvider>
      <AuthProvider signedIn={<Routed />} signedOut={<LoginPage />} />
    </ThemeProvider>
  );
}
