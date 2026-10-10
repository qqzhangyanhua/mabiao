import { AuthProvider, useAuth } from "./auth";
import { Layout } from "./components/Layout";
import { useHashRoute } from "./hooks";
import { allowedRoute, parseRoute } from "./lib/route";
import { LoginPage } from "./pages/LoginPage";
import { MemberPage } from "./pages/MemberPage";
import { MembersPage } from "./pages/MembersPage";
import { OverviewPage } from "./pages/OverviewPage";

function Routed() {
  const { profile } = useAuth();
  const hash = useHashRoute();
  // 越权的路由直接换成自己能看的页面；真正的拦截在服务端（成员访问别人数据得 403）。
  const route = allowedRoute(parseRoute(hash), profile.role, profile.id);
  return (
    <Layout route={route}>
      {route.page === "overview" && <OverviewPage />}
      {route.page === "members" && <MembersPage />}
      {/* 换成员时要重置区间、分页等页面状态。 */}
      {route.page === "member" && <MemberPage key={route.accountId} accountId={route.accountId} />}
    </Layout>
  );
}

export function App() {
  return <AuthProvider signedIn={<Routed />} signedOut={<LoginPage />} />;
}
